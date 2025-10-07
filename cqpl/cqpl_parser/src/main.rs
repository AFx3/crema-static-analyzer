use std::path::{PathBuf};
use std::fs;
use pest::Parser;
use std::env;
use std::process::exit;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;


mod ast;
mod test;
mod metainterpreter;
use crate::ast::{CQPLParser, build_ast, Rule};
use crate::metainterpreter::*;

/// Run:              cargo run      <rule_name>.cqpl    /path/to/cargo/project
/// e.g.: cargo run mem_leak.cqpl w
fn main() {
    // now store all the rules in the folder ./example
    let base_rule_path = "./examples/";
    // input cmd line
    let args: Vec<String> = env::args().collect();
    // if ars not provided -> exit
    if args.len() < 2 {
        eprintln!("Usage: cargo run --  /rule_name /path/to/cargo/project");  
        exit(1);
    }
    // read first arg
    let input_rule_file = &args[1];
    // read second arg
    let project_path = PathBuf::from(&args[2]);
    
    // create the full path of the rule cqpl file from base_rule_path and the 1st input arg
    let full_rule_path= PathBuf::from(base_rule_path).join(input_rule_file);

    // check if the rule cqpl file exists
    if !full_rule_path.exists() {
        eprintln!("Rule file not found: {}", full_rule_path.display());
        return; 
    }
   

    let input = fs::read_to_string(&full_rule_path).expect("failed to read example file");
    let parsed = CQPLParser::parse(Rule::file, &input).expect("parse error");
    let ast = build_ast(parsed);

    println!("Target project {:#?}", project_path);
    println!("Rule path {:#?}", full_rule_path);
    println!("AST: {:#?}", ast);

    // analysis classification:
    // The AST builder returns Vec<RuleDef> (one per rule in the file)
    if ast.is_empty() {
        eprintln!("No rules found in file {}", full_rule_path.display());
        return;
    }

    // take the first rule to classify
    let rule = &ast[0];

    // Register all rules for semantic inference (optional, for equivalence checking)
    register_rules_for_inference(&ast);

    // Infer which kind of analysis this rule represents
    let kind = infer_analysis_kind(rule);

    println!("\n>> Inferred analysis kind: {:?}", kind);
    match kind {
        AnalysisKind::MemoryLeak => {
            println!("Running Memory Leak analysis...");
            // call your memory leak analysis function here, e.g.:
            // run_memory_leak_analysis(&project_path, &ast);
        }
        AnalysisKind::UseAfterFree => {
            println!("Running Use-After-Free analysis...");
            // run_uaf_analysis(&project_path, &ast);
        }
        AnalysisKind::DoubleFree => {
            println!("Running Double-Free analysis...");
            // run_double_free_analysis(&project_path, &ast);
        }
        AnalysisKind::Unknown => {
            println!("Unknown or unclassified analysis kind.");
        }
    }




    let json = serde_json::to_string_pretty(&ast).unwrap();

    let path = Path::new("../ast.json");
    let display = path.display();

    // Open a file in write-only mode, returns io::Result<File>
    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    // Write the json to file, returns io::Result<()>
    match file.write_all(json.as_bytes()) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => println!("successfully wrote to {}", display),
    }
    
}
