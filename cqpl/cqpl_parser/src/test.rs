#[cfg(test)]
mod tests {
    use pest::Parser;
    use crate::ast::*;

    #[test]
    fn test_predicate_alloc() {
        let pair = CQPLParser::parse(Rule::predicate, "alloc").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert_eq!(
            stmt,
            Statement::Predicate(Predicate::Alloc(None))
        );
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
    fn test_parentheses_and_not() {
        let pair = CQPLParser::parse(Rule::not_expr, "!(read && write)").unwrap().next().unwrap();
        let stmt = parse_statement(pair);

        assert!(matches!(stmt, Statement::Not(_)));
    }

    #[test]
    fn test_invalid_type_in_fields() {
        let result = std::panic::catch_unwind(|| {
            let pair = CQPLParser::parse(Rule::quant_expr, "\\exists y in Int.fields").unwrap().next().unwrap();
            parse_statement(pair);
        });

        assert!(result.is_err());
    }
}
