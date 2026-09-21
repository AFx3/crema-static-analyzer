use cqpl_checker::ast::{LabelPredicate, MayPredicate, PathFormula, PathQuantifier, StateFormula};
use cqpl_checker::kripke::{
    AbstractCell, AbstractMemoryAnnotation, AnnotatedIcfg, AnnotatedNode, CellValue, EventKind,
    EventLabel, Kripke, ProgramLanguage, ProgramVariable,
};
use cqpl_checker::{Binding, Env, ModelChecker, Truth};
use std::collections::BTreeMap;

fn var(id: &str) -> ProgramVariable {
    ProgramVariable {
        id: id.to_string(),
        language: ProgramLanguage::Rust,
        display: None,
        function: None,
    }
}

fn label_read(logic_var: &str) -> StateFormula {
    StateFormula::Label {
        predicate: LabelPredicate::Read,
        logic_var: logic_var.to_string(),
    }
}

fn may_alloc(logic_var: &str) -> StateFormula {
    StateFormula::May {
        predicate: MayPredicate::Alloc,
        logic_var: logic_var.to_string(),
    }
}

/// One CQPL formula capable of taking every value in {ff, unk, tt}.
///
/// * read_l(v)       => tt
/// * alloc(v)=MAY    => unk when read_l(v)=ff
/// * neither         => ff
///
/// This lets the semantic-law tests enumerate arbitrary ternary atomic
/// valuations without adding any test-only primitive to the language.
fn ternary_atom(logic_var: &str) -> StateFormula {
    or(label_read(logic_var), may_alloc(logic_var))
}

fn not(f: StateFormula) -> StateFormula {
    StateFormula::Not(Box::new(f))
}

fn and(a: StateFormula, b: StateFormula) -> StateFormula {
    StateFormula::And(Box::new(a), Box::new(b))
}

fn or(a: StateFormula, b: StateFormula) -> StateFormula {
    StateFormula::Or(Box::new(a), Box::new(b))
}

fn path(q: PathQuantifier, f: PathFormula) -> StateFormula {
    StateFormula::Path {
        quantifier: q,
        formula: f,
    }
}

fn ex(f: StateFormula) -> StateFormula {
    path(PathQuantifier::Exists, PathFormula::Next(Box::new(f)))
}

fn ax(f: StateFormula) -> StateFormula {
    path(PathQuantifier::ForAll, PathFormula::Next(Box::new(f)))
}

fn ef(f: StateFormula) -> StateFormula {
    path(PathQuantifier::Exists, PathFormula::Eventually(Box::new(f)))
}

fn af(f: StateFormula) -> StateFormula {
    path(PathQuantifier::ForAll, PathFormula::Eventually(Box::new(f)))
}

fn eg(f: StateFormula) -> StateFormula {
    path(PathQuantifier::Exists, PathFormula::Globally(Box::new(f)))
}

fn ag(f: StateFormula) -> StateFormula {
    path(PathQuantifier::ForAll, PathFormula::Globally(Box::new(f)))
}

fn eu(lhs: StateFormula, rhs: StateFormula) -> StateFormula {
    path(
        PathQuantifier::Exists,
        PathFormula::Until(Box::new(lhs), Box::new(rhs)),
    )
}

fn au(lhs: StateFormula, rhs: StateFormula) -> StateFormula {
    path(
        PathQuantifier::ForAll,
        PathFormula::Until(Box::new(lhs), Box::new(rhs)),
    )
}

fn truth_digit(mut code: usize, index: usize) -> Truth {
    for _ in 0..index {
        code /= 3;
    }
    match code % 3 {
        0 => Truth::False,
        1 => Truth::Unknown,
        2 => Truth::True,
        _ => unreachable!(),
    }
}

fn truth_vector(code: usize, n: usize) -> Vec<Truth> {
    (0..n).map(|i| truth_digit(code, i)).collect()
}

fn all_successor_relations(n: usize) -> Vec<Vec<Vec<usize>>> {
    // Deliberately include empty producer successor sets. ModelChecker::new
    // must totalize those deadlocks before the CTL laws are evaluated.
    let subsets = 1usize << n;
    let relation_count = subsets.pow(n as u32);
    let mut out = Vec::with_capacity(relation_count);

    for mut code in 0..relation_count {
        let mut relation = Vec::with_capacity(n);
        for _ in 0..n {
            let mask = code % subsets;
            code /= subsets;
            relation.push((0..n).filter(|j| mask & (1usize << j) != 0).collect());
        }
        out.push(relation);
    }
    out
}

fn synthetic_model(
    relation: &[Vec<usize>],
    p_values: &[Truth],
    q_values: &[Truth],
    entry: usize,
) -> Kripke {
    let n = relation.len();
    assert_eq!(p_values.len(), n);
    assert_eq!(q_values.len(), n);

    let mut nodes = Vec::with_capacity(n);
    for i in 0..n {
        let mut labels = Vec::new();
        let mut cells = Vec::new();

        for (program_var, value) in [
            ("rust::x", p_values[i]),
            ("rust::y", q_values[i]),
        ] {
            match value {
                Truth::True => labels.push(EventLabel {
                    predicate: EventKind::Read,
                    variable: program_var.to_string(),
                }),
                Truth::Unknown => cells.push(AbstractCell {
                    aliases: vec![program_var.to_string()],
                    value: CellValue::Top,
                }),
                Truth::False => {}
            }
        }

        nodes.push(AnnotatedNode {
            id: format!("b{i}"),
            successors: relation[i].iter().map(|j| format!("b{j}")).collect(),
            labels,
            semantic_labels: vec![],
            allocation_labels: vec![],
            allocation_disposition: vec![],
            identity: None,
            event_identity: None,
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation { cells },
        });
    }

    Kripke::from_annotated_icfg(AnnotatedIcfg {
        schema_version: 1,
        entry: format!("b{entry}"),
        capabilities: vec![],
        variables: vec![var("rust::x"), var("rust::y")],
        allocations: vec![],
        external_deallocation_effects: vec![],
        llvm_memory_effects: None,
        svf_solved_points_to: None,
        ffi_argument_identity: vec![],
        nodes,
    })
    .unwrap()
}

fn env() -> Env {
    BTreeMap::from([
        ("x".to_string(), Binding::ProgramVar("rust::x".to_string())),
        ("y".to_string(), Binding::ProgramVar("rust::y".to_string())),
    ])
}

fn valid_ctl_laws() -> Vec<(&'static str, StateFormula, StateFormula)> {
    let p = ternary_atom("x");
    let q = ternary_atom("y");

    vec![
        (
            "AX p = !EX !p",
            ax(p.clone()),
            not(ex(not(p.clone()))),
        ),
        (
            "AG p = !EF !p",
            ag(p.clone()),
            not(ef(not(p.clone()))),
        ),
        (
            "AF p = !EG !p",
            af(p.clone()),
            not(eg(not(p.clone()))),
        ),
        (
            "EG p = !AF !p",
            eg(p.clone()),
            not(af(not(p.clone()))),
        ),
        (
            "EF p = !AG !p",
            ef(p.clone()),
            not(ag(not(p.clone()))),
        ),
        (
            "EF p = p || EX EF p",
            ef(p.clone()),
            or(p.clone(), ex(ef(p.clone()))),
        ),
        (
            "AF p = p || AX AF p",
            af(p.clone()),
            or(p.clone(), ax(af(p.clone()))),
        ),
        (
            "EG p = p && EX EG p",
            eg(p.clone()),
            and(p.clone(), ex(eg(p.clone()))),
        ),
        (
            "AG p = p && AX AG p",
            ag(p.clone()),
            and(p.clone(), ax(ag(p.clone()))),
        ),
        (
            "E[p U q] = q || (p && EX E[p U q])",
            eu(p.clone(), q.clone()),
            or(
                q.clone(),
                and(p.clone(), ex(eu(p.clone(), q.clone()))),
            ),
        ),
        (
            "A[p U q] = q || (p && AX A[p U q])",
            au(p.clone(), q.clone()),
            or(
                q.clone(),
                and(p.clone(), ax(au(p.clone(), q.clone()))),
            ),
        ),
        (
            "AU reduction through EU and EG",
            au(p.clone(), q.clone()),
            and(
                not(eu(
                    not(q.clone()),
                    and(not(p.clone()), not(q.clone())),
                )),
                not(eg(not(q.clone()))),
            ),
        ),
        (
            "EX distributes over ||",
            ex(or(p.clone(), q.clone())),
            or(ex(p.clone()), ex(q.clone())),
        ),
        (
            "AX distributes over &&",
            ax(and(p.clone(), q.clone())),
            and(ax(p.clone()), ax(q.clone())),
        ),
        (
            "EF distributes over ||",
            ef(or(p.clone(), q.clone())),
            or(ef(p.clone()), ef(q.clone())),
        ),
        (
            "AG distributes over &&",
            ag(and(p.clone(), q.clone())),
            and(ag(p.clone()), ag(q.clone())),
        ),
    ]
}

#[test]
fn exhaustive_totalized_ctl_laws_hold_on_all_one_and_two_state_ternary_models() {
    let environment = env();
    let laws = valid_ctl_laws();
    let mut checked_entry_configurations = 0usize;

    for n in 1usize..=2 {
        let valuation_count = 3usize.pow(n as u32);
        for relation in all_successor_relations(n) {
            for p_code in 0..valuation_count {
                let p_values = truth_vector(p_code, n);
                for q_code in 0..valuation_count {
                    let q_values = truth_vector(q_code, n);
                    for entry in 0..n {
                        let k = synthetic_model(&relation, &p_values, &q_values, entry);
                        let mc = ModelChecker::new(&k);
                        checked_entry_configurations += 1;

                        for (name, lhs, rhs) in &laws {
                            let left = mc.evaluate(lhs, &environment).unwrap();
                            let right = mc.evaluate(rhs, &environment).unwrap();
                            assert_eq!(
                                left, right,
                                "CTL law failed: {name}; n={n}; entry=b{entry}; relation={relation:?}; p={p_values:?}; q={q_values:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    // n=1: 2 relations * 3^2 valuations * 1 entry = 18
    // n=2: 4^2 relations * 3^4 valuations * 2 entries = 2592
    assert_eq!(checked_entry_configurations, 2610);
}

#[test]
fn three_valued_excluded_middle_is_not_assumed() {
    let k = synthetic_model(&[vec![0]], &[Truth::Unknown], &[Truth::False], 0);
    let mc = ModelChecker::new(&k);
    let p = ternary_atom("x");
    assert_eq!(
        mc.evaluate(&or(p.clone(), not(p.clone())), &env()).unwrap(),
        Truth::Unknown
    );
    assert_eq!(
        mc.evaluate(&and(p.clone(), not(p)), &env()).unwrap(),
        Truth::Unknown
    );
}

#[test]
fn negative_control_eg_does_not_distribute_over_disjunction() {
    // b1 -> b0 -> b0.  p holds only at b1; q holds only at b0.
    // Thus p||q holds globally on the unique infinite path, but neither p nor
    // q does.  The intentionally false distributivity law must be rejected.
    let relation = vec![vec![0], vec![0]];
    let p_values = vec![Truth::False, Truth::True];
    let q_values = vec![Truth::True, Truth::False];
    let k = synthetic_model(&relation, &p_values, &q_values, 1);
    let mc = ModelChecker::new(&k);
    let p = ternary_atom("x");
    let q = ternary_atom("y");

    let lhs = mc.evaluate(&eg(or(p.clone(), q.clone())), &env()).unwrap();
    let rhs = mc
        .evaluate(&or(eg(p.clone()), eg(q.clone())), &env())
        .unwrap();
    assert_eq!(lhs, Truth::True);
    assert_eq!(rhs, Truth::False);
    assert_ne!(lhs, rhs);
}

#[test]
fn negative_control_ex_does_not_distribute_over_conjunction() {
    // At b0 choose between a p-only state and a q-only state. EX p and EX q
    // are both true, but no successor satisfies p&&q.
    let relation = vec![vec![0, 1], vec![1]];
    let p_values = vec![Truth::False, Truth::True];
    let q_values = vec![Truth::True, Truth::False];
    let k = synthetic_model(&relation, &p_values, &q_values, 0);
    let mc = ModelChecker::new(&k);
    let p = ternary_atom("x");
    let q = ternary_atom("y");

    let lhs = mc.evaluate(&ex(and(p.clone(), q.clone())), &env()).unwrap();
    let rhs = mc
        .evaluate(&and(ex(p.clone()), ex(q.clone())), &env())
        .unwrap();
    assert_eq!(lhs, Truth::False);
    assert_eq!(rhs, Truth::True);
    assert_ne!(lhs, rhs);
}

#[test]
fn negative_control_partial_deadlock_breaks_next_duality_without_totalization() {
    // This is the old --intra compatibility semantics, deliberately *not* the
    // whole-program CQPL semantics. At a deadlock strong AX and EX both return
    // ff, so AX p != !EX !p. This witnesses why totality is a proof premise.
    let k = synthetic_model(&[vec![]], &[Truth::False], &[Truth::False], 0);
    let mc = ModelChecker::new_partial_for_intra(&k);
    let p = ternary_atom("x");
    let lhs = mc.evaluate(&ax(p.clone()), &env()).unwrap();
    let rhs = mc.evaluate(&not(ex(not(p))), &env()).unwrap();
    assert_eq!(lhs, Truth::False);
    assert_eq!(rhs, Truth::True);
    assert_ne!(lhs, rhs);
}
