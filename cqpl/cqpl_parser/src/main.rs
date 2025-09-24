use pest::Parser;
use pest::iterators::{Pair, Pairs}; 
use pest_derive::Parser; 
use std::fs; use std::path::Path; 
#[derive(Parser)] 
#[grammar = "cqpl.pest"] 
struct CQPLParser; 
// AST 
#[derive(Debug, Clone)] 
pub struct RuleDef { 
    pub name: Vec<String>, 
    pub domain: Domain, 
    pub variables: Vec<Variable>, 
    pub taint_src: Vec<Statement>, 
    pub taint_snk: Vec<Statement>, 
} 

#[derive(Debug, Clone)] 
pub enum Domain { 
    Memory, 
    General, 
} 

#[derive(Debug, Clone)] 
pub struct Variable { 
    pub v_type: Option<Type>, 
    pub qualifier: Option<Qualifier>, 
    pub name: Option<String>, 
}

#[derive(Debug, Clone)]
pub enum Type { 
    Box, Int, Float, Union, Function, Vec, Enum, Trait, Struct, Reference, Any, 
} 

#[derive(Debug, Clone)] 
pub enum Qualifier { Imm, Mut, Any, } 

#[derive(Debug, Clone)] 
pub enum Statement { 
    Predicate(Predicate), 
    And(Box<Statement>, Box<Statement>), 
        Or(Box<Statement>, Box<Statement>),
        Not(Box<Statement>), 
        Wildcard, // * Ordered(Vec<Statement>), 
        // |> sequence 
        Quantified {
            quant: Quantifier, 
            var: String, 
            domain: FieldDomain, 
            cond: Box<Statement> 
        },
    } 
    
#[derive(Debug, Clone)] 
pub enum Quantifier { 
    ForAll /* *@ */, 
    Exists 
} 

#[derive(Debug, Clone)]
 pub enum FieldDomain { 
    FieldsOf { 
        typename: String 
    }
} 

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
pub enum Term { 
    Var(String), 
    FieldAccess { 
        base: String, 
        field: String 
    }, 
    Literal(String), } 

fn build_ast(pairs: Pairs<Rule>) -> Vec<RuleDef> { 
    let mut rules = Vec::new(); // pairs qui è il risultato di CQPLParser::parse(Rule::file, ...) 
    for pair in pairs { 
        match pair.as_rule() { 
            Rule::file => { // Itera sui figli di file, che sono i rule_item 
                rules.extend(build_ast(pair.into_inner())); 
                } 
                Rule::rule_item => { 
                    let mut name = Vec::new(); 
                    let mut domain = Domain::General; 
                    let mut variables = Vec::new(); 
                    let mut taint_src = Vec::new(); 
                    let mut taint_snk = Vec::new(); 
                    
                    for inner in pair.into_inner() { 
                        match inner.as_rule() { 
                            Rule::rule_header => { 
                                let header_text = inner.as_str().trim(); 
                                if !header_text.is_empty() { 
                                    name.push(header_text.to_string());
                                } 
                                } 
                                Rule::domain_decl => { 
                                    let txt = inner.as_str().to_lowercase(); 
                                    if txt.contains("memory") { 
                                        domain = Domain::Memory; 
                                    } else { 
                                        domain = Domain::General; 
                                    } 
                                }
                                Rule::var_decl => { 
                                    let var_text = inner.as_str().trim(); 
                                    if !var_text.is_empty() { 
                                        variables.push(Variable { 
                                            v_type: None, 
                                            qualifier: None, 
                                            name: Some(var_text.to_string()),
                                         }); 
                                        } 
                                    } 
                                Rule::taint_src_decl => { 
                                    if let Some(expr) = parse_statement(inner.into_inner()) { 
                                        taint_src.push(expr); 
                                    } } 
                                Rule::taint_snk_decl => { 
                                    if let Some(expr) = parse_statement(inner.into_inner()) { 
                                        taint_snk.push(expr); 
                                    } } 
                                Rule::blank | Rule::COMMENT => {} _ => {} } } 
                                
                                if !name.is_empty() { 
                                    rules.push(RuleDef { name, domain, variables, taint_src, taint_snk, });
                                 } }
                                 
                                _ => {} 
                            } 
                        } 
                        rules 
                } 

fn parse_statement(pairs: Pairs<Rule>) -> Option<Statement> { 
    let mut iter = pairs.into_iter(); 
    let first = iter.next()?; match first.as_rule() { 
        Rule::predicate => Some(Statement::Predicate(parse_predicate(first))), 
        Rule::wildcard => Some(Statement::Wildcard), 
        Rule::logic_expr | Rule::or_expr | Rule::and_expr | Rule::paren_expr => { parse_statement(first.into_inner()) } 
        Rule::not_expr => { let mut inner = first.clone().into_inner(); // conta quanti '!' ci sono 
        let text = first.as_str(); let bang_count = text.chars().take_while(|c| *c == '!').count(); let core = parse_statement(inner)?; 
        let mut stmt = core; for _ in 0..bang_count { 
            stmt = Statement::Not(Box::new(stmt)); } 
            Some(stmt) }
        Rule::quant_expr => { 
            let mut inner = first.into_inner(); 
            let var = inner.next()?.as_str().to_string(); 
            let type_access = inner.next()?.as_str().to_string(); 
            let cond_pred = parse_statement(inner)?; 
            Some(Statement::Quantified { 
                quant: Quantifier::ForAll, 
                var, 
                domain: FieldDomain::FieldsOf { 
                    typename: type_access.split('.').next().unwrap().into() }, 
                    cond: Box::new(cond_pred), }) } _ => None, 
                }
             } 
             
             
fn parse_predicate(pair: Pair<Rule>) -> Predicate { 
    let txt = pair.as_str().trim(); // semplificazione: solo predicati noti per ora
   
    if txt.starts_with("alloc") { 
        Predicate::Alloc(None) 
    } else if txt.starts_with("drop") { 
        Predicate::Drop(None) 
    } else if txt.starts_with("use") { Predicate::Use(None) 
    } else if txt.starts_with("read") { Predicate::Read(None)
     } else if txt.starts_with("write") { Predicate::Write(None)
     } else if txt.starts_with("assign") { Predicate::Assign(None) 
    } else { Predicate::Custom(txt.to_string(), vec![]) } } 
    
    
    
fn main() { let path = "./examples/mem_leak.cqpl"; 
    if !Path::new(path).exists() { 
        eprintln!("example file not found: {}", path); 
        return; 
    } 
    
    let input = fs::read_to_string(path).expect("failed to read example file"); 
    let parsed = CQPLParser::parse(Rule::file, &input).expect("parse error"); 
    let ast = build_ast(parsed); 
    println!("AST: {:#?}", ast);
}