use crate::ast::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisKind {
    MemoryLeak,
    UseAfterFree,
    DoubleFree,
    Unknown,
}

/// Evaluates a user-defined CQPL rule to infer which analysis kind to run
pub fn infer_analysis_kind(rule: &RuleDef) -> AnalysisKind {
    if rule.domain != Domain::Memory {
        return AnalysisKind::Unknown;
    }
    let src = rule
        .taint_src
        .iter()
        .flat_map(|b| b.statements.iter())
        .find(|s| matches!(s, Statement::Predicate(Predicate::Alloc(_))));

    if src.is_none() {
        return AnalysisKind::Unknown;
    }
    // Collect all sink predicates (including negated ones)
    let mut snk_preds: Vec<Predicate> = Vec::new();
    for b in &rule.taint_snk {
        for stmt in &b.statements {
            match stmt {
                Statement::Predicate(p) => snk_preds.push(p.clone()),
                Statement::Not(inner) => {
                    if let Statement::Predicate(p) = &**inner {
                        snk_preds.push(p.clone());
                    }
                }
                _ => {}
            }
        }
    }
    // classify pattern
    match snk_preds.as_slice() {
        // alloc(x) |> !drop(x)
        [Predicate::Drop(_)] if has_not_drop(&rule.taint_snk) => {
            AnalysisKind::MemoryLeak
        }
        // alloc(x) |> drop(x) |> drop(x)
        [Predicate::Drop(_), Predicate::Drop(_)] => {
            AnalysisKind::DoubleFree
        }
        // alloc(x) |> drop(x) |> use(x)
        [Predicate::Drop(_), Predicate::Use(_)] => {
            AnalysisKind::UseAfterFree
        }
        _ => AnalysisKind::Unknown,
    }
}
/// Ccheck if the sink contains a negated Drop predicate
fn has_not_drop(blocks: &[TaintBlock]) -> bool {
    blocks.iter().any(|b| {
        b.statements.iter().any(|s| matches!(
            s,
            Statement::Not(inner)
            if matches!(&**inner, Statement::Predicate(Predicate::Drop(_)))
        ))
    })
}




/// Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    fn mk_rule(name: &str, src_stmts: Vec<Statement>, snk_stmts: Vec<Statement>) -> RuleDef {
        RuleDef {
            name: vec![name.into()],
            domain: Domain::Memory,
            variables: vec![],
            taint_src: vec![TaintBlock {
                kind: BlockKind::Src,
                statements: src_stmts,
                next_op: None,
            }],
            taint_snk: vec![TaintBlock {
                kind: BlockKind::Snk,
                statements: snk_stmts,
                next_op: None,
            }],
        }
    }

    #[test]
    fn test_memory_leak() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![Statement::Not(Box::new(
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into()))))))];
        let rule = mk_rule("mem_leak", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::MemoryLeak);
    }

    #[test]
    fn test_double_free() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into()))))];
        let rule = mk_rule("double_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::DoubleFree);
    }

    #[test]
    fn test_use_after_free() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))))];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }
}
