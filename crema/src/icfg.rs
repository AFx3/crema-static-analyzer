use rustc_driver::Callbacks;
use rustc_interface::Queries;
use rustc_middle::mir::{Place, PlaceElem, Statement, StatementKind, Terminator, TerminatorKind, Operand};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Write, Read};
use serde_json;
use std::error::Error;
use rustc_middle::mir::{Local, LocalDecl, Mutability};
use rustc_index::IndexVec;
use std::fs::read_dir;
use rustc_hir::def::DefKind;

use crate::structs::{MirStatement, MirTerminator, MirBasicBlock, MirRepresentation, SourceInfoData,
    LlvmRepresentation, LlvmFunction, LlvmJson, LlvmEdge, IcfgEdge, DummyNode, GlobalICFGNode, GlobalICFGOrdered, MirCallArgument };
use crate::utils::{unwind_action_to_string,compute_hash, load_ffi_functions};

// NOTE: i'm treating an unwind target terminate as a mir node, from 1.86, the unwind actions are:
/* 
Continue
No action is to be taken. Continue unwinding.

This is similar to Cleanup(bb) where bb does nothing but Resume, but they are not equivalent, as presence of Cleanup(_) will make a frame non-POF.

Unreachable
Triggers undefined behavior if unwind happens.

Terminate(UnwindTerminateReason)
Terminates the execution if unwind happens.

Depending on the platform and situation this may cause a non-unwindable panic or abort.

Cleanup(BasicBlock)
Cleanups to be done.
*/
pub struct MirExtractor {
    pub mir_representation: MirRepresentation,
    pub llvm_representation: Option<LlvmRepresentation>, // store parsed LLVM IR
}

impl MirExtractor {
    
    pub fn new() -> Self {
        MirExtractor {
            mir_representation: MirRepresentation { functions: HashMap::new() },
            llvm_representation: None,
        }
    }
    
    // NOTE: now pass the local declarations so that we can check a place’s mutability, store this info also in mir terminator's call arguments
    pub fn convert_statement(&self, statement: &Statement<'_>, local_decls: &IndexVec<Local, LocalDecl>) -> MirStatement {
        let source_info = statement.source_info.clone();

        let source_info_data = SourceInfoData {
            span: format!("{:?}", source_info.span),
            scope: format!("{:?}", source_info.scope),
        };

        let statement_kind = match &statement.kind {
            StatementKind::Assign(..) => "Assign",
            StatementKind::FakeRead(..) => "FakeRead",
            StatementKind::SetDiscriminant { .. } => "SetDiscriminant",
            StatementKind::Deinit(..) => "Deinit",
            StatementKind::StorageLive(..) => "StorageLive",
            StatementKind::StorageDead(..) => "StorageDead",
            StatementKind::Retag(..) => "Retag",
            StatementKind::PlaceMention(..) => "PlaceMention",
            StatementKind::AscribeUserType(..) => "AscribeUserType",
            StatementKind::Coverage(..) => "Coverage",
            StatementKind::Intrinsic(..) => "Intrinsic",
            StatementKind::ConstEvalCounter => "ConstEvalCounter",
            StatementKind::Nop => "Nop",
            StatementKind::BackwardIncompatibleDropHint { .. } => "BackwardIncompatibleDropHint",
        };

        // defaults for when a place isn’t present:
        let mut place_info: Option<String> = None;
        let mut rvalue: Option<String> = None;
        let mut is_mutable: Option<bool> = None;

        if let StatementKind::Assign(box (lhs, rhs)) = &statement.kind {
            let (place_desc, mutable_flag) = self.describe_place(lhs, local_decls);
            place_info = Some(place_desc);
            rvalue = Some(format!("{:?}", rhs));
            is_mutable = Some(mutable_flag);
        }

        MirStatement {
            source_info: source_info_data,
            kind: statement_kind.to_string(),
            details: format!("{:?}", statement.kind),
            place: place_info,
            rvalue,
            is_mutable,
        }
    }

    // modify the helper to also check the local’s mutability.
    // returns a tuple: (description string, is_mutable flag).
    fn describe_place(&self, place: &Place<'_>, local_decls: &IndexVec<Local, LocalDecl>) -> (String, bool) {
        // start by describing the local (e.g. Local(3))
        let mut description = format!("Local({:?})", place.local);
        // look up the local declaration and determine its mutability
        let is_mut = local_decls.get(place.local)
            .map(|local_decl| local_decl.mutability == Mutability::Mut)
            .unwrap_or(false);
        if is_mut {
            description.push_str(" [mutable]");
        }
        // continue describing the projections
        for elem in place.projection {
            description.push_str(" -> ");
            match elem {
                PlaceElem::Deref => description.push_str("*"),
                PlaceElem::Field(field_idx, ty) => {
                    description.push_str(&format!("Field({:?}, Type: {:?})", field_idx, ty))
                }
                PlaceElem::Index(local) => {
                    description.push_str(&format!("Index({:?})", local))
                }
                PlaceElem::ConstantIndex { offset, from_end, .. } => {
                    description.push_str(&format!("ConstantIndex(offset: {}, from_end: {})", offset, from_end))
                }
                PlaceElem::Subslice { from, to, from_end } => {
                    description.push_str(&format!(
                        "Subslice(from: {}, to: {}, from_end: {})",
                        from, to, from_end
                    ));
                }
                PlaceElem::Downcast(name, variant_idx) => {
                    description.push_str(&format!("Downcast(name: {:?}, variant: {:?})", name, variant_idx))
                }
                PlaceElem::OpaqueCast(ty) => {
                    description.push_str(&format!("OpaqueCast(Type: {:?})", ty));
                }
                PlaceElem::Subtype(ty) => {
                    description.push_str(&format!("Subtype(Type: {:?})", ty));
                }
        
            }
        }
        (description, is_mut)
    }

    // pass the local declarations here as well so that any place conversions can include mutability
    pub fn convert_terminator(&self, terminator: &Option<Terminator<'_>>,local_decls: &IndexVec<Local, LocalDecl>) -> Option<MirTerminator> {
        terminator.as_ref().map(|t| match &t.kind {
            TerminatorKind::Goto { target } => MirTerminator::Goto {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
                target: format!("{:?}", target),
            },
            TerminatorKind::SwitchInt { discr, targets, .. } => {
                let discr_str = format!("{:?}", discr);
                let target_blocks = targets
                    .iter()
                    .map(|(val, bb)| format!("({}, {:?})", val, bb))
                    .collect::<Vec<String>>();
                let otherwise_block = Some(format!("{:?}", targets.otherwise()));

                MirTerminator::SwitchInt {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    discr: discr_str,
                    targets: target_blocks,
                    otherwise: otherwise_block,
                }
            },
            TerminatorKind::Return => MirTerminator::Return {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::UnwindResume => MirTerminator::UnwindResume {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::Unreachable => MirTerminator::Unreachable {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::Drop { place, target, unwind, .. } => {
                let (dropped_desc, is_mut) = self.describe_place(place, local_decls);
                MirTerminator::Drop {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    return_target: format!("{:?}", target),
                    unwind_target: unwind_action_to_string(unwind),
                    dropped_value: dropped_desc,
                    is_mutable: is_mut,
                }
            },
            // --- Modified Call terminator for args local mut ---
            TerminatorKind::Call { func, args, destination, target, unwind, .. } => {
                // for each arg, extract the inner operand from the spanned wrapper
                let call_arguments: Vec<MirCallArgument> = args.iter().map(|spanned_arg| {
                    let operand = &spanned_arg.node;
                    match operand {
                        // if the operand is a Copy or Move, then extract the place and check its mutability
                        Operand::Copy(place) | Operand::Move(place) => {
                            let (desc, is_mut) = self.describe_place(place, local_decls);
                            MirCallArgument { arg: desc, is_mutable: Some(is_mut) }
                        },
                        // for other kinds of operands (e.g. constants) not record mutability
                        _ => MirCallArgument { arg: format!("{:?}", operand), is_mutable: None },
                    }
                }).collect();
                
                MirTerminator::Call {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    function_called: format!("{:?}", func),
                    arguments: call_arguments,
                    return_place: format!("{:?}", destination),
                    return_target: target.map(|t| format!("{:?}", t)),
                    unwind_target: unwind_action_to_string(unwind),
                }
            },
            TerminatorKind::Assert { cond, expected, msg, target, unwind } => {
                let cond_str = format!("{:?}", cond);
                let msg_str = format!("{:?}", msg);

                MirTerminator::Assert {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    return_target: format!("{:?}", target),
                    unwind_target: unwind_action_to_string(unwind),
                    cond: cond_str,
                    expected: *expected,
                    msg: msg_str,
                }
            },
            TerminatorKind::InlineAsm { template, operands, options, line_spans, unwind, .. } => {
                MirTerminator::InlineAsm {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    template: template.iter().map(|s| s.to_string()).collect(),
                    operands: operands.iter().map(|op| format!("{:?}", op)).collect(),
                    options: format!("{:?}", options),
                    line_spans: line_spans.iter().map(|span| format!("{:?}", span)).collect(),
                    unwind_target: Some(unwind_action_to_string(unwind)),
                }
            },
            // catch-all for unhandled variants
            _ => MirTerminator::Unhandled {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
        })
    }
}




impl Callbacks for MirExtractor {fn after_analysis<'tcx>(&mut self, _compiler: &rustc_interface::interface::Compiler, queries: &'tcx Queries<'tcx>) -> rustc_driver::Compilation {
    // helpers fn per costruire ID univoci
    fn get_dummy_call_id(rust_func: &str, bb: usize, call_suffix: &str, internal: bool) -> String {
        if internal {
            format!("dummyCall::rust::{}::bb{}::{}_internal", rust_func, bb, call_suffix)
        } else {
            format!("dummyCall::rust::{}::bb{}::{}", rust_func, bb, call_suffix)
        }
    }
    fn get_dummy_ret_id(rust_func: &str, ret_target: &str, call_suffix: &str, internal: bool) -> String {
        if internal {
            format!("dummyRet::rust::{}::{}::{}_internal", rust_func, ret_target, call_suffix)
        } else {
            format!("dummyRet::rust::{}::{}::{}", rust_func, ret_target, call_suffix)
        }
    }
    // hashmap per memorizzare, per ciascuna  FFI, tutti i call suffix (cioè le chiamate)
    let mut ffi_call_sites: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    queries.global_ctxt().unwrap().enter(|tcx| {
        // --- 1. costruisco la MIR per ogni funzione ---
        for def_id in tcx.hir().body_owners() {
            let _function_name = tcx.def_path_str(def_id.to_def_id());

            ///// QUESTO CI STO ANCORA LAVORANDO, E' IL CASO DELLA SPAZZATURA NELLA MIR...
            if !def_id.to_def_id().is_local() {
                continue;
            }
            let function_name = tcx.def_path_str(def_id);
            // filter
            if function_name.contains("::RE") {
                            println!("Skipping compiler-generated function: {}", function_name);
                                continue;
                            }
                          

             // 1) skip mir opitimized for constants
            match tcx.def_kind(def_id) {
                DefKind::Fn | DefKind::AssocFn | DefKind::Closure => { 
                     // ok, process
                     }
                _ => continue,
            }
         
            //println!("Processing function: {}", function_name);
            let body = tcx.optimized_mir(def_id.to_def_id());
            let mut function_blocks = Vec::new();
            for (bb, data) in body.basic_blocks.iter_enumerated() {
                let mir_basic_block = MirBasicBlock {
                    block_id: bb.index(),
                    statements: data.statements.iter().map(|stmt| self.convert_statement(stmt, &body.local_decls)).collect(),
                    terminator: self.convert_terminator(&data.terminator, &body.local_decls),
                };
                function_blocks.push(mir_basic_block);
            }
            self.mir_representation.functions.insert(function_name, function_blocks);
        }
        // --- 2. LOAD LLVM IR (svf) da JSON ---
        let llvm_dir = "../SVF-example/output/";
        match load_all_llvm_json(llvm_dir) {
            Ok(parsed) => self.llvm_representation = Some(parsed),
            Err(e) => eprintln!("Failed to parse LLVM JSON files: {:?}", e),
        }
        // --- 3. LOAD FFI functions da JSON ---
        let ffi_functions = match load_ffi_functions("./ffi_functions.json") {
            Ok(set) => set,
            Err(e) => {
                eprintln!("Failed to load FFI functions: {:?}", e);
                std::collections::HashSet::new()
            }
        };
        // --- 4. filters LLVM functions to reatins only the one marked as ffi ---
        if let Some(ref mut llvm_repr) = self.llvm_representation {
            llvm_repr.functions.retain(|func_name, _| ffi_functions.contains(func_name));
        }
        // --- 5. BUILD ICFG EDGES  ---
        let mut icfg_edges = Vec::new();
        for (rust_func, blocks) in &self.mir_representation.functions {
            for block in blocks {
                if let Some(MirTerminator::Call {
                    function_called,
                    return_target,
                    unwind_target,
                    arguments,
                    return_place,
                    ..
                }) = &block.terminator {
                    // definition of del call suffix as MIR caller node (es. "rust::main::bb2")
                    let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                    // if function called is FFI, registr call suffix for the function

                    if ffi_functions.contains(function_called) {
                        ffi_call_sites.entry(function_called.clone()).or_default().push(call_suffix.clone());
                        let mir_arg = arguments.first().map(|arg| arg.arg.clone());
                        let rust_call_site = format!("rust::{}::bb{}", rust_func, block.block_id);
                        let rust_return_node = if let Some(rt) = return_target {
                            format!("rust::{}::{}", rust_func, rt)
                        } else {
                            format!("rust::{}::end", rust_func)
                        };
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                if !llvm_func.nodes.is_empty() {
                                    let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &call_suffix, false);
                                    if let Some(entry_node) = llvm_func.nodes.first() {
                                        // set call_suffix to rename the entry node
                                        let llvm_entry = format!("llvm::{}::node{}::{}", function_called, entry_node.node_id, call_suffix);
                                        // MODIFIED: concatate  call_suffix to original var name
                                        let _llvm_var = entry_node.svf_statements.first().and_then(|stmt| {
                                            stmt.rhs_var_id.map(|id| format!("{}@{}", id, call_suffix))
                                        });
                                        icfg_edges.push(IcfgEdge {
                                            source: rust_call_site.clone(),
                                            destination: dummy_call_id.clone(),
                                            label: Some("FFI Call".to_string()),
                                            source_label: None,
                                            destination_label: mir_arg.clone(),
                                        });
                                        icfg_edges.push(IcfgEdge {
                                            source: dummy_call_id.clone(),
                                            destination: llvm_entry.clone(),
                                            label: Some("dummyCall->LLVM Entry".to_string()),
                                            source_label: mir_arg.clone(),
                                            destination_label: None,
                                        });
                                        if let Some(exit_node) = llvm_func.nodes.iter().find(|node| {
                                            node.node_kind_string == "FunExitBlock"
                                                && node.outgoing_edges.is_empty()
                                        }) {
                                            let llvm_exit = format!("llvm::{}::node{}::{}", function_called, exit_node.node_id, call_suffix);
                                            let ret_target = return_target.as_ref().unwrap_or(&"end".to_string()).clone();
                                            let dummy_ret_id = get_dummy_ret_id(rust_func, &ret_target, &call_suffix, false);
                                            icfg_edges.push(IcfgEdge {
                                                source: llvm_exit.clone(),
                                                destination: dummy_ret_id.clone(),
                                                label: Some("LLVM Exit->dummyRet".to_string()),
                                                source_label: Some(exit_node.basic_block_info.clone().unwrap_or_else(|| "llvm_param".to_string())),
                                                destination_label: None,
                                            });
                                            icfg_edges.push(IcfgEdge {
                                                source: dummy_ret_id.clone(),
                                                destination: rust_return_node.clone(),
                                                label: Some("dummyRet->MIR Return".to_string()),
                                                source_label: Some(exit_node.basic_block_info.clone().unwrap_or_else(|| "llvm_param".to_string())),
                                                destination_label: None,
                                            });
                                        }
                                    }
                                } else {
                                    icfg_edges.push(IcfgEdge {
                                        source: rust_call_site.clone(),
                                        destination: rust_return_node.clone(),
                                        label: Some("FFI Call (stdlib)".to_string()),
                                        source_label: Some(format!("Mir bb{}", block.block_id)),
                                        destination_label: None,
                                    });
                                }
                            } else {
                                icfg_edges.push(IcfgEdge {
                                    source: rust_call_site.clone(),
                                    destination: rust_return_node.clone(),
                                    label: Some("FFI Call (stdlib)".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                        } else {
                            icfg_edges.push(IcfgEdge {
                                source: rust_call_site.clone(),
                                destination: rust_return_node.clone(),
                                label: Some("FFI Call (no LLVM info)".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }
                        let effective_unwind = extract_target(unwind_target);
                        if effective_unwind != "unreachable" && effective_unwind != "continue" {
                            let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                            icfg_edges.push(IcfgEdge {
                                source: src,
                                destination: dst,
                                label: Some("Call unwind".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }

                        } else if function_called.contains("{closure") {
                            // --- 1) find closure key  in mir_representation.functions ---
                            let closure_key = self
                                .mir_representation
                                .functions
                                .keys()
                                .find(|k| k.starts_with(&format!("{}::{{closure", rust_func)))
                                .expect(&format!("No found closure `{}` in mir_representation.functions", rust_func))
                                .clone(); // es. "main::{closure#0}"
                        
                            // --- 2) build entry/exit IDs of the closure ---
                            let entry_id = format!("rust::{}::bb0", closure_key);
                            let exit_bb = self.mir_representation.functions[&closure_key]
                                .iter()
                                .find(|b| matches!(b.terminator, Some(MirTerminator::Return { .. })))
                                .unwrap()
                                .block_id;
                            let exit_id = format!("rust::{}::bb{}", closure_key, exit_bb);
                        
                            // --- 3) prepare dummyCall / dummyRet / mir_return ---
                            let call_site     = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let csuffix       = call_site.clone();
                            let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &csuffix, true);
                            let dummy_ret_id  = get_dummy_ret_id(rust_func, &return_place, &csuffix, true);
                            let mir_return    = return_target
                                .as_ref()
                                .map(|rt| format!("rust::{}::{}", rust_func, rt))
                                .unwrap_or_else(|| format!("rust::{}::end", rust_func));
                            let mir_arg       = arguments.first().map(|arg| arg.arg.clone()).unwrap_or_default();
                            let llvm_arg      = return_place.clone();
                        
                            // 3.1) call-site -> dummyCall
                            icfg_edges.push(IcfgEdge {
                                source: call_site.clone(),
                                destination: dummy_call_id.clone(),
                                label: Some("Closure Call (dummy inserted)".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: Some(mir_arg.clone()),
                            });
                            // 3.2) dummyCall -> entry della closure
                            icfg_edges.push(IcfgEdge {
                                source: dummy_call_id.clone(),
                                destination: entry_id.clone(),
                                label: Some("dummyCall->ClosureEntry".to_string()),
                                source_label: None,
                                destination_label: None,
                            });
                            // 3.3) fallback intenral: bb0 -> bb1
                            icfg_edges.push(IcfgEdge {
                                source: entry_id.clone(),
                                destination: format!("rust::{}::bb1", closure_key),
                                label: Some("Call return (fallback)".to_string()),
                                source_label: Some("Mir bb0".to_string()),
                                destination_label: Some("Mir bb1".to_string()),
                            });
                                                
                            // 3.4) closure exit -> dummyRet
                            icfg_edges.push(IcfgEdge {
                                source: exit_id.clone(),
                                destination: dummy_ret_id.clone(),
                                label: Some("ClosureExit->dummyRet".to_string()),
                                source_label: None,
                                destination_label: None,
                            });
                        
                            // 3.5) dummyRet -> caller return
                            icfg_edges.push(IcfgEdge {
                                source: dummy_ret_id.clone(),
                                destination: mir_return.clone(),
                                label: Some("dummyRet->Closure Return".to_string()),
                                source_label: Some(llvm_arg.clone()),
                                destination_label: None,
                            });
                        
                            // 3.6) unwind
                            let eff_unwind = extract_target(unwind_target);
                            if eff_unwind != "unreachable" && eff_unwind != "continue" {
                                icfg_edges.push(IcfgEdge {
                                    source: call_site,
                                    destination: format!("rust::{}::{}", rust_func, eff_unwind),
                                    label: Some("Call unwind".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                             
                                });
                            }

                    } else {
                        // --- rust internal calls (non FFI) – MOD:
                        // in this branch, handle rust internal calls, having is_internal=true,
                        // don't add edges to user defined function, but only edges:
                        // from call site -> dummy call,
                        // from dummy call -> dummy ret,
                        // finally, egde:
                        // from dummy ret -> next block
                        
                        if let Some(_callee_blocks) = self.mir_representation.functions.get(function_called) {
                            let caller_call_site = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let caller_return = if let Some(rt) = return_target {
                                format!("rust::{}::{}", rust_func, rt)
                            } else {
                                format!("rust::{}::end", rust_func)
                            };
                            let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &format!("rust::{}::bb{}", rust_func, block.block_id), true);
                            icfg_edges.push(IcfgEdge {
                                source: caller_call_site.clone(),
                                destination: dummy_call_id.clone(),
                                label: Some("Internal Call (dummy inserted)".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                            let dummy_ret_id = get_dummy_ret_id(rust_func, &block.block_id.to_string(), &format!("rust::{}::bb{}", rust_func, block.block_id), true);
                            // add edge between dummy call and dummy ret
                            icfg_edges.push(IcfgEdge {
                                source: dummy_call_id.clone(),
                                destination: dummy_ret_id.clone(),
                                label: Some("dummyCall->dummyRet".to_string()),
                                source_label: None,
                                destination_label: None,
                            });
                            icfg_edges.push(IcfgEdge {
                                source: dummy_ret_id.clone(),
                                destination: caller_return.clone(),
                                label: Some("dummyRet->Internal Return".to_string()),
                                source_label: None,
                                destination_label: Some(format!("Mir bb{}", return_target.clone().unwrap_or_else(|| "end".to_string()))),
                            });
                            let effective_unwind = extract_target(unwind_target);
                            if effective_unwind != "unreachable" && effective_unwind != "continue" {
                                let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                                let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                                icfg_edges.push(IcfgEdge {
                                    source: src,
                                    destination: dst,
                                    label: Some("Call unwind".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                        } else {
                            if let Some(rt) = return_target {
                                let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                                let dst = format!("rust::{}::{}", rust_func, rt);
                                icfg_edges.push(IcfgEdge {
                                    source: src,
                                    destination: dst,
                                    label: Some("Call return (fallback)".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                            
                            let effective_unwind = extract_target(unwind_target);
                            if effective_unwind != "unreachable" && effective_unwind != "continue" {
                                let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                                let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                                icfg_edges.push(IcfgEdge {
                                    source: src,
                                    destination: dst,
                                    label: Some("Call unwind".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                        }
                    }
                }
                //////////////////////////////////////////////////////////////////////////////////////////////////////
                // GOTO terminator
                if let Some(MirTerminator::Goto { target, details, source_info }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, target);
                    icfg_edges.push(IcfgEdge {
                        source: src,
                        destination: dst,
                        label: Some("Goto".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                }
                ///////////////////////////////////////////////////////////////////////////////////////////////////////
                // handle others terminators (SwitchInt, Assert, Drop) 
                if let Some(MirTerminator::SwitchInt { targets, otherwise, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    for target in targets {
                        let trimmed = target.trim_matches(|c| c == '(' || c == ')');
                        let parts: Vec<&str> = trimmed.split(',').collect();
                        if parts.len() >= 2 {
                            let target_block = parts[1].trim();
                            let dst = format!("rust::{}::{}", rust_func, target_block);
                            icfg_edges.push(IcfgEdge {
                                source: src.clone(),
                                destination: dst,
                                label: Some("SwitchInt target".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }
                    }
                    if let Some(otherwise_target) = otherwise {
                        let dst = format!("rust::{}::{}", rust_func, otherwise_target);
                        icfg_edges.push(IcfgEdge {
                            source: src.clone(),
                            destination: dst,
                            label: Some("SwitchInt otherwise".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
                if let Some(MirTerminator::Assert { return_target, unwind_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let rt = return_target.clone();
                    let dst = format!("rust::{}::{}", rust_func, rt);
                    icfg_edges.push(IcfgEdge {
                        source: src.clone(),
                        destination: dst,
                        label: Some("Assert success".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                    let effective_unwind = extract_target(unwind_target);
                    if effective_unwind != "unreachable" && effective_unwind != "continue" {
                        let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                        icfg_edges.push(IcfgEdge {
                            source: src,
                            destination: dst,
                            label: Some("Assert unwind".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
                if let Some(MirTerminator::Drop { return_target, unwind_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, return_target);
                    icfg_edges.push(IcfgEdge {
                        source: src.clone(),
                        destination: dst,
                        label: Some("Drop return".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                    let effective_unwind = extract_target(unwind_target);
                    if effective_unwind != "unreachable" && effective_unwind != "continue" {
                        let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                        icfg_edges.push(IcfgEdge {
                            source: src,
                            destination: dst,
                            label: Some("Drop unwind".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
            }
        }
        // --- 6. BUILD GLOBAL ICFG ordered NODES ---
        let mut ordered_icfg_nodes: Vec<(String, GlobalICFGNode)> = Vec::new();
        for (rust_func, blocks) in &self.mir_representation.functions {
            let mut sorted_blocks = blocks.clone();
            sorted_blocks.sort_by_key(|b| b.block_id);
            for block in sorted_blocks {
                let mir_node_id = format!("rust::{}::bb{}", rust_func, block.block_id);
                ordered_icfg_nodes.push((mir_node_id.clone(), GlobalICFGNode::Mir(block.clone())));
                if let Some(MirTerminator::Call {
                    function_called,
                    return_target,
                    arguments,
                    return_place,
                    ..
                }) = &block.terminator {
                    if ffi_functions.contains(function_called) {
                        // --- FFI CALL DUMMY NODES ---
                        let mir_arg = arguments.first().map(|arg| arg.arg.clone()).unwrap_or_else(|| "".to_string());
                        // use same call suffix usato in5
                        let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                        let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &call_suffix, false);
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                if let Some(entry_node) = llvm_func.nodes.first() {
                                    let llvm_entry = format!("llvm::{}::node{}::{}", function_called, entry_node.node_id, call_suffix);
                                    // Concateno il call_suffix anche in questo ramo per avere il riferimento univoco
                                    let llvm_var = entry_node.svf_statements.first().and_then(|stmt| {
                                        stmt.rhs_var_id.map(|id| format!("{}@{}", id, call_suffix))
                                    });
                                    ordered_icfg_nodes.push((
                                        dummy_call_id.clone(),
                                        GlobalICFGNode::DummyCall(DummyNode {
                                            dummy_node_name: "dummyCall".to_string(),
                                            incoming_edge: mir_node_id.clone(),
                                            outgoing_edge: llvm_entry.clone(),
                                            id: compute_hash(&(mir_node_id.clone(), llvm_entry.clone())),
                                            mir_var: Some(mir_arg.clone()),
                                            llvm_var,
                                            is_internal: Some(false),
                                        }),
                                    ));
                                }
                            }
                        }
                        // replicate llvm nodes to each FFI call: each call has its body copy
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                                for llvm_node in &llvm_func.nodes {
                                    let new_node_id = format!("llvm::{}::node{}::{}", function_called, llvm_node.node_id, call_suffix);
                                    ordered_icfg_nodes.push((new_node_id, GlobalICFGNode::Llvm(llvm_node.clone())));
                                }
                            }
                        }
                        let rust_return_node = if let Some(ret) = return_target {
                            format!("rust::{}::{}", rust_func, ret)
                        } else {
                            format!("rust::{}::end", rust_func)
                        };
                        let ret_target = return_target.as_ref().unwrap_or(&"end".to_string()).clone();
                        let dummy_ret_id = get_dummy_ret_id(rust_func, &ret_target, &format!("rust::{}::bb{}", rust_func, block.block_id), false);
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                if let Some(exit_node) = llvm_func.nodes.iter().find(|node| {
                                    node.node_kind_string == "FunExitBlock"
                                        && node.outgoing_edges.is_empty()
                                }) {
                                    let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                                    let llvm_exit = format!("llvm::{}::node{}::{}", function_called, exit_node.node_id, call_suffix);
                                    ordered_icfg_nodes.push((
                                        dummy_ret_id.clone(),
                                        GlobalICFGNode::DummyRet(DummyNode {
                                            dummy_node_name: "dummyRet".to_string(),
                                            incoming_edge: llvm_exit.clone(),
                                            outgoing_edge: rust_return_node.clone(),
                                            id: compute_hash(&(rust_return_node.clone(), llvm_exit.clone())),
                                            mir_var: None,
                                            llvm_var: exit_node.basic_block_info.clone().or_else(|| Some("llvm_param".to_string())),
                                            is_internal: Some(false),
                                        }),
                                    ));
                                }
                            }
                        }
///////////////////////////////////////////////////////////////////////////// CLOSURES
// sort of inlining like llvm ffi calls with dummy call and return
// this because ech time an arg is passed to a closure I get a closure with and id
                     } else if function_called.contains("{closure") {
                            // --- CLOSURE: add DummyCall e DummyRet nodes ---
                            let csuffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let mir_arg = arguments.first().map(|arg| arg.arg.clone()).unwrap_or_default();
                            let llvm_arg = return_place.clone();
                            let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &csuffix, true);
                            let dummy_ret_id = get_dummy_ret_id(rust_func, &llvm_arg, &csuffix, true);
                            let mir_ret = return_target.as_ref()
                                .map(|rt| format!("rust::{}::{}", rust_func, rt))
                                .unwrap_or_else(|| format!("rust::{}::end", rust_func));
                            // DummyCall node
                            ordered_icfg_nodes.push((
                                dummy_call_id.clone(),
                                GlobalICFGNode::DummyCall(DummyNode {
                                    dummy_node_name: "dummyCall".to_string(),
                                    incoming_edge: mir_node_id.clone(),
                                    outgoing_edge: dummy_ret_id.clone(),
                                    id: compute_hash(&(mir_node_id.clone(), dummy_ret_id.clone())),
                                    mir_var: Some(mir_arg.clone()),
                                    llvm_var: Some(llvm_arg.clone()),
                                    is_internal: Some(true),
                                }),
                            ));
                            // DummyRet node
                            ordered_icfg_nodes.push((
                                dummy_ret_id.clone(),
                                GlobalICFGNode::DummyRet(DummyNode {
                                    dummy_node_name: "dummyRet".to_string(),
                                    incoming_edge: dummy_call_id.clone(),
                                    outgoing_edge: mir_ret.clone(),
                                    id: compute_hash(&(mir_ret.clone(), dummy_call_id.clone())),
                                    //mir_var: Some(return_place.clone()),
                                    mir_var: None,
                                    //llvm_var: Some(llvm_arg.clone()),
                                    llvm_var: None,
                                    is_internal: Some(true),
                                }), // in tanto metto none 
                            ));


                    } else {
                        // --- INTERNAL CALL DUMMY NODES – MOD:
                        // for intranal function calls, create only dummy call and dummy ret
                        // an edge between them (interprocedural handled in the fixedpoint )
                        if let Some(_callee_blocks) = self.mir_representation.functions.get(function_called) {
                            let caller_call_site = mir_node_id.clone();
                            let dummy_call_mir_var = arguments.first().map(|arg| arg.arg.clone()).unwrap_or_else(|| "".to_string());
                            let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &format!("rust::{}::bb{}", rust_func, block.block_id), true);
                            let dummy_ret_id = get_dummy_ret_id(rust_func, &block.block_id.to_string(), &format!("rust::{}::bb{}", rust_func, block.block_id), true);
                            ordered_icfg_nodes.push((
                                dummy_call_id.clone(),
                                GlobalICFGNode::DummyCall(DummyNode {
                                    dummy_node_name: "dummyCall".to_string(),
                                    incoming_edge: caller_call_site.clone(),
                                    // the exit is the dummy ret
                                    outgoing_edge: dummy_ret_id.clone(),
                                    id: compute_hash(&(caller_call_site.clone(), dummy_ret_id.clone())),
                                    mir_var: Some(dummy_call_mir_var),
                                    llvm_var: None,
                                    is_internal: Some(true),
                                }),
                            ));
                            let dummy_ret_mir_var = return_place.clone();
                            let caller_return = if let Some(rt) = return_target {
                                format!("rust::{}::{}", rust_func, rt)
                            } else {
                                format!("rust::{}::end", rust_func)
                            };
                            ordered_icfg_nodes.push((
                                dummy_ret_id.clone(),
                                GlobalICFGNode::DummyRet(DummyNode {
                                    dummy_node_name: "dummyRet".to_string(),
                                    // the entry is the dummy_call_id
                                    incoming_edge: dummy_call_id.clone(),
                                    outgoing_edge: caller_return.clone(),
                                    id: compute_hash(&(caller_return.clone(), dummy_ret_id.clone())),
                                    mir_var: Some(dummy_ret_mir_var),
                                    llvm_var: Some("Local _0".to_string()),
                                    is_internal: Some(true),
                                }),
                            ));
                        }
                    }
                }
            }
        }
        // --- 7. REPLICATE global LLVM edges for ech FFI call ---
        let mut final_edges = icfg_edges.clone();
        let ordered_ids: std::collections::HashSet<String> =
            ordered_icfg_nodes.iter().map(|(id, _)| id.clone()).collect();
        if let Some(llvm_repr) = &self.llvm_representation {
            // for each replicated ffi, use the registered call suffixes
            for (func_name, call_sites) in &ffi_call_sites {
                // FILTER global edges related to the current function
                for edge in &llvm_repr.global_edges {
                    // edge.source and edge.destination are the original IDs (without call_suffix)
                    for call_suffix in call_sites {
                        let src_str = format!("llvm::{}::node{}::{}", func_name, edge.source, call_suffix);
                        let dst_str = format!("llvm::{}::node{}::{}", func_name, edge.destination, call_suffix);
                        if ordered_ids.contains(&src_str) && ordered_ids.contains(&dst_str) {
                            if !final_edges.iter().any(|e| e.source == src_str && e.destination == dst_str) {
                                final_edges.push(IcfgEdge {
                                    source: src_str,
                                    destination: dst_str,
                                    label: Some("LLVM".to_string()),
                                    source_label: None,
                                    destination_label: None,
                                });
                            }
                        }
                    }
                }
            }
        }
        let mut node_label_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (node_id, node) in &ordered_icfg_nodes {
            let label = match node {
                GlobalICFGNode::Mir(mir) => format!("Mir bb{}", mir.block_id),
                GlobalICFGNode::Llvm(llvm) => llvm.info.clone(),
                GlobalICFGNode::DummyCall(dummy) => format!("{} (id: {})", dummy.dummy_node_name, dummy.id),
                GlobalICFGNode::DummyRet(dummy) => format!("{} (id: {})", dummy.dummy_node_name, dummy.id),
            };
            node_label_map.insert(node_id.clone(), label);
        }
        let updated_edges: Vec<IcfgEdge> = final_edges.iter().map(|edge| {
            let source_label = node_label_map.get(&edge.source).cloned();
            let destination_label = node_label_map.get(&edge.destination).cloned();
            IcfgEdge {
                source: edge.source.clone(),
                destination: edge.destination.clone(),
                label: edge.label.clone(),
                source_label,
                destination_label,
            }
        }).collect();
        let global_icfg_ordered = GlobalICFGOrdered {
            ordered_nodes: ordered_icfg_nodes,
            icfg_edges: updated_edges,
        };
        let output_filename = "global_icfg.json";
        let mut file = File::create(output_filename)
            .expect("Failed to create output file");
        let json_output =
            serde_json::to_string_pretty(&global_icfg_ordered).expect("Failed to serialize JSON");
        file.write_all(json_output.as_bytes())
            .expect("Failed to write JSON output");
        println!("Global ICFG saved to {}", output_filename);
    });
    rustc_driver::Compilation::Stop
}
}



// ---------------------------------------------------------------------
// Helper functions 
// ---------------------------------------------------------------------
// extracts a clean target from an unwind_target string
fn extract_target<'a>(target: &'a str) -> &'a str {
    if target.starts_with("cleanup(") && target.ends_with(")") {
        &target["cleanup(".len()..target.len() - 1]
    } else {
        target
    }
}

pub fn load_all_llvm_json(dir: &str) -> Result<LlvmRepresentation, Box<dyn Error>> {
    let mut combined_functions: HashMap<String, LlvmFunction> = HashMap::new();
    let mut combined_global_edges: Vec<LlvmEdge> = Vec::new();
    
    for entry in read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
                if fname.ends_with("_A_FINAL_ICFG.json") {
                    println!("Loading LLVM JSON file: {}", fname);
                    let file_path = path.to_str().unwrap();
                    let representation = parse_llvm_json(file_path)?;
                    for (func_name, llvm_function) in representation.functions {
                        combined_functions
                            .entry(func_name.clone())
                            .and_modify(|existing_function| {
                                existing_function.nodes.extend(llvm_function.nodes.clone());
                            })
                            .or_insert(llvm_function);
                    }
                    combined_global_edges.extend(representation.global_edges);
                }
            }
        }
    }
    
    combined_global_edges.sort_by(|a, b| {
        a.source.cmp(&b.source)
            .then(a.destination.cmp(&b.destination))
            .then(a.edge_type.cmp(&b.edge_type))
    });
    combined_global_edges.dedup();
    
    Ok(LlvmRepresentation {
        functions: combined_functions,
        global_edges: combined_global_edges,
    })
}

pub fn parse_llvm_json(file_path: &str) -> Result<LlvmRepresentation, Box<dyn Error>> {
    let mut file = File::open(file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let llvm_json: LlvmJson = serde_json::from_str(&contents)?;
    
    let mut functions: HashMap<String, LlvmFunction> = HashMap::new();
    
    for node in llvm_json.nodes.into_iter() {
        if let Some(func_name) = node.function_name.clone() {
            functions
                .entry(func_name.clone())
                .and_modify(|func| {
                    func.nodes.push(node.clone());
                })
                .or_insert_with(|| LlvmFunction {
                    function_name: func_name,
                    nodes: vec![node],
                });
        }
    }
    
    let mut global_edges = llvm_json.edges;
    global_edges.sort_by(|a, b| a.source.cmp(&b.source).then(a.destination.cmp(&b.destination)));
    global_edges.dedup();
    
    Ok(LlvmRepresentation { functions, global_edges })
}
