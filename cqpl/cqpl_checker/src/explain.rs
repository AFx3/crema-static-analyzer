use crate::ast::{LabelPredicate, MayPredicate, PathFormula, PathQuantifier, QueryDocument, StateFormula, StructuralLabelKind};
use crate::kripke::{
    AllocationContract, AllocationDispositionKind, AllocationDispositionRecord, AllocationEventCertainty,
    AllocationObligationEffect, CellValue, EventKind,
};
use crate::model_checker::{Binding, Env, ModelChecker};
use crate::truth::Truth;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const EXPLAINABILITY_TAXONOMY_VERSION: &str = "cqpl_uncertainty_reasons_v1";
pub const ALLOCATION_OBLIGATION_DIAGNOSTICS_VERSION: &str = "allocation_obligation_diagnostics_v1";
pub const MEMORY_ERROR_DIAGNOSTICS_VERSION: &str = "memory_error_diagnostics_v1";
pub const ALLOCATION_CONTRACT_WITNESS_VERSION: &str = "allocation_contract_witness_v1";

/// Read-only bug-supporting evidence that is intentionally separate from CQPL truth.
///
/// `reason_frontier` answers why a three-valued query is unknown. These findings
/// answer the complementary diagnostic question: whether the already-annotated
/// abstract model contains an ordered witness supporting the queried memory-error
/// class despite that uncertainty. They never change `tt`/`ff`/`unk`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationObligationFindingKind {
    NormalReturnOpenManualObligation,
    DropThenUseWithoutReallocation,
    RepeatedDropWithoutReallocation,
    AllocatorFamilyMismatch,
    UnresolvedAllocatorContractCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationObligationFindingStrength {
    StrongAbstractEvidence,
    ObservationalCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationObligationEvidence {
    ProducerCertifiedBoxIntoRaw,
    ProducerCertifiedCStringIntoRaw,
    ProducerCertifiedCStringFromRaw,
    NormalReturnReachable,
    NoModeledDischargeOnWitnessPath,
    NoInterveningCallAfterHandoff,
    NonReturningDischargeObservedOffWitness,
    MayAllocationEventObserved,
    AllocationStateIncludesAllocated,
    MayDeallocationObserved,
    MayUseObserved,
    OrderedDropBeforeUse,
    TwoOrderedDropsObserved,
    NoReallocationBetweenEvents,
    KnownAllocatorFamily,
    KnownDeallocatorFamily,
    AllocatorFamiliesDiffer,
    AllocatorFamilyUnresolved,
    DeallocatorFamilyUnresolved,
    ProducerCertifiedDeallocatorContract,
}

impl AllocationObligationFindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NormalReturnOpenManualObligation => "normal_return_open_manual_obligation",
            Self::DropThenUseWithoutReallocation => "drop_then_use_without_reallocation",
            Self::RepeatedDropWithoutReallocation => "repeated_drop_without_reallocation",
            Self::AllocatorFamilyMismatch => "allocator_family_mismatch",
            Self::UnresolvedAllocatorContractCandidate => "unresolved_allocator_contract_candidate",
        }
    }
}

impl AllocationObligationFindingStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrongAbstractEvidence => "strong_abstract_evidence",
            Self::ObservationalCandidate => "observational_candidate",
        }
    }
}

/// Stable role of a producer-supplied allocation contract inside one diagnostic
/// witness.  The contract is presentation/evidence only: it never upgrades a MAY
/// event to MUST and therefore never changes CQPL truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationContractWitnessRole {
    AllocatorOrigin,
    FirstDeallocation,
    SecondDeallocation,
    MismatchDeallocation,
}

impl AllocationContractWitnessRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllocatorOrigin => "allocator_origin",
            Self::FirstDeallocation => "first_deallocation",
            Self::SecondDeallocation => "second_deallocation",
            Self::MismatchDeallocation => "mismatch_deallocation",
        }
    }
}

/// Provenance class for a serialized allocation-contract witness.
///
/// `allocation_contracts_v2` deliberately certifies deallocators only.  Allocator
/// origins remain the frozen v1 summary surface, so an absent allocator `basis`
/// is not silently presented as a failed v2 proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationContractWitnessProvenance {
    LegacyV1AllocatorSummary,
    LegacyV1DeallocatorSummary,
    ProducerCertifiedV2Deallocator,
    ExplicitlyUnresolvedV2Deallocator,
}

impl AllocationContractWitnessProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyV1AllocatorSummary => "legacy_v1_allocator_summary",
            Self::LegacyV1DeallocatorSummary => "legacy_v1_deallocator_summary",
            Self::ProducerCertifiedV2Deallocator => "producer_certified_v2_deallocator",
            Self::ExplicitlyUnresolvedV2Deallocator => "explicitly_unresolved_v2_deallocator",
        }
    }
}

pub const ALLOCATION_DISPOSITION_WITNESS_VERSION: &str = "allocation_disposition_witness_v1";

/// Role of a producer-certified ownership transition in a diagnostic witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationDispositionWitnessRole {
    OwnershipHandoff,
    OwnershipReclaim,
}

impl AllocationDispositionWitnessRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnershipHandoff => "ownership_handoff",
            Self::OwnershipReclaim => "ownership_reclaim",
        }
    }
}

/// Read-only projection of one producer disposition record onto an ordered
/// diagnostic witness.  It is presentation/evidence only and never participates
/// in CQPL truth evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AllocationDispositionWitness {
    pub schema: &'static str,
    pub role: AllocationDispositionWitnessRole,
    pub node: String,
    pub kind: AllocationDispositionKind,
    pub certainty: AllocationEventCertainty,
    pub obligation_effect: AllocationObligationEffect,
    pub basis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_variable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_variable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee_def_path: Option<String>,
}

/// Proof-carrying contract attached to an UNKNOWN supporting finding.
///
/// B1.1-r1 makes the contract basis explicit because explainability is part of
/// the scientific result: an UNKNOWN caused by MAY evidence must still state
/// which allocator/deallocator contract justified the abstract event, and which
/// parts remain unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AllocationContractWitness {
    pub schema: &'static str,
    pub role: AllocationContractWitnessRole,
    pub provenance: AllocationContractWitnessProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub contract: AllocationContract,
}

impl AllocationObligationEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProducerCertifiedBoxIntoRaw => "producer_certified_box_into_raw",
            Self::ProducerCertifiedCStringIntoRaw => "producer_certified_cstring_into_raw",
            Self::ProducerCertifiedCStringFromRaw => "producer_certified_cstring_from_raw",
            Self::NormalReturnReachable => "normal_return_reachable",
            Self::NoModeledDischargeOnWitnessPath => "no_modeled_discharge_on_witness_path",
            Self::NoInterveningCallAfterHandoff => "no_intervening_call_after_handoff",
            Self::NonReturningDischargeObservedOffWitness => "non_returning_discharge_observed_off_witness",
            Self::MayAllocationEventObserved => "may_allocation_event_observed",
            Self::AllocationStateIncludesAllocated => "allocation_state_includes_allocated",
            Self::MayDeallocationObserved => "may_deallocation_observed",
            Self::MayUseObserved => "may_use_observed",
            Self::OrderedDropBeforeUse => "ordered_drop_before_use",
            Self::TwoOrderedDropsObserved => "two_ordered_drops_observed",
            Self::NoReallocationBetweenEvents => "no_reallocation_between_events",
            Self::KnownAllocatorFamily => "known_allocator_family",
            Self::KnownDeallocatorFamily => "known_deallocator_family",
            Self::AllocatorFamiliesDiffer => "allocator_families_differ",
            Self::AllocatorFamilyUnresolved => "allocator_family_unresolved",
            Self::DeallocatorFamilyUnresolved => "deallocator_family_unresolved",
            Self::ProducerCertifiedDeallocatorContract => "producer_certified_deallocator_contract",
        }
    }
}

fn deallocator_contract_provenance(
    contract: &AllocationContract,
) -> AllocationContractWitnessProvenance {
    match contract.basis.as_deref() {
        Some("unresolved") => AllocationContractWitnessProvenance::ExplicitlyUnresolvedV2Deallocator,
        Some(_) => AllocationContractWitnessProvenance::ProducerCertifiedV2Deallocator,
        None => AllocationContractWitnessProvenance::LegacyV1DeallocatorSummary,
    }
}

fn allocation_disposition_kind_name(kind: AllocationDispositionKind) -> &'static str {
    match kind {
        AllocationDispositionKind::BoxIntoRaw => "box_into_raw",
        AllocationDispositionKind::BoxFromRaw => "box_from_raw",
        AllocationDispositionKind::BoxLeak => "box_leak",
        AllocationDispositionKind::CStringIntoRaw => "cstring_into_raw",
        AllocationDispositionKind::CStringFromRaw => "cstring_from_raw",
        AllocationDispositionKind::MemForgetOwnedBox => "mem_forget_owned_box",
        AllocationDispositionKind::RawPointerDropNoop => "raw_pointer_drop_noop",
        AllocationDispositionKind::ReturnEscape => "return_escape",
        AllocationDispositionKind::MayDeallocate => "may_deallocate",
    }
}

fn allocation_obligation_effect_name(effect: AllocationObligationEffect) -> &'static str {
    match effect {
        AllocationObligationEffect::PreserveManualObligation => "preserve_manual_obligation",
        AllocationObligationEffect::RestoreRaiiObligation => "restore_raii_obligation",
        AllocationObligationEffect::PreservePersistentObligation => "preserve_persistent_obligation",
        AllocationObligationEffect::PreserveUnreclaimedObligation => "preserve_unreclaimed_obligation",
        AllocationObligationEffect::NoPointeeLifecycleEffect => "no_pointee_lifecycle_effect",
        AllocationObligationEffect::MayEscapeToCaller => "may_escape_to_caller",
        AllocationObligationEffect::MayDischarge => "may_discharge",
    }
}

fn allocation_event_certainty_name(certainty: AllocationEventCertainty) -> &'static str {
    match certainty {
        AllocationEventCertainty::MayAbstract => "may_abstract",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AllocationObligationFinding {
    pub taxonomy: &'static str,
    pub kind: AllocationObligationFindingKind,
    pub strength: AllocationObligationFindingStrength,
    pub allocation: String,
    pub query_result: &'static str,
    pub witness_path: Vec<String>,
    pub evidence: Vec<AllocationObligationEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_drop_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub second_drop_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mismatch_node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocator_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deallocator_family: Option<String>,
    /// Producer-certified or explicitly unresolved contracts participating in
    /// this MAY witness.  Serialized even when the overall query remains `unk`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contracts: Vec<AllocationContractWitness>,
    /// Producer-certified ownership handoff/reclaim records participating in
    /// this ordered diagnostic witness.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dispositions: Vec<AllocationDispositionWitness>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub non_returning_discharge_nodes: Vec<String>,
    pub summary: String,
}

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
    MayPanicLifecycle,
    PanicLifecycleUnresolved,
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
            Self::MayPanicLifecycle => "MAY_PANIC_LIFECYCLE",
            Self::PanicLifecycleUnresolved => "PANIC_LIFECYCLE_UNRESOLVED",
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
    /// Positive bug-supporting evidence is deliberately separate from the
    /// uncertainty frontier.  An `unk` query may therefore still have a strong
    /// ownership-obligation witness without being silently promoted to `tt`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supporting_findings: Vec<AllocationObligationFinding>,
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

impl ExplanationReport {
    /// Stable human-readable rendering for interactive UNKNOWN diagnosis.
    ///
    /// This is presentation-only: it renders the same read-only report that is
    /// serialized by `--explain-json` and therefore cannot change CQPL truth.
    pub fn render_unknown_verbose(&self, query_name: &str) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();
        let bar = "=".repeat(80);
        let _ = writeln!(out, "{bar}");
        let _ = writeln!(out, "QUERY: {query_name}");
        let _ = writeln!(out, "{bar}");
        let _ = writeln!(out, "truth: {}", self.result);
        let _ = writeln!(out);
        let _ = writeln!(out, "why unknown:");
        for reason in &self.reason_frontier {
            let _ = writeln!(out, "  - {}", reason.code());
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "supporting findings:");
        if self.supporting_findings.is_empty() {
            let _ = writeln!(out, "  <none>");
            let _ = writeln!(out);
            let _ = writeln!(out, "  interpretation:");
            let _ = writeln!(out, "    No bug-specific supporting witness was certified beyond the uncertainty frontier. UNKNOWN therefore means insufficient abstract evidence for a definite verdict, not a positive finding by itself.");
        }
        for finding in &self.supporting_findings {
            let _ = writeln!(out);
            let _ = writeln!(out, "  kind       : {}", finding.kind.as_str());
            let _ = writeln!(out, "  strength   : {}", finding.strength.as_str());
            let _ = writeln!(out, "  allocation : {}", finding.allocation);
            if let Some(node) = &finding.origin_node {
                let _ = writeln!(out, "  origin     : {node}");
            }
            if let Some(node) = &finding.handoff_node {
                let _ = writeln!(out, "  handoff    : {node}");
            }
            if let Some(node) = &finding.return_node {
                let _ = writeln!(out, "  return     : {node}");
            }
            if let Some(node) = &finding.first_drop_node {
                let _ = writeln!(out, "  first drop : {node}");
            }
            if let Some(node) = &finding.second_drop_node {
                let _ = writeln!(out, "  second drop: {node}");
            }
            if let Some(node) = &finding.use_node {
                let _ = writeln!(out, "  use        : {node}");
            }
            if let Some(node) = &finding.mismatch_node {
                let _ = writeln!(out, "  mismatch   : {node}");
            }
            if let Some(family) = &finding.allocator_family {
                let _ = writeln!(out, "  allocator  : {family}");
            }
            if let Some(family) = &finding.deallocator_family {
                let _ = writeln!(out, "  deallocator: {family}");
            }
            if !finding.contracts.is_empty() {
                let _ = writeln!(out, "  contracts:");
                for witness in &finding.contracts {
                    let contract = &witness.contract;
                    let _ = writeln!(out, "    - role      : {}", witness.role.as_str());
                    if let Some(node) = &witness.node {
                        let _ = writeln!(out, "      node      : {node}");
                    }
                    let _ = writeln!(out, "      provenance: {}", witness.provenance.as_str());
                    let _ = writeln!(out, "      family    : {}", contract.family);
                    let _ = writeln!(out, "      operation : {}", contract.operation);
                    let _ = writeln!(out, "      language  : {}", contract.language);
                    let basis = contract.basis.as_deref().unwrap_or_else(|| {
                        if witness.provenance == AllocationContractWitnessProvenance::LegacyV1AllocatorSummary {
                            "<not-applicable-v1>"
                        } else {
                            "<none>"
                        }
                    });
                    let _ = writeln!(out, "      basis     : {basis}");
                    if let Some(owner) = &contract.owner_def_path {
                        let _ = writeln!(out, "      owner     : {owner}");
                    }
                    if let Some(allocator) = &contract.allocator_def_path {
                        let _ = writeln!(out, "      allocator : {allocator}");
                    }
                    if let Some(callee) = &contract.callee_def_path {
                        let _ = writeln!(out, "      callee    : {callee}");
                    }
                }
            }
            if !finding.dispositions.is_empty() {
                let _ = writeln!(out, "  ownership dispositions:");
                for witness in &finding.dispositions {
                    let _ = writeln!(out, "    - role      : {}", witness.role.as_str());
                    let _ = writeln!(out, "      node      : {}", witness.node);
                    let _ = writeln!(
                        out,
                        "      kind      : {}",
                        allocation_disposition_kind_name(witness.kind),
                    );
                    let _ = writeln!(
                        out,
                        "      certainty : {}",
                        allocation_event_certainty_name(witness.certainty),
                    );
                    let _ = writeln!(
                        out,
                        "      effect    : {}",
                        allocation_obligation_effect_name(witness.obligation_effect),
                    );
                    let _ = writeln!(out, "      basis     : {}", witness.basis);
                    if let Some(source) = &witness.source_variable {
                        let _ = writeln!(out, "      source    : {source}");
                    }
                    if let Some(target) = &witness.target_variable {
                        let _ = writeln!(out, "      target    : {target}");
                    }
                    if let Some(callee) = &witness.callee_def_path {
                        let _ = writeln!(out, "      callee    : {callee}");
                    }
                }
            }
            let _ = writeln!(out, "  path:");
            for node in &finding.witness_path {
                let _ = writeln!(out, "    -> {node}");
            }
            let _ = writeln!(out, "  evidence:");
            for evidence in &finding.evidence {
                let _ = writeln!(out, "    - {}", evidence.as_str());
            }
            if !finding.non_returning_discharge_nodes.is_empty() {
                let _ = writeln!(out, "  non-returning discharge nodes:");
                for node in &finding.non_returning_discharge_nodes {
                    let _ = writeln!(out, "    - {node}");
                }
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "  interpretation:");
            let _ = writeln!(out, "    {}", finding.summary);
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "uncertainty witnesses:");
        if self.witnesses.is_empty() {
            let _ = writeln!(out, "  <none>");
        }
        for (index, witness) in self.witnesses.iter().enumerate() {
            let _ = writeln!(out, "  witness {}: truth={}", index + 1, witness.truth);
            for atom in &witness.atomic_observations {
                let reasons = atom
                    .reasons
                    .iter()
                    .map(|reason| reason.code())
                    .collect::<Vec<_>>()
                    .join(",");
                let contract_suffix = [
                    atom.detail.get("allocator_contract_provenance")
                        .map(|value| format!("allocator_provenance={value}")),
                    atom.detail.get("allocator_contract_basis")
                        .map(|basis| format!("allocator_basis={basis}")),
                    atom.detail.get("deallocator_contract_provenance")
                        .map(|value| format!("deallocator_provenance={value}")),
                    atom.detail.get("deallocator_contract_basis")
                        .map(|basis| format!("deallocator_basis={basis}")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(",");
                let reasons_and_contracts = match (reasons.is_empty(), contract_suffix.is_empty()) {
                    (true, true) => String::new(),
                    (false, true) => reasons,
                    (true, false) => contract_suffix,
                    (false, false) => format!("{reasons} | {contract_suffix}"),
                };
                if reasons_and_contracts.is_empty() {
                    let _ = writeln!(
                        out,
                        "     {} | {} = {}",
                        atom.node, atom.atom, atom.truth
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "     {} | {} = {} | {}",
                        atom.node, atom.atom, atom.truth, reasons_and_contracts
                    );
                }
            }
        }
        out
    }
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
        MayPredicate::RepeatDrop => UncertaintyReason::MayPanicLifecycle,
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
        MayPredicate::RepeatDrop => "repeat_drop",
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

        let mut supporting_findings = Vec::new();
        if formula_contains_negated_drop(&document.formula) {
            supporting_findings.extend(self.allocation_obligation_findings(result));
        }
        if formula_is_use_after_free_shape(&document.formula) {
            supporting_findings.extend(self.use_after_free_findings(result));
        }
        if formula_is_double_free_shape(&document.formula) {
            supporting_findings.extend(self.double_free_findings(result));
        }
        if formula_uses_allocator_mismatch(&document.formula) {
            supporting_findings.extend(self.allocator_mismatch_findings(result));
        }
        supporting_findings.sort_by(|a, b| {
            (a.kind.as_str(), &a.allocation, &a.witness_path)
                .cmp(&(b.kind.as_str(), &b.allocation, &b.witness_path))
        });

        Ok(ExplanationReport {
            schema: "cqpl_explanation_v1",
            taxonomy: EXPLAINABILITY_TAXONOMY_VERSION,
            result: result.as_str(),
            entry: self.k.entry.clone(),
            scope_note: "explanations are over the already-projected annotated abstract Kripke model; they do not assert a concrete execution",
            reason_frontier,
            reason_counts,
            supporting_findings,
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

        if truth == Truth::Unknown && predicate != MayPredicate::RepeatDrop {
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
                detail.insert("allocation".into(), allocation.clone());
                if predicate == MayPredicate::RepeatDrop {
                    let matching = self.k.panic_lifecycle
                        .get(node_id)
                        .into_iter()
                        .flatten()
                        .find(|record| record.allocation == *allocation && record.may_repeat_drop());
                    if let Some(record) = matching {
                        reasons.insert(UncertaintyReason::MayPanicLifecycle);
                        detail.insert("lifecycle_record".into(), "present".into());
                        detail.insert("lifecycle_certainty".into(), format!("{:?}", record.certainty));
                        detail.insert("may_own".into(), record.may_own.to_string());
                        detail.insert("may_partial_drop".into(), record.may_partial_drop.to_string());
                        detail.insert("may_stale_owner".into(), record.may_stale_owner.to_string());
                        detail.insert("may_committed".into(), record.may_committed.to_string());
                        detail.insert("may_complete".into(), record.may_complete.to_string());
                        detail.insert("may_repeat_drop".into(), "true".into());
                        detail.insert("lifecycle_coverage".into(), format!(
                            "{:?}",
                            self.k.panic_lifecycle.coverage_at(node_id)
                        ));
                    } else if self.k.panic_lifecycle.coverage_at(node_id)
                        == Some(crate::kripke::PanicLifecycleCoverage::Unresolved)
                    {
                        reasons.insert(UncertaintyReason::PanicLifecycleUnresolved);
                        detail.insert("lifecycle_record".into(), "absent".into());
                        detail.insert("lifecycle_coverage".into(), "unresolved".into());
                        detail.insert("may_repeat_drop".into(), "unknown".into());
                    } else {
                        detail.insert("lifecycle_record".into(), "absent".into());
                        detail.insert("lifecycle_coverage".into(), "complete".into());
                        detail.insert("may_repeat_drop".into(), "false".into());
                    }
                } else {
                    let value = node.allocation_post.as_ref()
                        .map(|post| post.value_of(allocation))
                        .unwrap_or(CellValue::Bottom);
                    detail.insert("allocation_post_value".into(), format!("{:?}", value));
                    if value == CellValue::Top {
                        reasons.insert(UncertaintyReason::AbstractTopState);
                    }
                    if identity_component_is_merged(node, None, Some(allocation)) {
                        reasons.insert(UncertaintyReason::AbstractComponentMerge);
                    }
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

                // B1.1-r1: carry producer contracts into the uncertainty witness
                // itself, not only into higher-level supporting findings.  This
                // is read-only explainability metadata: MAY remains MAY.
                if let Some(contract) = self.k.allocations.get(allocation)
                    .and_then(|allocation| allocation.allocator_contract.as_ref())
                {
                    detail.insert("allocator_contract_family".into(), contract.family.clone());
                    detail.insert("allocator_contract_operation".into(), contract.operation.clone());
                    detail.insert(
                        "allocator_contract_provenance".into(),
                        AllocationContractWitnessProvenance::LegacyV1AllocatorSummary
                            .as_str()
                            .into(),
                    );
                    if let Some(basis) = &contract.basis {
                        detail.insert("allocator_contract_basis".into(), basis.clone());
                    }
                }
                let deallocator_contracts: Vec<_> = matching.iter()
                    .filter_map(|label| label.deallocator_contract.as_ref())
                    .collect();
                if !deallocator_contracts.is_empty() {
                    let families = deallocator_contracts.iter()
                        .map(|contract| contract.family.clone())
                        .collect::<BTreeSet<_>>();
                    let operations = deallocator_contracts.iter()
                        .map(|contract| contract.operation.clone())
                        .collect::<BTreeSet<_>>();
                    let bases = deallocator_contracts.iter()
                        .filter_map(|contract| contract.basis.clone())
                        .collect::<BTreeSet<_>>();
                    let provenances = deallocator_contracts.iter()
                        .map(|contract| deallocator_contract_provenance(contract).as_str().to_string())
                        .collect::<BTreeSet<_>>();
                    detail.insert(
                        "deallocator_contract_family".into(),
                        families.iter().cloned().collect::<Vec<_>>().join(";"),
                    );
                    detail.insert(
                        "deallocator_contract_operation".into(),
                        operations.iter().cloned().collect::<Vec<_>>().join(";"),
                    );
                    detail.insert(
                        "deallocator_contract_provenance".into(),
                        provenances.iter().cloned().collect::<Vec<_>>().join(";"),
                    );
                    if !bases.is_empty() {
                        detail.insert(
                            "deallocator_contract_basis".into(),
                            bases.iter().cloned().collect::<Vec<_>>().join(";"),
                        );
                    }
                }
                if truth == Truth::Unknown && matching.iter().any(|label| label.certainty == AllocationEventCertainty::MayAbstract) {
                    reasons.insert(label_reason(predicate));
                }
                let unresolved_allocator_contract = predicate == LabelPredicate::AllocatorMismatch
                    && self.k.allocations.get(allocation)
                        .and_then(|a| a.allocator_contract.as_ref())
                        .map(|contract| contract.family.as_str())
                        .unwrap_or("unknown") == "unknown";
                let unresolved_deallocator_contract = matching.iter().any(|label| {
                    label.deallocator_contract.as_ref().is_some_and(|contract| {
                        contract.family == "unknown" || contract.basis.as_deref() == Some("unresolved")
                    })
                });
                let missing_mismatch_deallocator_contract = predicate == LabelPredicate::AllocatorMismatch
                    && matching.iter().any(|label| label.deallocator_contract.is_none());
                if unresolved_allocator_contract
                    || unresolved_deallocator_contract
                    || missing_mismatch_deallocator_contract
                {
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

    fn allocation_obligation_findings(&self, result: Truth) -> Vec<AllocationObligationFinding> {
        if !self.k.capabilities.contains("allocation_disposition_v1")
            || !self.k.capabilities.contains("mir_semantic_labels_v1")
        {
            return Vec::new();
        }

        let mut findings = Vec::new();
        for (handoff_node, node) in &self.k.nodes {
            for record in &node.allocation_disposition {
                let (handoff_evidence, handoff_name) = match record.kind {
                    AllocationDispositionKind::BoxIntoRaw
                        if record.obligation_effect
                            == AllocationObligationEffect::PreserveManualObligation =>
                    {
                        (AllocationObligationEvidence::ProducerCertifiedBoxIntoRaw, "Box::into_raw")
                    }
                    AllocationDispositionKind::CStringIntoRaw
                        if record.obligation_effect
                            == AllocationObligationEffect::PreserveManualObligation =>
                    {
                        (
                            AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw,
                            "CString::into_raw",
                        )
                    }
                    _ => continue,
                };

                let allocation = record.allocation.clone();
                let path = self.bfs_to(
                    handoff_node,
                    |candidate| self.is_normal_return_node(candidate),
                    |candidate| !self.blocks_open_manual_obligation(candidate, &allocation),
                );
                let Some(witness_path) = path else { continue; };
                let Some(return_node) = witness_path.last().cloned() else { continue; };

                // Calls after the certified handoff can hide effects outside the
                // current disposition vocabulary.  Preserve the finding, but
                // downgrade it from strong evidence to an observational candidate.
                let has_intervening_call = witness_path
                    .iter()
                    .skip(1)
                    .take(witness_path.len().saturating_sub(2))
                    .any(|node_id| {
                        self.k.nodes.get(node_id).is_some_and(|n| {
                            n.semantic_labels.iter().any(|label| label == "term:call")
                        })
                    });

                let mut non_returning_discharge_nodes: Vec<String> = self.k.nodes
                    .iter()
                    .filter_map(|(node_id, candidate)| {
                        let has_discharge = candidate.allocation_disposition.iter().any(|d| {
                            d.allocation == allocation
                                && matches!(d.obligation_effect,
                                    AllocationObligationEffect::MayDischarge
                                        | AllocationObligationEffect::RestoreRaiiObligation)
                        }) || candidate.allocation_labels.iter().any(|label| {
                            label.allocation == allocation && label.predicate == EventKind::Drop
                        });
                        if has_discharge
                            && !witness_path.contains(node_id)
                            && !self.can_reach_normal_return(node_id)
                        {
                            Some(node_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                non_returning_discharge_nodes.sort();
                non_returning_discharge_nodes.dedup();

                let strength = if has_intervening_call {
                    AllocationObligationFindingStrength::ObservationalCandidate
                } else {
                    AllocationObligationFindingStrength::StrongAbstractEvidence
                };
                let mut evidence = vec![
                    handoff_evidence,
                    AllocationObligationEvidence::NormalReturnReachable,
                    AllocationObligationEvidence::NoModeledDischargeOnWitnessPath,
                ];
                if !has_intervening_call {
                    evidence.push(AllocationObligationEvidence::NoInterveningCallAfterHandoff);
                }
                if !non_returning_discharge_nodes.is_empty() {
                    evidence.push(AllocationObligationEvidence::NonReturningDischargeObservedOffWitness);
                }

                let summary = match strength {
                    AllocationObligationFindingStrength::StrongAbstractEvidence => format!(
                        "The abstract model contains a normal-return path after producer-certified {handoff_name} on which the manual deallocation obligation remains open and no modeled discharge or escape occurs. This is strong abstract evidence of a leak on normal completion; CQPL truth remains {} because this diagnostic does not alter three-valued query semantics.",
                        result.as_str(),
                    ),
                    AllocationObligationFindingStrength::ObservationalCandidate => format!(
                        "The abstract model contains a normal-return path after {handoff_name} with no modeled discharge, but the path contains an intervening call whose effects may be outside the current disposition vocabulary. This supports a leak candidate but is not strong enough to promote CQPL truth from {}.",
                        result.as_str(),
                    ),
                };

                let contracts = self
                    .allocator_contract_witness(
                        &allocation,
                        witness_path.first().cloned(),
                    )
                    .into_iter()
                    .collect();
                let dispositions = vec![self.disposition_witness_from_record(
                    handoff_node,
                    record,
                    AllocationDispositionWitnessRole::OwnershipHandoff,
                )];

                findings.push(AllocationObligationFinding {
                    taxonomy: ALLOCATION_OBLIGATION_DIAGNOSTICS_VERSION,
                    kind: AllocationObligationFindingKind::NormalReturnOpenManualObligation,
                    strength,
                    allocation,
                    query_result: result.as_str(),
                    witness_path,
                    evidence,
                    origin_node: None,
                    handoff_node: Some(handoff_node.clone()),
                    return_node: Some(return_node),
                    first_drop_node: None,
                    second_drop_node: None,
                    use_node: None,
                    mismatch_node: None,
                    allocator_family: None,
                    deallocator_family: None,
                    contracts,
                    dispositions,
                    non_returning_discharge_nodes,
                    summary,
                });
            }
        }

        findings.sort_by(|a, b| {
            (&a.allocation, &a.handoff_node, &a.return_node)
                .cmp(&(&b.allocation, &b.handoff_node, &b.return_node))
        });
        findings
    }

    fn use_after_free_findings(&self, result: Truth) -> Vec<AllocationObligationFinding> {
        let mut findings = Vec::new();
        for allocation in self.k.allocations.keys() {
            for first_drop in self.k.nodes.keys().filter(|node_id| {
                self.node_has_allocation_event(node_id, allocation, EventKind::Drop)
            }) {
                let Some((origin_path, origin_evidence)) = self.origin_path_to(allocation, first_drop) else {
                    continue;
                };

                for successor in self.successors(first_drop) {
                    let Some(suffix) = self.bfs_to(
                        &successor,
                        |candidate| self.node_has_use_event(candidate, allocation),
                        |candidate| {
                            self.node_has_use_event(candidate, allocation)
                                || !self.node_has_allocation_event(candidate, allocation, EventKind::Alloc)
                        },
                    ) else {
                        continue;
                    };
                    let Some(use_node) = suffix.last().cloned() else { continue; };

                    let mut witness_path = origin_path.clone();
                    witness_path.extend(suffix);
                    let strength = if origin_evidence == AllocationObligationEvidence::MayAllocationEventObserved {
                        AllocationObligationFindingStrength::StrongAbstractEvidence
                    } else {
                        AllocationObligationFindingStrength::ObservationalCandidate
                    };
                    let mut evidence = vec![
                        origin_evidence,
                        AllocationObligationEvidence::MayDeallocationObserved,
                        AllocationObligationEvidence::MayUseObserved,
                        AllocationObligationEvidence::OrderedDropBeforeUse,
                        AllocationObligationEvidence::NoReallocationBetweenEvents,
                    ];
                    let summary = match strength {
                        AllocationObligationFindingStrength::StrongAbstractEvidence => format!(
                            "The abstract model contains an ordered witness for allocation {allocation}: a MAY deallocation is followed by a MAY use/read/write of the same AbstractAllocId, with no intervening re-allocation event. This is strong abstract evidence of a use-after-free pattern; CQPL truth remains {} because allocation/event identity is MAY rather than MUST.",
                            result.as_str(),
                        ),
                        AllocationObligationFindingStrength::ObservationalCandidate => format!(
                            "The abstract model contains an ordered drop-then-use witness for allocation {allocation}, but the allocation origin is supported only by allocation-state membership rather than an explicit allocation event. This is a use-after-free candidate; CQPL truth remains {}.",
                            result.as_str(),
                        ),
                    };

                    let mut contracts = Vec::new();
                    if let Some(contract) = self.allocator_contract_witness(
                        allocation,
                        origin_path.first().cloned(),
                    ) {
                        contracts.push(contract);
                    }
                    if let Some(contract) = self.deallocator_contract_witness(
                        first_drop,
                        allocation,
                        AllocationContractWitnessRole::FirstDeallocation,
                    ) {
                        contracts.push(contract);
                    }

                    let mut dispositions = Vec::new();
                    if let Some(witness) = self.disposition_witness_before(
                        allocation,
                        first_drop,
                        AllocationDispositionKind::CStringIntoRaw,
                        AllocationDispositionWitnessRole::OwnershipHandoff,
                    ) {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw);
                        dispositions.push(witness);
                    }
                    if let Some(witness) = self.disposition_witness_on_path(
                        allocation,
                        &witness_path,
                        AllocationDispositionKind::CStringFromRaw,
                        AllocationDispositionWitnessRole::OwnershipReclaim,
                    ) {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedCStringFromRaw);
                        dispositions.push(witness);
                    }

                    findings.push(AllocationObligationFinding {
                        taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                        kind: AllocationObligationFindingKind::DropThenUseWithoutReallocation,
                        strength,
                        allocation: allocation.clone(),
                        query_result: result.as_str(),
                        witness_path,
                        evidence,
                        origin_node: origin_path.first().cloned(),
                        handoff_node: None,
                        return_node: None,
                        first_drop_node: Some(first_drop.clone()),
                        second_drop_node: None,
                        use_node: Some(use_node),
                        mismatch_node: None,
                        allocator_family: None,
                        deallocator_family: None,
                        contracts,
                        dispositions,
                        non_returning_discharge_nodes: Vec::new(),
                        summary,
                    });
                    break;
                }
            }
        }
        findings
    }

    fn double_free_findings(&self, result: Truth) -> Vec<AllocationObligationFinding> {
        let mut findings = Vec::new();
        for allocation in self.k.allocations.keys() {
            for first_drop in self.k.nodes.keys().filter(|node_id| {
                self.node_has_allocation_event(node_id, allocation, EventKind::Drop)
            }) {
                let Some((origin_path, origin_evidence)) = self.origin_path_to(allocation, first_drop) else {
                    continue;
                };

                for successor in self.successors(first_drop) {
                    let Some(suffix) = self.bfs_to(
                        &successor,
                        |candidate| self.node_has_allocation_event(candidate, allocation, EventKind::Drop),
                        |candidate| {
                            self.node_has_allocation_event(candidate, allocation, EventKind::Drop)
                                || !self.node_has_allocation_event(candidate, allocation, EventKind::Alloc)
                        },
                    ) else {
                        continue;
                    };
                    let Some(second_drop) = suffix.last().cloned() else { continue; };

                    let mut witness_path = origin_path.clone();
                    witness_path.extend(suffix);
                    let strength = if origin_evidence == AllocationObligationEvidence::MayAllocationEventObserved {
                        AllocationObligationFindingStrength::StrongAbstractEvidence
                    } else {
                        AllocationObligationFindingStrength::ObservationalCandidate
                    };
                    let mut evidence = vec![
                        origin_evidence,
                        AllocationObligationEvidence::MayDeallocationObserved,
                        AllocationObligationEvidence::TwoOrderedDropsObserved,
                        AllocationObligationEvidence::NoReallocationBetweenEvents,
                    ];
                    let summary = match strength {
                        AllocationObligationFindingStrength::StrongAbstractEvidence => format!(
                            "The abstract model contains two ordered MAY deallocation events for allocation {allocation}, with no intervening re-allocation event for the same AbstractAllocId. This is strong abstract evidence of a double-free pattern; CQPL truth remains {} because the allocation/drop facts are MAY rather than MUST.",
                            result.as_str(),
                        ),
                        AllocationObligationFindingStrength::ObservationalCandidate => format!(
                            "The abstract model contains two ordered drop events for allocation {allocation} without an intervening allocation event, but the allocation origin is supported only by allocation-state membership. This is a double-free candidate; CQPL truth remains {}.",
                            result.as_str(),
                        ),
                    };

                    let mut contracts = Vec::new();
                    if let Some(contract) = self.allocator_contract_witness(
                        allocation,
                        origin_path.first().cloned(),
                    ) {
                        contracts.push(contract);
                    }
                    if let Some(contract) = self.deallocator_contract_witness(
                        first_drop,
                        allocation,
                        AllocationContractWitnessRole::FirstDeallocation,
                    ) {
                        contracts.push(contract);
                    }
                    if let Some(contract) = self.deallocator_contract_witness(
                        &second_drop,
                        allocation,
                        AllocationContractWitnessRole::SecondDeallocation,
                    ) {
                        contracts.push(contract);
                    }

                    let mut dispositions = Vec::new();
                    if let Some(witness) = self.disposition_witness_before(
                        allocation,
                        first_drop,
                        AllocationDispositionKind::CStringIntoRaw,
                        AllocationDispositionWitnessRole::OwnershipHandoff,
                    ) {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw);
                        dispositions.push(witness);
                    }
                    if let Some(witness) = self.disposition_witness_on_path(
                        allocation,
                        &witness_path,
                        AllocationDispositionKind::CStringFromRaw,
                        AllocationDispositionWitnessRole::OwnershipReclaim,
                    ) {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedCStringFromRaw);
                        dispositions.push(witness);
                    }

                    findings.push(AllocationObligationFinding {
                        taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                        kind: AllocationObligationFindingKind::RepeatedDropWithoutReallocation,
                        strength,
                        allocation: allocation.clone(),
                        query_result: result.as_str(),
                        witness_path,
                        evidence,
                        origin_node: origin_path.first().cloned(),
                        handoff_node: None,
                        return_node: None,
                        first_drop_node: Some(first_drop.clone()),
                        second_drop_node: Some(second_drop),
                        use_node: None,
                        mismatch_node: None,
                        allocator_family: None,
                        deallocator_family: None,
                        contracts,
                        dispositions,
                        non_returning_discharge_nodes: Vec::new(),
                        summary,
                    });
                    break;
                }
            }
        }
        findings
    }

    fn allocator_mismatch_findings(&self, result: Truth) -> Vec<AllocationObligationFinding> {
        let mut findings = Vec::new();
        for (allocation, abstract_allocation) in &self.k.allocations {
            let allocator_family = abstract_allocation
                .allocator_contract
                .as_ref()
                .map(|contract| contract.family.clone())
                .unwrap_or_else(|| "unknown".into());

            for (node_id, node) in &self.k.nodes {
                for label in &node.allocation_labels {
                    if label.allocation != *allocation || label.predicate != EventKind::Drop {
                        continue;
                    }
                    let deallocator_family = label
                        .deallocator_contract
                        .as_ref()
                        .map(|contract| contract.family.clone())
                        .unwrap_or_else(|| "unknown".into());
                    let allocator_known = allocator_family != "unknown";
                    let deallocator_known = deallocator_family != "unknown";
                    if !allocation_label_matches(self, allocation, LabelPredicate::AllocatorMismatch, label) {
                        continue;
                    }

                    let Some((witness_path, origin_evidence)) = self.origin_path_to(allocation, node_id) else {
                        continue;
                    };
                    let (kind, strength) = if allocator_known && deallocator_known {
                        (
                            AllocationObligationFindingKind::AllocatorFamilyMismatch,
                            AllocationObligationFindingStrength::StrongAbstractEvidence,
                        )
                    } else {
                        (
                            AllocationObligationFindingKind::UnresolvedAllocatorContractCandidate,
                            AllocationObligationFindingStrength::ObservationalCandidate,
                        )
                    };
                    let mut evidence = vec![origin_evidence, AllocationObligationEvidence::MayDeallocationObserved];
                    if allocator_known {
                        evidence.push(AllocationObligationEvidence::KnownAllocatorFamily);
                    } else {
                        evidence.push(AllocationObligationEvidence::AllocatorFamilyUnresolved);
                    }
                    if deallocator_known {
                        evidence.push(AllocationObligationEvidence::KnownDeallocatorFamily);
                    } else {
                        evidence.push(AllocationObligationEvidence::DeallocatorFamilyUnresolved);
                    }
                    if allocator_known && deallocator_known {
                        evidence.push(AllocationObligationEvidence::AllocatorFamiliesDiffer);
                    }
                    if label.deallocator_contract.as_ref().and_then(|c| c.basis.as_deref()).is_some_and(|basis| basis != "unresolved") {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedDeallocatorContract);
                    }

                    let mut contracts = Vec::new();
                    if let Some(contract) = self.allocator_contract_witness(
                        allocation,
                        witness_path.first().cloned(),
                    ) {
                        contracts.push(contract);
                    }
                    if let Some(contract) = label.deallocator_contract.clone() {
                        let provenance = deallocator_contract_provenance(&contract);
                        contracts.push(AllocationContractWitness {
                            schema: ALLOCATION_CONTRACT_WITNESS_VERSION,
                            role: AllocationContractWitnessRole::MismatchDeallocation,
                            provenance,
                            node: Some(node_id.clone()),
                            contract,
                        });
                    }

                    let mut dispositions = Vec::new();
                    if let Some(witness) = self.disposition_witness_before(
                        allocation,
                        node_id,
                        AllocationDispositionKind::CStringIntoRaw,
                        AllocationDispositionWitnessRole::OwnershipHandoff,
                    ) {
                        evidence.push(AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw);
                        dispositions.push(witness);
                    }

                    let deallocator_basis = label
                        .deallocator_contract
                        .as_ref()
                        .and_then(|contract| contract.basis.as_deref())
                        .unwrap_or("<none>");
                    let summary = if allocator_known && deallocator_known {
                        format!(
                            "The abstract model associates allocation {allocation} with allocator family '{allocator_family}' and an ordered MAY deallocation at {node_id} with deallocator family '{deallocator_family}' under producer contract basis '{deallocator_basis}'. The families differ, which is strong abstract evidence of allocator-mismatch UB; CQPL truth remains {} because the allocation/deallocation relation is MAY rather than MUST.",
                            result.as_str(),
                        )
                    } else {
                        format!(
                            "The allocator-mismatch predicate is supported only by an unresolved allocator contract for allocation {allocation}: allocator family='{allocator_family}', deallocator family='{deallocator_family}', deallocator contract basis='{deallocator_basis}'. This is an observational mismatch candidate, not evidence that the families definitely differ; CQPL truth remains {}.",
                            result.as_str(),
                        )
                    };

                    findings.push(AllocationObligationFinding {
                        taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                        kind,
                        strength,
                        allocation: allocation.clone(),
                        query_result: result.as_str(),
                        witness_path: witness_path.clone(),
                        evidence,
                        origin_node: witness_path.first().cloned(),
                        handoff_node: None,
                        return_node: None,
                        first_drop_node: None,
                        second_drop_node: None,
                        use_node: None,
                        mismatch_node: Some(node_id.clone()),
                        allocator_family: Some(allocator_family.clone()),
                        deallocator_family: Some(deallocator_family),
                        contracts,
                        dispositions,
                        non_returning_discharge_nodes: Vec::new(),
                        summary,
                    });
                }
            }
        }
        findings
    }

    fn allocator_contract_witness(
        &self,
        allocation: &str,
        node: Option<String>,
    ) -> Option<AllocationContractWitness> {
        let contract = self.k.allocations.get(allocation)?.allocator_contract.clone()?;
        Some(AllocationContractWitness {
            schema: ALLOCATION_CONTRACT_WITNESS_VERSION,
            role: AllocationContractWitnessRole::AllocatorOrigin,
            provenance: AllocationContractWitnessProvenance::LegacyV1AllocatorSummary,
            node,
            contract,
        })
    }

    fn deallocator_contract_witness(
        &self,
        node_id: &str,
        allocation: &str,
        role: AllocationContractWitnessRole,
    ) -> Option<AllocationContractWitness> {
        let node = self.k.nodes.get(node_id)?;
        let contract = node
            .allocation_labels
            .iter()
            .find(|label| {
                label.allocation == allocation && label.predicate == EventKind::Drop
            })?
            .deallocator_contract
            .clone()?;
        let provenance = deallocator_contract_provenance(&contract);
        Some(AllocationContractWitness {
            schema: ALLOCATION_CONTRACT_WITNESS_VERSION,
            role,
            provenance,
            node: Some(node_id.to_string()),
            contract,
        })
    }

    fn disposition_witness_from_record(
        &self,
        node_id: &str,
        record: &AllocationDispositionRecord,
        role: AllocationDispositionWitnessRole,
    ) -> AllocationDispositionWitness {
        AllocationDispositionWitness {
            schema: ALLOCATION_DISPOSITION_WITNESS_VERSION,
            role,
            node: node_id.to_string(),
            kind: record.kind,
            certainty: record.certainty,
            obligation_effect: record.obligation_effect,
            basis: record.basis.clone(),
            source_variable: record.source_variable.clone(),
            target_variable: record.target_variable.clone(),
            callee_def_path: record.callee_def_path.clone(),
        }
    }

    /// Find the closest producer disposition that can reach `end_node` without
    /// crossing a new allocation event for the same AbstractAllocId.
    fn disposition_witness_before(
        &self,
        allocation: &str,
        end_node: &str,
        kind: AllocationDispositionKind,
        role: AllocationDispositionWitnessRole,
    ) -> Option<AllocationDispositionWitness> {
        let mut candidates = Vec::new();
        for (node_id, node) in &self.k.nodes {
            for record in &node.allocation_disposition {
                if record.allocation != allocation || record.kind != kind {
                    continue;
                }
                let Some(path) = self.bfs_to(
                    node_id,
                    |candidate| candidate == end_node,
                    |candidate| {
                        candidate == end_node
                            || !self.node_has_allocation_event(
                                candidate,
                                allocation,
                                EventKind::Alloc,
                            )
                    },
                ) else {
                    continue;
                };
                candidates.push((path.len(), node_id.clone(), record.clone()));
            }
        }
        candidates.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        candidates.into_iter().next().map(|(_, node_id, record)| {
            self.disposition_witness_from_record(&node_id, &record, role)
        })
    }

    /// Project the first matching producer disposition that lies on the
    /// already-selected diagnostic witness path.  This avoids importing
    /// ownership evidence from a different converging branch.
    fn disposition_witness_on_path(
        &self,
        allocation: &str,
        witness_path: &[String],
        kind: AllocationDispositionKind,
        role: AllocationDispositionWitnessRole,
    ) -> Option<AllocationDispositionWitness> {
        for node_id in witness_path {
            let Some(node) = self.k.nodes.get(node_id) else {
                continue;
            };
            if let Some(record) = node
                .allocation_disposition
                .iter()
                .find(|record| record.allocation == allocation && record.kind == kind)
            {
                return Some(self.disposition_witness_from_record(node_id, record, role));
            }
        }
        None
    }

    fn origin_path_to(
        &self,
        allocation: &str,
        target: &str,
    ) -> Option<(Vec<String>, AllocationObligationEvidence)> {
        let mut candidates = Vec::new();
        for node_id in self.k.nodes.keys() {
            let Some(evidence) = self.allocation_origin_evidence(node_id, allocation) else {
                continue;
            };
            let Some(path) = self.bfs_to(node_id, |candidate| candidate == target, |_| true) else {
                continue;
            };
            candidates.push((path, evidence));
        }
        candidates.sort_by(|a, b| {
            (a.0.len(), &a.0, a.1.as_str()).cmp(&(b.0.len(), &b.0, b.1.as_str()))
        });
        candidates.into_iter().next()
    }

    fn allocation_origin_evidence(
        &self,
        node_id: &str,
        allocation: &str,
    ) -> Option<AllocationObligationEvidence> {
        let node = self.k.nodes.get(node_id)?;
        if node.allocation_labels.iter().any(|label| {
            label.allocation == allocation && label.predicate == EventKind::Alloc
        }) {
            return Some(AllocationObligationEvidence::MayAllocationEventObserved);
        }
        if node.allocation_post.as_ref().is_some_and(|post| {
            CellValue::Alloc.leq(post.value_of(allocation))
        }) {
            return Some(AllocationObligationEvidence::AllocationStateIncludesAllocated);
        }
        None
    }

    fn node_has_allocation_event(
        &self,
        node_id: &str,
        allocation: &str,
        event: EventKind,
    ) -> bool {
        self.k.nodes.get(node_id).is_some_and(|node| {
            node.allocation_labels.iter().any(|label| {
                label.allocation == allocation && label.predicate == event
            })
        })
    }

    fn node_has_use_event(&self, node_id: &str, allocation: &str) -> bool {
        self.k.nodes.get(node_id).is_some_and(|node| {
            node.allocation_labels.iter().any(|label| {
                label.allocation == allocation
                    && matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write)
            })
        })
    }

    fn is_normal_return_node(&self, node_id: &str) -> bool {
        self.k.nodes.get(node_id).is_some_and(|node| {
            node.semantic_labels.iter().any(|label| label == "term:return")
        })
    }

    fn can_reach_normal_return(&self, start: &str) -> bool {
        self.bfs_to(start, |node| self.is_normal_return_node(node), |_| true)
            .is_some()
    }

    fn blocks_open_manual_obligation(&self, node_id: &str, allocation: &str) -> bool {
        let Some(node) = self.k.nodes.get(node_id) else { return true; };

        if node.allocation_disposition.iter().any(|record| {
            record.allocation == allocation
                && matches!(record.obligation_effect,
                    AllocationObligationEffect::RestoreRaiiObligation
                        | AllocationObligationEffect::MayEscapeToCaller
                        | AllocationObligationEffect::MayDischarge)
        }) {
            return true;
        }
        if node.allocation_labels.iter().any(|label| {
            label.allocation == allocation && label.predicate == EventKind::Drop
        }) {
            return true;
        }
        node.allocation_post.as_ref().is_some_and(|post| {
            post.value_of(allocation) == CellValue::Freed
        })
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

fn formula_contains_negated_drop(formula: &StateFormula) -> bool {
    match formula {
        StateFormula::Not(inner) => {
            matches!(inner.as_ref(),
                StateFormula::May { predicate: MayPredicate::Drop, .. }
                    | StateFormula::Label { predicate: LabelPredicate::Drop, .. })
                || formula_contains_negated_drop(inner)
        }
        StateFormula::And(a, b) | StateFormula::Or(a, b) => {
            formula_contains_negated_drop(a) || formula_contains_negated_drop(b)
        }
        StateFormula::Exists { body, .. }
        | StateFormula::ForAll { body, .. }
        | StateFormula::ExistsAlloc { body, .. }
        | StateFormula::ForAllAlloc { body, .. } => formula_contains_negated_drop(body),
        StateFormula::Path { formula, .. } => match formula {
            PathFormula::State(phi)
            | PathFormula::Next(phi)
            | PathFormula::Eventually(phi)
            | PathFormula::Globally(phi) => formula_contains_negated_drop(phi),
            PathFormula::Until(lhs, rhs) => {
                formula_contains_negated_drop(lhs) || formula_contains_negated_drop(rhs)
            }
        },
        StateFormula::May { .. }
        | StateFormula::Label { .. }
        | StateFormula::StructuralLabel { .. } => false,
    }
}

fn formula_positive_label_count(formula: &StateFormula, predicate: LabelPredicate) -> usize {
    fn visit(formula: &StateFormula, predicate: LabelPredicate, negated: bool) -> usize {
        match formula {
            StateFormula::Label { predicate: p, .. } => { if !negated && *p == predicate { 1 } else { 0 } },
            StateFormula::Not(inner) => visit(inner, predicate, !negated),
            StateFormula::And(a, b) | StateFormula::Or(a, b) => {
                visit(a, predicate, negated) + visit(b, predicate, negated)
            }
            StateFormula::Exists { body, .. }
            | StateFormula::ForAll { body, .. }
            | StateFormula::ExistsAlloc { body, .. }
            | StateFormula::ForAllAlloc { body, .. } => visit(body, predicate, negated),
            StateFormula::Path { formula, .. } => match formula {
                PathFormula::State(phi)
                | PathFormula::Next(phi)
                | PathFormula::Eventually(phi)
                | PathFormula::Globally(phi) => visit(phi, predicate, negated),
                PathFormula::Until(lhs, rhs) => {
                    visit(lhs, predicate, negated) + visit(rhs, predicate, negated)
                }
            },
            StateFormula::May { .. } | StateFormula::StructuralLabel { .. } => 0,
        }
    }
    visit(formula, predicate, false)
}

fn formula_is_use_after_free_shape(formula: &StateFormula) -> bool {
    formula_positive_label_count(formula, LabelPredicate::Drop) >= 1
        && formula_positive_label_count(formula, LabelPredicate::Use) >= 1
}

fn formula_is_double_free_shape(formula: &StateFormula) -> bool {
    formula_positive_label_count(formula, LabelPredicate::Drop) >= 2
}

fn formula_uses_allocator_mismatch(formula: &StateFormula) -> bool {
    formula_positive_label_count(formula, LabelPredicate::AllocatorMismatch) >= 1
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
        AbstractAllocation, AbstractAllocationCell, AbstractAllocationMemoryAnnotation, AbstractMemoryAnnotation, AllocationContract, AllocationDispositionRecord, AllocationEventLabel, AnnotatedIcfg, AnnotatedNode,
        Kripke, NodeIdentityAnnotation, ProgramLanguage, ProgramVariable,
    };
    use crate::parser::parse_query_document;
    use std::collections::BTreeSet;

    fn one_allocation_graph() -> Kripke {
        Kripke::from_annotated_icfg(AnnotatedIcfg {
            external_deallocation_effects: vec![],
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
                allocation_disposition: vec![],
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
    fn unknown_leak_query_reports_strong_normal_return_open_obligation_finding() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.capabilities.insert("allocation_disposition_v1".into());
        k.capabilities.insert("mir_semantic_labels_v1".into());
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.successors = vec!["b1".into(), "cleanup".into()];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
            });
            b0.allocation_disposition = vec![AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::BoxIntoRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::PreserveManualObligation,
                basis: "rustc_box_into_raw_v1".into(),
                source_variable: Some("rust::main::_1".into()),
                target_variable: Some("rust::main::_2".into()),
                callee_def_path: Some("std::boxed::Box::into_raw".into()),
            }];
        }
        k.nodes.insert("b1".into(), AnnotatedNode {
            id: "b1".into(), successors: vec![], labels: vec![],
            semantic_labels: vec!["term:return".into()], allocation_labels: vec![],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Mv }],
            }),
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("cleanup".into(), AnnotatedNode {
            id: "cleanup".into(), successors: vec!["resume".into()], labels: vec![],
            semantic_labels: vec!["term:drop".into()], allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None,
            }],
            allocation_disposition: vec![AllocationDispositionRecord {
                allocation: "A".into(), kind: AllocationDispositionKind::MayDeallocate,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::MayDischarge,
                basis: "allocation_drop_label_v1".into(), source_variable: None,
                target_variable: None, callee_def_path: None,
            }],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None, pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("resume".into(), AnnotatedNode {
            id: "resume".into(), successors: vec![], labels: vec![],
            semantic_labels: vec!["term:unwind_resume".into()], allocation_labels: vec![],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });

        let checker = ModelChecker::new(&k);
        let formula = StateFormula::ExistsAlloc {
            logic_var: "a".into(),
            body: Box::new(StateFormula::And(
                Box::new(StateFormula::May { predicate: MayPredicate::Alloc, logic_var: "a".into() }),
                Box::new(StateFormula::Not(Box::new(StateFormula::May {
                    predicate: MayPredicate::Drop, logic_var: "a".into(),
                }))),
            )),
        };
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["allocation_state_v1".into()]), formula),
            &Env::new(), 4,
        ).unwrap();

        assert_eq!(report.result, "unk");
        assert_eq!(report.supporting_findings.len(), 1);
        let finding = &report.supporting_findings[0];
        assert_eq!(finding.kind, AllocationObligationFindingKind::NormalReturnOpenManualObligation);
        assert_eq!(finding.strength, AllocationObligationFindingStrength::StrongAbstractEvidence);
        assert_eq!(finding.witness_path, vec!["b0", "b1"]);
        assert_eq!(finding.non_returning_discharge_nodes, vec!["cleanup"]);
        assert!(finding.evidence.contains(&AllocationObligationEvidence::NoInterveningCallAfterHandoff));
        assert!(finding.summary.contains("strong abstract evidence of a leak on normal completion"));

        let rendered = report.render_unknown_verbose("leak_alloc_state");
        assert!(rendered.contains("QUERY: leak_alloc_state"));
        assert!(rendered.contains("truth: unk"));
        assert!(rendered.contains("- MAY_ALLOCATION"));
        assert!(rendered.contains("kind       : normal_return_open_manual_obligation"));
        assert!(rendered.contains("strength   : strong_abstract_evidence"));
        assert!(rendered.contains("-> b1"));
        assert!(rendered.contains("producer_certified_box_into_raw"));
        assert!(rendered.contains("uncertainty witnesses:"));
    }

    #[test]
    fn b1_1_cstring_into_raw_unknown_leak_reports_producer_contract() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.capabilities.insert("allocation_disposition_v1".into());
        k.capabilities.insert("allocation_disposition_v2".into());
        k.capabilities.insert("mir_semantic_labels_v1".into());
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.successors = vec!["b1".into()];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Top }],
            });
            b0.allocation_disposition = vec![AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::CStringIntoRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::PreserveManualObligation,
                basis: "rustc_cstring_into_raw_v1".into(),
                source_variable: None,
                target_variable: None,
                callee_def_path: Some("alloc::ffi::c_str::CString::into_raw".into()),
            }];
        }
        k.nodes.insert("b1".into(), AnnotatedNode {
            id: "b1".into(), successors: vec![], labels: vec![],
            semantic_labels: vec!["term:return".into()], allocation_labels: vec![],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });

        let findings = ModelChecker::new(&k).allocation_obligation_findings(Truth::Unknown);
        let finding = findings.iter().find(|f| {
            f.evidence.contains(&AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw)
        }).expect("CString::into_raw leak finding");
        assert_eq!(finding.dispositions.len(), 1);
        assert_eq!(
            finding.dispositions[0].role,
            AllocationDispositionWitnessRole::OwnershipHandoff,
        );
        assert_eq!(finding.dispositions[0].basis, "rustc_cstring_into_raw_v1");
    }

    #[test]
    fn b1_1_cstring_double_free_explanation_carries_handoff_reclaim_chain() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_disposition_v1".into());
        k.capabilities.insert("allocation_disposition_v2".into());
        k.capabilities.insert("allocation_contracts_v1".into());
        k.capabilities.insert("allocation_contracts_v2".into());
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "rust_global".into(),
            operation: "cstring_allocation".into(),
            language: "rust".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["handoff".into()];

        k.nodes.insert("handoff".into(), AnnotatedNode {
            id: "handoff".into(), successors: vec!["drop1".into()], labels: vec![],
            semantic_labels: vec!["term:call".into()], allocation_labels: vec![],
            allocation_disposition: vec![AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::CStringIntoRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::PreserveManualObligation,
                basis: "rustc_cstring_into_raw_v1".into(),
                source_variable: None,
                target_variable: None,
                callee_def_path: Some("std::ffi::CString::into_raw".into()),
            }],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("drop1".into(), AnnotatedNode {
            id: "drop1".into(), successors: vec!["reclaim".into()], labels: vec![],
            semantic_labels: vec![], allocation_labels: vec![AllocationEventLabel {
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
            allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("reclaim".into(), AnnotatedNode {
            id: "reclaim".into(), successors: vec!["drop2".into()], labels: vec![],
            semantic_labels: vec!["term:call".into()], allocation_labels: vec![],
            allocation_disposition: vec![AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::CStringFromRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::RestoreRaiiObligation,
                basis: "rustc_cstring_from_raw_v1".into(),
                source_variable: None,
                target_variable: None,
                callee_def_path: Some("std::ffi::CString::from_raw".into()),
            }],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("drop2".into(), AnnotatedNode {
            id: "drop2".into(), successors: vec![], labels: vec![],
            semantic_labels: vec![], allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop,
                allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                deallocator_contract: Some(AllocationContract {
                    family: "rust_global".into(),
                    operation: "drop".into(),
                    language: "rust".into(),
                    basis: Some("rust_cstring_global_drop".into()),
                    owner_def_path: Some("std::ffi::CString".into()),
                    allocator_def_path: Some("std::alloc::Global".into()),
                    callee_def_path: None,
                }),
            }],
            allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });

        let doc = parse_query_document(
            "exists_alloc a. EF (alloc_l(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::RepeatedDropWithoutReallocation)
            .expect("CString double-free supporting finding");

        assert!(finding.evidence.contains(
            &AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw,
        ));
        assert!(finding.evidence.contains(
            &AllocationObligationEvidence::ProducerCertifiedCStringFromRaw,
        ));
        assert_eq!(finding.dispositions.len(), 2);
        assert_eq!(
            finding.dispositions[0].role,
            AllocationDispositionWitnessRole::OwnershipHandoff,
        );
        assert_eq!(finding.dispositions[0].node, "handoff");
        assert_eq!(finding.dispositions[0].basis, "rustc_cstring_into_raw_v1");
        assert_eq!(
            finding.dispositions[1].role,
            AllocationDispositionWitnessRole::OwnershipReclaim,
        );
        assert_eq!(finding.dispositions[1].node, "reclaim");
        assert_eq!(finding.dispositions[1].basis, "rustc_cstring_from_raw_v1");

        assert_eq!(finding.contracts.len(), 3);
        assert_eq!(
            finding.contracts[0].provenance,
            AllocationContractWitnessProvenance::LegacyV1AllocatorSummary,
        );
        assert_eq!(
            finding.contracts[1].provenance,
            AllocationContractWitnessProvenance::ProducerCertifiedV2Deallocator,
        );
        assert_eq!(
            finding.contracts[2].provenance,
            AllocationContractWitnessProvenance::ProducerCertifiedV2Deallocator,
        );

        let rendered = report.render_unknown_verbose("double_free_alloc_state");
        assert!(rendered.contains("ownership dispositions:"));
        assert!(rendered.contains("role      : ownership_handoff"));
        assert!(rendered.contains("basis     : rustc_cstring_into_raw_v1"));
        assert!(rendered.contains("role      : ownership_reclaim"));
        assert!(rendered.contains("basis     : rustc_cstring_from_raw_v1"));
        assert!(rendered.contains("provenance: legacy_v1_allocator_summary"));
        assert!(rendered.contains("basis     : <not-applicable-v1>"));
        assert!(rendered.contains("allocator_provenance=legacy_v1_allocator_summary"));
        assert!(!rendered.contains("allocator_basis=<none>"));

        let encoded = serde_json::to_value(&report).unwrap();
        let dispositions = encoded["supporting_findings"][0]["dispositions"]
            .as_array()
            .expect("serialized disposition witnesses");
        assert_eq!(dispositions.len(), 2);
    }

    #[test]
    fn restored_raii_obligation_blocks_normal_return_open_obligation_finding() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.capabilities.insert("allocation_disposition_v1".into());
        k.capabilities.insert("mir_semantic_labels_v1".into());
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.successors = vec!["b1".into()];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
            });
            b0.allocation_disposition = vec![AllocationDispositionRecord {
                allocation: "A".into(), kind: AllocationDispositionKind::BoxIntoRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::PreserveManualObligation,
                basis: "rustc_box_into_raw_v1".into(), source_variable: None,
                target_variable: None, callee_def_path: Some("std::boxed::Box::into_raw".into()),
            }];
        }
        k.nodes.insert("b1".into(), AnnotatedNode {
            id: "b1".into(), successors: vec!["b2".into()], labels: vec![],
            semantic_labels: vec!["term:call".into()], allocation_labels: vec![],
            allocation_disposition: vec![AllocationDispositionRecord {
                allocation: "A".into(), kind: AllocationDispositionKind::BoxFromRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::RestoreRaiiObligation,
                basis: "rustc_box_from_raw_v1".into(), source_variable: None,
                target_variable: None, callee_def_path: Some("std::boxed::Box::from_raw".into()),
            }],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("b2".into(), AnnotatedNode {
            id: "b2".into(), successors: vec![], labels: vec![],
            semantic_labels: vec!["term:return".into()], allocation_labels: vec![],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });

        let checker = ModelChecker::new(&k);
        let formula = StateFormula::Not(Box::new(StateFormula::May {
            predicate: MayPredicate::Drop, logic_var: "a".into(),
        }));
        let mut env = Env::new();
        env.insert("a".into(), Binding::Allocation("A".into()));
        let report = checker.explain_document(
            &QueryDocument::new(BTreeSet::from(["allocation_state_v1".into()]), formula),
            &env, 4,
        ).unwrap();
        assert!(report.supporting_findings.is_empty());
    }

    #[test]
    fn unknown_uaf_query_reports_ordered_drop_then_use_finding() {
        let mut k = one_allocation_graph();
        k.nodes.get_mut("b0").unwrap().successors = vec!["drop1".into()];
        k.nodes.insert("drop1".into(), AnnotatedNode {
            id: "drop1".into(), successors: vec!["use1".into()], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                deallocator_contract: Some(AllocationContract {
                    family: "c_malloc".into(),
                    operation: "free".into(),
                    language: "c".into(),
                    basis: Some("structural_c_free_v1".into()),
                    owner_def_path: None,
                    allocator_def_path: None,
                    callee_def_path: Some("free".into()),
                }),
            }],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("use1".into(), AnnotatedNode {
            id: "use1".into(), successors: vec![], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Read, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None,
            }],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });

        let doc = parse_query_document(
            "exists_alloc a. EF (alloc_l(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U use_l(a)]))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::DropThenUseWithoutReallocation)
            .expect("UAF supporting finding");
        assert_eq!(finding.strength, AllocationObligationFindingStrength::StrongAbstractEvidence);
        assert_eq!(finding.first_drop_node.as_deref(), Some("drop1"));
        assert_eq!(finding.use_node.as_deref(), Some("use1"));
        assert_eq!(finding.witness_path, vec!["b0", "drop1", "use1"]);
        assert!(finding.evidence.contains(&AllocationObligationEvidence::NoReallocationBetweenEvents));
        assert!(finding.summary.contains("strong abstract evidence of a use-after-free pattern"));
        let rendered = report.render_unknown_verbose("use_after_free_alloc");
        assert!(rendered.contains("kind       : drop_then_use_without_reallocation"));
        assert!(rendered.contains("first drop : drop1"));
        assert!(rendered.contains("use        : use1"));
        assert!(rendered.contains("ordered_drop_before_use"));
        assert!(rendered.contains("role      : first_deallocation"));
        assert!(rendered.contains("basis     : structural_c_free_v1"));
        assert_eq!(finding.contracts.len(), 1);
        assert_eq!(finding.contracts[0].role, AllocationContractWitnessRole::FirstDeallocation);
        let drop_atom = report.witnesses.iter()
            .flat_map(|witness| witness.atomic_observations.iter())
            .find(|atom| atom.node == "drop1")
            .expect("drop atom with contract detail");
        assert_eq!(
            drop_atom.detail.get("deallocator_contract_basis").map(String::as_str),
            Some("structural_c_free_v1"),
        );
        assert!(rendered.contains("deallocator_basis=structural_c_free_v1"));
    }

    #[test]
    fn unknown_double_free_query_reports_two_ordered_drops() {
        let mut k = one_allocation_graph();
        k.nodes.get_mut("b0").unwrap().successors = vec!["drop1".into()];
        for (id, successors) in [("drop1", vec!["drop2".into()]), ("drop2", Vec::new())] {
            k.nodes.insert(id.into(), AnnotatedNode {
                id: id.into(), successors, labels: vec![], semantic_labels: vec![],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop, allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: Some(if id == "drop1" {
                        AllocationContract {
                            family: "c_malloc".into(), operation: "free".into(), language: "c".into(),
                            basis: Some("structural_c_free_v1".into()), owner_def_path: None,
                            allocator_def_path: None, callee_def_path: Some("free".into()),
                        }
                    } else {
                        AllocationContract {
                            family: "rust_global".into(), operation: "drop".into(), language: "rust".into(),
                            basis: Some("rust_cstring_global_drop".into()),
                            owner_def_path: Some("alloc::ffi::c_str::CString".into()),
                            allocator_def_path: Some("alloc::alloc::Global".into()), callee_def_path: None,
                        }
                    }),
                }],
                allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            });
        }

        let doc = parse_query_document(
            "exists_alloc a. EF (alloc_l(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::RepeatedDropWithoutReallocation)
            .expect("double-free supporting finding");
        assert_eq!(finding.strength, AllocationObligationFindingStrength::StrongAbstractEvidence);
        assert_eq!(finding.first_drop_node.as_deref(), Some("drop1"));
        assert_eq!(finding.second_drop_node.as_deref(), Some("drop2"));
        assert_eq!(finding.witness_path, vec!["b0", "drop1", "drop2"]);
        assert!(finding.evidence.contains(&AllocationObligationEvidence::TwoOrderedDropsObserved));
        let rendered = report.render_unknown_verbose("double_free_alloc");
        assert!(rendered.contains("kind       : repeated_drop_without_reallocation"));
        assert!(rendered.contains("first drop : drop1"));
        assert!(rendered.contains("second drop: drop2"));
        assert!(rendered.contains("role      : first_deallocation"));
        assert!(rendered.contains("basis     : structural_c_free_v1"));
        assert!(rendered.contains("role      : second_deallocation"));
        assert!(rendered.contains("basis     : rust_cstring_global_drop"));
        assert_eq!(finding.contracts.len(), 2);
    }

    #[test]
    fn unknown_allocator_mismatch_reports_known_family_difference() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_contracts_v1".into());
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "c_malloc".into(), operation: "malloc".into(), language: "c".into(),
            basis: Some("rust_origin_test_v1".into()), owner_def_path: None, allocator_def_path: None, callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["free".into()];
        k.nodes.insert("free".into(), AnnotatedNode {
            id: "free".into(), successors: vec![], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                deallocator_contract: Some(AllocationContract {
                    family: "rust_global".into(), operation: "drop".into(), language: "rust".into(),
                    basis: Some("rust_cstring_global_drop".into()),
                    owner_def_path: Some("alloc::ffi::c_str::CString".into()),
                    allocator_def_path: Some("alloc::alloc::Global".into()), callee_def_path: None,
                }),
            }],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });

        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF (alloc_l(a) && EX EF allocator_mismatch_l(a))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::AllocatorFamilyMismatch)
            .expect("allocator mismatch supporting finding");
        assert_eq!(finding.strength, AllocationObligationFindingStrength::StrongAbstractEvidence);
        assert_eq!(finding.mismatch_node.as_deref(), Some("free"));
        assert_eq!(finding.allocator_family.as_deref(), Some("c_malloc"));
        assert_eq!(finding.deallocator_family.as_deref(), Some("rust_global"));
        assert!(finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamiliesDiffer));
        let rendered = report.render_unknown_verbose("allocator_mismatch_ub");
        assert!(rendered.contains("kind       : allocator_family_mismatch"));
        assert!(rendered.contains("allocator  : c_malloc"));
        assert!(rendered.contains("deallocator: rust_global"));
        assert!(rendered.contains("role      : allocator_origin"));
        assert!(rendered.contains("basis     : rust_origin_test_v1"));
        assert!(rendered.contains("role      : mismatch_deallocation"));
        assert!(rendered.contains("basis     : rust_cstring_global_drop"));
        assert!(finding.summary.contains("producer contract basis 'rust_cstring_global_drop'"));
        assert_eq!(finding.contracts.len(), 2);
    }

    #[test]
    fn unresolved_allocator_contract_is_candidate_not_proven_mismatch() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_contracts_v1".into());
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "unknown".into(), operation: "unknown".into(), language: "unknown".into(),
            basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["free".into()];
        k.nodes.insert("free".into(), AnnotatedNode {
            id: "free".into(), successors: vec![], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                deallocator_contract: Some(AllocationContract {
                    family: "c_malloc".into(), operation: "free".into(), language: "c".into(),
                    basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None,
                }),
            }],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF (alloc_l(a) && EX EF allocator_mismatch_l(a))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::UnresolvedAllocatorContractCandidate)
            .expect("unresolved-contract candidate");
        assert_eq!(finding.strength, AllocationObligationFindingStrength::ObservationalCandidate);
        assert!(finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamilyUnresolved));
        assert!(!finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamiliesDiffer));
        assert!(finding.summary.contains("not evidence that the families definitely differ"));
    }

    #[test]
    fn reallocation_between_drop_and_use_blocks_strong_uaf_finding() {
        let mut k = one_allocation_graph();
        k.nodes.get_mut("b0").unwrap().successors = vec!["drop1".into()];
        for (id, successors, predicate) in [
            ("drop1", vec!["realloc".into()], EventKind::Drop),
            ("realloc", vec!["use1".into()], EventKind::Alloc),
            ("use1", Vec::new(), EventKind::Read),
        ] {
            k.nodes.insert(id.into(), AnnotatedNode {
                id: id.into(), successors, labels: vec![], semantic_labels: vec![],
                allocation_labels: vec![AllocationEventLabel {
                    predicate, allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None,
                }],
                allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            });
        }
        let findings = ModelChecker::new(&k).use_after_free_findings(Truth::Unknown);
        assert!(findings.is_empty());
    }

    #[test]
    fn same_allocator_family_does_not_emit_mismatch_finding() {
        let mut k = one_allocation_graph();
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "c_malloc".into(), operation: "malloc".into(), language: "c".into(),
            basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["free".into()];
        k.nodes.insert("free".into(), AnnotatedNode {
            id: "free".into(), successors: vec![], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                deallocator_contract: Some(AllocationContract {
                    family: "c_malloc".into(), operation: "free".into(), language: "c".into(),
                    basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None,
                }),
            }],
            allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()), allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        let findings = ModelChecker::new(&k).allocator_mismatch_findings(Truth::Unknown);
        assert!(findings.is_empty());
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

    #[test]
    fn repeat_drop_complete_absence_is_refuted_without_unknown_explanation() {
        let mut k = one_allocation_graph();
        for capability in [
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1",
            "panic_lifecycle_state_v2",
        ] {
            k.capabilities.insert(capability.into());
        }
        k.panic_lifecycle.insert("b0".into(), vec![]);
        k.panic_lifecycle.set_coverage("b0", crate::kripke::PanicLifecycleCoverage::Complete);
        let doc = parse_query_document(
            "requires panic_lifecycle_state_v2; exists_alloc a. EF repeat_drop(a)"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "ff");
        assert!(report.witnesses.is_empty());
    }

    #[test]
    fn repeat_drop_unresolved_coverage_has_specific_frontier() {
        let mut k = one_allocation_graph();
        for capability in [
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1",
            "panic_lifecycle_state_v2",
        ] {
            k.capabilities.insert(capability.into());
        }
        k.panic_lifecycle.insert("b0".into(), vec![]);
        k.panic_lifecycle.set_coverage("b0", crate::kripke::PanicLifecycleCoverage::Unresolved);
        let doc = parse_query_document(
            "requires panic_lifecycle_state_v2; exists_alloc a. EF repeat_drop(a)"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        assert!(report.reason_frontier.contains(&UncertaintyReason::PanicLifecycleUnresolved));
        assert!(!report.reason_frontier.contains(&UncertaintyReason::MayPanicLifecycle));
        assert!(report.witnesses.iter().flat_map(|w| &w.atomic_observations).any(|atom| {
            atom.atom == "repeat_drop(a)"
                && atom.truth == "unk"
                && atom.detail.get("lifecycle_record").map(String::as_str) == Some("absent")
                && atom.detail.get("lifecycle_coverage").map(String::as_str) == Some("unresolved")
        }));
    }

    #[test]
    fn repeat_drop_unknown_explanation_reports_panic_lifecycle_may_reason() {
        let mut k = one_allocation_graph();
        for capability in [
            "mir_semantic_labels_v1",
            "mir_semantics_v2",
            "panic_unwind_lifecycle_v1",
            "panic_lifecycle_state_v1",
            "panic_lifecycle_state_v2",
        ] {
            k.capabilities.insert(capability.into());
        }
        k.panic_lifecycle.insert(
            "b0".into(),
            vec![crate::kripke::PanicLifecycleRecord {
                allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                may_own: true,
                may_partial_drop: true,
                may_stale_owner: true,
                may_committed: false,
                may_complete: false,
            }],
        );
        k.panic_lifecycle.set_coverage("b0", crate::kripke::PanicLifecycleCoverage::Complete);
        let doc = parse_query_document(
            "requires panic_lifecycle_state_v2; exists_alloc a. EF repeat_drop(a)"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        assert!(report.reason_frontier.contains(&UncertaintyReason::MayPanicLifecycle));
        assert!(!report.reason_frontier.contains(&UncertaintyReason::PanicLifecycleUnresolved));
        assert!(report.witnesses.iter().flat_map(|w| &w.atomic_observations).any(|atom| {
            atom.atom == "repeat_drop(a)"
                && atom.truth == "unk"
                && atom.detail.get("may_repeat_drop").map(String::as_str) == Some("true")
        }));
    }

}
