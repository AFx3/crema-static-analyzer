#[cfg(test)]
mod tests {
    use pest::Parser;
    use crate::ast::*;
    // Tests parsing of basic predicates like alloc, use, read, write, assign, drop
    #[test]
    fn test_predicates() {
        let predicates = ["alloc", "use", "read", "write", "assign", "drop"];

        for &p in predicates.iter() {
            // 1. No argument
            let pair = CQPLParser::parse(Rule::predicate, p).unwrap().next().unwrap();
            let stmt = Statement::Predicate(parse_predicate(pair));
            let expect = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(None)),
                "drop" => Statement::Predicate(Predicate::Drop(None)),
                "use" => Statement::Predicate(Predicate::Use(None)),
                "read" => Statement::Predicate(Predicate::Read(None)),
                "write" => Statement::Predicate(Predicate::Write(None)),
                "assign" => Statement::Predicate(Predicate::Assign(None)),
                _ => unreachable!(),
            };
            assert_eq!(stmt, expect);

            // 2. Empty parentheses, like alloc()
            let input = format!("{}()", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt2 = Statement::Predicate(parse_predicate(pair));
            assert_eq!(stmt2, expect);

            // 3. Single variable argument like alloc(x)
            let input = format!("{}(x)", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt3 = Statement::Predicate(parse_predicate(pair));
            let expect3 = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string())))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::Var("x".to_string())))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::Var("x".to_string())))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string())))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string())))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::Var("x".to_string())))),
                _ => unreachable!(),
            };
            assert_eq!(stmt3, expect3);

            // 4. Field access argument like alloc(x.y)
            let input = format!("{}(x.y)", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt4 = Statement::Predicate(parse_predicate(pair));
            let expect4 = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                _ => unreachable!(),
            };
            assert_eq!(stmt4, expect4);
        }
    }
    // Test parsing of the drop predicate specifically
    #[test]
    fn test_predicate_drop() {
        let pair = CQPLParser::parse(Rule::predicate, "drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(stmt, Statement::Predicate(Predicate::Drop(None)));
    }
    // Test parsing of wildcard *
    #[test]
    fn test_wildcard() {
        let pair = CQPLParser::parse(Rule::wildcard, "*").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(stmt, Statement::Wildcard);
    }
    // Test parsing of double negation: !!drop
    #[test]
    fn test_double_negation() {
        let pair = CQPLParser::parse(Rule::not_expr, "!!drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::Not(Box::new(
                Statement::Not(Box::new(Statement::Predicate(Predicate::Drop(None))))
            ))
        );
    }
    // Test forall quantifier without variable name
    // \forall alloc 
    #[test]
    fn test_quantifier_no_var() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\forall alloc").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Any),
                cond: Box::new(Statement::Predicate(Predicate::Alloc(None)))
            }
        );
    }
    // Test exists quantifier with in-fields and predicate
    // \exists x in Vec.fields && drop
    #[test]
    fn test_quantifier_with_in_fields() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\exists x in Vec.fields && drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::Exists,
                var: Some(VarName::Named("x".to_string())),
                cond: Box::new(
                    Statement::And(
                        Box::new(Statement::Predicate(Predicate::InFields(Type::Vec))),
                        Box::new(Statement::Predicate(Predicate::Drop(None))),
                    )
                )
            }
        );
    }
    // Test parsing of AND expression
    // read && write
    #[test]
    fn test_and_expression() {
        let pair = CQPLParser::parse(Rule::and_expr, "read && write").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::And(
                Box::new(Statement::Predicate(Predicate::Read(None))),
                Box::new(Statement::Predicate(Predicate::Write(None)))
            )
        );
    }
    // Test AND expression with variable 
    // read(x) && write(x)
    #[test]
    fn test_and_expression_with_var() {
        let pair = CQPLParser::parse(Rule::and_expr, "read(x) && write(x)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::And(
                Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string()))))),
            )
        );
    }
    // Test OR expression 
    // read || write
    #[test]
    fn test_or_expression() {
        let pair = CQPLParser::parse(Rule::or_expr, "read || write").unwrap().next().unwrap();
        let stmt = parse_statement(pair);
        assert_eq!(
            stmt,
            Statement::Or(
                Box::new(Statement::Predicate(Predicate::Read(None))),
                Box::new(Statement::Predicate(Predicate::Write(None)))
            )
        );
    }
    // Test OR expression with variable 
    // read(x) || write(x)
    #[test]
    fn test_or_expression_with_var() {
        let pair = CQPLParser::parse(Rule::or_expr, "read(x) || write(x)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Or(
                Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string()))))),
            )
        );
    }
    // Test NOT applied to AND
    // !(read && write)
    #[test]
    fn test_parentheses_and_not() {
        let pair = CQPLParser::parse(Rule::not_expr, "!(read && write)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt, Statement::Not(
                                        Box::new(Statement::And(
                                                                Box::new(Statement::Predicate(Predicate::Read(None))),
                                                                Box::new(Statement::Predicate(Predicate::Write(None)))))));

    }
    // Test NOT applied to OR
    // !(read || write)
    #[test]
    fn test_parentheses_or_not() {
        let pair = CQPLParser::parse(Rule::not_expr, "!(read || write)").unwrap().next().unwrap();
        println!("PAIR {:#?}", pair);
        let stmt = parse_statement(pair);
        println!("stmnt aka parsed pair {:#?}", stmt);

        assert_eq!(stmt, Statement::Not(
                                        Box::new(Statement::Or(
                                                                Box::new(Statement::Predicate(Predicate::Read(None))),
                                                                Box::new(Statement::Predicate(Predicate::Write(None)))))));

    }
    // Test INVALID type in quantifier in-fields
    // \exists y in Int.fields (invalid) : only tup, array, vec ecc..
    #[test]
    fn test_invalid_type_in_fields() {
        let result = std::panic::catch_unwind(|| {
            let pair = CQPLParser::parse(Rule::quant_expr, "\\exists y in Int.fields").unwrap().next().unwrap();
            parse_statement(pair);
         });

        assert!(result.is_err()); // only tup, array,
    }
    /// TEST ALLOCTORS
    // Test allocator predicate parsing with various languages and allocators
     #[test]
    fn test_allocator_predicates() {
        let languages = vec![("rust", Language::Rust), ("c", Language::C)];
        let allocator_types = vec![
            ("default", AllocatorType::Default),
            ("jemalloc", AllocatorType::Jemalloc),
            ("mimalloc", AllocatorType::Mimalloc),
            ("rpmalloc", AllocatorType::Rpmalloc),
            ("snmalloc", AllocatorType::Snmalloc),
            ("weealloc", AllocatorType::Weealloc),
            ("dlmalloc", AllocatorType::Dlmalloc),
        ];

        for (lang_str, lang_enum) in &languages {
            for (alloc_str, alloc_enum) in &allocator_types {
                let input = format!("allocator({}, {})", lang_str, alloc_str);
                let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
                let stmt = parse_statement(pair);

                assert_eq!(stmt, Statement::Predicate(Predicate::Allocator(lang_enum.clone(), alloc_enum.clone())), "Failed for input: {}", input);
            }
        }
    }
    // Test allocator predicate with wrong number of arguments (should panic)
    #[test]
    #[should_panic(expected = "Allocator predicate requires 2 arguments")]
    fn test_allocator_wrong_number_of_args() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(rust)").unwrap().next().unwrap();
        parse_statement(pair); //  panic -> only one argument provided
    }
    // Test allocator with unknown language (should panic)
    #[test]
    #[should_panic(expected = "Unknown language")]
    fn test_allocator_unknown_language() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(java, jemalloc)").unwrap().next().unwrap();
        parse_statement(pair); // panic because is invalid
    }
    // Test allocator with unknown type (should panic)
    #[test]
    #[should_panic(expected = "Unknown allocator type")]
    fn test_allocator_unknown_type() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(rust, garbagecollector)").unwrap().next().unwrap();
        parse_statement(pair); // panic -> unknown is invalid
    }
    ////////////
    /// Semanitc equivalence tests
    
    // now mock
    fn dummy_eval(_pred: &Predicate, _vars: &std::collections::HashMap<String, String>) -> bool {
        true
    }
    // Test commutativity AND
    // read && write <=> write && read
    #[test]
    fn test_and_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write && read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test commutativity OR
    // read || write <=> write || read
    #[test]
    fn test_or_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write || read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test associativity AND
    // read && (write && assign) <=> (read && write) && assign
    #[test]
    fn test_and_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write && assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) && assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test associtivity OR
    // read || (write || assign) <=> (read || write) || assign
    #[test]
    fn test_or_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read || write) || assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test distributivity
    // read && (write || assign) <=> (read && write) || (read && assign)
    #[test]
    fn test_distributivity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) || (read && assign)").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test De Morgan
    // !(read && write) <=> !read || !write
    #[test]
    fn test_de_morgan_and_or() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read && write)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read || !write").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars: Vec<Variable> = vec![];
        let possible_values: Vec<String> = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    
    // Tests with named vars: the same as before but with named variables
    #[test]
    fn test_and_commutativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write(y) && read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    #[test]
    fn test_or_commutativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(a) || write(b)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write(b) || read(a)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("a".to_string()))),
            Predicate::Write(Some(Term::Var("b".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("a".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("b".to_string())) },
        ];
        let possible_values = vec!["a".to_string(), "b".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    #[test]
    fn test_and_associativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && (write(y) && assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) && write(y)) && assign(z)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("z".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    #[test]
    fn test_or_associativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) || (write(y) || assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) || write(y)) || assign(z)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("z".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }


    #[test]
    fn test_distributivity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && (write(y) || assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) && write(y)) || (read(x) && assign(z))").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("z".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string(), "z".to_string()];
    assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    #[test]
    fn test_de_morgan_and_or_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read(x) && write(y))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read(x) || !write(y)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    /// Test quantifiers
    
    // Test Forall commutativuty
    // \forall x. \forall y. read(x) && write(y) <=> \forall y. \forall x. write(y) && read(x)
    #[test]
    fn test_forall_commutativity() {
    let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\forall y. read(x) && write(y)").unwrap().next().unwrap());
    let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall y. \\forall x. write(y) && read(x)").unwrap().next().unwrap());
    let preds = vec![
        Predicate::Read(Some(Term::Var("x".to_string()))),
        Predicate::Write(Some(Term::Var("y".to_string()))),
    ];
    let vars = vec![
        Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
        Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
    ];
    let possible_values = vec!["x".to_string(), "y".to_string()];
    assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test Exists commutativity
    // \exists x. \exists y. read(x) || write(y) <=> \exists y. \\exists x. write(y) || read(x)
    #[test]
    fn test_exists_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists x. \\exists y. read(x) || write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists y. \\exists x. write(y) || read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }
    // Test nested quantifiers
    // \forall x. \exists y. read(x) && write(y) <=> \forall x. \exists y. write(y) && read(x)
    #[test]
    fn test_nested_quantifiers() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\exists y. read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\exists y. write(y) && read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".to_string())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".to_string())) },
        ];
        let possible_values = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &possible_values, &dummy_eval));
    }

    // new tests
    
    // - Predicate::Alloc(Some(Term::Var(name))) => true iff env[name] == "alloced"
    fn sample_eval(pred: &Predicate, env: &Env) -> bool {
        match pred {
            Predicate::Alloc(Some(Term::Var(v))) => {
                env.get(v).map(|s| s == "alloced").unwrap_or(false)
            }
            Predicate::Alloc(None) => false,
            _ => false,
        }
    }

    #[test]
    fn semantically_eq_basic() {
        let s_x = Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string()))));
        let s_y = Statement::Predicate(Predicate::Alloc(Some(Term::Var("y".to_string()))));

        // dichiaro una variabile anonima (Any)
        let vars = vec![Variable { v_type: None, qualifier: None, name: Some(VarName::Any) }];

        let possible = vec!["alloced".to_string(), "not_alloced".to_string()];

        // Con l'interpretazione sample_eval, s_x e s_y sono semanticamente eq su tutte le assegnazioni?
        // dipende: se x e y sono la stessa variabile anonima
        // caso easy: confronto s_x con se stesso -> deve essere true
        assert!(semantically_eq(&s_x, &s_x, &[], &vars, &possible, &sample_eval));
    }
    #[test]
    fn test_eval_predicate_true() {
        let stmt = Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string()))));
        let mut env = Env::new();
        env.insert("x".to_string(), "1".to_string());
        assert!(stmt.eval_with_env(&dummy_eval, &env, &[], &["0".into(), "1".into()]));
    }
    /// TEST THEN (|>)
    // Caso valido: src |> src
    #[test]
    fn test_then_src_src_ok() {
        let pair = CQPLParser::parse(Rule::logic_expr, "alloc(x) |> alloc(y)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Then(
                Box::new(Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Alloc(Some(Term::Var("y".to_string()))))),
            )
        );
    }
    // Caso valido: snk |> snk
    #[test]
    fn test_then_snk_snk_ok() {
        let pair = CQPLParser::parse(Rule::logic_expr, "drop(x) |> drop(y)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Then(
                Box::new(Statement::Predicate(Predicate::Drop(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Drop(Some(Term::Var("y".to_string()))))),
            )
        );
    }
    // Should painc src |> snk 
    #[test]
    #[should_panic]
    fn test_then_src_snk_should_fail() {
        let pairs = CQPLParser::parse(Rule::file, r#"
            # test
            ## domain: memory
            v: *|*|*
            taint_src: alloc(x) |> drop(x)
        "#).unwrap();
        let rules = build_ast(pairs);
        // la validazione interna a build_ast farà panic
    }
    /// new
    #[test]
    fn test_eval_with_env_alloc() {
        let stmt = Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string()))));
        let mut env = Env::new();
        env.insert("x".into(), "alloced".into());

       
        assert!(stmt.eval_with_env(&sample_eval, &env, &[], &["alloced".into(), "free".into()]));
    }

    #[test]
    fn test_semantically_eq_fail() {
        let s1 = Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string()))));
        let s2 = Statement::Predicate(Predicate::Alloc(Some(Term::Var("y".to_string()))));

        let vars = vec![
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".into())) },
            Variable { v_type: None, qualifier: None, name: Some(VarName::Named("y".into())) },
        ];
        let possible = vec!["alloced".into(), "free".into()];

        assert!(!semantically_eq(&s1, &s2, &[], &vars, &possible, &sample_eval));
    }
    ///// to check
    #[test]
    fn test_forall_semantics() {
        let stmt = parse_statement(
            CQPLParser::parse(Rule::logic_expr, "\\forall x. alloc(x)").unwrap().next().unwrap()
        );

        let vars = vec![Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".into())) }];
        let possible = vec!["alloced".into(), "free".into()];

        // sample_eval richiede che x == "alloced" per essere vero
        // quindi \forall x. alloc(x) deve essere falso
        assert!(!stmt.eval_with_env(&sample_eval, &Env::new(), &vars, &possible));
    }

    #[test]
    fn test_exists_semantics() {
        let stmt = parse_statement(
            CQPLParser::parse(Rule::logic_expr, "\\exists x. alloc(x)").unwrap().next().unwrap()
        );

        let vars = vec![Variable { v_type: None, qualifier: None, name: Some(VarName::Named("x".into())) }];
        let possible = vec!["alloced".into(), "free".into()];

        // c’è almeno un valore alloced che rende vera alloc(x)
        assert!(stmt.eval_with_env(&sample_eval, &Env::new(), &vars, &possible));
    }



}


/* 
#[cfg(test)]
mod tests {
    use pest::Parser;
    use crate::ast::*;

    #[test]
    fn test_predicates() {
        let predicates = ["alloc", "use", "read", "write", "assign", "drop"];

        for &p in predicates.iter() {
            // 1. No argument
            let pair = CQPLParser::parse(Rule::predicate, p).unwrap().next().unwrap();
            let stmt = Statement::Predicate(parse_predicate(pair));
            let expect = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(None)),
                "drop" => Statement::Predicate(Predicate::Drop(None)),
                "use" => Statement::Predicate(Predicate::Use(None)),
                "read" => Statement::Predicate(Predicate::Read(None)),
                "write" => Statement::Predicate(Predicate::Write(None)),
                "assign" => Statement::Predicate(Predicate::Assign(None)),
                _ => unreachable!(),
            };
            assert_eq!(stmt, expect);

            // 2. Empty parentheses, like alloc()
            let input = format!("{}()", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt2 = Statement::Predicate(parse_predicate(pair));
            assert_eq!(stmt2, expect);

            // 3. Single variable argument like alloc(x)
            let input = format!("{}(x)", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt3 = Statement::Predicate(parse_predicate(pair));
            let expect3 = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string())))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::Var("x".to_string())))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::Var("x".to_string())))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string())))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string())))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::Var("x".to_string())))),
                _ => unreachable!(),
            };
            assert_eq!(stmt3, expect3);

            // 4. Field access argument like alloc(x.y)
            let input = format!("{}(x.y)", p);
            let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
            let stmt4 = Statement::Predicate(parse_predicate(pair));
            let expect4 = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                _ => unreachable!(),
            };
            assert_eq!(stmt4, expect4);
        }
    }


    #[test]
    fn test_wildcard() {
        let pair = CQPLParser::parse(Rule::wildcard, "*").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt, Statement::Wildcard);
    }

    #[test]
    fn test_double_negation() {
        let pair = CQPLParser::parse(Rule::not_expr, "!!drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt, Statement::Not(Box::new(
                                                Statement::Not(
                                                                Box::new(Statement::Predicate(Predicate::Drop(None)))))));
    }

    #[test]
    fn test_quantifier_no_var() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\forall alloc").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt,Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Any),
                cond: Box::new(Statement::Predicate(Predicate::Alloc(None)))
            }
        );
    }

    #[test]
    fn test_quantifier_with_in_fields() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\exists x in Vec.fields && drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::Exists,
                var: Some(VarName::Named("x".to_string())),
                cond: Box::new(
                    Statement::And(
                        Box::new(Statement::Predicate(Predicate::InFields(Type::Vec))),
                        Box::new(Statement::Predicate(Predicate::Drop(None)))
                    )
                )
            }
        );
    }

    #[test]
    fn test_and_expression() {
        let pair = CQPLParser::parse(Rule::and_expr, "read && write").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::And(
                Box::new(Statement::Predicate(Predicate::Read(None))),
                Box::new(Statement::Predicate(Predicate::Write(None)))
            )
        );
    }


    #[test]
    fn test_and_expression_with_var() {
        let pair = CQPLParser::parse(Rule::and_expr, "read(x) && write(x)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::And(
                Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string()))))),
            )
        );
    }


    #[test]
    fn test_or_expression() {
        let pair = CQPLParser::parse(Rule::or_expr, "read || write").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Or(
                Box::new(Statement::Predicate(Predicate::Read(None))),
                Box::new(Statement::Predicate(Predicate::Write(None)))
            )
        );
    }

     #[test]
    fn test_or_expression_with_var() {
        let pair = CQPLParser::parse(Rule::or_expr, "read(x) || write(x)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Or(
                Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string()))))),
                Box::new(Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string()))))),
            )
        );
    }


    #[test]
    fn test_parentheses_and_not() {
        let pair = CQPLParser::parse(Rule::not_expr, "!(read && write)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt, Statement::Not(
                                        Box::new(Statement::And(
                                                                Box::new(Statement::Predicate(Predicate::Read(None))),
                                                                Box::new(Statement::Predicate(Predicate::Write(None)))))));

    }

    #[test]
    fn test_parentheses_or_not() {
        let pair = CQPLParser::parse(Rule::not_expr, "!(read || write)").unwrap().next().unwrap();
        println!("PAIR {:#?}", pair);
        let stmt = parse_statement(pair);
        println!("stmnt aka parsed pair {:#?}", stmt);

        assert_eq!(stmt, Statement::Not(
                                        Box::new(Statement::Or(
                                                                Box::new(Statement::Predicate(Predicate::Read(None))),
                                                                Box::new(Statement::Predicate(Predicate::Write(None)))))));

    }

    #[test]
    fn test_invalid_type_in_fields() {
        let result = std::panic::catch_unwind(|| {
            let pair = CQPLParser::parse(Rule::quant_expr, "\\exists y in Int.fields").unwrap().next().unwrap();
            parse_statement(pair);
         });

        assert!(result.is_err()); // only tup, array, vec ecc..
    }


    /// TEST ALLOCTORS
     #[test]
    fn test_allocator_predicates() {
        let languages = vec![("rust", Language::Rust), ("c", Language::C)];
        let allocator_types = vec![
            ("default", AllocatorType::Default),
            ("jemalloc", AllocatorType::Jemalloc),
            ("mimalloc", AllocatorType::Mimalloc),
            ("rpmalloc", AllocatorType::Rpmalloc),
            ("snmalloc", AllocatorType::Snmalloc),
            ("weealloc", AllocatorType::Weealloc),
            ("dlmalloc", AllocatorType::Dlmalloc),
        ];

        for (lang_str, lang_enum) in &languages {
            for (alloc_str, alloc_enum) in &allocator_types {
                let input = format!("allocator({}, {})", lang_str, alloc_str);
                let pair = CQPLParser::parse(Rule::predicate, &input).unwrap().next().unwrap();
                let stmt = parse_statement(pair);

                assert_eq!(stmt, Statement::Predicate(Predicate::Allocator(lang_enum.clone(), alloc_enum.clone())), "Failed for input: {}", input);
            }
        }
    }

    #[test]
    #[should_panic(expected = "Allocator predicate requires 2 arguments")]
    fn test_allocator_wrong_number_of_args() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(rust)").unwrap().next().unwrap();
        parse_statement(pair); //  panic -> only one argument provided
    }

    #[test]
    #[should_panic(expected = "Unknown language")]
    fn test_allocator_unknown_language() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(java, jemalloc)").unwrap().next().unwrap();
        parse_statement(pair); // panic because is invalid
    }

    #[test]
    #[should_panic(expected = "Unknown allocator type")]
    fn test_allocator_unknown_type() {
        let pair = CQPLParser::parse(Rule::predicate, "allocator(rust, unknown)").unwrap().next().unwrap();
        parse_statement(pair); // panic -> unknown is invalid
    }

    /// COMMUTATIVITY AND
    #[test]
    fn test_and_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write && read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    /// COMMUTATIVITY OR
    #[test]
    fn test_or_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write || read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    /// ASSOCIATIVITY AND
    #[test]
    fn test_and_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write && assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) && assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    /// ASSOCIATIVITY OR
    #[test]
    fn test_or_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read || write) || assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    /// DISTRIBUTIVITY
    #[test]
    fn test_distributivity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) || (read && assign)").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    /// DE MORGAN
    #[test]
    fn test_de_morgan_and() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read && write)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read || !write").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }


    #[test]
    fn test_de_morgan_or() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read || write)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read && !write").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    //Test quantifiers:
    
    #[test]
    fn test_quantifier_and_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x read(x) && write(x)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x write(x) && read(x)").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }



    #[test]
    fn test_quantifier_and_commutativity_t2() {
        let stmt1 = parse_statement(
            CQPLParser::parse(Rule::and_expr, "\\forall alloc && read").unwrap().next().unwrap());

        let stmt2 = parse_statement(
            CQPLParser::parse(Rule::and_expr, "\\forall read && alloc").unwrap().next().unwrap());

        let preds = vec![Predicate::Read(None), Predicate::Alloc(None)];
        assert!(semantically_eq(&stmt1, &stmt2, &preds));
    }

    #[test]
    fn test_quantifier_forall_no_var() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\forall alloc").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Any),
                cond: Box::new(Statement::Predicate(Predicate::Alloc(None)))
            }
        );
    }

    #[test]
    fn test_quantifier_exists_with_var_and_type() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\exists x in Vec.fields && drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::Exists,
                var: Some(VarName::Named("x".to_string())),
                cond: Box::new(Statement::And(
                    Box::new(Statement::Predicate(Predicate::InFields(Type::Vec))),
                    Box::new(Statement::Predicate(Predicate::Drop(None)))
                ))
            }
        );
    }

    #[test]
    fn test_quantifier_with_predicate_arg() {
        let pair = CQPLParser::parse(Rule::logic_expr, "\\forall read(x)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Any),
                cond: Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string())))))
            }
        );
    }


    #[test]
    fn test_quantifier_and_expression() {
        let pair = CQPLParser::parse(Rule::and_expr, "\\forall alloc && read").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        let expected = Statement::And(
            Box::new(Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Any),
                cond: Box::new(Statement::Predicate(Predicate::Alloc(None))),
            }),
            Box::new(Statement::Predicate(Predicate::Read(None))),
        );

        assert_eq!(stmt, expected);
    }

    #[test]
    fn test_nested_quantifiers() {
        let pair = CQPLParser::parse(Rule::and_expr, "\\forall x in Vec.fields && \\exists y in Struct.fields && read(y)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        let expected = Statement::And(
            Box::new(Statement::Quantified {
                quant: Quantifier::ForAll,
                var: Some(VarName::Named("x".to_string())),
                cond: Box::new(Statement::Predicate(Predicate::InFields(Type::Vec))),
            }),
            Box::new(Statement::Quantified {
                quant: Quantifier::Exists,
                var: Some(VarName::Named("y".to_string())),
                cond: Box::new(Statement::And(
                    Box::new(Statement::Predicate(Predicate::InFields(Type::Struct))),
                    Box::new(Statement::Predicate(Predicate::Read(Some(Term::Var("y".to_string()))))),
                )),
            }),
        );

        assert_eq!(stmt, expected);
    }

    #[test]
    #[should_panic(expected = "Type 'int' is not allowed for .fields")]
    fn test_invalid_type_in_fields_panics() {
        let pair = CQPLParser::parse(Rule::quant_expr, "\\exists y in Int.fields").unwrap().next().unwrap();
        parse_statement(pair); // should panic
    }

*/




/* 
   fn dummy_eval(_pred: &Predicate, _vars: &std::collections::HashMap<String, String>) -> bool {
    true
    }
    
   
    #[test]
    fn test_and_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write && read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_or_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || write").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write || read").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_and_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write && assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) && assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_or_associativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read || (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read || write) || assign").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_distributivity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read && (write || assign)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read && write) || (read && assign)").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None), Predicate::Assign(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_de_morgan_and() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read && write)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read || !write").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_de_morgan_or() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read || write)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read && !write").unwrap().next().unwrap());
        let preds = vec![Predicate::Read(None), Predicate::Write(None)];
        let vars = vec![];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }
   
    #[test]
    fn test_and_commutativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write(y) && read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_or_commutativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(a) || write(b)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "write(b) || read(a)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("a".to_string()))),
            Predicate::Write(Some(Term::Var("b".to_string()))),
        ];
        let vars = vec!["a".to_string(), "b".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_and_associativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && (write(y) && assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) && write(y)) && assign(z)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }



    #[test]
    fn test_or_associativity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) || (write(y) || assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) || write(y)) || assign(z)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_distributivity_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "read(x) && (write(y) || assign(z))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(read(x) && write(y)) || (read(x) && assign(z))").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
            Predicate::Assign(Some(Term::Var("z".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_de_morgan_and_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read(x) && write(y))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read(x) || !write(y)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_de_morgan_or_named_vars() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!(read(x) || write(y))").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!read(x) && !write(y)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_forall_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\forall y. read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall y. \\forall x. write(y) && read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }


    #[test]
    fn test_exists_commutativity() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists x. \\exists y. read(x) || write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists y. \\exists x. write(y) || read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_nested_quantifiers() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\exists y. read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\exists y. write(y) && read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_quantifier_distributivity_and() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. read(x) && write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(\\forall x. read(x)) && write(y)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_quantifier_negation() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!\\forall x. read(x)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists x. !read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
        ];
        let vars = vec!["x".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_quantifier_negation_exists() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "!\\exists y. write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall y. !write(y)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_quantifiers_with_logical_ops() {
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. read(x) || \\exists y. write(y)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "(\\forall x. read(x)) || (\\exists y. write(y))").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
            Predicate::Write(Some(Term::Var("y".to_string()))),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        assert!(semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));
    }

    #[test]
    fn test_quantifier_variable_shadowing() {
        // tests that forall x. exists x. read(x) != exists x. forall x. read(x)
        let stmt1 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\forall x. \\exists x. read(x)").unwrap().next().unwrap());
        let stmt2 = parse_statement(CQPLParser::parse(Rule::logic_expr, "\\exists x. \\forall x. read(x)").unwrap().next().unwrap());
        let preds = vec![
            Predicate::Read(Some(Term::Var("x".to_string()))),
        ];
        let vars = vec!["x".to_string()];
        // these are NOT semantically equal because quantifier order and scopes differ
        assert!(!semantically_eq(&stmt1, &stmt2, &preds, &vars, &dummy_eval));

    }*/