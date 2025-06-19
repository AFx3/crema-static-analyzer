#![feature(rustc_private)]

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_hir;
extern crate rustc_span;

mod ffi_scan;

use std::env;
use std::process;
use std::process::Command;
use std::path::PathBuf;
use rustc_driver::RunCompiler;
use cargo_metadata::MetadataCommand;

use ffi_scan::FfiExtractor;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --package ffi_extraction -- <path/to/cargo/project>");
        process::exit(1);
    }

    let project_path = PathBuf::from(&args[1]);
    let metadata = MetadataCommand::new()
        .manifest_path(project_path.join("Cargo.toml"))
        .exec()
        .unwrap();
    let output_dir = PathBuf::from(&args[2]);
    
    // prepare the rustc arguments
    let mut compiler_args = vec![
        "rustc".to_string(),
        "--edition=2021".to_string(),
        "-Zunstable-options".to_string(),        
    ];

    // get the sysroot so rustc finds the standard libraries
    let sysroot_output = Command::new("rustc")
        .args(&["--print", "sysroot"])
        .output()
        .expect("Failed to get sysroot");
    let sysroot = String::from_utf8(sysroot_output.stdout)
        .expect("sysroot not UTF8")
        .trim()
        .to_string();
    compiler_args.push("--sysroot".to_string());
    compiler_args.push(sysroot);

    
    // add the dir that contains built dependencies
    // This is the directory where cargo places compiled dependencies
    // These directories are normally passed by cargo ensuring that dependencies are found by rustc
    let debug_dir = project_path.join("target").join("debug");
    let deps_dir = debug_dir.join("deps");

    // check that dirs exist
    if !debug_dir.exists() || !deps_dir.exists() {
        eprintln!("Target directories not found. Please run `cargo build` in your target project first at {}", project_path.join("target").display());
        process::exit(1);
    }

    // add the deps directory to the library search path
    compiler_args.push("-L".to_string());
    compiler_args.push(format!("dependency={}", deps_dir.display()));
    // also add the top-level debug directory (sometimes artifacts are placed there as well)
    compiler_args.push("-L".to_string());
    compiler_args.push(format!("{}", debug_dir.display()));

    // REMOVE THE HARDCODING
    if let Ok(entries) = fs::read_dir(&deps_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                if file_name.ends_with(".rlib") {
                    if let Some(stripped_name) = file_name.strip_prefix("lib") {
                        // Extract crate name from the filename (part before the first '-')
                        let crate_name = stripped_name.splitn(2, '-').next().unwrap();
                        compiler_args.push(format!("--extern={}={}", crate_name, path.display()));
                    }
                }
            }
        }
    }

    // find the main target in the project
    let root_package = metadata.root_package().expect("No root package found");
    let main_target = root_package.targets.iter()
        .find(|t| t.kind.contains(&"bin".into()))
        .unwrap();
    // Now add the source file path at the end.
    compiler_args.push(main_target.src_path.to_string());

    let mut ffi_extractor = FfiExtractor::new();

    // set up environment variables to mimic Cargo
    env::set_var("RUSTC_BOOTSTRAP", "1");
    env::set_var("CARGO_MANIFEST_DIR", &project_path);

    // run the compiler with the custom callbacks that extract FFI functions
    RunCompiler::new(&compiler_args, &mut ffi_extractor)
        .run()
        .unwrap();

    // serialize the extracted FFI functions into pretty JSON
    let json_output = serde_json::json!({"ffi_functions": ffi_extractor.ffi_functions});

    let output_path = output_dir.join("ffi_functions.json"); 

    fs::write(output_path,serde_json::to_string_pretty(&json_output).unwrap()).unwrap();

}
