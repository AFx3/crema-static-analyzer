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

static GLOBAL_ICFG_JSON: &str = "global_icfg.json";

fn main() {
    // --- step 0: process cmd args ---
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run -- /path/to/cargo/project [-f <entry>]");  
        exit(1);
    }
    let project_path = PathBuf::from(&args[1]);

    // entry‐point override:
    // if user sets -f flag, override the entry point (no longer the main function but the one as input) 
    let mut entry_override: Option<String> = None;
    let mut idx = 2;
    while idx + 1 < args.len() {
        match args[idx].as_str() {
            "-f" => {
                entry_override = Some(args[idx + 1].clone());
                idx += 2;
            }
            other => {
                eprintln!("Unknown flag: {}", other);
                exit(1);
            }
        }
    }

    let cargo_toml_path = project_path.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        eprintln!("Error: {} is not a valid Cargo project", project_path.display());
        exit(1);
    }
    println!("Analyzing Cargo project at: {}", project_path.display());

    // --- step 0.1: run FFI xxtraction ---
    let tool_dir = env::current_dir().expect("Failed to get tool directory");
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


    // --- step 1: discover and Process C Files ---
    let c_files = find_c_files(&project_path);
    let lib_output = if !c_files.is_empty() {
        compile_c_files(&c_files, &project_path)
    } else {
        println!("No C files found in the project.");
        String::new()
    };
    println!("ffi_extraction finished with status: {:?}", extraction_status);
    // --- step 2: analyze target cargo project (Only Workspace Members) ---
    analyze_cargo_project(&project_path, &lib_output);


    // --- Step 3: dump and analyze the global ICFG ---
    dump_dot_from_global_icfg(GLOBAL_ICFG_JSON);

    let mut file = File::open(GLOBAL_ICFG_JSON)
        .expect("Failed to open global_icfg.json file");
    let mut json_str = String::new();
    file.read_to_string(&mut json_str)
        .expect("Failed to read global_icfg.json file");

    let global_icfg: GlobalICFGOrdered = serde_json::from_str(&json_str)
        .expect("Failed to deserialize global ICFG");


    // if entry point is given as input, set the entry point
    if let Some(ep) = entry_override {
            set_entrypoint(ep);
        }

    let (abstract_state, taint_state) = fixed_point_analysis(&global_icfg);
   // println!("Final Abstract State: {:#?}", abstract_state);
   // println!("Final Taint State: {:#?}", taint_state);

    detect_mem_issues(&global_icfg, &taint_state, &abstract_state);
}

// use cargo_metadata to resolve all packages and targets, then analyzes each target that belongs to the workspace 
fn analyze_cargo_project(project_path: &PathBuf, lib_output: &str) {
    let cargo_toml_path = project_path.join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&cargo_toml_path)
        .exec()
        .expect("Failed to run cargo metadata");

    // only analyze packages that are workspace members
    let workspace_members = metadata.workspace_members;

    for package in metadata.packages {
        
        if !workspace_members.contains(&package.id) {
            //println!("Skipping external dependency package: {}", package.name);
            continue;
        }

        for target in package.targets {
            println!("Analyzing package {} target {} at {}", package.name, target.name, target.src_path
            );
            // convert the target kind to a string
            let target_kind = target.kind.get(0)
                .map(|k| k.to_string())
                .unwrap_or_default();
            let crate_type = match target_kind.as_str() {
                "bin" => "bin",
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" => "lib",
                _ => {
                    println!("Skipping unsupported target kind: {:?}", target.kind);
                    continue;
                }
            };
            analyze_target(&target, crate_type, project_path, lib_output);
        }
    }
}



// invokes rustc with the MIR extractor for a specific target, appending dependency search paths and extern flags
fn analyze_target(target: &Target, crate_type: &str, project_path: &PathBuf, _lib_output: &str) {
   
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
    let mut callbacks = MirExtractor::new();
    if let Err(err) = RunCompiler::new(&rustc_args, &mut callbacks).run() {
        eprintln!("Error analyzing target {}: {:?}", target.name, err);
        exit(1);
    }
}

// recursively searches for C files (files ending in .c) within the project directory
fn find_c_files(project_path: &PathBuf) -> Vec<String> {
    let mut c_files = Vec::new();
    visit_dirs(project_path, &mut |entry| {
        if let Some(ext) = entry.path().extension() {
            if ext == "c" {
                c_files.push(entry.path().to_string_lossy().to_string());
            }
        }
    }).expect("Failed to traverse project directory");
    c_files
}

// recursively visits directories and applies the callback to every entry
fn visit_dirs(dir: &Path, cb: &mut dyn FnMut(&fs::DirEntry)) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            cb(&entry);
            if entry.path().is_dir() {
                visit_dirs(&entry.path(), cb)?;
            }
        }
    }
    Ok(())
}

// compiles the first C file found into LLVM IR, runs the SVF driver;
// compiles the C file into an object file, and creates a static library;
// returns the path to the created library.
fn compile_c_files(c_files: &Vec<String>, project_path: &PathBuf) -> String {
    // use the first C file in the list
    let c_file = &c_files[0];

    // define paths 
    let output_llvm_cfile = "../SVF-example/ffi.ll";
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

    // 2. run the SVF driver
    let svf_status = Command::new(svf_driver)
        .arg("ffi.ll")
        .current_dir(svf_working_dir)
        .status()
        .expect("Failed to run svf-driver");
    println!("svf-driver finished with status: {:?}", svf_status);

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


