use crate::ast::{LabelPredicate, MayPredicate, PathFormula, PathQuantifier, QueryDocument, StateFormula, StructuralLabelKind};
use crate::kripke::{AllocationEventCertainty, CellValue, EventKind};
use crate::model_checker::{Binding, Env, ModelChecker};
use crate::truth::Truth;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const EXPLAINABILITY_TAXONOMY_VERSION: &str = "cqpl_uncertainty_reasons_v1";

/// Stable diagnostic vocabulary for CQPL v6R.
///
/// Scientific rule: a reason is emitted only when it is directly supported by
/// the annotated ICFG or by the three-valued model-checking derivation.  Codes
/// that require producer-side provenance (EXTERNAL_EFFECT, UNRESOLVED_ESCAPE,
/// GLOBAL_TOP_EFFECT, CONTROL_FLOW_UNRESOLVED, HIGHER_ORDER_UNRESOLVED) are
/// reserved here but are never guessed from a TOP value or from a DefPath.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UncertaintyReason {
    MayAllocation,
    MayDeallocation,
    MayUse,
    MayOwnership,
    AliasJoin,
    AbstractComponentMerge,
    AbstractTopState,
    UnresolvedContract,
    ExternalEffect,
    UnresolvedEscape,
    GlobalTopEffect,
    ControlFlowUnresolved,
    HigherOrderUnresolved,
    PathJoin,
    QueryThreeValuedPropagation,
}

impl UncertaintyReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::MayAllocation => "MAY_ALLOCATION",
            Self::MayDeallocation => "MAY_DEALLOCATION",
            Self::MayUse => "MAY_USE",
            Self::MayOwnership => "MAY_OWNERSHIP",
            Self::AliasJoin => "ALIAS_JOIN",
            Self::AbstractComponentMerge => "ABSTRACT_COMPONENT_MERGE",
            Self::AbstractTopState => "ABSTRACT_TOP_STATE",
            Self::UnresolvedContract => "UNRESOLVED_CONTRACT",
            Self::ExternalEffect => "EXTERNAL_EFFECT",
            Self::UnresolvedEscape => "UNRESOLVED_ESCAPE",
            Self::GlobalTopEffect => "GLOBAL_TOP_EFFECT",
            Self::ControlFlowUnresolved => "CONTROL_FLOW_UNRESOLVED",
            Self::HigherOrderUnresolved => "HIGHER_ORDER_UNRESOLVED",
            Self::PathJoin => "PATH_JOIN",
            Self::QueryThreeValuedPropagation => "QUERY_THREE_VALUED_PROPAGATION",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    ProgramVar,
    Allocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplanationBinding {
    pub kind: BindingKind,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DerivationStep {
    pub node: String,
    pub formula_kind: String,
    pub truth: &'static str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub detail: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtomicObservation {
    pub node: String,
    pub atom: String,
    pub truth: &'static str,
    pub binding: BTreeMap<String, ExplanationBinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<UncertaintyReason>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub detail: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplanationWitness {
    pub truth: &'static str,
    pub binding: BTreeMap<String, ExplanationBinding>,
    pub relevant_nodes: Vec<String>,
    pub reasons: Vec<UncertaintyReason>,
    pub derivation: Vec<DerivationStep>,
    pub atomic_observations: Vec<AtomicObservation>,
    /// `true` for the existential CTL fragment used by the official memory
    /// queries.  Universal CTL explanations are representative dependency
    /// traces rather than complete proof trees and therefore set this false.
    pub complete_dependency_trace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplanationReport {
    pub schema: &'static str,
    pub taxonomy: &'static str,
    pub result: &'static str,
    pub entry: String,
    pub scope_note: &'static str,
    pub reason_frontier: Vec<UncertaintyReason>,
    pub reason_counts: BTreeMap<String, usize>,
    pub witnesses: Vec<ExplanationWitness>,
    pub diagnostics: ExplanationDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplanationDiagnostics {
    pub witnesses_requested: usize,
    pub witnesses_emitted: usize,
    pub unknown_has_reason_frontier: bool,
    pub unknown_has_specific_origin: bool,
    pub true_has_witness: bool,
    pub true_has_atomic_witness: bool,
    pub producer_provenance_capability_present: bool,
    pub reserved_reason_codes_not_inferred: Vec<UncertaintyReason>,
}

#[derive(Debug, Clone)]
struct Trace {
    relevant_nodes: Vec<String>,
    reasons: BTreeSet<UncertaintyReason>,
    derivation: Vec<DerivationStep>,
    atoms: Vec<AtomicObservation>,
    complete: bool,
}

impl Trace {
    fn new(node: &str, formula_kind: &str, truth: Truth) -> Self {
        Self {
            relevant_nodes: vec![node.to_string()],
            reasons: BTreeSet::new(),
            derivation: vec![DerivationStep {
                node: node.to_string(),
                formula_kind: formula_kind.to_string(),
                truth: truth.as_str(),
                detail: BTreeMap::new(),
            }],
            atoms: Vec::new(),
            complete: true,
        }
    }

    fn add_node(&mut self, node: &str) {
        if !self.relevant_nodes.iter().any(|n| n == node) {
            self.relevant_nodes.push(node.to_string());
        }
    }

    fn merge(&mut self, other: Trace) {
        for node in other.relevant_nodes {
            self.add_node(&node);
        }
        self.reasons.extend(other.reasons);
        self.derivation.extend(other.derivation);
        self.atoms.extend(other.atoms);
        self.complete &= other.complete;
    }
}

fn snapshot_env(env: &Env) -> BTreeMap<String, ExplanationBinding> {
    env.iter()
        .map(|(logic, binding)| {
            let value = match binding {
                Binding::ProgramVar(v) => ExplanationBinding {
                    kind: BindingKind::ProgramVar,
                    value: v.clone(),
                },
                Binding::Allocation(a) => ExplanationBinding {
                    kind: BindingKind::Allocation,
                    value: a.clone(),
                },
            };
            (logic.clone(), value)
        })
        .collect()
}

fn formula_kind(formula: &StateFormula) -> &'static str {
    match formula {
        StateFormula::May { .. } => "may_atom",
        StateFormula::Label { .. } => "event_atom",
        StateFormula::StructuralLabel { .. } => "structural_atom",
        StateFormula::Not(_) => "not",
        StateFormula::And(_, _) => "and",
        StateFormula::Or(_, _) => "or",
        StateFormula::Exists { .. } => "exists_program_var",
        StateFormula::ForAll { .. } => "forall_program_var",
        StateFormula::ExistsAlloc { .. } => "exists_allocation",
        StateFormula::ForAllAlloc { .. } => "forall_allocation",
        StateFormula::Path { formula, .. } => match formula {
            PathFormula::State(_) => "path_state",
            PathFormula::Next(_) => "next",
            PathFormula::Until(_, _) => "until",
            PathFormula::Eventually(_) => "eventually",
            PathFormula::Globally(_) => "globally",
        },
    }
}

fn may_reason(predicate: MayPredicate) -> UncertaintyReason {
    match predicate {
        MayPredicate::Alloc => UncertaintyReason::MayAllocation,
        MayPredicate::Drop => UncertaintyReason::MayDeallocation,
        MayPredicate::OwnForg => UncertaintyReason::MayOwnership,
    }
}

fn label_reason(predicate: LabelPredicate) -> UncertaintyReason {
    match predicate {
        LabelPredicate::Alloc => UncertaintyReason::MayAllocation,
        LabelPredicate::Drop | LabelPredicate::AllocatorMismatch => UncertaintyReason::MayDeallocation,
        LabelPredicate::Read | LabelPredicate::Write | LabelPredicate::Use => UncertaintyReason::MayUse,
    }
}

fn may_name(predicate: MayPredicate) -> &'static str {
    match predicate {
        MayPredicate::Alloc => "alloc",
        MayPredicate::Drop => "drop",
        MayPredicate::OwnForg => "own_forg",
    }
}

fn label_name(predicate: LabelPredicate) -> &'static str {
    match predicate {
        LabelPredicate::Alloc => "alloc_l",
        LabelPredicate::Drop => "drop_l",
        LabelPredicate::Read => "read_l",
        LabelPredicate::Write => "write_l",
        LabelPredicate::Use => "use_l",
        LabelPredicate::AllocatorMismatch => "allocator_mismatch_l",
    }
}

fn structural_name(kind: StructuralLabelKind, name: &str) -> String {
    let prefix = match kind {
        StructuralLabelKind::Statement => "stmt_l",
        StructuralLabelKind::Rvalue => "rvalue_l",
        StructuralLabelKind::Terminator => "term_l",
    };
    format!("{prefix}({name})")
}

impl<'a> ModelChecker<'a> {
    /// Explain one capability-checked CQPL document without changing its
    /// semantics.  The semantic result is obtained first through the frozen
    /// evaluator.  Explanation is a read-only second pass over the same
    /// valuations and annotated ICFG.
    pub fn explain_document(
        &self,
        document: &QueryDocument,
        initial_env: &Env,
        max_witnesses: usize,
    ) -> Result<ExplanationReport, String> {
        let result = self.evaluate_document(document, initial_env)?;
        let max_witnesses = max_witnesses.max(1);

        let mut witnesses = if result == Truth::False {
            Vec::new()
        } else {
            self.explain_state(
                &document.formula,
                initial_env,
                &self.k.entry,
                result,
                max_witnesses,
                0,
                &mut BTreeSet::new(),
            )?
        };
        witnesses.truncate(max_witnesses);

        if result == Truth::Unknown {
            for witness in &mut witnesses {
                witness.reasons.insert(
                    witness.reasons.len(),
                    UncertaintyReason::QueryThreeValuedPropagation,
                );
                witness.reasons.sort();
                witness.reasons.dedup();
            }
        }

        let mut frontier = BTreeSet::new();
        let mut reason_counts: BTreeMap<String, usize> = BTreeMap::new();
        for witness in &witnesses {
            for reason in &witness.reasons {
                frontier.insert(*reason);
                *reason_counts.entry(reason.code().to_string()).or_insert(0) += 1;
            }
        }
        let reason_frontier: Vec<_> = frontier.into_iter().collect();

        let unknown_has_reason_frontier = result != Truth::Unknown || !reason_frontier.is_empty();
        let unknown_has_specific_origin = result != Truth::Unknown || reason_frontier.iter().any(|reason| {
            !matches!(reason,
                UncertaintyReason::QueryThreeValuedPropagation | UncertaintyReason::PathJoin
            )
        });
        let true_has_witness = result != Truth::True || !witnesses.is_empty();
        let true_has_atomic_witness = result != Truth::True || witnesses.iter().any(|w| !w.atomic_observations.is_empty());
        if !unknown_has_reason_frontier {
            return Err("v6R explainability invariant violated: unk result has empty reason frontier".into());
        }
        if !unknown_has_specific_origin {
            return Err("v6R explainability invariant violated: unk result has no atomically supported uncertainty origin".into());
        }
        if !true_has_witness {
            return Err("v6R explainability invariant violated: tt result has no witness".into());
        }
        if !true_has_atomic_witness {
            return Err("v6R explainability invariant violated: tt result has no atomic witness endpoint".into());
        }

        Ok(ExplanationReport {
            schema: "cqpl_explanation_v1",
            taxonomy: EXPLAINABILITY_TAXONOMY_VERSION,
            result: result.as_str(),
            entry: self.k.entry.clone(),
            scope_note: "explanations are over the already-projected annotated abstract Kripke model; they do not assert a concrete execution",
            reason_frontier,
            reason_counts,
            diagnostics: ExplanationDiagnostics {
                witnesses_requested: max_witnesses,
                witnesses_emitted: witnesses.len(),
                unknown_has_reason_frontier,
                unknown_has_specific_origin,
                true_has_witness,
                true_has_atomic_witness,
                producer_provenance_capability_present: self.k.capabilities.contains("uncertainty_provenance_v1"),
                reserved_reason_codes_not_inferred: vec![
                    UncertaintyReason::ExternalEffect,
                    UncertaintyReason::UnresolvedEscape,
                    UncertaintyReason::GlobalTopEffect,
                    UncertaintyReason::ControlFlowUnresolved,
                    UncertaintyReason::HigherOrderUnresolved,
                ],
            },
            witnesses,
        })
    }

    fn value_at(&self, formula: &StateFormula, env: &Env, node: &str) -> Result<Truth, String> {
        self.eval_all(formula, env)?
            .get(node)
            .copied()
            .ok_or_else(|| format!("explainability: node '{node}' missing from valuation"))
    }

    fn explain_state(
        &self,
        formula: &StateFormula,
        env: &Env,
        node: &str,
        target: Truth,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        if depth > self.k.nodes.len().saturating_mul(4).max(64) {
            let mut trace = Trace::new(node, formula_kind(formula), target);
            trace.reasons.insert(UncertaintyReason::PathJoin);
            trace.complete = false;
            return Ok(vec![self.finish_trace(trace, env, target)]);
        }

        let actual = self.value_at(formula, env, node)?;
        if actual != target {
            return Ok(Vec::new());
        }

        use StateFormula::*;
        match formula {
            May { predicate, logic_var } => {
                let trace = self.atomic_may_trace(node, env, *predicate, logic_var, target)?;
                Ok(vec![self.finish_trace(trace, env, target)])
            }
            Label { predicate, logic_var } => {
                let trace = self.atomic_label_trace(node, env, *predicate, logic_var, target)?;
                Ok(vec![self.finish_trace(trace, env, target)])
            }
            StructuralLabel { kind, name } => {
                let mut trace = Trace::new(node, "structural_atom", target);
                trace.atoms.push(AtomicObservation {
                    node: node.to_string(),
                    atom: structural_name(*kind, name),
                    truth: target.as_str(),
                    binding: snapshot_env(env),
                    reasons: Vec::new(),
                    detail: BTreeMap::new(),
                });
                Ok(vec![self.finish_trace(trace, env, target)])
            }
            Not(inner) => {
                let child_target = target.not();
                let mut traces = self.explain_state(inner, env, node, child_target, limit, depth + 1, seen)?;
                for witness in &mut traces {
                    prepend_node(witness, node, "not", target);
                }
                Ok(traces)
            }
            And(a, b) => {
                let av = self.value_at(a, env, node)?;
                let bv = self.value_at(b, env, node)?;
                let mut selected = Vec::new();
                match target {
                    Truth::True => selected.extend([(a.as_ref(), av), (b.as_ref(), bv)]),
                    Truth::Unknown => {
                        if av == Truth::Unknown { selected.push((a.as_ref(), av)); }
                        if bv == Truth::Unknown { selected.push((b.as_ref(), bv)); }
                    }
                    Truth::False => {
                        if av == Truth::False { selected.push((a.as_ref(), av)); }
                        else if bv == Truth::False { selected.push((b.as_ref(), bv)); }
                    }
                }
                self.merge_selected(formula, env, node, target, selected, limit, depth, seen)
            }
            Or(a, b) => {
                let av = self.value_at(a, env, node)?;
                let bv = self.value_at(b, env, node)?;
                let mut selected = Vec::new();
                match target {
                    Truth::True => {
                        if av == Truth::True { selected.push((a.as_ref(), av)); }
                        else if bv == Truth::True { selected.push((b.as_ref(), bv)); }
                    }
                    Truth::Unknown => {
                        if av == Truth::Unknown { selected.push((a.as_ref(), av)); }
                        if bv == Truth::Unknown { selected.push((b.as_ref(), bv)); }
                    }
                    Truth::False => selected.extend([(a.as_ref(), av), (b.as_ref(), bv)]),
                }
                self.merge_selected(formula, env, node, target, selected, limit, depth, seen)
            }
            Exists { logic_var, body } => {
                let mut out = Vec::new();
                for value in self.k.variable_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::ProgramVar(value.clone()));
                    if self.value_at(body, &next_env, node)? == target {
                        let mut traces = self.explain_state(body, &next_env, node, target, limit - out.len(), depth + 1, seen)?;
                        for witness in &mut traces { prepend_node(witness, node, "exists_program_var", target); }
                        out.extend(traces);
                        if out.len() >= limit { break; }
                    }
                }
                Ok(out)
            }
            ExistsAlloc { logic_var, body } => {
                let mut out = Vec::new();
                for value in self.k.allocation_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::Allocation(value.clone()));
                    if self.value_at(body, &next_env, node)? == target {
                        let mut traces = self.explain_state(body, &next_env, node, target, limit - out.len(), depth + 1, seen)?;
                        for witness in &mut traces { prepend_node(witness, node, "exists_allocation", target); }
                        out.extend(traces);
                        if out.len() >= limit { break; }
                    }
                }
                Ok(out)
            }
            ForAll { logic_var, body } => {
                let mut out = Vec::new();
                for value in self.k.variable_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::ProgramVar(value.clone()));
                    if self.value_at(body, &next_env, node)? == target {
                        let mut traces = self.explain_state(body, &next_env, node, target, limit - out.len(), depth + 1, seen)?;
                        for witness in &mut traces {
                            prepend_node(witness, node, "forall_program_var", target);
                            witness.complete_dependency_trace = false;
                        }
                        out.extend(traces);
                        if out.len() >= limit { break; }
                    }
                }
                Ok(out)
            }
            ForAllAlloc { logic_var, body } => {
                let mut out = Vec::new();
                for value in self.k.allocation_ids() {
                    let mut next_env = env.clone();
                    next_env.insert(logic_var.clone(), Binding::Allocation(value.clone()));
                    if self.value_at(body, &next_env, node)? == target {
                        let mut traces = self.explain_state(body, &next_env, node, target, limit - out.len(), depth + 1, seen)?;
                        for witness in &mut traces {
                            prepend_node(witness, node, "forall_allocation", target);
                            witness.complete_dependency_trace = false;
                        }
                        out.extend(traces);
                        if out.len() >= limit { break; }
                    }
                }
                Ok(out)
            }
            Path { quantifier, formula: path } => {
                self.explain_path(*quantifier, path, env, node, target, limit, depth, seen)
            }
        }
    }

    fn merge_selected(
        &self,
        parent: &StateFormula,
        env: &Env,
        node: &str,
        target: Truth,
        selected: Vec<(&StateFormula, Truth)>,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        if selected.is_empty() {
            let mut trace = Trace::new(node, formula_kind(parent), target);
            if target == Truth::Unknown {
                trace.reasons.insert(UncertaintyReason::QueryThreeValuedPropagation);
            }
            trace.complete = false;
            return Ok(vec![self.finish_trace(trace, env, target)]);
        }

        let mut merged: Option<ExplanationWitness> = None;
        for (child, child_target) in selected {
            let mut traces = self.explain_state(child, env, node, child_target, 1, depth + 1, seen)?;
            let Some(next) = traces.pop() else { continue; };
            merged = Some(match merged {
                None => next,
                Some(mut current) => {
                    merge_witness(&mut current, next);
                    current
                }
            });
        }
        let mut out = merged.into_iter().collect::<Vec<_>>();
        for witness in &mut out {
            prepend_node(witness, node, formula_kind(parent), target);
        }
        out.truncate(limit);
        Ok(out)
    }

    fn explain_path(
        &self,
        quantifier: PathQuantifier,
        path: &PathFormula,
        env: &Env,
        node: &str,
        target: Truth,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        match path {
            PathFormula::State(phi) => self.explain_state(phi, env, node, target, limit, depth + 1, seen),
            PathFormula::Next(phi) => {
                let successors = self.successors(node);
                if successors.is_empty() { return Ok(Vec::new()); }
                let mut out = Vec::new();
                for succ in successors {
                    let value = self.value_at(phi, env, &succ)?;
                    if value == target {
                        let mut traces = self.explain_state(phi, env, &succ, target, limit - out.len(), depth + 1, seen)?;
                        for witness in &mut traces {
                            prepend_node(witness, node, "next", target);
                            if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
                        }
                        out.extend(traces);
                        if out.len() >= limit { break; }
                    }
                }
                Ok(out)
            }
            PathFormula::Eventually(phi) => {
                self.explain_eventually(quantifier, phi, env, node, target, limit, depth, seen)
            }
            PathFormula::Globally(phi) => {
                self.explain_globally(quantifier, phi, env, node, target, limit, depth, seen)
            }
            PathFormula::Until(lhs, rhs) => {
                self.explain_until(quantifier, lhs, rhs, env, node, target, limit, depth, seen)
            }
        }
    }

    fn explain_eventually(
        &self,
        quantifier: PathQuantifier,
        phi: &StateFormula,
        env: &Env,
        start: &str,
        target: Truth,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        let wrapped = StateFormula::Path {
            quantifier,
            formula: PathFormula::Eventually(Box::new(phi.clone())),
        };
        let z = self.eval_all(&wrapped, env)?;
        let phi_v = self.eval_all(phi, env)?;
        let path = self.bfs_to(start, |n| phi_v.get(n).copied() == Some(target), |n| {
            z.get(n).copied().unwrap_or(Truth::False) != Truth::False
        });
        let Some(path_nodes) = path else {
            let mut trace = Trace::new(start, "eventually", target);
            trace.reasons.insert(UncertaintyReason::QueryThreeValuedPropagation);
            trace.complete = false;
            return Ok(vec![self.finish_trace(trace, env, target)]);
        };
        let end = path_nodes.last().unwrap().clone();
        let mut traces = self.explain_state(phi, env, &end, target, limit, depth + 1, seen)?;
        for witness in &mut traces {
            prepend_path(witness, &path_nodes, "eventually", target);
            if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
            if target == Truth::Unknown && self.path_has_mixed_successor_truth(&z, &path_nodes) {
                push_reason(witness, UncertaintyReason::PathJoin);
            }
        }
        Ok(traces)
    }

    fn explain_globally(
        &self,
        quantifier: PathQuantifier,
        phi: &StateFormula,
        env: &Env,
        start: &str,
        target: Truth,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        let wrapped = StateFormula::Path {
            quantifier,
            formula: PathFormula::Globally(Box::new(phi.clone())),
        };
        let z = self.eval_all(&wrapped, env)?;
        let phi_v = self.eval_all(phi, env)?;

        if target == Truth::Unknown {
            if let Some(path_nodes) = self.bfs_to(start, |n| phi_v.get(n).copied() == Some(Truth::Unknown), |n| {
                z.get(n).copied().unwrap_or(Truth::False) != Truth::False
            }) {
                let end = path_nodes.last().unwrap().clone();
                let mut traces = self.explain_state(phi, env, &end, Truth::Unknown, limit, depth + 1, seen)?;
                for witness in &mut traces {
                    prepend_path(witness, &path_nodes, "globally", target);
                    if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
                    if self.path_has_mixed_successor_truth(&z, &path_nodes) {
                        push_reason(witness, UncertaintyReason::PathJoin);
                    }
                }
                return Ok(traces);
            }
        }

        // For tt (or a conservative fallback), return a lasso/maximal prefix in
        // the non-refuted subgraph.  This is an abstract-model witness only.
        let path_nodes = self.maximal_prefix(start, |n| {
            let p = phi_v.get(n).copied().unwrap_or(Truth::False);
            let zv = z.get(n).copied().unwrap_or(Truth::False);
            p != Truth::False && zv != Truth::False
        });
        let mut trace = Trace::new(start, "globally", target);
        for n in &path_nodes { trace.add_node(n); }
        trace.derivation.push(DerivationStep {
            node: start.to_string(),
            formula_kind: "globally_abstract_path".into(),
            truth: target.as_str(),
            detail: BTreeMap::from([("path_nodes".into(), path_nodes.len().to_string())]),
        });
        if quantifier == PathQuantifier::ForAll { trace.complete = false; }
        Ok(vec![self.finish_trace(trace, env, target)])
    }

    fn explain_until(
        &self,
        quantifier: PathQuantifier,
        lhs: &StateFormula,
        rhs: &StateFormula,
        env: &Env,
        start: &str,
        target: Truth,
        limit: usize,
        depth: usize,
        seen: &mut BTreeSet<(String, String)>,
    ) -> Result<Vec<ExplanationWitness>, String> {
        let wrapped = StateFormula::Path {
            quantifier,
            formula: PathFormula::Until(Box::new(lhs.clone()), Box::new(rhs.clone())),
        };
        let z = self.eval_all(&wrapped, env)?;
        let rhs_v = self.eval_all(rhs, env)?;
        if let Some(path_nodes) = self.bfs_to(start, |n| rhs_v.get(n).copied() == Some(target), |n| {
            z.get(n).copied().unwrap_or(Truth::False) != Truth::False
        }) {
            let end = path_nodes.last().unwrap().clone();
            let mut traces = self.explain_state(rhs, env, &end, target, limit, depth + 1, seen)?;
            for witness in &mut traces {
                prepend_path(witness, &path_nodes, "until_rhs", target);
                if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
            }
            return Ok(traces);
        }

        let lhs_v = self.eval_all(lhs, env)?;
        if target == Truth::Unknown {
            if let Some(path_nodes) = self.bfs_to(start, |n| lhs_v.get(n).copied() == Some(Truth::Unknown), |n| {
                z.get(n).copied().unwrap_or(Truth::False) != Truth::False
            }) {
                let end = path_nodes.last().unwrap().clone();
                let mut traces = self.explain_state(lhs, env, &end, Truth::Unknown, limit, depth + 1, seen)?;
                for witness in &mut traces {
                    prepend_path(witness, &path_nodes, "until_lhs", target);
                    if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
                }
                return Ok(traces);
            }
        }

        let mut trace = Trace::new(start, "until", target);
        trace.reasons.insert(UncertaintyReason::QueryThreeValuedPropagation);
        trace.complete = false;
        Ok(vec![self.finish_trace(trace, env, target)])
    }

    fn atomic_may_trace(
        &self,
        node_id: &str,
        env: &Env,
        predicate: MayPredicate,
        logic_var: &str,
        truth: Truth,
    ) -> Result<Trace, String> {
        let binding = env.get(logic_var)
            .ok_or_else(|| format!("logical variable '{logic_var}' is unbound"))?;
        let mut trace = Trace::new(node_id, "may_atom", truth);
        let mut reasons = BTreeSet::new();
        let mut detail = BTreeMap::new();
        detail.insert("predicate".into(), may_name(predicate).into());

        if truth == Truth::Unknown {
            reasons.insert(may_reason(predicate));
        }

        let node = self.k.nodes.get(node_id)
            .ok_or_else(|| format!("explainability: missing node '{node_id}'"))?;
        match binding {
            Binding::ProgramVar(var) => {
                let value = node.post.value_of(var);
                detail.insert("program_var".into(), var.clone());
                detail.insert("post_value".into(), format!("{:?}", value));
                if value == CellValue::Top {
                    reasons.insert(UncertaintyReason::AbstractTopState);
                }
                if self.k.aliases_at(node_id, var).len() > 1 {
                    reasons.insert(UncertaintyReason::AliasJoin);
                }
                if identity_component_is_merged(node, Some(var), None) {
                    reasons.insert(UncertaintyReason::AbstractComponentMerge);
                }
            }
            Binding::Allocation(allocation) => {
                let value = node.allocation_post.as_ref()
                    .map(|post| post.value_of(allocation))
                    .unwrap_or(CellValue::Bottom);
                detail.insert("allocation".into(), allocation.clone());
                detail.insert("allocation_post_value".into(), format!("{:?}", value));
                if value == CellValue::Top {
                    reasons.insert(UncertaintyReason::AbstractTopState);
                }
                if identity_component_is_merged(node, None, Some(allocation)) {
                    reasons.insert(UncertaintyReason::AbstractComponentMerge);
                }
            }
        }

        trace.reasons.extend(reasons.iter().copied());
        trace.atoms.push(AtomicObservation {
            node: node_id.to_string(),
            atom: format!("{}({logic_var})", may_name(predicate)),
            truth: truth.as_str(),
            binding: snapshot_env(env),
            reasons: reasons.into_iter().collect(),
            detail,
        });
        Ok(trace)
    }

    fn atomic_label_trace(
        &self,
        node_id: &str,
        env: &Env,
        predicate: LabelPredicate,
        logic_var: &str,
        truth: Truth,
    ) -> Result<Trace, String> {
        let binding = env.get(logic_var)
            .ok_or_else(|| format!("logical variable '{logic_var}' is unbound"))?;
        let mut trace = Trace::new(node_id, "event_atom", truth);
        let mut reasons = BTreeSet::new();
        let mut detail = BTreeMap::new();
        detail.insert("predicate".into(), label_name(predicate).into());

        match binding {
            Binding::ProgramVar(var) => {
                detail.insert("program_var".into(), var.clone());
                let aliases = self.k.aliases_at(node_id, var);
                detail.insert("alias_component_size".into(), aliases.len().to_string());
                if truth == Truth::Unknown && aliases.len() > 1 {
                    reasons.insert(UncertaintyReason::AliasJoin);
                }
            }
            Binding::Allocation(allocation) => {
                detail.insert("allocation".into(), allocation.clone());
                let node = self.k.nodes.get(node_id)
                    .ok_or_else(|| format!("explainability: missing node '{node_id}'"))?;
                let matching: Vec<_> = node.allocation_labels.iter()
                    .filter(|label| allocation_label_matches(self, allocation, predicate, label))
                    .collect();
                detail.insert("matching_allocation_labels".into(), matching.len().to_string());
                if truth == Truth::Unknown && matching.iter().any(|label| label.certainty == AllocationEventCertainty::MayAbstract) {
                    reasons.insert(label_reason(predicate));
                }
                if matching.iter().any(|label| {
                    label.deallocator_contract.as_ref().is_some_and(|contract| {
                        contract.family == "unknown" || contract.basis.as_deref() == Some("unresolved")
                    })
                }) {
                    reasons.insert(UncertaintyReason::UnresolvedContract);
                }
                if identity_component_is_merged(node, None, Some(allocation)) {
                    reasons.insert(UncertaintyReason::AbstractComponentMerge);
                }
            }
        }

        trace.reasons.extend(reasons.iter().copied());
        trace.atoms.push(AtomicObservation {
            node: node_id.to_string(),
            atom: format!("{}({logic_var})", label_name(predicate)),
            truth: truth.as_str(),
            binding: snapshot_env(env),
            reasons: reasons.into_iter().collect(),
            detail,
        });
        Ok(trace)
    }

    fn finish_trace(&self, trace: Trace, env: &Env, truth: Truth) -> ExplanationWitness {
        ExplanationWitness {
            truth: truth.as_str(),
            binding: snapshot_env(env),
            relevant_nodes: trace.relevant_nodes,
            reasons: trace.reasons.into_iter().collect(),
            derivation: trace.derivation,
            atomic_observations: trace.atoms,
            complete_dependency_trace: trace.complete,
        }
    }

    fn successors(&self, node: &str) -> Vec<String> {
        self.k.nodes.get(node)
            .map(|n| n.successors.clone())
            .unwrap_or_default()
    }

    fn bfs_to<Goal, Allowed>(&self, start: &str, goal: Goal, allowed: Allowed) -> Option<Vec<String>>
    where
        Goal: Fn(&str) -> bool,
        Allowed: Fn(&str) -> bool,
    {
        if !allowed(start) { return None; }
        let mut queue = VecDeque::from([start.to_string()]);
        let mut parent: BTreeMap<String, Option<String>> = BTreeMap::from([(start.to_string(), None)]);
        while let Some(node) = queue.pop_front() {
            if goal(&node) {
                let mut path = vec![node.clone()];
                let mut current = node;
                while let Some(Some(prev)) = parent.get(&current) {
                    path.push(prev.clone());
                    current = prev.clone();
                }
                path.reverse();
                return Some(path);
            }
            for succ in self.successors(&node) {
                if allowed(&succ) && !parent.contains_key(&succ) {
                    parent.insert(succ.clone(), Some(node.clone()));
                    queue.push_back(succ);
                }
            }
        }
        None
    }

    fn maximal_prefix<Allowed>(&self, start: &str, allowed: Allowed) -> Vec<String>
    where
        Allowed: Fn(&str) -> bool,
    {
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = start.to_string();
        while allowed(&current) {
            path.push(current.clone());
            if !seen.insert(current.clone()) { break; }
            let Some(next) = self.successors(&current).into_iter().find(|s| allowed(s)) else { break; };
            current = next;
        }
        path
    }

    fn path_has_mixed_successor_truth(&self, z: &BTreeMap<String, Truth>, path: &[String]) -> bool {
        path.iter().any(|node| {
            let values: BTreeSet<_> = self.successors(node).into_iter()
                .filter_map(|s| z.get(&s).copied())
                .collect();
            values.len() > 1
        })
    }
}

fn allocation_label_matches(
    checker: &ModelChecker<'_>,
    allocation: &str,
    predicate: LabelPredicate,
    label: &crate::kripke::AllocationEventLabel,
) -> bool {
    if label.allocation != allocation { return false; }
    match predicate {
        LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
        LabelPredicate::Drop => label.predicate == EventKind::Drop,
        LabelPredicate::Read => label.predicate == EventKind::Read,
        LabelPredicate::Write => label.predicate == EventKind::Write,
        LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
        LabelPredicate::AllocatorMismatch => {
            if label.predicate != EventKind::Drop { return false; }
            let allocator = checker.k.allocations.get(allocation)
                .and_then(|a| a.allocator_contract.as_ref())
                .map(|c| c.family.as_str())
                .unwrap_or("unknown");
            let deallocator = label.deallocator_contract.as_ref()
                .map(|c| c.family.as_str())
                .unwrap_or("unknown");
            allocator == "unknown" || deallocator == "unknown" || allocator != deallocator
        }
    }
}

fn identity_component_is_merged(
    node: &crate::kripke::AnnotatedNode,
    program_var: Option<&str>,
    allocation: Option<&str>,
) -> bool {
    for identity in [node.identity.as_ref(), node.event_identity.as_ref()].into_iter().flatten() {
        for record in &identity.points_to {
            let relevant = program_var.is_some_and(|v| record.variable.as_str() == v)
                || allocation.is_some_and(|a| record.allocations.iter().any(|x| x.as_str() == a));
            if relevant && record.allocations.len() > 1 {
                return true;
            }
        }
    }
    false
}

fn prepend_node(witness: &mut ExplanationWitness, node: &str, kind: &str, truth: Truth) {
    if !witness.relevant_nodes.iter().any(|n| n == node) {
        witness.relevant_nodes.insert(0, node.to_string());
    }
    witness.derivation.insert(0, DerivationStep {
        node: node.to_string(),
        formula_kind: kind.to_string(),
        truth: truth.as_str(),
        detail: BTreeMap::new(),
    });
}

fn prepend_path(witness: &mut ExplanationWitness, path: &[String], kind: &str, truth: Truth) {
    for node in path.iter().rev() {
        if !witness.relevant_nodes.iter().any(|n| n == node) {
            witness.relevant_nodes.insert(0, node.clone());
        }
    }
    if let Some(first) = path.first() {
        witness.derivation.insert(0, DerivationStep {
            node: first.clone(),
            formula_kind: kind.to_string(),
            truth: truth.as_str(),
            detail: BTreeMap::from([("path_nodes".into(), path.len().to_string())]),
        });
    }
}

fn push_reason(witness: &mut ExplanationWitness, reason: UncertaintyReason) {
    if !witness.reasons.contains(&reason) {
        witness.reasons.push(reason);
        witness.reasons.sort();
    }
}

fn merge_witness(current: &mut ExplanationWitness, other: ExplanationWitness) {
    for node in other.relevant_nodes {
        if !current.relevant_nodes.contains(&node) {
            current.relevant_nodes.push(node);
        }
    }
    for reason in other.reasons {
        push_reason(current, reason);
    }
    current.derivation.extend(other.derivation);
    current.atomic_observations.extend(other.atomic_observations);
    current.complete_dependency_trace &= other.complete_dependency_trace;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{PathFormula, PathQuantifier, QueryDocument, StateFormula};
    use crate::kripke::{
        AbstractAllocation, AbstractAllocationCell, AbstractAllocationMemoryAnnotation, AbstractMemoryAnnotation, AllocationEventLabel, AnnotatedIcfg, AnnotatedNode,
        Kripke, NodeIdentityAnnotation, ProgramLanguage, ProgramVariable,
    };
    use std::collections::BTreeSet;

    fn one_allocation_graph() -> Kripke {
        Kripke::from_annotated_icfg(AnnotatedIcfg {
            schema_version: 2,
            entry: "b0".into(),
            capabilities: vec![],
            variables: vec![ProgramVariable {
                id: "rust::main::_1".into(),
                language: ProgramLanguage::Rust,
                display: None,
                function: None,
            }],
            allocations: vec![AbstractAllocation {
                id: "A".into(),
                display: None,
                site: None,
                context: vec![],
                allocator_contract: None,
            }],
            nodes: vec![AnnotatedNode {
                id: "b0".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec![],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Alloc,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: None,
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            }],
        }).unwrap()
    }

    #[test]
    fn unknown_allocation_event_has_stable_may_allocation_frontier() {
        let k = one_allocation_graph();
        let checker = ModelChecker::new(&k);
        let formula = StateFormula::ExistsAlloc {
            logic_var: "a".into(),
            body: Box::new(StateFormula::Path {
                quantifier: PathQuantifier::Exists,
                formula: PathFormula::Eventually(Box::new(StateFormula::Label {
                    predicate: LabelPredicate::Alloc,
                    logic_var: "a".into(),
                })),
            }),
        };
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::new(), formula),
            &Env::new(),
            4,
        ).unwrap();
        assert_eq!(report.result, "unk");
        assert!(report.reason_frontier.contains(&UncertaintyReason::MayAllocation));
        assert!(report.reason_frontier.contains(&UncertaintyReason::QueryThreeValuedPropagation));
        assert_eq!(report.witnesses[0].binding["a"].value, "A");
        assert_eq!(report.witnesses[0].relevant_nodes, vec!["b0"]);
        let encoded = serde_json::to_value(&report).unwrap();
        assert_eq!(encoded["result"], "unk");
        assert!(encoded["reason_frontier"].as_array().unwrap().iter().any(|x| x.as_str() == Some("MAY_ALLOCATION")));
    }

    #[test]
    fn unknown_allocation_state_has_stable_may_allocation_frontier() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.nodes.get_mut("b0").unwrap().allocation_post = Some(AbstractAllocationMemoryAnnotation {
            cells: vec![AbstractAllocationCell {
                allocation: "A".into(),
                value: CellValue::Alloc,
            }],
        });
        let checker = ModelChecker::new(&k);
        let formula = StateFormula::ExistsAlloc {
            logic_var: "a".into(),
            body: Box::new(StateFormula::Path {
                quantifier: PathQuantifier::Exists,
                formula: PathFormula::Eventually(Box::new(StateFormula::May {
                    predicate: MayPredicate::Alloc,
                    logic_var: "a".into(),
                })),
            }),
        };
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["allocation_state_v1".into()]), formula),
            &Env::new(),
            4,
        ).unwrap();
        assert_eq!(report.result, "unk");
        assert!(report.reason_frontier.contains(&UncertaintyReason::MayAllocation));
        assert_eq!(report.witnesses[0].atomic_observations[0].detail["allocation_post_value"], "Alloc");
    }

    #[test]
    fn true_existential_structural_query_has_abstract_witness() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("mir_semantic_labels_v1".into());
        k.nodes.get_mut("b0").unwrap().semantic_labels.push("term:return".into());
        let checker = ModelChecker::new(&k);
        let formula = StateFormula::Path {
            quantifier: PathQuantifier::Exists,
            formula: PathFormula::Eventually(Box::new(StateFormula::StructuralLabel {
                kind: StructuralLabelKind::Terminator,
                name: "return".into(),
            })),
        };
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["mir_semantic_labels_v1".into()]), formula),
            &Env::new(),
            4,
        ).unwrap();
        assert_eq!(report.result, "tt");
        assert_eq!(report.witnesses.len(), 1);
        assert_eq!(report.witnesses[0].relevant_nodes, vec!["b0"]);
        assert_eq!(report.witnesses[0].atomic_observations[0].atom, "term_l(return)");
    }

    #[test]
    fn true_structural_witness_has_no_uncertainty_frontier() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("mir_semantic_labels_v1".into());
        k.nodes.get_mut("b0").unwrap().semantic_labels.push("term:return".into());
        let checker = ModelChecker::new(&k);
        let formula = StateFormula::StructuralLabel {
            kind: StructuralLabelKind::Terminator,
            name: "return".into(),
        };
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["mir_semantic_labels_v1".into()]), formula),
            &Env::new(),
            4,
        ).unwrap();
        assert_eq!(report.result, "tt");
        assert!(report.reason_frontier.is_empty());
        assert!(report.diagnostics.true_has_atomic_witness);
    }

    #[test]
    fn false_query_needs_no_positive_witness() {
        let k = one_allocation_graph();
        let formula = StateFormula::StructuralLabel {
            kind: StructuralLabelKind::Terminator,
            name: "return".into(),
        };
        // Missing capability is a hard error, not ff; add it to the model for
        // this test so the formula is legitimately refuted.
        let mut k = k;
        k.capabilities.insert("mir_semantic_labels_v1".into());
        let checker = ModelChecker::new(&k);
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["mir_semantic_labels_v1".into()]), formula),
            &Env::new(),
            4,
        ).unwrap();
        assert_eq!(report.result, "ff");
        assert!(report.witnesses.is_empty());
    }
}
