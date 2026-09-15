#![feature(rustc_private)]
#![feature(box_patterns)]

extern crate cargo_metadata;

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_hir;
extern crate rustc_span;
extern crate rustc_index;


mod structs;         
mod utils;           
mod icfg;            
mod dumpdot;         
mod abstract_domain; 
mod cqpl_export;
mod identity;
mod memory_events;
mod cargo_project;
mod semantic_coverage;
mod mir_semantics;

use cargo_metadata::{MetadataCommand, Target};
use icfg::MirExtractor;
use rustc_driver::RunCompiler;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, exit};
use serde_json;
use utils::dump_dot_from_global_icfg;
use crate::structs::GlobalICFGOrdered;
use crate::abstract_domain::{fixed_point_analysis, detect_mem_issues};
use std::path::PathBuf;
use crate::abstract_domain::set_entrypoint;
use crate::cqpl_export::{export_cqpl_annotated_icfg, export_cqpl_annotated_icfg_with_identity_and_disposition};
use crate::identity::{fixed_point_disposition_identity_analysis, fixed_point_identity_analysis};
use crate::cargo_project::{
    AnalysisMode, CargoBuildInvocation, CargoCliConfig, CargoTargetKind,
    discover_analysis_plan, isolated_cargo_target_dir, normalize_local_def_path_request, run_selected_target_with_cargo,
    wrapper_selected_source_matches,
};
use crate::semantic_coverage::{collect_root_coverage, SemanticCoverageBundle};
use crate::mir_semantics::set_mir_semantics_v2_enabled;
use serde_json::json;

static GLOBAL_ICFG_JSON: &str = "global_icfg.json";
static CQPL_ANNOTATED_ICFG_JSON: &str = "cqpl_annotated_icfg.json";

/// Resolve a CLI entry specification to one canonical ICFG node.
///
/// Scientific boundary: resolution is deterministic and ambiguity is an error.
/// We never choose the first HashMap/substring match.  Exact node ids and exact
/// MIR def-paths win; an unqualified suffix such as `foo` is accepted only when
/// it denotes one Rust function uniquely.
fn resolve_entrypoint(icfg: &GlobalICFGOrdered, requested: &str) -> Result<String, String> {
    if icfg.ordered_nodes.iter().any(|(id, _)| id == requested) {
        return Ok(requested.to_string());
    }
    if let Some(function) = icfg.rust_functions.get(requested) {
        return Ok(function.entry_node.clone());
    }

    let normalized = requested
        .strip_prefix("rust::")
        .unwrap_or(requested)
        .trim_end_matches("::bb0");
    let mut candidates: Vec<_> = icfg
        .rust_functions
        .iter()
        .filter(|(name, _)| {
            name.as_str() == normalized
                || name.ends_with(&format!("::{normalized}"))
        })
        .map(|(name, meta)| (name.clone(), meta.entry_node.clone()))
        .collect();
    candidates.sort();
    candidates.dedup();

    match candidates.as_slice() {
        [(_, entry)] => Ok(entry.clone()),
        [] => Err(format!("no Rust function/node matches '{requested}'")),
        many => Err(format!(
            "ambiguous entry '{requested}'; candidates: {}",
            many.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn main() {
    if env::var_os("CREMA_V6O_RUSTC_WRAPPER_MODE").is_some() {
        exit(run_v6o_rustc_wrapper());
    }

    // --- step 0: process cmd args ---
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Usage: cargo run -- /path/to/cargo/project [-f <entry>|--entry <entry>] \
[--only-icfg-annotated] [--annotated-icfg-out <path>] \
[--allocation-identity-out <path>] [--cqpl-schema-version <1|2>] [--cargo-target <name|package:name>] \
[--analysis-mode <application|library|workspace>] [-p|--package <name>] \
[--cargo-kind <bin|lib|example|test|bench>] [--api-root <def-path>]... \
[--features <a,b>] [--all-features] [--no-default-features] [--target-triple <triple>] \
[--analysis-out-dir <dir>] [--cargo-plan-out <path>] [--semantic-coverage-out <path>] [--mir-semantics-v2]"
        );
        exit(1);
    }
    let project_path = PathBuf::from(&args[1]);

    // CLI options. Default execution remains the legacy CREMA pipeline.
    // --only-icfg-annotated still computes the same ICFG and fixed point,
    // exports the CQPL boundary artifact, then returns before detect_mem_issues.
    let mut entry_override: Option<String> = None;
    let mut only_icfg_annotated = false;
    let mut annotated_icfg_out = PathBuf::from(CQPL_ANNOTATED_ICFG_JSON);
    let mut allocation_identity_out: Option<PathBuf> = None;
    let mut cqpl_schema_version: u32 = 1;
    let mut cargo_target_override: Option<String> = None;
    // v6O opt-in Cargo/crate generalization.  Leaving --analysis-mode absent
    // preserves the frozen v6N execution path byte-for-byte at the CLI level.
    let mut analysis_mode: Option<AnalysisMode> = None;
    let mut package_override: Option<String> = None;
    let mut cargo_kind: Option<CargoTargetKind> = None;
    let mut api_roots: Vec<String> = Vec::new();
    let mut features: Vec<String> = Vec::new();
    let mut all_features = false;
    let mut no_default_features = false;
    let mut target_triple: Option<String> = None;
    let mut analysis_out_dir: Option<PathBuf> = None;
    let mut cargo_plan_out: Option<PathBuf> = None;
    let mut semantic_coverage_out: Option<PathBuf> = None;
    // v6P opt-in extension profile. The frozen v6O/v6N semantics remain the default.
    let mut mir_semantics_v2 = false;
    let mut idx = 2;
    while idx < args.len() {
        match args[idx].as_str() {
            "-f" | "--entry" => {
                if idx + 1 >= args.len() {
                    eprintln!("Missing value after {}", args[idx]);
                    exit(1);
                }
                entry_override = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--only-icfg-annotated" => {
                only_icfg_annotated = true;
                idx += 1;
            }
            "--annotated-icfg-out" => {
                if idx + 1 >= args.len() {
                    eprintln!("Missing path after --annotated-icfg-out");
                    exit(1);
                }
                annotated_icfg_out = PathBuf::from(&args[idx + 1]);
                idx += 2;
            }
            "--allocation-identity-out" => {
                if idx + 1 >= args.len() {
                    eprintln!("Missing path after --allocation-identity-out");
                    exit(1);
                }
                allocation_identity_out = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--cqpl-schema-version" => {
                if idx + 1 >= args.len() {
                    eprintln!("Missing value after --cqpl-schema-version");
                    exit(1);
                }
                cqpl_schema_version = args[idx + 1].parse::<u32>().unwrap_or_else(|_| {
                    eprintln!("Invalid --cqpl-schema-version {}; expected 1 or 2", args[idx + 1]);
                    exit(1);
                });
                if !matches!(cqpl_schema_version, 1 | 2) {
                    eprintln!("Invalid --cqpl-schema-version {}; expected 1 or 2", cqpl_schema_version);
                    exit(1);
                }
                idx += 2;
            }
            "--cargo-target" => {
                if idx + 1 >= args.len() {
                    eprintln!("Missing value after --cargo-target");
                    exit(1);
                }
                cargo_target_override = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--analysis-mode" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --analysis-mode"); exit(1); }
                analysis_mode = Some(AnalysisMode::parse(&args[idx + 1]).unwrap_or_else(|e| { eprintln!("{e}"); exit(1); }));
                idx += 2;
            }
            "-p" | "--package" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after {}", args[idx]); exit(1); }
                package_override = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--cargo-kind" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --cargo-kind"); exit(1); }
                cargo_kind = Some(CargoTargetKind::parse(&args[idx + 1]).unwrap_or_else(|e| { eprintln!("{e}"); exit(1); }));
                idx += 2;
            }
            "--api-root" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --api-root"); exit(1); }
                api_roots.push(args[idx + 1].clone());
                idx += 2;
            }
            "--features" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --features"); exit(1); }
                features.extend(args[idx + 1].split(',').map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned));
                idx += 2;
            }
            "--all-features" => { all_features = true; idx += 1; }
            "--no-default-features" => { no_default_features = true; idx += 1; }
            "--target-triple" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --target-triple"); exit(1); }
                target_triple = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--analysis-out-dir" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --analysis-out-dir"); exit(1); }
                analysis_out_dir = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--cargo-plan-out" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --cargo-plan-out"); exit(1); }
                cargo_plan_out = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--semantic-coverage-out" => {
                if idx + 1 >= args.len() { eprintln!("Missing value after --semantic-coverage-out"); exit(1); }
                semantic_coverage_out = Some(PathBuf::from(&args[idx + 1]));
                idx += 2;
            }
            "--mir-semantics-v2" => {
                mir_semantics_v2 = true;
                idx += 1;
            }
            other => {
                eprintln!("Unknown flag: {}", other);
                exit(1);
            }
        }
    }

    set_mir_semantics_v2_enabled(mir_semantics_v2);
    if mir_semantics_v2 && cqpl_schema_version != 2 {
        eprintln!("--mir-semantics-v2 requires --cqpl-schema-version 2 so structural MIR labels are capability-gated");
        exit(1);
    }
    if mir_semantics_v2 {
        println!("mir_semantics_profile=v2-extension-over-v6O");
    }

    let cargo_toml_path = project_path.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        eprintln!("Error: {} is not a valid Cargo project", project_path.display());
        exit(1);
    }
    println!("Analyzing Cargo project at: {}", project_path.display());

    if analysis_mode.is_some() {
        let cfg = CargoCliConfig {
            mode: analysis_mode,
            package: package_override,
            target_name: cargo_target_override,
            target_kind: cargo_kind,
            entry: entry_override,
            api_roots,
            features,
            all_features,
            no_default_features,
            target_triple,
        };
        if let Err(err) = run_v6o_pipeline(
            &project_path,
            &cfg,
            only_icfg_annotated,
            &annotated_icfg_out,
            allocation_identity_out.as_ref(),
            cqpl_schema_version,
            analysis_out_dir.as_ref(),
            cargo_plan_out.as_ref(),
            semantic_coverage_out.as_ref(),
        ) {
            eprintln!("CREMA v6O analysis failed: {err}");
            exit(1);
        }
        return;
    }

    // --- step 0.1: run FFI xxtraction ---
    let tool_dir = env::current_dir().expect("Failed to get tool directory");
    let ffi_functions_path = tool_dir.join("ffi_functions.json");
    // The FFI summary is process-global in the historical pipeline.  Remove it
    // before extraction and fail closed on extraction failure so a stale file
    // from a previous target can never become an implicit input to this run.
    if ffi_functions_path.exists() {
        fs::remove_file(&ffi_functions_path).unwrap_or_else(|e| {
            panic!("Failed to remove stale FFI summary {}: {e}", ffi_functions_path.display())
        });
    }
    let extraction_status = Command::new("cargo")
    .args(&[
        "run", 
        "--package", 
        "ffi_extraction", 
        "--", 
        project_path.to_str().unwrap(),
        tool_dir.to_str().unwrap() // pass tool's dir as output location
    ])
    .status()
    .expect("Failed to run FFI extraction");
    if !extraction_status.success() {
        eprintln!("FFI extraction failed with status: {:?}", extraction_status);
        exit(1);
    }
    if !ffi_functions_path.is_file() {
        eprintln!(
            "FFI extraction succeeded but did not produce {}",
            ffi_functions_path.display()
        );
        exit(1);
    }


    // --- step 1: discover and process C files in an isolated SVF run dir ---
    let svf_output_dir = prepare_svf_output_dir(&project_path);
    let c_files = find_c_files(&project_path);
    if cqpl_schema_version == 2 && c_files.len() > 1 {
        eprintln!(
            "schema-v2 fail-closed: {} authored C translation units found; \
current SVF integration analyzes one translation unit at a time and therefore \
cannot claim whole-program C/FFI coverage. Files: {}",
            c_files.len(),
            c_files.join(", ")
        );
        exit(1);
    }
    let lib_output = if !c_files.is_empty() {
        compile_c_files(&c_files, &project_path, &svf_output_dir)
    } else {
        println!(
            "No C files found in the project; isolated SVF directory remains empty: {}",
            svf_output_dir.display()
        );
        String::new()
    };
    println!("ffi_extraction finished with status: {:?}", extraction_status);
    // --- step 2: analyze target cargo project (Only Workspace Members) ---
    let instance_entry_hint = entry_override.as_deref().unwrap_or("main");
    analyze_cargo_project(
        &project_path,
        &lib_output,
        &svf_output_dir,
        cargo_target_override.as_deref(),
        instance_entry_hint,
    );


    // --- Step 3: dump and analyze the global ICFG ---
    dump_dot_from_global_icfg(GLOBAL_ICFG_JSON);

    let mut file = File::open(GLOBAL_ICFG_JSON)
        .expect("Failed to open global_icfg.json file");
    let mut json_str = String::new();
    file.read_to_string(&mut json_str)
        .expect("Failed to read global_icfg.json file");

    let global_icfg: GlobalICFGOrdered = serde_json::from_str(&json_str)
        .expect("Failed to deserialize global ICFG");


    // v6K: resolve the user-facing entry specification once and use the exact
    // same node for legacy detection, the abstract fixed point, allocation
    // identity, and CQPL export.  `-f main`, `-f rust::main::bb0`, and an exact
    // def-path are supported; ambiguous suffixes fail closed.
    let requested_entry = entry_override.as_deref().unwrap_or("main");
    let selected_entry = resolve_entrypoint(&global_icfg, requested_entry)
        .unwrap_or_else(|err| {
            eprintln!("Invalid CREMA entry point '{}': {}", requested_entry, err);
            exit(1);
        });
    println!("entrypoint: {}", selected_entry);
    set_entrypoint(selected_entry.clone());

    let (abstract_state, taint_state) = fixed_point_analysis(&global_icfg);

    // Phase 6C: run the canonical, function-scoped allocation-identity fixed
    // point alongside the historical CellValue analysis.  Legacy detection
    // remains byte-for-byte driven by abstract_state/taint_state in this slice.
    let allocation_identity_state =
        fixed_point_identity_analysis(&global_icfg, &selected_entry);
    let allocation_disposition_identity_state =
        fixed_point_disposition_identity_analysis(&global_icfg, &selected_entry);

    if let Some(path) = allocation_identity_out.as_ref() {
        let json = serde_json::to_string_pretty(&allocation_identity_state.to_dump())
            .expect("Failed to serialize allocation identity state");
        fs::write(path, json)
            .unwrap_or_else(|err| panic!("Failed to write allocation identity state {}: {}", path.display(), err));
        println!("Allocation identity state saved to {}", path.display());
    }

    // Read-only side export. In default mode an exporter failure must not
    // suppress CREMA's legacy detector; in --only-icfg-annotated mode the
    // requested artifact is the primary output, so an export failure is fatal.
    let export_result = if cqpl_schema_version == 1 {
        export_cqpl_annotated_icfg(
            &global_icfg,
            &abstract_state,
            &selected_entry,
            &annotated_icfg_out,
        )
    } else {
        export_cqpl_annotated_icfg_with_identity_and_disposition(
            &global_icfg,
            &abstract_state,
            &allocation_identity_state,
            &allocation_disposition_identity_state,
            &selected_entry,
            cqpl_schema_version,
            &annotated_icfg_out,
        )
    };

    if only_icfg_annotated {
        match export_result {
            Ok(()) => {
                println!(
                    "CQPL annotated ICFG saved to {}",
                    annotated_icfg_out.display()
                );
                return;
            }
            Err(err) => {
                eprintln!("Failed to export CQPL annotated ICFG: {}", err);
                exit(1);
            }
        }
    } else if let Err(err) = export_result {
        eprintln!(
            "CREMA warning: CQPL annotated ICFG export failed; \
continuing legacy detection unchanged: {}",
            err
        );
    }

    // Legacy output and detector remain unchanged in default mode.
    println!("Final Abstract State: {:#?}", abstract_state);
    println!("Final Taint State: {:#?}", taint_state);
    detect_mem_issues(&global_icfg, &taint_state, &abstract_state);
}


fn root_slug(root: &str) -> String {
    let mut out: String = root
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while out.contains("__") { out = out.replace("__", "_"); }
    out.trim_matches('_').to_string()
}

fn run_v6o_rustc_wrapper() -> i32 {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("CREMA v6O rustc wrapper: missing real rustc argv[1]");
        return 2;
    }
    let real_rustc = &args[1];
    let rustc_tail = &args[2..];
    let selected_src = match env::var("CREMA_V6O_SELECTED_SRC") {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            eprintln!("CREMA v6O rustc wrapper: missing CREMA_V6O_SELECTED_SRC");
            return 2;
        }
    };

    if !wrapper_selected_source_matches(rustc_tail, &selected_src) {
        return match Command::new(real_rustc).args(rustc_tail).status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(err) => {
                eprintln!("CREMA v6O wrapper failed to forward rustc: {err}");
                2
            }
        };
    }

    let svf_output = env::var("CREMA_V6O_SVF_OUTPUT_DIR").unwrap_or_default();
    let ffi_path = env::var("CREMA_V6O_FFI_FUNCTIONS_PATH").unwrap_or_else(|_| "ffi_functions.json".to_string());
    let icfg_path = env::var("CREMA_V6O_ICFG_OUTPUT_PATH").unwrap_or_else(|_| "global_icfg.json".to_string());
    let entry_hint = env::var("CREMA_V6O_ENTRY_HINT").unwrap_or_else(|_| "main".to_string());

    let mut compiler_args = Vec::with_capacity(args.len() - 1);
    compiler_args.push(real_rustc.clone());
    compiler_args.extend(rustc_tail.iter().cloned());
    let mut callbacks = MirExtractor::new_with_operational_paths(
        svf_output,
        entry_hint,
        ffi_path,
        icfg_path,
        true,
    );
    if let Err(err) = RunCompiler::new(&compiler_args, &mut callbacks).run() {
        eprintln!("CREMA v6O selected-target rustc analysis failed: {err:?}");
        return 2;
    }
    if let Some(boundary) = callbacks.instance_dispatch_boundary.as_deref() {
        eprintln!("{boundary}");
        return 3;
    }
    0
}

fn run_v6o_pipeline(
    project_path: &PathBuf,
    cfg: &CargoCliConfig,
    only_icfg_annotated: bool,
    annotated_icfg_out: &PathBuf,
    allocation_identity_out: Option<&PathBuf>,
    cqpl_schema_version: u32,
    analysis_out_dir: Option<&PathBuf>,
    cargo_plan_out: Option<&PathBuf>,
    semantic_coverage_out: Option<&PathBuf>,
) -> Result<(), String> {
    let plan = discover_analysis_plan(project_path, cfg)?;
    if plan.analysis_roots.len() > 1 && analysis_out_dir.is_none() {
        return Err("multiple library --api-root values require --analysis-out-dir so each root has a distinct artifact".to_string());
    }
    println!(
        "CREMA v6O Cargo plan: mode={:?} package={} {} target={:?}:{} roots={}",
        plan.analysis_mode,
        plan.package_name,
        plan.package_version,
        plan.selected_target.kind,
        plan.selected_target.name,
        plan.analysis_roots.join(",")
    );
    println!("cargo_metadata_format_version=1");
    println!("resolved_features={}", plan.resolved_package_features.join(","));
    println!("resolved_features_source={}", plan.resolved_package_features_source);
    if let Some(triple) = plan.target_triple.as_deref() {
        println!("target_triple={triple}");
    }

    let tool_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let plan_path = cargo_plan_out.cloned().unwrap_or_else(|| tool_dir.join("cargo_analysis_plan.json"));
    plan.write_json(&plan_path)?;
    println!("Cargo analysis plan saved to {}", plan_path.display());

    let package_root = PathBuf::from(&plan.package_root);
    let ffi_functions_path = tool_dir.join("ffi_functions.json");
    if ffi_functions_path.exists() {
        fs::remove_file(&ffi_functions_path)
            .map_err(|e| format!("failed to remove stale {}: {e}", ffi_functions_path.display()))?;
    }

    // v6O-r1a runtime fix: do not run the historical standalone
    // ffi_extraction pre-pass here. That tool reconstructs rustc from
    // target/debug and assumes a root bin, which is incompatible with the
    // Cargo-target-generalization boundary (lib/example/test/workspace,
    // feature selection, and --target). In v6O wrapper mode MirExtractor
    // writes ffi_functions.json directly from the exact selected crate HIR.
    // The legacy non-v6O path above remains byte-for-byte on its historical
    // extractor behavior.
    let svf_output_dir = prepare_svf_output_dir(&package_root);
    let c_files = find_c_files(&package_root);
    if cqpl_schema_version == 2 && c_files.len() > 1 {
        return Err(format!(
            "schema-v2 fail-closed: selected package {} contains {} authored C translation units; current SVF boundary supports one. Files: {}",
            plan.package_name,
            c_files.len(),
            c_files.join(", ")
        ));
    }
    if !c_files.is_empty() {
        let _ = compile_c_files(&c_files, &package_root, &svf_output_dir);
    } else {
        println!("No authored C files found in selected package {}", plan.package_name);
    }

    let exe = env::current_exe().map_err(|e| format!("failed to locate running CREMA executable: {e}"))?;
    let operational_dir = tool_dir
        .join("target")
        .join("crema-v6o-operational")
        .join(format!("{}", std::process::id()));
    fs::create_dir_all(&operational_dir)
        .map_err(|e| format!("failed to create {}: {e}", operational_dir.display()))?;

    let mut coverage_roots = Vec::new();
    let mut artifact_records = Vec::new();
    let project_manifest = project_path.join("Cargo.toml");

    for requested_root in &plan.analysis_roots {
        let normalized_root = normalize_local_def_path_request(
            requested_root,
            &plan.selected_target.name,
        );
        if normalized_root.is_empty() {
            return Err(format!(
                "invalid v6O root '{requested_root}': crate namespace alone does not denote a function"
            ));
        }
        println!(
            "V6O_LOCAL_ROOT_NAMESPACE: requested={} normalized={} crate={}",
            requested_root, normalized_root, plan.selected_target.name
        );
        let slug = root_slug(requested_root);
        let icfg_path = operational_dir.join(format!("global_icfg--{slug}.json"));
        let cargo_target_dir = isolated_cargo_target_dir(&tool_dir, requested_root);
        run_selected_target_with_cargo(
            &plan,
            cfg,
            CargoBuildInvocation {
                project_manifest: &project_manifest,
                tool_executable: &exe,
                tool_dir: &tool_dir,
                svf_output_dir: &svf_output_dir,
                ffi_functions_path: &ffi_functions_path,
                icfg_output_path: &icfg_path,
                semantic_root: &normalized_root,
                cargo_target_dir: &cargo_target_dir,
            },
        )?;

        // Preserve compatibility with existing DOT tooling while reading the
        // root-specific ICFG from its collision-free operational path.
        let staged_icfg = tool_dir.join(GLOBAL_ICFG_JSON);
        fs::copy(&icfg_path, &staged_icfg)
            .map_err(|e| format!("failed to stage global ICFG for DOT export: {e}"))?;
        dump_dot_from_global_icfg(staged_icfg.to_string_lossy().as_ref());
        let json_str = fs::read_to_string(&icfg_path)
            .map_err(|e| format!("failed to read {}: {e}", icfg_path.display()))?;
        let global_icfg: GlobalICFGOrdered = serde_json::from_str(&json_str)
            .map_err(|e| format!("failed to deserialize {}: {e}", icfg_path.display()))?;
        let selected_entry = resolve_entrypoint(&global_icfg, &normalized_root)
            .map_err(|e| format!(
                "invalid v6O root '{requested_root}' (normalized local DefPath '{normalized_root}'): {e}"
            ))?;
        println!("entrypoint: {}", selected_entry);
        set_entrypoint(selected_entry.clone());

        let (abstract_state, taint_state) = fixed_point_analysis(&global_icfg);
        let allocation_identity_state = fixed_point_identity_analysis(&global_icfg, &selected_entry);
        let allocation_disposition_identity_state =
            fixed_point_disposition_identity_analysis(&global_icfg, &selected_entry);

        let (annotated_path, identity_path) = if plan.analysis_roots.len() == 1 {
            (
                annotated_icfg_out.clone(),
                allocation_identity_out
                    .cloned()
                    .unwrap_or_else(|| tool_dir.join("allocation_identity.json")),
            )
        } else {
            let out_dir = analysis_out_dir.expect("checked above");
            fs::create_dir_all(out_dir)
                .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;
            (
                out_dir.join(format!("annotated_icfg_v{cqpl_schema_version}--{slug}.json")),
                out_dir.join(format!("allocation_identity--{slug}.json")),
            )
        };

        let identity_json = serde_json::to_string_pretty(&allocation_identity_state.to_dump())
            .map_err(|e| format!("failed to serialize allocation identity: {e}"))?;
        fs::write(&identity_path, identity_json)
            .map_err(|e| format!("failed to write {}: {e}", identity_path.display()))?;

        // v6O-r1e: collect telemetry before CQPL export.  A schema-v2
        // fail-closed rejection is itself coverage evidence and must not censor
        // the real-crate sample.  This does not relax CQPL: export failure is
        // still returned to the caller as a non-zero analysis result.
        coverage_roots.push(collect_root_coverage(
            &global_icfg,
            requested_root,
            &selected_entry,
        ));

        let export_result = if cqpl_schema_version == 1 {
            export_cqpl_annotated_icfg(
                &global_icfg,
                &abstract_state,
                &selected_entry,
                &annotated_path,
            )
        } else {
            export_cqpl_annotated_icfg_with_identity_and_disposition(
                &global_icfg,
                &abstract_state,
                &allocation_identity_state,
                &allocation_disposition_identity_state,
                &selected_entry,
                cqpl_schema_version,
                &annotated_path,
            )
        };

        match export_result {
            Ok(()) => {
                println!("CQPL annotated ICFG saved to {}", annotated_path.display());
                artifact_records.push(json!({
                    "requested_root": requested_root,
                    "normalized_local_def_path": normalized_root,
                    "root_resolution": "local_def_path_namespace_v1",
                    "selected_entry": selected_entry,
                    "icfg": icfg_path,
                    "annotated_icfg": annotated_path,
                    "allocation_identity": identity_path,
                    "cargo_target_dir": cargo_target_dir,
                    "cqpl_export_status": "pass",
                    "cqpl_export_error": serde_json::Value::Null,
                }));
            }
            Err(err) => {
                let failure = format!("failed CQPL export for root {requested_root}: {err}");
                artifact_records.push(json!({
                    "requested_root": requested_root,
                    "normalized_local_def_path": normalized_root,
                    "root_resolution": "local_def_path_namespace_v1",
                    "selected_entry": selected_entry,
                    "icfg": icfg_path,
                    "annotated_icfg": serde_json::Value::Null,
                    "allocation_identity": identity_path,
                    "cargo_target_dir": cargo_target_dir,
                    "cqpl_export_status": "failed",
                    "cqpl_export_error": failure,
                }));
                write_v6o_telemetry_outputs(
                    &plan,
                    &plan_path,
                    &coverage_roots,
                    &artifact_records,
                    analysis_out_dir,
                    semantic_coverage_out,
                    &tool_dir,
                    "incomplete_cqpl_export",
                )?;
                return Err(failure);
            }
        }

        if !only_icfg_annotated {
            println!("=== CREMA v6O detector root: {requested_root} ===");
            detect_mem_issues(&global_icfg, &taint_state, &abstract_state);
        }
    }

    write_v6o_telemetry_outputs(
        &plan,
        &plan_path,
        &coverage_roots,
        &artifact_records,
        analysis_out_dir,
        semantic_coverage_out,
        &tool_dir,
        "complete",
    )?;
    Ok(())
}

fn write_v6o_telemetry_outputs(
    plan: &crate::cargo_project::CargoAnalysisPlan,
    plan_path: &PathBuf,
    coverage_roots: &[crate::semantic_coverage::RootCoverageReport],
    artifact_records: &[serde_json::Value],
    analysis_out_dir: Option<&PathBuf>,
    semantic_coverage_out: Option<&PathBuf>,
    tool_dir: &Path,
    analysis_status: &str,
) -> Result<(), String> {
    let coverage = SemanticCoverageBundle::new(plan, coverage_roots.to_vec());
    let coverage_path = semantic_coverage_out
        .cloned()
        .or_else(|| analysis_out_dir.map(|d| d.join("semantic_coverage.json")))
        .unwrap_or_else(|| tool_dir.join("semantic_coverage.json"));
    coverage.write_json(&coverage_path)?;
    println!("Semantic coverage saved to {}", coverage_path.display());

    let manifest_path = analysis_out_dir
        .map(|d| d.join("analysis_manifest.json"))
        .unwrap_or_else(|| tool_dir.join("analysis_manifest.json"));
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let cqpl_complete = artifact_records
        .iter()
        .filter(|a| a.get("cqpl_export_status").and_then(|v| v.as_str()) == Some("pass"))
        .count();
    let cqpl_incomplete = artifact_records.len().saturating_sub(cqpl_complete);
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&json!({
            "schema_version": 2,
            "feature": "cargo_crate_generalization_v1",
            "telemetry": "semantic_coverage_v2",
            "analysis_status": analysis_status,
            "cargo_plan": plan_path,
            "semantic_coverage": coverage_path,
            "cqpl_export_complete_roots": cqpl_complete,
            "cqpl_export_incomplete_roots": cqpl_incomplete,
            "artifacts": artifact_records,
        }))
        .map_err(|e| format!("failed to serialize analysis manifest: {e}"))?,
    )
    .map_err(|e| format!("failed to write {}: {e}", manifest_path.display()))?;
    println!("CREMA v6O analysis manifest saved to {}", manifest_path.display());
    Ok(())
}

// Resolve exactly one supported workspace target.  Historically CREMA iterated
// every bin/lib target and overwrote `global_icfg.json`; that made the analyzed
// program depend on Cargo metadata order.  v6K fails closed on ambiguity unless
// the user selects `--cargo-target name` or `--cargo-target package:name`.
fn analyze_cargo_project(
    project_path: &PathBuf,
    lib_output: &str,
    svf_output_dir: &Path,
    requested_target: Option<&str>,
    instance_entry_hint: &str,
) {
    let cargo_toml_path = project_path.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&cargo_toml_path)
        .exec()
        .expect("Failed to run cargo metadata");

    let workspace_members = metadata.workspace_members;
    let mut candidates: Vec<(String, Target, String)> = Vec::new();
    for package in metadata.packages {
        if !workspace_members.contains(&package.id) {
            continue;
        }
        for target in package.targets {
            let target_kind = target.kind.get(0).map(|k| k.to_string()).unwrap_or_default();
            let crate_type = match target_kind.as_str() {
                "bin" => Some("bin"),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" => Some("lib"),
                _ => None,
            };
            if let Some(crate_type) = crate_type {
                candidates.push((package.name.clone(), target, crate_type.to_string()));
            }
        }
    }
    candidates.sort_by(|a, b| {
        (&a.0, &a.1.name, a.1.src_path.as_str())
            .cmp(&(&b.0, &b.1.name, b.1.src_path.as_str()))
    });

    let mut selected: Vec<(String, Target, String)> = if let Some(requested) = requested_target {
        candidates
            .into_iter()
            .filter(|(package, target, _)| {
                target.name == requested || format!("{}:{}", package, target.name) == requested
            })
            .collect()
    } else {
        candidates
    };

    if selected.len() != 1 {
        let choices = selected
            .iter()
            .map(|(package, target, _)| format!("{}:{} ({})", package, target.name, target.src_path))
            .collect::<Vec<_>>()
            .join(", ");
        if requested_target.is_some() {
            eprintln!(
                "--cargo-target must select exactly one supported workspace target; matches={} [{}]",
                selected.len(), choices
            );
        } else {
            eprintln!(
                "CREMA requires an unambiguous Cargo target; found {} supported targets. \
Use --cargo-target <name|package:name>. Candidates: [{}]",
                selected.len(), choices
            );
        }
        exit(1);
    }

    let (package_name, target, crate_type) = selected.pop().unwrap();
    println!(
        "Analyzing package {} target {} at {}",
        package_name, target.name, target.src_path
    );
    analyze_target(&target, &crate_type, project_path, lib_output, svf_output_dir, instance_entry_hint);
}



// invokes rustc with the MIR extractor for a specific target, appending dependency search paths and extern flags
fn analyze_target(
    target: &Target,
    crate_type: &str,
    project_path: &PathBuf,
    _lib_output: &str,
    svf_output_dir: &Path,
    instance_entry_hint: &str,
) {
   
    let mut rustc_args = vec![
        "rustc".to_string(),
        target.src_path.to_string(),
        "--crate-type".to_string(),
        crate_type.to_string(),
        "--edition=2021".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
        
    ];

    // --- append sysroot ---
    let sysroot_output = Command::new("rustc")
        .args(&["--print", "sysroot"])
        .output()
        .expect("Failed to get sysroot");
    let sysroot = String::from_utf8(sysroot_output.stdout)
        .expect("sysroot not UTF8")
        .trim()
        .to_string();
    rustc_args.push("--sysroot".to_string());
    rustc_args.push(sysroot);


    // --- append dependency search paths ---
    let debug_dir = project_path.join("target").join("debug");
    let deps_dir = debug_dir.join("deps");

    if !debug_dir.exists() || !deps_dir.exists() {
        eprintln!("Target directories not found. Please run 'cargo build' in your target project first at {}", project_path.join("target").display());
        exit(1);
    }
    rustc_args.push("-L".to_string());
    rustc_args.push(format!("dependency={}", deps_dir.display()));
    rustc_args.push("-L".to_string());
    rustc_args.push(debug_dir.display().to_string());

    // NO HARDCODED LIBS
    if let Ok(entries) = fs::read_dir(&deps_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                if file_name.ends_with(".rlib") {
                    if let Some(stripped_name) = file_name.strip_prefix("lib") {
                        // extract crate name from the filename (part before the first '-')
                        let crate_name = stripped_name.splitn(2, '-').next().unwrap();
                        rustc_args.push(format!("--extern={}={}", crate_name, path.display()));
                    }
                }
            }
        }
    }
    eprintln!(">>> POST-DEP COLLECTION rustc_args = \n{:#?}", rustc_args);

    // initialize MIR extractor callbacks
    let mut callbacks = MirExtractor::new(
        svf_output_dir.to_string_lossy().to_string(),
        instance_entry_hint.to_string(),
    );
    if let Err(err) = RunCompiler::new(&rustc_args, &mut callbacks).run() {
        eprintln!("Error analyzing target {}: {:?}", target.name, err);
        exit(1);
    }
    if let Some(boundary) = callbacks.instance_dispatch_boundary.as_deref() {
        eprintln!("{}", boundary);
        exit(2);
    }
}

/// Create a collision-resistant per-invocation directory for SVF artifacts.
/// The directory path is operational metadata only; it is not part of
/// AbstractAllocId or any CQPL semantic identity.
fn prepare_svf_output_dir(project_path: &Path) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_nanos();
    let dir = project_path
        .join("target")
        .join("crema-svf")
        .join(format!("run-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        panic!("Failed to create isolated SVF directory {}: {e}", dir.display())
    });
    fs::canonicalize(&dir).unwrap_or(dir)
}

// Discover authored C sources deterministically.
//
// Cargo/build-script output lives under `target/` and may itself contain C
// helper files (for example cc-rs `flag_check.c`). Those generated files are
// analyzer inputs neither semantically nor reproducibly: `read_dir` order is
// unspecified, so selecting the first recursively observed `.c` can change
// which program is sent to SVF. Exclude generated trees and sort explicitly.
fn find_c_files(project_path: &PathBuf) -> Vec<String> {
    let mut c_files = Vec::new();
    visit_c_source_dirs(project_path, &mut c_files)
        .expect("Failed to traverse project directory");
    c_files.sort();
    c_files
}

fn visit_c_source_dirs(dir: &Path, c_files: &mut Vec<String>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "target" || name == ".git" {
                continue;
            }
            visit_c_source_dirs(&path, c_files)?;
        } else if path.extension().is_some_and(|ext| ext == "c") {
            c_files.push(path.to_string_lossy().to_string());
        }
    }

    Ok(())
}

// compiles the first C file found into LLVM IR, runs the SVF driver;
// compiles the C file into an object file, and creates a static library;
// returns the path to the created library.
fn compile_c_files(c_files: &Vec<String>, project_path: &PathBuf, svf_output_dir: &Path) -> String {
    // Preserve the historical one-C-module analysis scope, but make the
    // choice deterministic and restricted to authored source files.
    let c_file = &c_files[0];
    if c_files.len() > 1 {
        eprintln!(
            "CREMA note: {} authored C files found; Phase-5 analyzes the first \
lexicographically: {}",
            c_files.len(),
            c_file
        );
    }
    println!("Selected C source for SVF: {}", c_file);

    // Define paths. Both the LLVM module and SVF output are private to this run.
    let output_llvm_cfile = svf_output_dir.join("ffi.ll");
    let svf_driver = "./src/svf-example";
    let svf_working_dir = "../SVF-example";

    // create output paths relative to the project directory
    let output_dir = project_path.join("target");
    fs::create_dir_all(&output_dir).expect("Failed to create target directory");
    let object_file = output_dir.join(format!(
        "{}.o",
        Path::new(c_file).file_stem().unwrap().to_string_lossy()
    ));
    let output_lib = output_dir.join("libffi.a");

    // 1. compile the C file into LLVM IR
    let clang_status = Command::new("clang")
        .args(&["-S", "-c", "-fno-discard-value-names", "-emit-llvm", c_file, "-o"])
        .arg(&output_llvm_cfile)
        .status()
        .expect("Failed to compile C code into LLVM IR");
    println!("LLVM IR compilation finished with status: {:?}", clang_status);
    if !clang_status.success() {
        panic!("clang failed to produce isolated LLVM IR {}", output_llvm_cfile.display());
    }

    // 2. run the SVF driver
    let svf_status = Command::new(svf_driver)
        .arg(&output_llvm_cfile)
        .env("CREMA_SVF_OUTPUT_DIR", svf_output_dir)
        .current_dir(svf_working_dir)
        .status()
        .expect("Failed to run svf-driver");
    println!("svf-driver finished with status: {:?}", svf_status);
    if !svf_status.success() {
        panic!("svf-driver failed for isolated LLVM IR {}", output_llvm_cfile.display());
    }
    let produced_final_icfg = fs::read_dir(svf_output_dir)
        .unwrap_or_else(|e| panic!("Failed to inspect isolated SVF directory {}: {e}", svf_output_dir.display()))
        .filter_map(Result::ok)
        .any(|entry| {
            entry.path().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with("_A_FINAL_ICFG.json"))
        });
    if !produced_final_icfg {
        panic!(
            "SVF produced no *_A_FINAL_ICFG.json in isolated directory {}. \
Rebuild SVF-example/src/svf-example from the v6G source before running CREMA.",
            svf_output_dir.display()
        );
    }
    println!("SVF artifacts isolated at: {}", svf_output_dir.display());

    // 3. compile the C file into an object file
    let obj_status = Command::new("clang")
        .args(&["-c", c_file, "-o"])
        .arg(object_file.to_str().unwrap())
        .status()
        .expect("Failed to compile C code into object file");
    if !obj_status.success() {
        panic!("clang failed to compile C code into an object file");
    }

    // 4. create the static library from the object file
    let ar_status = Command::new("ar")
        .args(&["rcs", output_lib.to_str().unwrap(), object_file.to_str().unwrap()])
        .status()
        .expect("Failed to create static library");
    if !ar_status.success() {
        panic!("ar failed to create static library");
    }

    // tell Cargo to re-run this process if the C file changes.
    println!("cargo:rerun-if-changed={}", c_file);

    output_lib.to_string_lossy().to_string()
}

#[cfg(test)]
mod c_source_discovery_tests {
    use super::find_c_files;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn ignores_generated_target_c_and_sorts_authored_sources() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crema-c-source-discovery-{}-{}",
            std::process::id(),
            nonce
        ));
        let src = root.join("src");
        let generated = root.join("target/debug/build/cc/out");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&generated).unwrap();

        let a = src.join("a.c");
        let z = src.join("z.c");
        fs::write(&z, "int z(void){return 0;}\n").unwrap();
        fs::write(&a, "int a(void){return 0;}\n").unwrap();
        fs::write(generated.join("flag_check.c"), "int main(void){return 0;}\n")
            .unwrap();

        let got = find_c_files(&PathBuf::from(&root));
        assert_eq!(
            got,
            vec![
                a.to_string_lossy().to_string(),
                z.to_string_lossy().to_string(),
            ]
        );

        fs::remove_dir_all(root).unwrap();
    }
}
