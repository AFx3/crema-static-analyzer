use std::path::{Path, PathBuf};
use std::fs;
use pest::Parser;
use std::env;
use std::process::exit;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

mod ast;
mod test;
mod metainterpreter;
use crate::ast::{CQPLParser, build_ast, Rule};
use crate::metainterpreter::*;

/// Run: cargo run <rule_name>.cqpl /path/to/cargo/project
fn main() {
    let base_rule_path = "./examples/";
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: cargo run -- <rule_name>.cqpl /path/to/cargo/project");
        exit(1);
    }

    let input_rule_file = &args[1];
    let project_path = PathBuf::from(&args[2]);
    let full_rule_path = PathBuf::from(base_rule_path).join(input_rule_file);

    if !full_rule_path.exists() {
        eprintln!("Rule file not found: {}", full_rule_path.display());
        exit(1);
    }

    let input = fs::read_to_string(&full_rule_path).expect("failed to read rule file");
    let parsed = CQPLParser::parse(Rule::file, &input).expect("parse error");
    let ast = build_ast(parsed);

    println!("Target project: {:?}", project_path);
    println!("Rule path: {:?}", full_rule_path);
    println!("AST: {:#?}", ast);

    if ast.is_empty() {
        eprintln!("No rules found in file {}", full_rule_path.display());
        return;
    }

    let rule = &ast[0];
    register_rules_for_inference(&ast);

    let kind = infer_analysis_kind(rule);
    println!("\n>> Inferred analysis kind: {:?}", kind);

    // run CREMA for the supported analyses
    match kind {
        AnalysisKind::MemoryLeak => {
            println!("Running Memory-Leak analysis with CREMA...");
            run_crema_analysis(&project_path);
        }
        AnalysisKind::UseAfterFree => {
            println!("Running Use-After-Free analysis with CREMA...");
            run_crema_analysis(&project_path);
        }
        AnalysisKind::DoubleFree => {
            println!("Running Double-Free analysis with CREMA...");
            run_crema_analysis(&project_path);
        }
        AnalysisKind::Unknown => {
            println!("Unknown or unclassified analysis kind.");
        }
    }

    let json = serde_json::to_string_pretty(&ast).unwrap();
    let path = Path::new("../ast.json");
    if let Err(why) = File::create(&path).and_then(|mut f| f.write_all(json.as_bytes())) {
        eprintln!("Couldn't write AST JSON: {}", why);
    } else {
        println!("Successfully wrote AST to {}", path.display());
    }
}

/// Finds the nearest Cargo project root upward from a given path
fn find_cargo_project_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = fs::canonicalize(start_path).ok()?;
    loop {
        if current.join("Cargo.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Runs the CREMA analysis tool on the Cargo project root
fn run_crema_analysis(project_path: &Path) {
    let project_root = find_cargo_project_root(project_path).unwrap_or_else(|| {
        eprintln!("No Cargo.toml found upward from {:?}", project_path);
        std::process::exit(1);
    });

    println!("Resolved Cargo project root: {:?}", project_root);

    let crema_path = Path::new("../../crema");

    let status = Command::new("cargo")
        .arg("run")
        .arg(project_root.to_str().unwrap())
        .current_dir(&crema_path)
        .status()
        .expect("Failed to execute CREMA");

    if !status.success() {
        eprintln!("CREMA exited with status: {}", status);
    }
}
