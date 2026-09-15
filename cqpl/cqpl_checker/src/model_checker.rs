use crate::ast::{LabelPredicate, MayPredicate, PathFormula, PathQuantifier, QueryDocument, StateFormula, StructuralLabelKind};
use crate::kripke::{CellValue, Kripke};
use crate::truth::Truth;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    ProgramVar(String),
    Allocation(String),
}

pub type Env = BTreeMap<String, Binding>;
pub type Valuation = BTreeMap<String, Truth>; // node id -> truth value

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicSort {
    ProgramVar,
    Allocation,
}

pub struct ModelChecker<'a> {
    k: &'a Kripke,
}

impl<'a> ModelChecker<'a> {
    pub fn new(k: &'a Kripke) -> Self { Self { k } }

    /// Evaluate a legacy/formula-only query. Capability-gated predicates are
    /// deliberately rejected here so a missing `requires` declaration can
    /// never be interpreted as logical refutation.
    pub fn evaluate(&self, formula: &StateFormula, initial_env: &Env) -> Result<Truth, String> {
        if formula_uses_allocator_mismatch(formula) {
            return Err("allocator_mismatch_l requires a query document declaring `requires allocation_contracts_v1;` or `requires allocation_contracts_v2;`".into());
        }
        if formula_uses_allocation_state(formula, initial_env) {
            return Err("allocation-bound state predicates require a query document declaring `requires allocation_state_v1;`".into());
        }
        if formula_uses_structural_labels(formula) {
            return Err("stmt_l/rvalue_l/term_l require a query document declaring `requires mir_semantic_labels_v1;`".into());
        }
        self.evaluate_formula(formula, initial_env)
    }

    /// Evaluate a capability-aware CQPL document. Unsupported is a hard error,
    /// never `ff`: this is part of the no-refutation theorem boundary.
    pub fn evaluate_document(&self, document: &QueryDocument, initial_env: &Env) -> Result<Truth, String> {
        if formula_uses_allocator_mismatch(&document.formula)
            && !document.required_capabilities.contains("allocation_contracts_v1")
            && !document.required_capabilities.contains("allocation_contracts_v2")
        {
            return Err("allocator_mismatch_l requires explicit `requires allocation_contracts_v1;` or `requires allocation_contracts_v2;`".into());
        }
        if formula_uses_allocation_state(&document.formula, initial_env)
            && !document.required_capabilities.contains("allocation_state_v1")
        {
            return Err("allocation-bound state predicates require explicit `requires allocation_state_v1;`".into());
        }
        if formula_uses_structural_labels(&document.formula)
            && !document.required_capabilities.contains("mir_semantic_labels_v1")
        {
            return Err("stmt_l/rvalue_l/term_l require explicit `requires mir_semantic_labels_v1;`".into());
        }
        for capability in &document.required_capabilities {
            if !self.k.capabilities.contains(capability) {
                return Err(format!(
                    "CQPL query requires capability '{capability}', but the annotated ICFG does not declare it"
                ));
            }
        }
        self.evaluate_formula(&document.formula, initial_env)
    }

    fn evaluate_formula(&self, formula: &StateFormula, initial_env: &Env) -> Result<Truth, String> {
        let unbound: Vec<_> = formula
            .free_vars()
            .into_iter()
            .filter(|v| !initial_env.contains_key(v))
            .collect();
        if !unbound.is_empty() {
            return Err(format!(
                "CQPL query has unbound logical variable(s): {}. Official queries should be closed; alternatively pass --bind name=PROGRAM_VAR_ID or --bind-alloc name=ABSTRACT_ALLOC_ID.",
                unbound.join(", ")
            ));
        }
        for (logic, binding) in initial_env {
            match binding {
                Binding::ProgramVar(program) => {
                    if !self.k.variables.contains_key(program) {
                        return Err(format!(
                            "binding {logic}={program} refers to an unknown Rust/C program variable"
                        ));
                    }
                }
                Binding::Allocation(allocation) => {
                    if self.k.schema_version < 2 {
                        return Err(format!(
                            "allocation binding '{logic}' requires CQPL annotated-ICFG schema v2+"
                        ));
                    }
                    if !self.k.allocations.contains_key(allocation) {
                        return Err(format!(
                            "allocation binding {logic}={allocation} refers to an unknown abstract allocation"
                        ));
                    }
                }
            }
        }
        self.validate_formula_sorts(formula, initial_env)?;
        Ok(self.eval_all(formula, initial_env)?[&self.k.entry])
    }

    /// Static CQPL sort checking.  This runs before model-checking and therefore
    /// cannot be bypassed by the evaluator's semantics-preserving short cuts.
    /// Program-variable quantifiers bind ProgramVar; allocation quantifiers bind
    /// AbstractAllocId. allocation_state_v1 lifts the existing MAY state predicates
    /// to allocation bindings without changing their three-valued semantics.
    fn validate_formula_sorts(&self, formula: &StateFormula, initial_env: &Env) -> Result<(), String> {
        let mut sorts = BTreeMap::new();
        for (logic, binding) in initial_env {
            let sort = match binding {
                Binding::ProgramVar(_) => LogicSort::ProgramVar,
                Binding::Allocation(_) => LogicSort::Allocation,
            };
            sorts.insert(logic.clone(), sort);
        }
        self.validate_state_sorts(formula, &mut sorts)
    }

    fn validate_state_sorts(
        &self,
        formula: &StateFormula,
        sorts: &mut BTreeMap<String, LogicSort>,
    ) -> Result<(), String> {
        use StateFormula::*;
        match formula {
            May { logic_var, .. } => match sorts.get(logic_var) {
                Some(LogicSort::ProgramVar) => Ok(()),
                Some(LogicSort::Allocation) if self.k.capabilities.contains("allocation_state_v1") => Ok(()),
                Some(LogicSort::Allocation) => Err(format!(
                    "state may-predicate on allocation variable '{logic_var}' requires annotated-ICFG capability allocation_state_v1"
                )),
                None => Err(format!("logical variable '{logic_var}' is unbound")),
            },
            Label { predicate, logic_var } => match sorts.get(logic_var) {
                Some(LogicSort::ProgramVar) if *predicate == LabelPredicate::AllocatorMismatch => Err(format!(
                    "allocator_mismatch_l requires an allocation variable bound by exists_alloc/forall_alloc ('{logic_var}')"
                )),
                Some(LogicSort::ProgramVar) => Ok(()),
                Some(LogicSort::Allocation) if *predicate == LabelPredicate::AllocatorMismatch
                    && (self.k.capabilities.contains("allocation_contracts_v1")
                        || self.k.capabilities.contains("allocation_contracts_v2")) => Ok(()),
                Some(LogicSort::Allocation) if *predicate == LabelPredicate::AllocatorMismatch => Err(format!(
                    "allocator_mismatch_l requires annotated-ICFG capability allocation_contracts_v1 or allocation_contracts_v2 (variable '{logic_var}')"
                )),
                Some(LogicSort::Allocation) if self.k.schema_version >= 2 => Ok(()),
                Some(LogicSort::Allocation) => Err(format!(
                    "allocation event predicates require CQPL annotated-ICFG schema v2+ (variable '{logic_var}')"
                )),
                None => Err(format!("logical variable '{logic_var}' is unbound")),
            },
            StructuralLabel { .. } => {
                if self.k.capabilities.contains("mir_semantic_labels_v1") { Ok(()) }
                else { Err("structural MIR label predicate requires annotated-ICFG capability mir_semantic_labels_v1".into()) }
            }
            Not(inner) => self.validate_state_sorts(inner, sorts),
            And(a, b) | Or(a, b) => {
                self.validate_state_sorts(a, sorts)?;
                self.validate_state_sorts(b, sorts)
            }
            Exists { logic_var, body } | ForAll { logic_var, body } => {
                let previous = sorts.insert(logic_var.clone(), LogicSort::ProgramVar);
                let result = self.validate_state_sorts(body, sorts);
                match previous {
                    Some(sort) => { sorts.insert(logic_var.clone(), sort); }
                    None => { sorts.remove(logic_var); }
                }
                result
            }
            ExistsAlloc { logic_var, body } | ForAllAlloc { logic_var, body } => {
                if self.k.schema_version < 2 {
                    return Err(format!(
                        "allocation quantifier for '{logic_var}' requires CQPL annotated-ICFG schema v2+"
                    ));
                }
                let previous = sorts.insert(logic_var.clone(), LogicSort::Allocation);
                let result = self.validate_state_sorts(body, sorts);
                match previous {
                    Some(sort) => { sorts.insert(logic_var.clone(), sort); }
                    None => { sorts.remove(logic_var); }
                }
                result
            }
            Path { formula, .. } => match formula {
                PathFormula::State(s)
                | PathFormula::Next(s)
                | PathFormula::Eventually(s)
                | PathFormula::Globally(s) => self.validate_state_sorts(s, sorts),
                PathFormula::Until(a, b) => {
                    self.validate_state_sorts(a, sorts)?;
                    self.validate_state_sorts(b, sorts)
                }
            },
        }
    }

    fn eval_all(&self, formula: &StateFormula, env: &Env) -> Result<Valuation, String> {
        use StateFormula::*;
        match formula {
            May { predicate, logic_var } => {
                let Some(binding) = env.get(logic_var) else {
                    return Err(format!("logical variable '{logic_var}' is unbound"));
                };
                Ok(match binding {
                    Binding::ProgramVar(program_var) => self.k.nodes.keys()
                        .map(|n| (n.clone(), self.k.may_hold(n, program_var, *predicate)))
                        .collect(),
                    Binding::Allocation(allocation) => self.k.nodes.keys()
                        .map(|n| (n.clone(), self.k.allocation_may_hold(n, allocation, *predicate)))
                        .collect(),
                })
            }
            Label { predicate, logic_var } => {
                let Some(binding) = env.get(logic_var) else {
                    return Err(format!("logical variable '{logic_var}' is unbound"));
                };
                Ok(match binding {
                    Binding::ProgramVar(program_var) => self.k.nodes.keys()
                        .map(|n| (n.clone(), self.k.label_hold(n, program_var, *predicate)))
                        .collect(),
                    Binding::Allocation(allocation) => self.k.nodes.keys()
                        .map(|n| (n.clone(), self.k.allocation_label_hold(n, allocation, *predicate)))
                        .collect(),
                })
            }
            StructuralLabel { kind, name } => {
                Ok(self.k.nodes.keys()
                    .map(|n| (n.clone(), self.k.structural_label_hold(n, *kind, name)))
                    .collect())
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
                    next_env.insert(logic_var.clone(), Binding::ProgramVar(program_var));
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
                    next_env.insert(logic_var.clone(), Binding::ProgramVar(program_var.clone()));
                    acc = map_binary(acc, self.eval_all(body, &next_env)?, Truth::meet);
                    if valuation_is_constant(&acc, Truth::False) {
                        break;
                    }
                }
                Ok(acc)
            }
            ExistsAlloc { logic_var, body } => {
                let mut acc = self.constant(Truth::False);
                for allocation in self.k.allocation_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::Allocation(allocation.clone()));
                    acc = map_binary(acc, self.eval_all(body, &next_env)?, Truth::join);
                    if valuation_is_constant(&acc, Truth::True) {
                        break;
                    }
                }
                Ok(acc)
            }
            ForAllAlloc { logic_var, body } => {
                let mut acc = self.constant(Truth::True);
                for allocation in self.k.allocation_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::Allocation(allocation.clone()));
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

fn formula_uses_allocation_state(formula: &StateFormula, initial_env: &Env) -> bool {
    fn visit(
        formula: &StateFormula,
        sorts: &mut BTreeMap<String, LogicSort>,
    ) -> bool {
        use StateFormula::*;
        match formula {
            May { logic_var, .. } => sorts.get(logic_var) == Some(&LogicSort::Allocation),
            Label { .. } | StructuralLabel { .. } => false,
            Not(inner) => visit(inner, sorts),
            And(a, b) | Or(a, b) => visit(a, sorts) || visit(b, sorts),
            Exists { logic_var, body } | ForAll { logic_var, body } => {
                let previous = sorts.insert(logic_var.clone(), LogicSort::ProgramVar);
                let result = visit(body, sorts);
                match previous {
                    Some(sort) => { sorts.insert(logic_var.clone(), sort); }
                    None => { sorts.remove(logic_var); }
                }
                result
            }
            ExistsAlloc { logic_var, body } | ForAllAlloc { logic_var, body } => {
                let previous = sorts.insert(logic_var.clone(), LogicSort::Allocation);
                let result = visit(body, sorts);
                match previous {
                    Some(sort) => { sorts.insert(logic_var.clone(), sort); }
                    None => { sorts.remove(logic_var); }
                }
                result
            }
            Path { formula, .. } => match formula {
                PathFormula::State(s)
                | PathFormula::Next(s)
                | PathFormula::Eventually(s)
                | PathFormula::Globally(s) => visit(s, sorts),
                PathFormula::Until(a, b) => visit(a, sorts) || visit(b, sorts),
            },
        }
    }

    let mut sorts = BTreeMap::new();
    for (logic, binding) in initial_env {
        sorts.insert(
            logic.clone(),
            match binding {
                Binding::ProgramVar(_) => LogicSort::ProgramVar,
                Binding::Allocation(_) => LogicSort::Allocation,
            },
        );
    }
    visit(formula, &mut sorts)
}

fn formula_uses_structural_labels(formula: &StateFormula) -> bool {
    use StateFormula::*;
    match formula {
        StructuralLabel { .. } => true,
        May { .. } | Label { .. } => false,
        Not(inner) => formula_uses_structural_labels(inner),
        And(a, b) | Or(a, b) => formula_uses_structural_labels(a) || formula_uses_structural_labels(b),
        Exists { body, .. } | ForAll { body, .. } | ExistsAlloc { body, .. } | ForAllAlloc { body, .. } =>
            formula_uses_structural_labels(body),
        Path { formula, .. } => match formula {
            PathFormula::State(s) | PathFormula::Next(s) | PathFormula::Eventually(s) | PathFormula::Globally(s) =>
                formula_uses_structural_labels(s),
            PathFormula::Until(a, b) => formula_uses_structural_labels(a) || formula_uses_structural_labels(b),
        },
    }
}

fn formula_uses_allocator_mismatch(formula: &StateFormula) -> bool {
    use StateFormula::*;
    match formula {
        Label { predicate: LabelPredicate::AllocatorMismatch, .. } => true,
        May { .. } | Label { .. } | StructuralLabel { .. } => false,
        Not(inner) => formula_uses_allocator_mismatch(inner),
        And(a, b) | Or(a, b) => {
            formula_uses_allocator_mismatch(a) || formula_uses_allocator_mismatch(b)
        }
        Exists { body, .. }
        | ForAll { body, .. }
        | ExistsAlloc { body, .. }
        | ForAllAlloc { body, .. } => formula_uses_allocator_mismatch(body),
        Path { formula, .. } => match formula {
            PathFormula::State(s)
            | PathFormula::Next(s)
            | PathFormula::Eventually(s)
            | PathFormula::Globally(s) => formula_uses_allocator_mismatch(s),
            PathFormula::Until(a, b) => {
                formula_uses_allocator_mismatch(a) || formula_uses_allocator_mismatch(b)
            }
        },
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
        May { .. } | Label { .. } | StructuralLabel { .. } => 0,
        Not(_) => 0,
        And(a, b) => {
            necessary_positive_may_mask(a, logic_var)
                | necessary_positive_may_mask(b, logic_var)
        }
        Or(a, b) => {
            necessary_positive_may_mask(a, logic_var)
                & necessary_positive_may_mask(b, logic_var)
        }
        Exists { logic_var: bound, .. }
        | ForAll { logic_var: bound, .. }
        | ExistsAlloc { logic_var: bound, .. }
        | ForAllAlloc { logic_var: bound, .. }
            if bound == logic_var =>
        {
            // The nested binder shadows the outer variable.
            0
        }
        Exists { body, .. }
        | ForAll { body, .. }
        | ExistsAlloc { body, .. }
        | ForAllAlloc { body, .. } => {
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
    use crate::parser::{parse_query, parse_query_document};
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
            labels,
            semantic_labels: vec![],
            allocation_labels: vec![],
            identity: None,
            event_identity: None,
                    allocation_post: None,
            pre,
            post,
        }
    }

    #[test]
    fn quantifier_domain_includes_c_variables() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "c0".into(),
            variables: vec![var("c::malloc::ret", ProgramLanguage::C)],
            allocations: vec![],
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
            capabilities: vec![],
            entry: "c0".into(),
            variables: vec![var("c::p", ProgramLanguage::C)],
            allocations: vec![],
            nodes: vec![node(
                "c0", &[],
                vec![EventLabel { predicate: EventKind::Read, variable: "c::p".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query("use_l(x)").unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("c::p".into()))]);
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::True);
    }

    #[test]
    fn top_makes_all_supported_may_atoms_unknown_not_true() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![node(
                "b0", &[], vec![], AbstractMemoryAnnotation::default(),
                mem(&[(&["rust::x"], CellValue::Top)]),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let mc = ModelChecker::new(&k);
        for text in ["alloc(x)", "drop(x)", "own_forg(x)"] {
            let q = parse_query(text).unwrap();
            let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
            assert_eq!(mc.evaluate(&q, &env).unwrap(), Truth::Unknown, "{text}");
        }
    }

    #[test]
    fn strong_next_is_false_at_terminal_nodes() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(), variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
        for text in ["EX use_l(x)", "AX use_l(x)"] {
            let q = parse_query(text).unwrap();
            assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::False);
        }
    }

    #[test]
    fn global_at_terminal_checks_current_position_only() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(), variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }],
                AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default(),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::p", ProgramLanguage::Rust), var("c::arg0", ProgramLanguage::C)],
            allocations: vec![],
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![EventLabel { predicate: EventKind::Read, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![use_x()], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![use_x()], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![
                node("b0", &["b1", "b2"], vec![EventLabel { predicate: EventKind::Alloc, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![EventLabel { predicate: EventKind::Drop, variable: "rust::x".into() }], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b2", &[], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
            nodes: vec![
                node("b0", &["b1"], vec![], AbstractMemoryAnnotation::default(), AbstractMemoryAnnotation::default()),
                node("b1", &[], vec![], AbstractMemoryAnnotation::default(), mem(&[(&["rust::x"], CellValue::Top)])),
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::x".into()))]);
        assert_eq!(ModelChecker::new(&k).evaluate(&parse_query("EF alloc(x)").unwrap(), &env).unwrap(), Truth::Unknown);
    }

    #[test]
    fn forall_program_variables_ranges_over_rust_and_c() {
        // Only the Rust variable is used. The implementation quantifier domain
        // contains both Rust and C variables, so exists is tt and forall is ff.
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust), var("c::p", ProgramLanguage::C)],
            allocations: vec![],
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::p", ProgramLanguage::Rust), var("c::arg0", ProgramLanguage::C)],
            allocations: vec![],
            nodes: vec![node(
                "b0", &[], vec![EventLabel { predicate: EventKind::Drop, variable: "c::arg0".into() }],
                mem(&[(&["rust::p", "c::arg0"], CellValue::Alloc)]),
                mem(&[(&["rust::p", "c::arg0"], CellValue::Freed)]),
            )],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let env = BTreeMap::from([("x".into(), Binding::ProgramVar("rust::p".into()))]);
        let q = parse_query("drop_l(x)").unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &env).unwrap(), Truth::True);
    }

    #[test]
    fn existential_candidate_pruning_preserves_refutation_when_required_alloc_is_absent() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![
                var("rust::x", ProgramLanguage::Rust),
                var("c::p", ProgramLanguage::C),
            ],
            allocations: vec![],
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
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![var("rust::x", ProgramLanguage::Rust)],
            allocations: vec![],
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


    #[test]
    fn allocation_quantifier_correlates_may_events_by_abstract_allocation_id() {
        use crate::kripke::{
            AbstractAllocation, AllocationEventCertainty, AllocationEventLabel, AnnotatedIcfg,
            AnnotatedNode, EventKind, ProgramLanguage, ProgramVariable,
        };

        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec![],
            entry: "b0".into(),
            variables: vec![ProgramVariable {
                id: "Local(_1)".into(),
                language: ProgramLanguage::Rust,
                display: None,
                function: None,
            }],
            allocations: vec![AbstractAllocation { id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None }],
            nodes: vec![
                AnnotatedNode {
                    semantic_labels: vec![],
                    id: "b0".into(), successors: vec!["b1".into()], labels: vec![],
                    allocation_labels: vec![AllocationEventLabel {
                        predicate: EventKind::Alloc, allocation: "A".into(),
                        certainty: AllocationEventCertainty::MayAbstract,
                        deallocator_contract: None,
                    }],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
                AnnotatedNode {
                    semantic_labels: vec![],
                    id: "b1".into(), successors: vec![], labels: vec![],
                    allocation_labels: vec![AllocationEventLabel {
                        predicate: EventKind::Drop, allocation: "A".into(),
                        certainty: AllocationEventCertainty::MayAbstract,
                        deallocator_contract: None,
                    }],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query("exists_alloc a. EF (alloc_l(a) && EX EF drop_l(a))").unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap(), Truth::Unknown);
    }

    #[test]
    fn allocation_state_may_predicate_requires_query_capability_and_preserves_may_truth() {
        use crate::kripke::{
            AbstractAllocation, AbstractAllocationCell, AbstractAllocationMemoryAnnotation,
            AnnotatedIcfg, AnnotatedNode, ProgramLanguage, ProgramVariable,
        };
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec!["allocation_state_v1".into()], entry: "b0".into(),
            variables: vec![ProgramVariable { id: "Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: None }],
            allocations: vec![AbstractAllocation { id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None }],
            nodes: vec![AnnotatedNode {
                id: "b0".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![],
                identity: None, event_identity: None,
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
                }),
                pre: Default::default(), post: Default::default(),
            }],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();

        let legacy = parse_query("exists_alloc a. alloc(a)").unwrap();
        let err = ModelChecker::new(&k).evaluate(&legacy, &Env::new()).unwrap_err();
        assert!(err.contains("query document"), "unexpected error: {err}");
        assert!(err.contains("allocation_state_v1"), "unexpected error: {err}");

        let missing_requires = parse_query_document("exists_alloc a. alloc(a)").unwrap();
        let err = ModelChecker::new(&k).evaluate_document(&missing_requires, &Env::new()).unwrap_err();
        assert!(err.contains("explicit"), "unexpected error: {err}");
        assert!(err.contains("allocation_state_v1"), "unexpected error: {err}");

        let doc = parse_query_document(
            "requires allocation_state_v1; exists_alloc a. alloc(a)"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap(), Truth::Unknown);
    }

    #[test]
    fn allocation_state_query_requires_artifact_capability() {
        use crate::kripke::{AbstractAllocation, AnnotatedIcfg, AnnotatedNode, ProgramLanguage, ProgramVariable};
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec![], entry: "b0".into(),
            variables: vec![ProgramVariable { id: "Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: None }],
            allocations: vec![AbstractAllocation { id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None }],
            nodes: vec![AnnotatedNode { id: "b0".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() }],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let doc = parse_query_document(
            "requires allocation_state_v1; exists_alloc a. alloc(a)"
        ).unwrap();
        let err = ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap_err();
        assert!(err.contains("does not declare"));
    }

    #[test]
    fn allocation_quantifier_requires_schema_v2() {
        use crate::kripke::{AnnotatedIcfg, AnnotatedNode, ProgramLanguage, ProgramVariable};
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![], entry: "b0".into(),
            variables: vec![ProgramVariable { id: "Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: None }],
            allocations: vec![],
            nodes: vec![AnnotatedNode { id: "b0".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() }],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let q = parse_query("exists_alloc a. drop_l(a)").unwrap();
        let err = ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap_err();
        assert!(err.contains("requires CQPL annotated-ICFG schema v2+"));
    }

    fn contract(family: &str, operation: &str, language: &str) -> AllocationContract {
        AllocationContract { family: family.into(), operation: operation.into(), language: language.into(), basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None }
    }

    fn mismatch_fixture(allocator_family: &str, deallocator_family: &str) -> Kripke {
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec!["allocation_contracts_v1".into()],
            entry: "b0".into(),
            variables: vec![var("rust::main::Local(_1)", ProgramLanguage::Rust)],
            allocations: vec![AbstractAllocation {
                id: "A".into(), display: None, site: None, context: vec![],
                allocator_contract: Some(contract(
                    allocator_family,
                    if allocator_family == "c_malloc" { "malloc" } else if allocator_family == "rust_global" { "box_allocation" } else { "unknown" },
                    if allocator_family == "c_malloc" { "c" } else if allocator_family == "rust_global" { "rust" } else { "unknown" },
                )),
            }],
            nodes: vec![AnnotatedNode {
                semantic_labels: vec![],
                id: "b0".into(), successors: vec![], labels: vec![],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop, allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: Some(contract(
                        deallocator_family,
                        if deallocator_family == "c_malloc" { "free" } else if deallocator_family == "rust_global" { "dealloc" } else { "drop" },
                        if deallocator_family == "c_malloc" { "c" } else if deallocator_family == "rust_global" { "rust" } else { "unknown" },
                    )),
                }],
                identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
            }],
        };
        Kripke::from_annotated_icfg(input).unwrap()
    }

    #[test]
    fn allocator_mismatch_query_requires_explicit_query_capability() {
        let k = mismatch_fixture("rust_global", "c_malloc");
        let q = parse_query("exists_alloc a. EF allocator_mismatch_l(a)").unwrap();
        let err = ModelChecker::new(&k).evaluate(&q, &Env::new()).unwrap_err();
        assert!(err.contains("requires a query document"));

        let doc = parse_query_document("exists_alloc a. EF allocator_mismatch_l(a)").unwrap();
        let err = ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap_err();
        assert!(err.contains("requires explicit"));
    }

    #[test]
    fn allocator_mismatch_query_requires_artifact_capability() {
        let mut input = AnnotatedIcfg {
            schema_version: 2, capabilities: vec![], entry: "b0".into(),
            variables: vec![var("v", ProgramLanguage::Rust)], allocations: vec![],
            nodes: vec![node("b0", &[], vec![], Default::default(), Default::default())],
        };
        let k = Kripke::from_annotated_icfg(input.clone()).unwrap();
        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF allocator_mismatch_l(a)"
        ).unwrap();
        let err = ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap_err();
        assert!(err.contains("does not declare"));

        // The fixture remains valid without the capability even though contracts
        // are absent: unsupported is rejected by the query boundary, not refuted.
        input.capabilities.clear();
        assert!(Kripke::from_annotated_icfg(input).is_ok());
    }

    #[test]
    fn allocator_mismatch_semantics_are_narrow_and_may_only() {
        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF allocator_mismatch_l(a)"
        ).unwrap();
        for (alloc, dealloc, expected) in [
            ("rust_global", "c_malloc", Truth::Unknown),
            ("c_malloc", "c_malloc", Truth::False),
            ("c_malloc", "rust_global", Truth::Unknown),
            ("unknown", "c_malloc", Truth::Unknown),
            ("rust_global", "unknown", Truth::Unknown),
        ] {
            let k = mismatch_fixture(alloc, dealloc);
            assert_eq!(
                ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap(),
                expected,
                "allocator={alloc} deallocator={dealloc}"
            );
        }
    }

    #[test]
    fn allocator_mismatch_absence_is_refuted() {
        let mut k = mismatch_fixture("rust_global", "c_malloc");
        k.nodes.get_mut("b0").unwrap().allocation_labels.clear();
        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF allocator_mismatch_l(a)"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap(), Truth::False);
    }


    #[test]
    fn allocator_mismatch_query_accepts_allocation_contracts_v2_requirement() {
        let mut input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec!["allocation_contracts_v1".into(), "allocation_contracts_v2".into()],
            entry: "b0".into(),
            variables: vec![var("rust::main::Local(_1)", ProgramLanguage::Rust)],
            allocations: vec![AbstractAllocation {
                id: "A".into(),
                display: None,
                site: None,
                context: vec![],
                // v2 deliberately preserves the frozen v1 allocator-origin contract.
                allocator_contract: Some(contract("c_malloc", "malloc", "c")),
            }],
            nodes: vec![AnnotatedNode {
                id: "b0".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec![],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: Some(AllocationContract {
                        family: "c_malloc".into(),
                        operation: "free".into(),
                        language: "c".into(),
                        basis: Some("structural_c_free_v1".into()),
                        owner_def_path: None,
                        allocator_def_path: None,
                        callee_def_path: None,
                    }),
                }],
                identity: None,
                event_identity: None,
                allocation_post: None,
                pre: Default::default(),
                post: Default::default(),
            }],
        };
        let k = Kripke::from_annotated_icfg(input.clone()).unwrap();
        let doc = parse_query_document(
            "requires allocation_contracts_v2; exists_alloc a. EF allocator_mismatch_l(a)"
        ).unwrap();
        assert_eq!(ModelChecker::new(&k).evaluate_document(&doc, &Env::new()).unwrap(), Truth::False);

        // A v2 query must not silently fall back to v1 artifacts.
        input.capabilities = vec!["allocation_contracts_v1".into()];
        let deallocator = input.nodes[0].allocation_labels[0].deallocator_contract.as_mut().unwrap();
        deallocator.basis = None;
        let k_v1 = Kripke::from_annotated_icfg(input).unwrap();
        let err = ModelChecker::new(&k_v1).evaluate_document(&doc, &Env::new()).unwrap_err();
        assert!(err.contains("does not declare"), "unexpected error: {err}");
    }


    #[test]
    fn structural_mir_labels_are_capability_gated_and_queryable() {
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec!["mir_semantic_labels_v1".into(), "mir_semantics_v2".into()],
            entry: "b0".into(),
            variables: vec![ProgramVariable {
                id: "Local(_0)".into(),
                language: ProgramLanguage::Rust,
                display: None,
                function: None,
            }],
            allocations: vec![],
            nodes: vec![AnnotatedNode {
                id: "b0".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec![
                    "stmt:assign".into(),
                    "rvalue:ptr_metadata".into(),
                    "term:return".into(),
                ],
                allocation_labels: vec![],
                identity: Some(Default::default()),
                event_identity: Some(Default::default()),
                allocation_post: None,
                pre: Default::default(),
                post: Default::default(),
            }],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let mc = ModelChecker::new(&k);
        for query in [
            "requires mir_semantic_labels_v1; EF stmt_l(assign)",
            "requires mir_semantic_labels_v1; EF rvalue_l(ptr_metadata)",
            "requires mir_semantic_labels_v1; EF term_l(return)",
        ] {
            let doc = parse_query_document(query).unwrap();
            assert_eq!(mc.evaluate_document(&doc, &Env::new()).unwrap(), Truth::True);
        }
        let missing = parse_query_document("EF term_l(return)").unwrap();
        assert!(mc.evaluate_document(&missing, &Env::new()).is_err());
    }

}
