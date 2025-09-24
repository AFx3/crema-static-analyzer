use pest::Parser;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[grammar = "cqpl.pest"]
struct CQPLParser;

#[derive(Debug, Clone)]
pub struct RuleDef {
    pub name: Vec<String>,
    pub domain: Domain,
    pub variables: Vec<Variable>,
    pub taint_src: Vec<Statement>,
    pub taint_snk: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub enum Domain { Memory, General }

#[derive(Debug, Clone)]
pub struct Variable {
    pub v_type: Option<Type>,
    pub qualifier: Option<Qualifier>,
    pub name: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Type { Box, Int, Float, Union, Function, Vec, Enum, Trait, Struct, Reference, Any }

#[derive(Debug, Clone)]
pub enum Qualifier { Imm, Mut, Any }

#[derive(Debug, Clone)]
pub enum Statement {
    Predicate(Predicate),
    And(Box<Statement>, Box<Statement>),
    Or(Box<Statement>, Box<Statement>),
    Not(Box<Statement>),
    Wildcard,
    Quantified { quant: Quantifier, var: String, domain: FieldDomain, cond: Box<Statement> },
}
#[derive(Debug, Clone)]
pub enum Quantifier { ForAll, Exists }

#[derive(Debug, Clone)]
pub enum FieldDomain { FieldsOf { typename: String } }

#[derive(Debug, Clone)]
pub enum Predicate {
    Alloc(Option<Term>),
    Drop(Option<Term>),
    Use(Option<Term>),
    Read(Option<Term>),
    Write(Option<Term>),
    Assign(Option<Term>),
    Custom(String, Vec<Term>),
}
#[derive(Debug, Clone)]
pub enum Term { Var(String), FieldAccess { base: String, field: String }, Literal(String) }

// === AST Builder ===
fn build_ast(pairs: Pairs<Rule>) -> Vec<RuleDef> {
    let mut rules = Vec::new();
    for pair in pairs {
        match pair.as_rule() {
            Rule::file => rules.extend(build_ast(pair.into_inner())),
            Rule::rule_item => {
                let mut name = Vec::new();
                let mut domain = Domain::General;
                let mut variables = Vec::new();
                let mut taint_src = Vec::new();
                let mut taint_snk = Vec::new();

                for inner in pair.into_inner() {
                    match inner.as_rule() {
                        Rule::rule_header => name.push(inner.as_str().trim().to_string()),
                        Rule::domain_decl => {
                            domain = if inner.as_str().to_lowercase().contains("memory") {
                                Domain::Memory
                            } else { Domain::General }
                        }
                        Rule::var_decl => {
                            if let Some((name_part, _)) = inner.as_str().trim().split_once(':') {
                                variables.push(Variable {
                                    name: Some(name_part.trim().to_string()),
                                    v_type: Some(Type::Any),
                                    qualifier: Some(Qualifier::Any),
                                });
                            }
                        }
                        Rule::taint_src_decl => {
                            for stmt in inner.into_inner() {
                                taint_src.push(parse_statement(stmt));
                            }
                        }
                        Rule::taint_snk_decl => {
                            for stmt in inner.into_inner() {
                                taint_snk.push(parse_statement(stmt));
                            }
                        }
                        _ => {}
                    }
                }

                if !name.is_empty() {
                    rules.push(RuleDef { name, domain, variables, taint_src, taint_snk });
                }
            }
            _ => {}
        }
    }
    rules
}

// Parse a single statement
fn parse_statement(pair: Pair<Rule>) -> Statement {
    match pair.as_rule() {
        Rule::primary => {
            // primary contiene un solo figlio: predicate, quant_expr, wildcard o paren_expr
            parse_statement(pair.into_inner().next().unwrap())
        }
        Rule::predicate => Statement::Predicate(parse_predicate(pair)),
        Rule::wildcard => Statement::Wildcard,
        Rule::not_expr => {
            let bang_count = pair.as_str().chars().take_while(|c| *c == '!').count();
            let inner_pair = pair.into_inner().next().unwrap();
            let mut stmt = parse_statement(inner_pair);
            for _ in 0..bang_count { stmt = Statement::Not(Box::new(stmt)); }
            stmt
        }
        Rule::quant_expr => {
            let mut inner = pair.into_inner();
            let first = inner.next().unwrap();
            match first.as_rule() {
                Rule::predicate => Statement::Predicate(parse_predicate(first)), // caso *@ alloc
                Rule::ident => {
                    // caso *@ var in type && predicate
                    let var = first.as_str().to_string();
                    let cond = inner.next().map(parse_statement).unwrap_or(Statement::Wildcard);
                    Statement::Quantified {
                        quant: Quantifier::ForAll,
                        var,
                        domain: FieldDomain::FieldsOf { typename: "Any".to_string() },
                        cond: Box::new(cond),
                    }
                }
                _ => panic!("Unexpected inner in quant_expr: {:?}", first.as_rule()),
            }
        }

        Rule::paren_expr | Rule::logic_expr | Rule::or_expr | Rule::and_expr => {
            parse_statement(pair.into_inner().next().unwrap())
        }
        _ => panic!("Unexpected rule in parse_statement: {:?}", pair.as_rule()),
    }
}


fn parse_predicate(pair: Pair<Rule>) -> Predicate {
    let txt = pair.as_str().trim();
    if txt.starts_with("alloc") { Predicate::Alloc(None) }
    else if txt.starts_with("drop") { Predicate::Drop(None) }
    else if txt.starts_with("use") { Predicate::Use(None) }
    else if txt.starts_with("read") { Predicate::Read(None) }
    else if txt.starts_with("write") { Predicate::Write(None) }
    else if txt.starts_with("assign") { Predicate::Assign(None) }
    else { Predicate::Custom(txt.to_string(), vec![]) }
}

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
