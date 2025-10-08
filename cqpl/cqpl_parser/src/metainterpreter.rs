use crate::ast::*;
use once_cell::sync::OnceCell;
use std::sync::Mutex;

/* 
Semantic inference of Memory domain
Atomatically checks if a rule is a Mem Leak, DF or UAF
*/

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisKind {
    MemoryLeak,
    UseAfterFree,
    DoubleFree,
    Unknown,            // no corrispondence
}

/// Define Global Rules Registry (uaf, df, ml) that infer_analysis_kind can consult
static ALL_RULES: OnceCell<Mutex<Vec<RuleDef>>> = OnceCell::new();  // use OnceCell for inti only 1 time the global list Vec<RuleDef>. Mutex to be thead safe

/// Function to register the set of rules to be consulted by infer_analysis_kind to compare the rules
pub fn register_rules_for_inference(rules: &[RuleDef]) {
    let vec = rules.to_vec();
    let _ = ALL_RULES.get_or_init(|| Mutex::new(vec));
}

/// Function to combine rules taint_snk into a single statement by chaining statements with Then (unitl operator) and preserving Not wrappers
/// If the rule has many blocks, the statements are taken in order
fn sink_to_statement(rule: &RuleDef) -> Option<Statement> {
    let mut stmts: Vec<Statement> = Vec::new();
    for block in &rule.taint_snk {
        for s in &block.statements {
            stmts.push(s.clone());
        }
    }
    if stmts.is_empty() {
        None
    } else {
        // chain with Then: s1 |> s2 |> s3 => Then(Then(s1, s2), s3)
        let mut iter = stmts.into_iter();
        let first = iter.next().unwrap();
        let combined = iter.fold(first, |acc, s| Statement::Then(Box::new(acc), Box::new(s)));
        Some(combined)
    }
}

/// Predicate evaluator to be used by semantically_eq.
/// Simple deterministic mapping to bool
fn deterministic_pred_eval(pred: &Predicate, env: &Env) -> bool {
    use std::fmt::Write;
    let mut s = String::new();
    // predicate kind and optional term
    match pred {
        Predicate::Alloc(t) => {
            write!(&mut s, "alloc:{:?}", t).ok();
        }
        Predicate::Drop(t) => {
            write!(&mut s, "drop:{:?}", t).ok();
        }
        Predicate::Use(t) => {
            write!(&mut s, "use:{:?}", t).ok();
        }
        Predicate::Read(t) => {
            write!(&mut s, "read:{:?}", t).ok();
        }
        Predicate::Write(t) => {
            write!(&mut s, "write:{:?}", t).ok();
        }
        Predicate::Assign(t) => {
            write!(&mut s, "assign:{:?}", t).ok();
        }
        Predicate::Allocator(lang, at) => {
            write!(&mut s, "allocator:{:?}:{:?}", lang, at).ok();
        }
        Predicate::InFields(t) => {
            write!(&mut s, "infields:{:?}", t).ok();
        }
        Predicate::Custom(name, terms) => {
            write!(&mut s, "custom:{}:{:?}", name, terms).ok();
        }
        Predicate::OwnForg(t) => {
            write!(&mut s, "ownforg:{:?}", t).ok();
        }
        Predicate::OwnBack(t) => {
            write!(&mut s, "ownback:{:?}", t).ok();
        }
    }
    // append env sorted keys for determinism
    let mut keys: Vec<_> = env.keys().cloned().collect();
    keys.sort();
    for k in keys {
        if let Some(v) = env.get(&k) {
            write!(&mut s, "|{}={}", k, v).ok();
        }
    }
    let sum: usize = s.bytes().map(|b| b as usize).sum();
    sum % 2 == 0
}

/// Evaluate simple syntactic classification for a rule. This function does not consult other rules
fn syntactic_infer(rule: &RuleDef) -> AnalysisKind {
    if rule.domain != Domain::Memory {
        return AnalysisKind::Unknown;
    }

    // require that the source contains an allocation (supports quantified allocs)
    if !rule_has_alloc_src(rule) {
        return AnalysisKind::Unknown;
    }

    // collect all snk predicates (including negated ones)
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

    // classify pattern match
    match snk_preds.as_slice() {
        [Predicate::Drop(_)] if has_not_drop(&rule.taint_snk) => AnalysisKind::MemoryLeak,
        [Predicate::Drop(_), Predicate::Drop(_)] => AnalysisKind::DoubleFree,
        [Predicate::Drop(_), Predicate::Use(_)] => AnalysisKind::UseAfterFree,
        [Predicate::Drop(_), Predicate::Read(_)] => AnalysisKind::UseAfterFree,
        [Predicate::Drop(_), Predicate::Write(_)] => AnalysisKind::UseAfterFree,
        _ => AnalysisKind::Unknown,
    }
}


/// Recursively check whether a Statement (or any nested sub-statement) contains an Alloc predicate.
fn statement_contains_alloc(stmt: &Statement) -> bool {
    match stmt {
        // direct predicate alloc
        Statement::Predicate(Predicate::Alloc(_)) => true,

        // Not(inner) — check inside
        Statement::Not(inner) => statement_contains_alloc(&*inner),

        // Or / Then: check both sides
        Statement::Or(left, right) | Statement::Then(left, right) => {
            statement_contains_alloc(&*left) || statement_contains_alloc(&*right)
        }

        // Quantified { cond: Box<Statement>, ... } — check the condition
        Statement::Quantified { cond, .. } => statement_contains_alloc(&*cond),

        // handle other statement kinds that may contain nested statements
        _ => false,
    }
}

/// Return true if the rule's taint_src contains an Alloc predicate (recursively).
fn rule_has_alloc_src(rule: &RuleDef) -> bool {
    rule.taint_src
        .iter()
        .flat_map(|b| b.statements.iter())
        .any(|s| statement_contains_alloc(s))
}



/// Flatten a Statement built from Then(...) into a Vec of Statements in left-to-right order
/// For non-Then nodes push the node itself (so Not(Predicate(...)) is preserved)
fn statement_seq(stmt: &Statement, out: &mut Vec<Statement>) {
    match stmt {
        Statement::Then(left, right) => {
            statement_seq(left, out);
            statement_seq(right, out);
        }
        // For or/not/predicate ... keep the node as-is so Not(...) is preserved
        other => out.push(other.clone()),
    }
}

/// Build an ordered Vec<Statement> representing the snk of a rule (in block/statement order)
fn sink_sequence_for_rule(rule: &RuleDef) -> Vec<Statement> {
    // collect all stmnts in taint_snk in order, then if we have multiple build Then chain
    let mut stmts: Vec<Statement> = Vec::new();
    for b in &rule.taint_snk {
        for s in &b.statements {
            stmts.push(s.clone());
        }
    }
    if stmts.is_empty() {
        return Vec::new();
    }
    // if already have a single stmnt that is a |> chain, flatten it;
    // othw treat the list as the ordered sequence directly
    // convert the explicit vector into a unified Then chain and then flatten it deterministically
    let mut iter = stmts.into_iter();
    let first = iter.next().unwrap();
    let combined = iter.fold(first, |acc, s| Statement::Then(Box::new(acc), Box::new(s)));
    let mut seq = Vec::new();
    statement_seq(&combined, &mut seq);
    seq
}

pub fn infer_analysis_kind(rule: &RuleDef) -> AnalysisKind {
    // if domain is not Memory -> Unknown analysis for the moment
    if rule.domain != Domain::Memory {
        return AnalysisKind::Unknown;
    }

    // gestisco direttamente OR: provo ciascun ramo
    if let Some(k) = try_or_branches(rule) {
        return k;
    }

    // if the rule does not have an Alloc in its source, don't try to classify it
    if !rule_has_alloc_src(rule) {
        return AnalysisKind::Unknown;
    }

    // 1st try syntactic inference
    let kind = syntactic_infer(rule);
    if kind != AnalysisKind::Unknown {
        return kind;
    }

    // if unknown, try to consult registered rules (if any)
    if let Some(mutex) = ALL_RULES.get() {
        let rules = mutex.lock().expect("ALL_RULES lock poisoned");

        // build the sink statement for the target rule
        let target_snk_stmt = match sink_to_statement(rule) {
            Some(s) => s,
            None => return AnalysisKind::Unknown,
        };

        // precompute ordered sequence for structural comparison
        let seq_target = sink_sequence_for_rule(rule);

        // iterate other rules and try to find a semantically equivalent snk
        for other in rules.iter() {
            // skip self (compare names or pointer equality semantically)
            if &other.name == &rule.name && other.domain == rule.domain && other.variables == rule.variables {
                continue;
            }
            // only consider memory-domain rules (classification here is for memory errors)
            if other.domain != Domain::Memory {
                continue;
            }

            let other_snk_stmt = match sink_to_statement(other) {
                Some(s) => s,
                None => continue,
            };

            // structural check: require same # atomic statements and same ordering of predicate/negation nodes before attempting semantic equivalence.
            let seq_other = sink_sequence_for_rule(other);
            if seq_target.len() != seq_other.len() {
                continue;
            }
            // compare shape/ kinds preserving Not wrappers, compare the discriminator of each stmnt (Predicate variant or Not(Predicate variant)), this enforces order
            fn stmt_kind_key(s: &Statement) -> String {
                match s {
                    Statement::Predicate(p) => format!("{:?}", predicate_kind(p)),
                    Statement::Not(inner) => match &**inner {
                        Statement::Predicate(p) => format!("!{:?}", predicate_kind(p)),
                        other => format!("!OTHER({:?})", other),
                    },
                    Statement::Or(_, _) => "Or".into(),
                    Statement::Then(_, _) => "Then".into(),
                    other => format!("Other({:?})", other),
                }
            }
            // extract only the predicate variant (ignoring terms) for comparison
            fn predicate_kind(p: &Predicate) -> &'static str {
                match p {
                    Predicate::Alloc(_) => "Alloc",
                    Predicate::Drop(_) => "Drop",
                    Predicate::Use(_) => "Use",
                    Predicate::Read(_) => "Read",
                    Predicate::Write(_) => "Write",
                    Predicate::Assign(_) => "Assign",
                    Predicate::Allocator(_, _) => "Allocator",
                    Predicate::InFields(_) => "InFields",
                    Predicate::Custom(_, _) => "Custom",
                    Predicate::OwnForg(_) => "OwnForg",
                    Predicate::OwnBack(_) => "OwnBack",
                }
            }
            let mut shape_mismatch = false;
            for (a, b) in seq_target.iter().zip(seq_other.iter()) {
                if stmt_kind_key(a) != stmt_kind_key(b) {
                    shape_mismatch = true;
                    break;
                }
            }
            if shape_mismatch {
                continue;
            }
            // variables and possible values to use for semantic check
            let vars = if other.variables.is_empty() { &rule.variables } else { &other.variables };
            let possible_values = vec!["A".to_string(), "B".to_string()];

            // use the ast::semantically_eq helper with deterministic eval fn
            let eval_fn = |p: &Predicate, env: &Env| -> bool { deterministic_pred_eval(p, env) };

            if semantically_eq(&target_snk_stmt, &other_snk_stmt, &[], vars, &possible_values, &eval_fn) {
                // found semantically equivalent sink; use syntactic inference on the other rule
                let other_kind = syntactic_infer(other);
                if other_kind != AnalysisKind::Unknown {
                    return other_kind;
                }
                // else continue searching
            }
        }
    }

    AnalysisKind::Unknown
}

/// If the sink is a single OR expression, try to infer each branch separately.
/// This lets classify cases like !drop(x) || use(x) as either MemoryLeak or UseAfterFree.
fn try_or_branches(rule: &RuleDef) -> Option<AnalysisKind> {
    // only handle a single OR statement at the top level of the sink
    if rule.taint_snk.len() == 1 && rule.taint_snk[0].statements.len() == 1 {
        if let Statement::Or(lhs, rhs) = &rule.taint_snk[0].statements[0] {
            // branch 1
            let mut left_rule = rule.clone();
            left_rule.taint_snk = vec![TaintBlock {
                kind: BlockKind::Snk,
                statements: vec![(*lhs.clone())],
                next_op: None,
            }];
            let left_kind = infer_analysis_kind(&left_rule);
            if left_kind != AnalysisKind::Unknown {
                return Some(left_kind);
            }
            // branch 2
            let mut right_rule = rule.clone();
            right_rule.taint_snk = vec![TaintBlock {
                kind: BlockKind::Snk,
                statements: vec![(*rhs.clone())],
                next_op: None,
            }];
            let right_kind = infer_analysis_kind(&right_rule);
            if right_kind != AnalysisKind::Unknown {
                return Some(right_kind);
            }
        }
    }
    None
}

/// Check if the sink contains a negated Drop predicate
fn has_not_drop(blocks: &[TaintBlock]) -> bool {
    blocks.iter().any(|b| {
        b.statements.iter().any(|s| {
            matches!(
                s,
                Statement::Not(inner) if matches!(&**inner, Statement::Predicate(Predicate::Drop(_)))
            )
        })
    })
}

/// TEST
#[cfg(test)]
mod tests {
    use super::*;


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
        let snk = vec![Statement::Not(Box::new(Statement::Predicate(
            Predicate::Drop(Some(Term::Var("x".into()))),
        )))];
        let rule = mk_rule("mem_leak", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::MemoryLeak);
    }

    #[test]
    fn test_double_free() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
        ];
        let rule = mk_rule("double_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::DoubleFree);
    }

    #[test]
    fn test_uaf() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Use(Some(Term::Var("x".into())))),
        ];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }

    #[test]
    fn test_uaf2() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Read(Some(Term::Var("x".into())))),
        ];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }

    #[test]
    fn test_uaf3() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Write(Some(Term::Var("x".into())))),
        ];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }

    // test demonstrating semantic equivalence lookup
    #[test]
    fn test_semantic_equivalence_lookup() {
        // rule A: uses a custom syntactic form that is semantically equivalent to drop(x) |> use(x)
        let src_a = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        // syntactically different but should be semantically equivalent as a pattern
        let snk_a = vec![
            // e.g. some pattern that composes into same logical thing - here we keep simple for the test
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Use(Some(Term::Var("x".into())))),
        ];
        let rule_a = mk_rule("rule_a", src_a, snk_a);

        // rule B: another rule but identical semantics (will act as the registered one)
        let src_b = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("y".into()))))];
        let snk_b = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("y".into())))),
            Statement::Predicate(Predicate::Read(Some(Term::Var("y".into())))),
        ];
        let rule_b = mk_rule("rule_b", src_b, snk_b);

        // register both rules for inference
        register_rules_for_inference(&[rule_a.clone(), rule_b.clone()]);

        // syntactic inference on rule_a is UseAfterFree (direct), so result should be UAF
        assert_eq!(infer_analysis_kind(&rule_a), AnalysisKind::UseAfterFree);

        // but even if rule_b had a slightly different sink shape,
        // infer_analysis_kind(&rule_b) should be UseAfterFree as well
        assert_eq!(infer_analysis_kind(&rule_b), AnalysisKind::UseAfterFree);
    }

       

    #[test]
    fn test_equivalence_or_condition() {
        // construct an explicit or (read(x) || write(x))
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk_read = Statement::Predicate(Predicate::Read(Some(Term::Var("x".into()))));
        let snk_write = Statement::Predicate(Predicate::Write(Some(Term::Var("x".into()))));
        let snk_or = Statement::Or(Box::new(snk_read.clone()), Box::new(snk_write.clone()));

        let rule_or = RuleDef {
            name: vec!["or_sink".into()],
            domain: Domain::Memory,
            variables: vec![], // <- empty
            taint_src: vec![TaintBlock {
                kind: BlockKind::Src,
                statements: src.clone(),
                next_op: None,
            }],
            taint_snk: vec![TaintBlock {
                kind: BlockKind::Snk,
                statements: vec![snk_or.clone()],
                next_op: None,
            }],
        };

        let rule_read = mk_rule("read_sink", src.clone(), vec![snk_read]);
        let rule_write = mk_rule("write_sink", src, vec![snk_write]);

        // check that the OR version is semantically eq to each of its branches
        let eval_fn = |p: &Predicate, env: &Env| deterministic_pred_eval(p, env);
        let possible_values = vec!["A".to_string(), "B".to_string()];

        let s_or = sink_to_statement(&rule_or).unwrap();
        let s_read = sink_to_statement(&rule_read).unwrap();
        let s_write = sink_to_statement(&rule_write).unwrap();

        let eq1 = semantically_eq(&s_or, &s_read, &[], &rule_or.variables, &possible_values, &eval_fn);
        let eq2 = semantically_eq(&s_or, &s_write, &[], &rule_or.variables, &possible_values, &eval_fn);

        assert!(eq1 || eq2, "Expected (read(x) || write(x)) to be equivalent to at least one branch");
    }

    #[test]
    fn test_unknown_domain_is_ignored() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))))];
        let mut rule = mk_rule("non_memory", src, snk);
        rule.domain = Domain::General; // not a memory domain
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::Unknown);
    }

    /// NNNN
    #[test]
    fn test_no_alloc_in_src_is_unknown() {
        let src = vec![Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))))];
        let snk = vec![Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into()))))];
        let rule = mk_rule("no_alloc", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::Unknown);
    }

    #[test]
    fn test_not_drop_without_alloc_is_not_leak() {
        // !drop(x) but no alloc -> should NOT count as leak
        let src = vec![];
        let snk = vec![Statement::Not(Box::new(Statement::Predicate(
            Predicate::Drop(Some(Term::Var("x".into()))),
        )))];
        let rule = mk_rule("not_drop_no_alloc", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::Unknown);
    }

    #[test]
    fn not_semantic_equivalence_then_commutation() {
        // drop(x) |> read(x) vs read(x) |> drop(x) — ordine invertito -> NON equivalenti
        let src1 = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk1 = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Read(Some(Term::Var("x".into())))),
        ];
        let src2 = src1.clone();
        let snk2 = vec![
            Statement::Predicate(Predicate::Read(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
        ];

        let rule1 = mk_rule("r1", src1, snk1);
        let rule2 = mk_rule("r2", src2, snk2);
        register_rules_for_inference(&[rule1.clone(), rule2.clone()]);

        // dato che Then è sequenziale come Until, la proprietà commutativa non vale
        assert_eq!(infer_analysis_kind(&rule2), AnalysisKind::Unknown);
    }


    #[test]
    fn test_semantic_equivalence_with_not_and_or() {
        // !drop(x) || use(x) should be mem leak
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let drop_stmt = Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into()))));
        let not_drop = Statement::Not(Box::new(drop_stmt.clone()));
        let use_stmt = Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))));
        let snk = vec![Statement::Or(Box::new(not_drop.clone()), Box::new(use_stmt.clone()))];

        let rule = mk_rule("or_notdrop_use", src.clone(), snk);
        let leak_rule = mk_rule("leak", src.clone(), vec![not_drop.clone()]);
        let uaf_rule = mk_rule("uaf", src.clone(), vec![
            drop_stmt.clone(),
            use_stmt.clone()
        ]);

        register_rules_for_inference(&[leak_rule.clone(), uaf_rule.clone(), rule.clone()]);

        // should be recognized as at least one of the known ones
        let inferred = infer_analysis_kind(&rule);
        assert!(inferred == AnalysisKind::MemoryLeak, "Expected OR condition to map to known mem leak kind");
    }


    #[test]
    fn test_semantic_equivalence_with_and() {
        // !drop(x) && use(x) should NOT be mem leak
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let drop_stmt = Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into()))));
        let not_drop = Statement::Not(Box::new(drop_stmt.clone()));
        let use_stmt = Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))));
        let snk = vec![Statement::And(Box::new(not_drop.clone()), Box::new(use_stmt.clone()))];

        let rule = mk_rule("AND_notdrop_use", src.clone(), snk);
        let leak_rule = mk_rule("leak", src.clone(), vec![not_drop.clone()]);
        let uaf_rule = mk_rule("uaf", src.clone(), vec![
            drop_stmt.clone(),
            use_stmt.clone()
        ]);

        register_rules_for_inference(&[leak_rule.clone(), uaf_rule.clone(), rule.clone()]);

        // should be recognized as at least one of the known ones
        let inferred = infer_analysis_kind(&rule);
        assert!(inferred == AnalysisKind::Unknown, "Expected unknown case");
    }


    #[test]
    fn test_semantic_equivalence_with_then_chaining() {
        // alloc |> drop |> use |> drop (nonsense chain)
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Use(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
        ];
        let rule = mk_rule("complex_then_chain", src, snk);

        // should not match a known 2-step pattern, thus unknown
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::Unknown);
    }



}



/* 
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
        [Predicate::Drop(_), Predicate::Read(_)] => {
            AnalysisKind::UseAfterFree
        }
        [Predicate::Drop(_), Predicate::Write(_)] => {
            AnalysisKind::UseAfterFree
        }

        _ => AnalysisKind::Unknown,
    }
}
/// check if the sink contains a negated Drop predicate
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
    fn test_uaf() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Use(Some(Term::Var("x".into()))))];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }
    
    #[test]
    fn test_uaf2() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Read(Some(Term::Var("x".into()))))];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }

    #[test]
    fn test_uaf3() {
        let src = vec![Statement::Predicate(Predicate::Alloc(Some(Term::Var("x".into()))))];
        let snk = vec![
            Statement::Predicate(Predicate::Drop(Some(Term::Var("x".into())))),
            Statement::Predicate(Predicate::Write(Some(Term::Var("x".into()))))];
        let rule = mk_rule("use_after_free", src, snk);
        assert_eq!(infer_analysis_kind(&rule), AnalysisKind::UseAfterFree);
    }
}
*/