use std::path::Path;
use std::fs;
use pest::Parser;

mod ast;
use crate::ast::{CQPLParser, build_ast, Rule} ;

fn main() {
    let path = "./examples/mem_leak.cqpl";
    if !Path::new(path).exists() { eprintln!("example file not found: {}", path); 
    return; 
}

    let input = fs::read_to_string(path).expect("failed to read example file");
    let parsed = CQPLParser::parse(Rule::file, &input).expect("parse error");
    let ast = build_ast(parsed);
    println!("AST: {:#?}", ast);
}
