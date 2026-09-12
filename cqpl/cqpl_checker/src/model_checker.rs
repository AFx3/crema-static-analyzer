use crate::ast::{MayPredicate, PathFormula, PathQuantifier, StateFormula};
use crate::kripke::{CellValue, Kripke};
use crate::truth::Truth;
use std::collections::{BTreeMap, BTreeSet};

pub type Env = BTreeMap<String, String>; // logic variable -> cross-language ProgramVar id
pub type Valuation = BTreeMap<String, Truth>; // node id -> truth value

pub struct ModelChecker<'a> {
    k: &'a Kripke,
}

impl<'a> ModelChecker<'a> {
    pub fn new(k: &'a Kripke) -> Self { Self { k } }

    pub fn evaluate(&self, formula: &StateFormula, initial_env: &Env) -> Result<Truth, String> {
        let unbound: Vec<_> = formula
            .free_vars()
            .into_iter()
            .filter(|v| !initial_env.contains_key(v))
            .collect();
        if !unbound.is_empty() {
            return Err(format!(
                "CQPL query has unbound logical variable(s): {}. Official queries should be closed; alternatively pass --bind name=PROGRAM_VAR_ID.",
                unbound.join(", ")
            ));
        }
        for (logic, program) in initial_env {
            if !self.k.variables.contains_key(program) {
                return Err(format!("binding {logic}={program} refers to an unknown Rust/C program variable"));
            }
        }
        Ok(self.eval_all(formula, initial_env)?[&self.k.entry])
    }

    fn eval_all(&self, formula: &StateFormula, env: &Env) -> Result<Valuation, String> {
        use StateFormula::*;
        match formula {
            May { predicate, logic_var } => {
                let Some(program_var) = env.get(logic_var) else {
                    return Err(format!("logical variable '{logic_var}' is unbound"));
                };
                Ok(self.k.nodes.keys().map(|n| (n.clone(), self.k.may_hold(n, program_var, *predicate))).collect())
            }
            Label { predicate, logic_var } => {
                let Some(program_var) = env.get(logic_var) else {
                    return Err(format!("logical variable '{logic_var}' is unbound"));
                };
                Ok(self.k.nodes.keys().map(|n| (n.clone(), self.k.label_hold(n, program_var, *predicate))).collect())
            }
            Not(inner) => Ok(map_unary(self.eval_all(inner, env)?, Truth::not)),
            And(a, b) => {
                // Semantics-preserving global short-circuit:
                // ff ∧ x = ff pointwise.  This is particularly important for
                // official error queries of the form `alloc(x) && temporal...`:
                // when `alloc(x)` is refuted at every node, evaluating the
                // nested temporal suffix is provably unnecessary.
                let left = self.eval_all(a, env)?;
                if valuation_is_constant(&left, Truth::False) {
                    return Ok(left);
                }
                Ok(map_binary(left, self.eval_all(b, env)?, Truth::meet))
            }
            Or(a, b) => {
                // Dual optimization: tt ∨ x = tt pointwise.
                let left = self.eval_all(a, env)?;
                if valuation_is_constant(&left, Truth::True) {
                    return Ok(left);
                }
                Ok(map_binary(left, self.eval_all(b, env)?, Truth::join))
            }
            Exists { logic_var, body } => {
                let mut acc = self.constant(Truth::False);

                // Semantics-preserving candidate pruning for existentially
                // quantified program variables.  If the body contains a
                // positive may-predicate p(x) that is necessary for the body
                // to be non-ff, then a program variable for which p is ff at
                // every node contributes the all-ff valuation to the
                // existential join and can be skipped.
                //
                // The syntactic analysis is deliberately conservative: it
                // does not propagate requirements through negation, and for
                // disjunction it keeps only requirements common to both
                // branches.  Therefore it may miss pruning opportunities but
                // cannot change the CQPL result.
                let candidates = self.existential_candidates(logic_var, body);
                let program_vars: Vec<String> = match candidates {
                    Some(ids) => ids.into_iter().collect(),
                    None => self.k.variable_ids().cloned().collect(),
                };

                for program_var in program_vars {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), program_var);
                    acc = map_binary(acc, self.eval_all(body, &next_env)?, Truth::join);
                    if valuation_is_constant(&acc, Truth::True) {
                        break;
                    }
                }
                Ok(acc)
            }
            ForAll { logic_var, body } => {
                let mut acc = self.constant(Truth::True);
                for program_var in self.k.variable_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), program_var.clone());
                    acc = map_binary(acc, self.eval_all(body, &next_env)?, Truth::meet);
                    if valuation_is_constant(&acc, Truth::False) {
                        break;
                    }
                }
                Ok(acc)
            }
            Path { quantifier, formula } => self.eval_path(*quantifier, formula, env),
        }
    }

    fn eval_path(&self, q: PathQuantifier, path: &PathFormula, env: &Env) -> Result<Valuation, String> {
        match path {
            PathFormula::State(phi) => self.eval_all(phi, env),
            PathFormula::Next(phi) => {
                let phi = self.eval_all(phi, env)?;
                Ok(self.pre(q, &phi, NextMode::Strong))
            }
            PathFormula::Eventually(phi) => {
                let phi = self.eval_all(phi, env)?;
                let mut z = self.constant(Truth::False); // least fixpoint
                loop {
                    let next = map_binary(phi.clone(), self.pre(q, &z, NextMode::Strong), Truth::join);
                    if next == z { return Ok(z); }
                    z = next;
                }
            }
            PathFormula::Globally(phi) => {
                let phi = self.eval_all(phi, env)?;
                let mut z = self.constant(Truth::True); // greatest fixpoint
                loop {
                    // On a maximal finite path, G phi at a terminal node is exactly phi
                    // at that node; therefore the continuation is vacuously tt.
                    let next = map_binary(phi.clone(), self.pre(q, &z, NextMode::WeakForGlobal), Truth::meet);
                    if next == z { return Ok(z); }
                    z = next;
                }
            }
            PathFormula::Until(lhs, rhs) => {
                let lhs = self.eval_all(lhs, env)?;
                let rhs = self.eval_all(rhs, env)?;
                let mut z = self.constant(Truth::False); // least fixpoint
                loop {
                    let continuation = map_binary(lhs.clone(), self.pre(q, &z, NextMode::Strong), Truth::meet);
                    let next = map_binary(rhs.clone(), continuation, Truth::join);
                    if next == z { return Ok(z); }
                    z = next;
                }
            }
        }
    }

    fn existential_candidates(
        &self,
        logic_var: &str,
        body: &StateFormula,
    ) -> Option<BTreeSet<String>> {
        let mask = necessary_positive_may_mask(body, logic_var);
        if mask == 0 {
            return None;
        }

        let mut candidates: Option<BTreeSet<String>> = None;
        for predicate in [MayPredicate::Alloc, MayPredicate::Drop, MayPredicate::OwnForg] {
            if mask & may_predicate_bit(predicate) == 0 {
                continue;
            }
            let current = self.may_candidates(predicate);
            candidates = Some(match candidates {
                None => current,
                Some(previous) => previous.intersection(&current).cloned().collect(),
            });
        }
        candidates
    }

    fn may_candidates(&self, predicate: MayPredicate) -> BTreeSet<String> {
        let atom = may_atom(predicate);
        let mut out = BTreeSet::new();
        for node in self.k.nodes.values() {
            for cell in &node.post.cells {
                if atom.leq(cell.value) {
                    out.extend(cell.aliases.iter().cloned());
                }
            }
        }
        out
    }

    fn constant(&self, value: Truth) -> Valuation {
        self.k.nodes.keys().map(|n| (n.clone(), value)).collect()
    }

    fn pre(&self, q: PathQuantifier, values: &Valuation, mode: NextMode) -> Valuation {
        self.k.nodes.iter().map(|(id, node)| {
            let value = if node.successors.is_empty() {
                match mode {
                    // X is strong in the CQPL theory: no successor => ff for E and A.
                    NextMode::Strong => Truth::False,
                    // G quantifies only over positions that actually exist on a maximal path.
                    NextMode::WeakForGlobal => Truth::True,
                }
            } else {
                let mut it = node.successors.iter().map(|s| values[s]);
                let first = it.next().expect("non-empty successor list");
                match q {
                    PathQuantifier::Exists => it.fold(first, Truth::join),
                    PathQuantifier::ForAll => it.fold(first, Truth::meet),
                }
            };
            (id.clone(), value)
        }).collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum NextMode { Strong, WeakForGlobal }

fn map_unary(mut a: Valuation, f: fn(Truth) -> Truth) -> Valuation {
    for v in a.values_mut() { *v = f(*v); }
    a
}

fn map_binary(a: Valuation, b: Valuation, f: fn(Truth, Truth) -> Truth) -> Valuation {
    a.into_iter().map(|(k, va)| (k.clone(), f(va, b[&k]))).collect()
}

fn may_atom(predicate: MayPredicate) -> CellValue {
    match predicate {
        MayPredicate::Alloc => CellValue::Alloc,
        MayPredicate::Drop => CellValue::Freed,
        MayPredicate::OwnForg => CellValue::Mv,
    }
}

fn may_predicate_bit(predicate: MayPredicate) -> u8 {
    match predicate {
        MayPredicate::Alloc => 0b001,
        MayPredicate::Drop => 0b010,
        MayPredicate::OwnForg => 0b100,
    }
}

/// Return a conservative set of positive may-predicates on `logic_var` that
/// are necessary for `formula` to be non-ff.  A zero mask means only that no
/// safe pruning fact was established; it does not mean the formula is
/// independent of may-predicates.
fn necessary_positive_may_mask(formula: &StateFormula, logic_var: &str) -> u8 {
    use StateFormula::*;
    match formula {
        May { predicate, logic_var: atom_var } if atom_var == logic_var => {
            may_predicate_bit(*predicate)
        }
        May { .. } | Label { .. } => 0,
        Not(_) => 0,
        And(a, b) => {
            necessary_positive_may_mask(a, logic_var)
                | necessary_positive_may_mask(b, logic_var)
        }
        Or(a, b) => {
            necessary_positive_may_mask(a, logic_var)
                & necessary_positive_may_mask(b, logic_var)
        }
        Exists { logic_var: bound, .. } | ForAll { logic_var: bound, .. }
            if bound == logic_var =>
        {
            // The nested binder shadows the outer variable.
            0
        }
        Exists { body, .. } | ForAll { body, .. } => {
            necessary_positive_may_mask(body, logic_var)
        }
        Path { formula: path, .. } => match path {
            PathFormula::State(phi)
            | PathFormula::Next(phi)
            | PathFormula::Eventually(phi)
            | PathFormula::Globally(phi) => necessary_positive_may_mask(phi, logic_var),
            // For phi U psi, psi must eventually be non-ff; phi need not hold
            // when psi is already satisfied at the current position.
            PathFormula::Until(_, rhs) => necessary_positive_may_mask(rhs, logic_var),
        },
    }
}

fn valuation_is_constant(v: &Valuation, expected: Truth) -> bool {
    v.values().all(|actual| *actual == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kripke::*;
    use crate::parser::parse_query;
    use std::collections::BTreeMap;

    fn var(id: &str, language: ProgramLanguage) -> ProgramVariable {
        ProgramVariable { id: id.into(), language, display: None, function: None }
    }
    fn mem(groups: &[(&[&str], CellValue)]) -> AbstractMemoryAnnotation {
        AbstractMemoryAnnotation {
            cells: groups.iter().map(|(vars, value)| AbstractCell {
                aliases: vars.iter().map(|v| v.to_string()).collect(),
                value: *value,
            }).collect()
        }
    }
    fn node(
        id: &str,
        succ: &[&str],
        labels: Vec<EventLabel>,
        pre: AbstractMemoryAnnotation,
        post: AbstractMemoryAnnotation,
    ) -> AnnotatedNode {
        AnnotatedNode {
            id: id.into(),
            successors: succ.iter().map(|s| s.to_string()).collect(),
            labels, pre, post,
        }
    }

    #[test]
    fn quantifier_domain_includes_c_variables() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "c0".into(),
            variables: vec![var("c::malloc::ret", ProgramLanguage::C)],
            nodes: vec![node(
                "c0", &[], vec![],
                AbstractMemoryAnnotation::default(),
                mem(&[(&["c::malloc::ret"], CellValue::Alloc)]),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query("exists x. alloc(x)").unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap(), Truth::Unknown);
    }

    #[test]
    fn explicit_binding_can_bind_a_logic_variable_to_c_program_variable() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "c0".into(),
            variables: vec![var("c::p", ProgramLanguage::C)],
            nodes: vec![node(
                "c0", &[],
                vec![EventLabel { predicate: EventKind::Read, variable: "c::p".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query("use_l(x)").unwrap();
        let env = BTreeMap::from([("x".into(), "c::p".into())]);
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::True);
    }

    #[test]
    fn top_makes_all_supported_may_atoms_unknown_not_true() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![node(
                "b0", &[], vec![], AbstractMemoryAnnotation::default(),
                mem(&[(&["rust::x"], CellValue::Top)]),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let mc = ModelChecker::new(&k);
        for text in ["alloc(x)", "drop(x)", "own_forg(x)"] {
            let q = parse_query(text).unwrap();
            let env = BTreeMap::from([("x".into(), "rust::x".into())]);
            assert_eq!(mc.evaluate(&q, &env).unwrap(), Truth::Unknown, "{text}");
        }
    }

    #[test]
    fn strong_next_is_false_at_terminal_nodes() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(), variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        for text in ["EX use_l(x)", "AX use_l(x)"] {
            let q = parse_query(text).unwrap();
            assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::False);
        }
    }

    #[test]
    fn global_at_terminal_checks_current_position_only() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(), variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        for text in ["EG use_l(x)", "AG use_l(x)"] {
            let q = parse_query(text).unwrap();
            assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::True);
        }
    }


    #[test]
    fn uaf_is_refuted_when_alloc_is_globally_refuted_even_if_suffix_has_events() {
        // The official UAF formula begins with alloc(x) && temporal_suffix.
        // With no abstract allocation witness anywhere, the whole query is ff.
        // The implementation may short-circuit the suffix, but the expected
        // truth value is a semantic property, not a performance assumption.
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![
                node(
                    "b0", &["b1"],
                    vec![EventLabel { predicate: EventKind::Drop, variable: "rust::x".into() }],
                    AbstractMemoryAnnotation::default(),
                    AbstractMemoryAnnotation::default(),
                ),
                node(
                    "b1", &["b1"],
                    vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                    AbstractMemoryAnnotation::default(),
                    AbstractMemoryAnnotation::default(),
                ),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query(
            "exists x. EF (alloc(x) && EX EF (drop_l(x) && EX E[(!alloc_l(x)) U use_l(x)]))"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap(), Truth::False);
    }

    #[test]
    fn theoretical_uaf_query_is_unknown_on_cross_language_may_witness() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::p", ProgramLanguage::Rust), var("c::arg0", ProgramLanguage::C)],
            nodes: vec![
                node(
                    "b0", &["c0"],
                    vec![EventLabel { predicate: EventKind::Alloc, variable: "rust::p".into() }],
                    AbstractMemoryAnnotation::default(),
                    mem(&[(&["rust::p"], CellValue::Alloc)]),
                ),
                node(
                    "c0", &["b1"],
                    vec![EventLabel { predicate: EventKind::Drop, variable: "c::arg0".into() }],
                    mem(&[(&["rust::p", "c::arg0"], CellValue::Alloc)]),
                    mem(&[(&["rust::p", "c::arg0"], CellValue::Freed)]),
                ),
                node(
                    "b1", &[],
                    vec![EventLabel { predicate: EventKind::Read, variable: "rust::p".into() }],
                    mem(&[(&["rust::p"], CellValue::Freed)]),
                    mem(&[(&["rust::p"], CellValue::Freed)]),
                ),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query(
            "exists x. EF (alloc(x) && EX EF (drop_l(x) && EX E[(!alloc_l(x)) U use_l(x)]))"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap(), Truth::Unknown);
    }



    #[test]
    fn existential_and_universal_eventually_differ_on_branching_graph() {
        // b0 branches to b1 (use) and b2 (no use). EF sees the witness path;
        // AF is refuted by the maximal path ending in b2.
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        let mc = ModelChecker::new(&k);
        assert_eq!(mc.evaluate(&parse_query("EF use_l(x)").unwrap(), &env).unwrap(), Truth::True);
        assert_eq!(mc.evaluate(&parse_query("AF use_l(x)").unwrap(), &env).unwrap(), Truth::False);
    }

    #[test]
    fn existential_and_universal_globally_differ_on_branching_graph() {
        // Every visited state on b0->b1 satisfies use_l(x), while b0->b2
        // reaches a terminal state that does not. Hence EG=tt and AG=ff.
        let use_x = || EventLabel { predicate: EventKind::Read, variable: "rust::x".into() };
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![use_x()], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![use_x()], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        let mc = ModelChecker::new(&k);
        assert_eq!(mc.evaluate(&parse_query("EG use_l(x)").unwrap(), &env).unwrap(), Truth::True);
        assert_eq!(mc.evaluate(&parse_query("AG use_l(x)").unwrap(), &env).unwrap(), Truth::False);
    }

    #[test]
    fn existential_and_universal_until_differ_on_branching_graph() {
        // b0 carries alloc_l(x). One branch reaches drop_l(x); the other ends
        // without a drop. E[alloc_l U drop_l] has a witness; A[...] is refuted.
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![EventLabel { predicate: EventKind::Alloc, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![EventLabel { predicate: EventKind::Drop, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        let mc = ModelChecker::new(&k);
        assert_eq!(mc.evaluate(&parse_query("E[alloc_l(x) U drop_l(x)]").unwrap(), &env).unwrap(), Truth::True);
        assert_eq!(mc.evaluate(&parse_query("A[alloc_l(x) U drop_l(x)]").unwrap(), &env).unwrap(), Truth::False);
    }

    #[test]
    fn three_valued_eventually_preserves_unknown_may_witness() {
        // The only allocation witness is abstract (ALLOC <= TOP), so EF alloc
        // must remain unk rather than being promoted to tt or refuted to ff.
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![
                node("b0", &["b1"], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![], AbstractMemoryAnnotation::default(), mem(&[(&["rust::x"], CellValue::Top)])),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::x".into())]);
        assert_eq!(ModelChecker::new(&k).evaluate(&parse_query("EF alloc(x)").unwrap(), &env).unwrap(), Truth::Unknown);
    }

    #[test]
    fn forall_program_variables_ranges_over_rust_and_c() {
        // Only the Rust variable is used. The implementation quantifier domain
        // contains both Rust and C variables, so exists is tt and forall is ff.
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust), var("c::p", ProgramLanguage::C)],
            nodes: vec![node(
                "b0", &[],
                vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let mc = ModelChecker::new(&k);
        assert_eq!(mc.evaluate(&parse_query("exists x. use_l(x)").unwrap(), &Env::new()).unwrap(), Truth::True);
        assert_eq!(mc.evaluate(&parse_query("forall x. use_l(x)").unwrap(), &Env::new()).unwrap(), Truth::False);
    }
    #[test]
    fn c_free_label_satisfies_rust_alias_drop_label_at_same_program_point() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::p", ProgramLanguage::Rust), var("c::arg0", ProgramLanguage::C)],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Drop, variable: "c::arg0".into() }],
                mem(&[(&["rust::p", "c::arg0"], CellValue::Alloc)]),
                mem(&[(&["rust::p", "c::arg0"], CellValue::Freed)]),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), "rust::p".into())]);
        let q = parse_query("drop_l(x)").unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::True);
    }

    #[test]
    fn existential_candidate_pruning_preserves_refutation_when_required_alloc_is_absent() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![
                var("rust::x", ProgramLanguage::Rust),
                var("c::p", ProgramLanguage::C),
            ],
            nodes: vec![
                node(
                    "b0", &[],
                    vec![EventLabel { predicate: EventKind::Use, variable: "rust::x".into() }],
                    AbstractMemoryAnnotation::default(),
                    AbstractMemoryAnnotation::default(),
                ),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query(
            "exists x. EF (alloc(x) && EX EF (drop_l(x) && EX E[(!alloc_l(x)) U use_l(x)]))"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap(), Truth::False);
    }

    #[test]
    fn existential_candidate_pruning_is_not_applied_through_negation_or_disjunction() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            nodes: vec![node(
                "b0", &[],
                vec![EventLabel { predicate: EventKind::Use, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(),
                AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let checker = ModelChecker::new(&k);

        let negated = parse_query("exists x. (!alloc(x) && use_l(x))").unwrap();
        assert_eq!(checker.evaluate(&negated, &Env::new()).unwrap(), Truth::True);

        let disjunctive = parse_query("exists x. (alloc(x) || use_l(x))").unwrap();
        assert_eq!(checker.evaluate(&disjunctive, &Env::new()).unwrap(), Truth::True);
    }

}
