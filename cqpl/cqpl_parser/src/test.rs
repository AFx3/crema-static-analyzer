#[cfg(test)]
mod tests {
    use pest::Parser;
    use crate::ast::*;

    fn parse_predicate(input: &str) -> Statement {
        let pair = CQPLParser::parse(Rule::predicate, input).unwrap().next().unwrap();
        parse_statement(pair)
    }

     #[test]
    fn test_predicates() {
        let predicates = ["alloc", "use", "read", "write", "assign"];
        
        for &p in predicates.iter() {
            // predicates with no ()
            let stmt = parse_predicate(p);
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

            // predicate with ()
            let stmt = parse_predicate(&format!("{}()", p));
            assert_eq!(stmt, expect);


            //  predicate with an x var
            let stmt = parse_predicate(&format!("{}(x)", p));
            let expect = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".to_string())))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::Var("x".to_string())))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::Var("x".to_string())))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::Var("x".to_string())))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::Var("x".to_string())))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::Var("x".to_string())))),
                _ => unreachable!(),
            };
            assert_eq!(stmt, expect);


            // predicate with field access
            let stmt = parse_predicate(&format!("{}(x.y)", p));
            let expect = match p {
                "alloc" => Statement::Predicate(Predicate::Alloc(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "drop" => Statement::Predicate(Predicate::Drop(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "use" => Statement::Predicate(Predicate::Use(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "read" => Statement::Predicate(Predicate::Read(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "write" => Statement::Predicate(Predicate::Write(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                "assign" => Statement::Predicate(Predicate::Assign(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
               // "allocator" => Statement::Predicate(Predicate::Allocator(Some(Term::FieldAccess { base: "x".to_string(), field: "y".to_string() }))),
                _ => unreachable!(),
            };
            assert_eq!(stmt, expect);
        }         
    }



    #[test]
    fn test_predicate_drop() {
        let pair = CQPLParser::parse(Rule::predicate, "drop").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(stmt, Statement::Predicate(Predicate::Drop(None)));
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


    ///// TEST ALLOCTORS
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






}
