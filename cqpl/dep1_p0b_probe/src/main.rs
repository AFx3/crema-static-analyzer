#![feature(rustc_private)]
extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;
extern crate rustc_hir;

use rustc_driver::{Callbacks, RunCompiler};
use rustc_interface::Queries;
use rustc_middle::{mir::TerminatorKind, ty::{self, TyKind}};
use rustc_span::def_id::DefId;
use serde_json::{json, Value};
use std::{env, fs::{self, OpenOptions}, io::Write, process::{Command, ExitCode}};

struct Probe { output: String }

fn identity(tcx: rustc_middle::ty::TyCtxt<'_>, def: DefId) -> Value {
    json!({
        "crate_number": format!("{:?}", def.krate),
        "crate_name": tcx.crate_name(def.krate).to_string(),
        "stable_crate_id": format!("{:?}", tcx.stable_crate_id(def.krate)),
        "def_id": format!("{:?}", def), "def_path": tcx.def_path_str(def),
        "is_local": def.is_local()
    })
}

fn mir_stats(body: &rustc_middle::mir::Body<'_>) -> Value {
    json!({"basic_blocks":body.basic_blocks.len(),"locals":body.local_decls.len(),"statements":body.basic_blocks.iter().map(|bb|bb.statements.len()).sum::<usize>()})
}

fn compiler_log(rustc: &str, args: &[String]) {
    let Ok(path) = env::var("DEP1_P0B_RUSTC_LOG") else { return };
    let crate_name = args.windows(2).find(|w| w[0] == "--crate-name").map(|w| w[1].clone()).unwrap_or_default();
    let row = json!({"rustc":rustc,"crate_name":crate_name,"argv":args,"always_encode_mir":args.iter().any(|a|a=="-Zalways-encode-mir=yes")});
    let lock=format!("{path}.lock");
    let mut guard=None;
    for _ in 0..2000 {
        if let Ok(f)=OpenOptions::new().write(true).create_new(true).open(&lock) { guard=Some(f); break; }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    if guard.is_none() { eprintln!("failed to acquire compiler invocation log lock: {lock}"); return; }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) { let _=writeln!(f,"{}",row); }
    drop(guard);
    let _=fs::remove_file(lock);
}

impl Callbacks for Probe {
    fn after_analysis<'tcx>(&mut self, _compiler: &rustc_interface::interface::Compiler, queries: &'tcx Queries<'tcx>) -> rustc_driver::Compilation {
        let result = queries.global_ctxt().unwrap().enter(|tcx| {
            let Some((entry_def, _)) = tcx.entry_fn(()) else { return json!({"error":"selected crate has no entry function"}); };
            let entry_instance = ty::Instance::mono(tcx, entry_def);
            let mut queue = std::collections::VecDeque::from([entry_instance]);
            let mut visited: Vec<ty::Instance<'tcx>> = Vec::new();
            let mut calls = Vec::<Value>::new();
            let mut bodies = Vec::<Value>::new();
            let typing_env = ty::TypingEnv::fully_monomorphized();
            while let Some(caller_instance) = queue.pop_front() {
                if visited.contains(&caller_instance) { continue; }
                visited.push(caller_instance);
                let caller_def = caller_instance.def_id();
                if !caller_def.is_local() && !matches!(tcx.crate_name(caller_def.krate).as_str(), "dep" | "dep2") { continue; }
                let body = tcx.optimized_mir(caller_def);
                if !caller_def.is_local() {
                    let mut b = identity(tcx, caller_def);
                    b["instance"] = json!(format!("{:?}", caller_instance));
                    b["stats"] = mir_stats(body);
                    b["source_provenance"] = json!(format!("{:?}", tcx.def_span(caller_def)));
                    bodies.push(b);
                }
                for (bb, data) in body.basic_blocks.iter_enumerated() {
                    let Some(term) = data.terminator.as_ref() else { continue };
                    let TerminatorKind::Call { func, .. } = &term.kind else { continue };
                    let fty = func.ty(&body.local_decls, tcx);
                    let TyKind::FnDef(fn_def, generic_args) = fty.kind() else { continue };
                    let resolution = ty::Instance::try_resolve(tcx, typing_env, *fn_def, *generic_args);
                    let (resolved_instance, query_def, resolution_state) = match resolution {
                        Ok(Some(instance)) => (Some(instance), instance.def_id(), "resolved"),
                        Ok(None) => (None, *fn_def, "no_instance"),
                        Err(_) => (None, *fn_def, "resolution_error"),
                    };
                    let mut rec = json!({
                        "caller":identity(tcx,caller_def),
                        "caller_instance":format!("{:?}",caller_instance),
                        "call_basic_block":bb.index(),
                        "call_operand":{"fn_def_id":format!("{:?}",fn_def),"fn_def_path":tcx.def_path_str(*fn_def),"crate_identity":identity(tcx,*fn_def),"generic_args":format!("{:?}",generic_args)},
                        "resolved_instance":resolved_instance.map(|i|json!({"instance":format!("{:?}",i),"def_id":format!("{:?}",i.def_id()),"def_path":tcx.def_path_str(i.def_id()),"crate_identity":identity(tcx,i.def_id())})),
                        "resolution_state":resolution_state,
                        "mir_query":{"def_id":format!("{:?}",query_def),"def_path":tcx.def_path_str(query_def),"crate_identity":identity(tcx,query_def),"is_mir_available":false,"optimized_mir_attempted":false,"body_status":"unavailable","basic_blocks":null,"locals":null,"statements":null,"failure":null},
                        "source_span":format!("{:?}",term.source_info.span)
                    });
                    if let Some(instance) = resolved_instance {
                        let qdef = instance.def_id();
                        if !qdef.is_local() && matches!(tcx.crate_name(qdef.krate).as_str(), "dep" | "dep2") {
                            let available = tcx.is_mir_available(qdef);
                            rec["mir_query"]["is_mir_available"] = json!(available);
                            if available {
                                rec["mir_query"]["optimized_mir_attempted"] = json!(true);
                                let foreign_body = tcx.optimized_mir(qdef);
                                let stats = mir_stats(foreign_body);
                                rec["mir_query"]["body_status"] = json!("available");
                                for k in ["basic_blocks","locals","statements"] { rec["mir_query"][k] = stats[k].clone(); }
                                queue.push_back(instance);
                            }
                        }
                    }
                    calls.push(rec);
                }
            }
            let loaded_crates=tcx.crates(()).iter().map(|&krate|json!({"crate_number":format!("{:?}",krate),"crate_name":tcx.crate_name(krate).to_string(),"stable_crate_id":format!("{:?}",tcx.stable_crate_id(krate))})).collect::<Vec<_>>();
            json!({"entry":identity(tcx,entry_def),"entry_instance":format!("{:?}",entry_instance),"loaded_crates":loaded_crates,"calls":calls,"foreign_bodies":bodies,"visited_instances":visited.iter().map(|i|format!("{:?}",i)).collect::<Vec<_>>()})
        });
        let _ = fs::write(&self.output, serde_json::to_vec_pretty(&result).unwrap());
        rustc_driver::Compilation::Continue
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().collect();
    if argv.len() < 2 { eprintln!("wrapper expects rustc argv"); return ExitCode::from(2); }
    let rustc = &argv[1]; let args = &argv[2..];
    compiler_log(rustc, args);
    if !args.windows(2).any(|w|w[0]=="--crate-name" && w[1]=="dep1_app") {
        return Command::new(rustc).args(args).status().map(|s| ExitCode::from(s.code().unwrap_or(1) as u8)).unwrap_or(ExitCode::FAILURE);
    }
    let output = env::var("DEP1_P0B_OUTPUT").expect("DEP1_P0B_OUTPUT");
    let mut cb = Probe { output };
    let mut compiler_args = vec![rustc.clone()]; compiler_args.extend(args.iter().cloned());
    let code = rustc_driver::catch_with_exit_code(|| RunCompiler::new(&compiler_args, &mut cb).run());
    ExitCode::from(code.clamp(0, 255) as u8)
}
