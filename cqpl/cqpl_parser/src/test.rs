
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



/* 
#[cfg(test)]
mod tests {
    use pest::Parser;
    use crate::ast::*;
 
    

    /// A simple evaluation function for tests: 
    /// returns true for all predicates without a variable, and
    /// returns true if the variable is bound in env (stub logic).
    fn simple_eval(pred: &Predicate, env: &Env) -> bool {
        match pred {
            Predicate::Alloc(None)
            | Predicate::Drop(None)
            | Predicate::Use(None)
            | Predicate::Read(None)
            | Predicate::Write(None)
            | Predicate::Assign(None)
            | Predicate::InFields(_)
            | Predicate::Allocator(_, _) => {
                // predicates with no variable or special ones: accept
                true
            }
            Predicate::Alloc(Some(term))
            | Predicate::Drop(Some(term))
            | Predicate::Use(Some(term))
            | Predicate::Read(Some(term))
            | Predicate::Write(Some(term))
            | Predicate::Assign(Some(term)) => {
                match term {
                    Term::Var(name) => env.get(name).is_some(),
                    Term::FieldAccess { base, .. } => env.get(base).is_some(),
                    Term::Literal(_) => todo!("Handle literal terms"),

                }
            }
            // fallback
            _ => false,
        }
    }
*/




/* 
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
        assert_eq!(
            stmt,
            Statement::Not(Box::new(
                Statement::Not(Box::new(Statement::Predicate(Predicate::Drop(None))))
            ))
        );
    }

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

    }
*/

}