use std::path::{PathBuf};
use std::fs;
use pest::Parser;
use std::env;
use std::process::exit;

mod ast;
use crate::ast::{CQPLParser, build_ast, Rule} ;

/// Run:              cargo run      <rule_name>.cqpl    /path/to/cargo/project
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
}
