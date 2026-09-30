use crate::ast::{AssessmentScope, LabelPredicate, MayPredicate, PathFormula, PathQuantifier, QueryDocument, StateFormula, StructuralLabelKind};
use crate::kripke::{
    AllocationContract, AllocationDispositionKind, AllocationDispositionRecord, AllocationEventCertainty,
    AllocationObligationEffect, CellValue, EventKind, ExternalDeallocationEffectRecord,
    FfiArgumentIdentityRecord, SourceAnchor, TypedEdgeFlow,
};
use crate::model_checker::{Binding, Env, ModelChecker};
use crate::truth::Truth;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const EXPLAINABILITY_TAXONOMY_VERSION: &str = "cqpl_uncertainty_reasons_v1";
pub const ALLOCATION_OBLIGATION_DIAGNOSTICS_VERSION: &str = "allocation_obligation_diagnostics_v1";
pub const MEMORY_ERROR_DIAGNOSTICS_VERSION: &str = "memory_error_diagnostics_v1";
pub const ALLOCATION_CONTRACT_WITNESS_VERSION: &str = "allocation_contract_witness_v1";
pub const QUERY_RESULT_ASSESSMENT_VERSION: &str = "cqpl_result_assessment_v1";
pub const QUERY_WITNESS_CERTIFICATE_VERSION: &str = "query_witness_certificate_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuerySubresult { Tt, Ff, UnkTrue, UnkFalse, UnkMixed, UnkUnoriented }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryEvidenceDirection { True, False, Mixed, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryResultStrength { Unresolved, ObservationalCandidate, StrongAbstractEvidence, AbstractEstablished }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryResultAssessment {
    pub schema: &'static str,
    pub result: &'static str,
    pub subresult: QuerySubresult,
    pub direction: QueryEvidenceDirection,
    pub strength: QueryResultStrength,
    pub basis: Vec<String>,
    pub caveats: Vec<&'static str>,
}

impl QuerySubresult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tt => "tt",
            Self::Ff => "ff",
            Self::UnkTrue => "unk_true",
            Self::UnkFalse => "unk_false",
            Self::UnkMixed => "unk_mixed",
            Self::UnkUnoriented => "unk_unoriented",
        }
    }
}

impl QueryEvidenceDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Mixed => "mixed",
            Self::None => "none",
        }
    }
}

impl QueryResultStrength {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::ObservationalCandidate => "observational_candidate",
            Self::StrongAbstractEvidence => "strong_abstract_evidence",
            Self::AbstractEstablished => "abstract_established",
        }
    }
}


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
    NormalReturnUnreleasedExternalAllocation,
    AllCandidateSuffixesCrossModeledDrop,
    AllCandidateDropSuffixesExcludeRepeatedDrop,
    ConditionalReallocationObligationsClosed,
    SuccessfulReallocationThenOldUse,
    SuccessfulReallocationThenOldDrop,
    DropThenUseWithoutReallocation,
    RepeatedDropWithoutReallocation,
    AllocatorFamilyMismatch,
    AllocatorFamilyConsumerMismatch,
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
    NoInterveningCallAfterAllocation,
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
    KnownRequiredAllocatorFamily,
    AllocatorFamiliesDiffer,
    AllocatorFamilyUnresolved,
    DeallocatorFamilyUnresolved,
    ProducerCertifiedDeallocatorContract,
    ProducerCertifiedAllocatorConsumerContract,
    AllAllocationCandidatesCovered,
    AllAllocationOriginsCovered,
    NoDropFreeTerminalSuffix,
    NoDropFreeCyclicSuffix,
    ModeledFreedStateBarrier,
    ProducerCertifiedExistenceGuard,
    CompatibleDeallocationBarrier,
    ProducerCertifiedConditionalReallocation,
    ReallocationFailurePreservesSource,
    ReallocationSuccessInvalidatesSource,
    PositiveNonzeroReallocationSize,
    ReallocationResultObligationTransferred,
    CompatibleResultDeallocationBarrier,
    AllFirstDeallocationCandidatesCovered,
    NoRepeatedDropBeforeReallocation,
    NoUnresolvedDeallocationEffectOnNormalProjection,
}

impl AllocationObligationFindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NormalReturnOpenManualObligation => "normal_return_open_manual_obligation",
            Self::NormalReturnUnreleasedExternalAllocation => "normal_return_unreleased_external_allocation",
            Self::AllCandidateSuffixesCrossModeledDrop => "all_candidate_suffixes_cross_modeled_drop",
            Self::AllCandidateDropSuffixesExcludeRepeatedDrop => "all_candidate_drop_suffixes_exclude_repeated_drop",
            Self::ConditionalReallocationObligationsClosed => "conditional_reallocation_obligations_closed",
            Self::SuccessfulReallocationThenOldUse => "successful_reallocation_then_old_use",
            Self::SuccessfulReallocationThenOldDrop => "successful_reallocation_then_old_drop",
            Self::DropThenUseWithoutReallocation => "drop_then_use_without_reallocation",
            Self::RepeatedDropWithoutReallocation => "repeated_drop_without_reallocation",
            Self::AllocatorFamilyMismatch => "allocator_family_mismatch",
            Self::AllocatorFamilyConsumerMismatch => "allocator_family_consumer_mismatch",
            Self::UnresolvedAllocatorContractCandidate => "unresolved_allocator_contract_candidate",
        }
    }

    /// Whether this finding is directional evidence for the queried property.
    ///
    /// An unresolved allocator/deallocator family is diagnostically relevant,
    /// but it is symmetric with respect to "families differ" versus "families
    /// match".  It therefore cannot orient an UNKNOWN result toward true.
    fn query_direction(self) -> QueryEvidenceDirection {
        match self {
            Self::AllCandidateSuffixesCrossModeledDrop
            | Self::AllCandidateDropSuffixesExcludeRepeatedDrop
            | Self::ConditionalReallocationObligationsClosed => QueryEvidenceDirection::False,
            Self::UnresolvedAllocatorContractCandidate => QueryEvidenceDirection::None,
            _ => QueryEvidenceDirection::True,
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
            Self::ProducerCertifiedCStringIntoRaw => "producer_certified_c_string_into_raw",
            Self::ProducerCertifiedCStringFromRaw => "producer_certified_c_string_from_raw",
            Self::NormalReturnReachable => "normal_return_reachable",
            Self::NoModeledDischargeOnWitnessPath => "no_modeled_discharge_on_witness_path",
            Self::NoInterveningCallAfterHandoff => "no_intervening_call_after_handoff",
            Self::NoInterveningCallAfterAllocation => "no_intervening_call_after_allocation",
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
            Self::KnownRequiredAllocatorFamily => "known_required_allocator_family",
            Self::AllocatorFamiliesDiffer => "allocator_families_differ",
            Self::AllocatorFamilyUnresolved => "allocator_family_unresolved",
            Self::DeallocatorFamilyUnresolved => "deallocator_family_unresolved",
            Self::ProducerCertifiedDeallocatorContract => "producer_certified_deallocator_contract",
            Self::ProducerCertifiedAllocatorConsumerContract => "producer_certified_allocator_consumer_contract",
            Self::AllAllocationCandidatesCovered => "all_allocation_candidates_covered",
            Self::AllAllocationOriginsCovered => "all_allocation_origins_covered",
            Self::NoDropFreeTerminalSuffix => "no_drop_free_terminal_suffix",
            Self::NoDropFreeCyclicSuffix => "no_drop_free_cyclic_suffix",
            Self::ModeledFreedStateBarrier => "modeled_freed_state_barrier",
            Self::ProducerCertifiedExistenceGuard => "producer_certified_existence_guard",
            Self::CompatibleDeallocationBarrier => "compatible_deallocation_barrier",
            Self::ProducerCertifiedConditionalReallocation => "producer_certified_conditional_reallocation",
            Self::ReallocationFailurePreservesSource => "reallocation_failure_preserves_source",
            Self::ReallocationSuccessInvalidatesSource => "reallocation_success_invalidates_source",
            Self::PositiveNonzeroReallocationSize => "positive_nonzero_reallocation_size",
            Self::ReallocationResultObligationTransferred => "reallocation_result_obligation_transferred",
            Self::CompatibleResultDeallocationBarrier => "compatible_result_deallocation_barrier",
            Self::AllFirstDeallocationCandidatesCovered => "all_first_deallocation_candidates_covered",
            Self::NoRepeatedDropBeforeReallocation => "no_repeated_drop_before_reallocation",
            Self::NoUnresolvedDeallocationEffectOnNormalProjection => "no_unresolved_deallocation_effect_on_normal_projection",
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ffi_argument_identity: Vec<FfiArgumentIdentityRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_effects: Vec<ExternalDeallocationEffectRecord>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceGroundingStatus {
    Grounded,
    MultipleCandidateAnchors,
    SourceUnavailable,
    SyntheticNoSourceAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEventRole {
    WitnessEntry,
    OwnershipHandoff,
    NormalReturn,
    FirstDeallocation,
    SecondDeallocation,
    UseAfterDeallocation,
    MismatchDeallocation,
    AllocatorFamilyConsumer,
    ModeledFreedStateBarrier,
    NonReturningDischarge,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticAllocationSite {
    pub abstract_alloc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<serde_json::Value>,
    pub context: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub source_status: SourceGroundingStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_anchors: Vec<SourceAnchor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceGroundedDiagnosticEvent {
    pub role: DiagnosticEventRole,
    pub node: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub observed_predicates: Vec<EventKind>,
    pub source_status: SourceGroundingStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_anchors: Vec<SourceAnchor>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticWitnessEdge {
    pub source: String,
    pub destination: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flows: Vec<TypedEdgeFlow>,
    pub basis: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticAbstractWitness {
    pub model: &'static str,
    pub concrete_execution: bool,
    pub nodes: Vec<String>,
    pub edges: Vec<DiagnosticWitnessEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticUncertaintyFrontierEntry {
    pub reason: UncertaintyReason,
    pub layer: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryWitnessCertificate {
    pub schema: &'static str,
    /// Filled by the CLI from the query filename when that context exists.
    /// Library callers may leave it absent rather than inventing an identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub direction: QueryEvidenceDirection,
    pub finding_kind: AllocationObligationFindingKind,
    pub strength: AllocationObligationFindingStrength,
    pub query_result: &'static str,
    pub assessment_scope: &'static str,
    pub allocation: DiagnosticAllocationSite,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness_entry: Option<SourceGroundedDiagnosticEvent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<SourceGroundedDiagnosticEvent>,
    pub abstract_witness: DiagnosticAbstractWitness,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uncertainty_frontier: Vec<DiagnosticUncertaintyFrontierEntry>,
    pub evidence: Vec<AllocationObligationEvidence>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplanationReport {
    pub schema: &'static str,
    pub taxonomy: &'static str,
    pub result: &'static str,
    pub assessment: QueryResultAssessment,
    pub entry: String,
    pub scope_note: &'static str,
    pub reason_frontier: Vec<UncertaintyReason>,
    pub reason_counts: BTreeMap<String, usize>,
    /// Positive bug-supporting evidence is deliberately separate from the
    /// uncertainty frontier.  An `unk` query may therefore still have a strong
    /// ownership-obligation witness without being silently promoted to `tt`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supporting_findings: Vec<AllocationObligationFinding>,
    /// Directionally negative diagnostic evidence.  This is intentionally
    /// separate from `supporting_findings`: absence of a positive finding is
    /// never treated as refuting evidence, and these findings never change
    /// CQPL truth.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refuting_findings: Vec<AllocationObligationFinding>,
    /// W1 source-grounded, deterministic projection of the findings above.
    /// Absent on historical artifacts that do not declare source_provenance_v1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_certificates: Vec<QueryWitnessCertificate>,
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
    pub source_provenance_capability_present: bool,
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
        let _ = writeln!(out, "subresult: {}", self.assessment.subresult.as_str());
        let _ = writeln!(out, "direction: {}", self.assessment.direction.as_str());
        let _ = writeln!(out, "assessment strength: {}", self.assessment.strength.as_str());
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
            if !finding.ffi_argument_identity.is_empty() {
                let _ = writeln!(out, "  ffi identity:");
                for record in &finding.ffi_argument_identity {
                    let _ = writeln!(out, "    - {} arg{}: {} -> {} allocations={:?} basis={}",
                        record.node, record.arg_index, record.actual_variable, record.formal_variable,
                        record.allocations, record.basis);
                    if !record.svf_may_points_to.is_empty() {
                        let _ = writeln!(
                            out,
                            "      svf MAY points-to={:?} basis={}",
                            record.svf_may_points_to,
                            record.svf_points_to_basis.as_deref().unwrap_or("<missing>"),
                        );
                    }
                }
            }
            if !finding.external_effects.is_empty() {
                let _ = writeln!(out, "  external effects:");
                for record in &finding.external_effects {
                    let _ = writeln!(out, "    - {} callee={} status={:?} basis={}",
                        record.node, record.callee, record.status, record.basis);
                    for corroborating in &record.corroborating_bases {
                        let _ = writeln!(out, "      corroborated by: {corroborating}");
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
        let _ = writeln!(out, "refuting findings:");
        if self.refuting_findings.is_empty() {
            let _ = writeln!(out, "  <none>");
        }
        for finding in &self.refuting_findings {
            let _ = writeln!(out);
            let _ = writeln!(out, "  kind       : {}", finding.kind.as_str());
            let _ = writeln!(out, "  strength   : {}", finding.strength.as_str());
            let _ = writeln!(out, "  allocation : {}", finding.allocation);
            if let Some(node) = &finding.origin_node {
                let _ = writeln!(out, "  origin     : {node}");
            }
            let _ = writeln!(out, "  path:");
            for node in &finding.witness_path {
                let _ = writeln!(out, "    -> {node}");
            }
            let _ = writeln!(out, "  evidence:");
            for evidence in &finding.evidence {
                let _ = writeln!(out, "    - {}", evidence.as_str());
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

        let supporting_findings = self.supporting_findings_for_document(document, result);
        let refuting_findings = self.refuting_findings_for_document(document, result);
        let mut assessment_findings = supporting_findings.clone();
        assessment_findings.extend(refuting_findings.iter().cloned());
        let assessment = self.assessment_from_findings_for_document(document, result, &assessment_findings);
        let diagnostic_certificates = self.diagnostic_certificates(
            &assessment_findings,
            document.assessment_scope,
            &reason_frontier,
        )?;

        Ok(ExplanationReport {
            schema: "cqpl_explanation_v1",
            taxonomy: EXPLAINABILITY_TAXONOMY_VERSION,
            result: result.as_str(),
            assessment,
            entry: self.k.entry.clone(),
            scope_note: if !self.truth_model_is_totalized() {
                "temporary --intra compatibility mode: CQPL truth/formula witnesses retain the pre-cqpl4 maximal-finite-path semantics; diagnostics remain over the same scoped producer graph"
            } else if document.assessment_scope == AssessmentScope::NormalExecution {
                "CQPL truth/formula witnesses use the totalized already-projected abstract Kripke truth model with quiescent completion states; directional assessment findings remain over the original typed normal-edge program projection and do not assert a concrete execution"
            } else {
                "CQPL truth/formula witnesses use the totalized already-projected abstract Kripke truth model with quiescent completion states; diagnostic findings remain over the original projected annotated ICFG and do not assert a concrete execution"
            },
            reason_frontier,
            reason_counts,
            supporting_findings,
            refuting_findings,
            diagnostic_certificates,
            diagnostics: ExplanationDiagnostics {
                witnesses_requested: max_witnesses,
                witnesses_emitted: witnesses.len(),
                unknown_has_reason_frontier,
                unknown_has_specific_origin,
                true_has_witness,
                true_has_atomic_witness,
                producer_provenance_capability_present: self.k.capabilities.contains("uncertainty_provenance_v1"),
                source_provenance_capability_present: self.k.capabilities.contains("source_provenance_v1"),
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

    pub fn assess_document(
        &self,
        document: &QueryDocument,
        result: Truth,
    ) -> QueryResultAssessment {
        let mut findings = self.supporting_findings_for_document(document, result);
        findings.extend(self.refuting_findings_for_document(document, result));
        self.assessment_from_findings_for_document(document, result, &findings)
    }

    fn refuting_findings_for_document(
        &self,
        document: &QueryDocument,
        result: Truth,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown || !document.required_capabilities.contains("allocation_state_v1") {
            return Vec::new();
        }

        if formula_is_allocation_state_leak_shape(&document.formula) {
            let mut findings = self.leak_state_refuting_findings(result, document.assessment_scope);
            findings.extend(self.conditional_reallocation_leak_refuting_findings(result, document.assessment_scope));
            return findings;
        }

        if document.assessment_scope == AssessmentScope::NormalExecution
            && formula_is_allocation_state_double_free_shape(&document.formula)
        {
            return self.double_free_state_refuting_findings(result, document.assessment_scope);
        }

        Vec::new()
    }

    fn supporting_findings_for_document(
        &self,
        document: &QueryDocument,
        result: Truth,
    ) -> Vec<AllocationObligationFinding> {
        let mut findings = Vec::new();
        if formula_contains_negated_drop(&document.formula) {
            findings.extend(self.allocation_obligation_findings(result, document.assessment_scope));
        }
        if formula_is_allocation_state_leak_shape(&document.formula) {
            findings.extend(self.external_allocation_leak_findings(result, document.assessment_scope));
        }
        if formula_is_use_after_free_shape(&document.formula) {
            findings.extend(self.use_after_free_findings(result));
            findings.extend(self.conditional_reallocation_uaf_findings(result, document.assessment_scope));
        }
        if formula_is_double_free_shape(&document.formula) {
            findings.extend(self.double_free_findings(result, document.assessment_scope));
            findings.extend(self.conditional_reallocation_double_free_findings(result, document.assessment_scope));
        }
        if formula_uses_allocator_mismatch(&document.formula) {
            findings.extend(self.allocator_mismatch_findings(result));
        }
        for finding in &mut findings {
            self.attach_boundary_evidence(finding);
        }
        findings.sort_by(|a, b| {
            (a.kind.as_str(), &a.allocation, &a.witness_path)
                .cmp(&(b.kind.as_str(), &b.allocation, &b.witness_path))
        });
        findings
    }

    fn source_grounding_status(anchors: &[SourceAnchor]) -> SourceGroundingStatus {
        match anchors.len() {
            0 => SourceGroundingStatus::SourceUnavailable,
            1 => SourceGroundingStatus::Grounded,
            _ => SourceGroundingStatus::MultipleCandidateAnchors,
        }
    }

    fn generic_source_anchors(&self, node_id: &str, prefer_terminator: bool) -> Vec<SourceAnchor> {
        let Some(provenance) = self.k.source_provenance_at(node_id) else {
            return Vec::new();
        };
        let mut anchors = if prefer_terminator {
            provenance
                .anchors
                .iter()
                .filter(|anchor| anchor.kind == "mir_terminator" || anchor.kind == "llvm_node")
                .cloned()
                .collect::<Vec<_>>()
        } else {
            provenance.anchors.clone()
        };
        if anchors.is_empty() && prefer_terminator {
            anchors = provenance.anchors.clone();
        }
        anchors.sort();
        anchors.dedup();
        anchors
    }

    fn source_grounded_event(
        &self,
        allocation: &str,
        role: DiagnosticEventRole,
        node_id: &str,
        predicates: &[EventKind],
        prefer_terminator: bool,
    ) -> SourceGroundedDiagnosticEvent {
        let mut observed_predicates = Vec::new();
        let mut anchors = Vec::new();
        if predicates.is_empty() {
            anchors = self.generic_source_anchors(node_id, prefer_terminator);
        } else {
            if let Some(node) = self.k.nodes.get(node_id) {
                for predicate in predicates {
                    if node.allocation_labels.iter().any(|label| {
                        label.allocation == allocation && label.predicate == *predicate
                    }) {
                        observed_predicates.push(*predicate);
                        anchors.extend(self.k.allocation_event_source_anchors(
                            node_id,
                            allocation,
                            *predicate,
                        ));
                    }
                }
            }
            anchors.sort();
            anchors.dedup();
        }
        SourceGroundedDiagnosticEvent {
            role,
            node: node_id.to_string(),
            observed_predicates,
            source_status: Self::source_grounding_status(&anchors),
            source_anchors: anchors,
        }
    }

    fn diagnostic_allocation_site(&self, allocation: &str) -> Result<DiagnosticAllocationSite, String> {
        let record = self.k.allocations.get(allocation).ok_or_else(|| {
            format!("W1 certificate references undeclared allocation '{allocation}'")
        })?;
        let site_kind = self.k.allocation_site_kind(allocation);
        let node = self.k.allocation_site_node(allocation).map(str::to_string);
        let mut source_anchors = node
            .as_deref()
            .map(|node_id| {
                self.k
                    .allocation_event_source_anchors(node_id, allocation, EventKind::Alloc)
            })
            .unwrap_or_default();
        source_anchors.sort();
        source_anchors.dedup();

        let source_status = if site_kind == Some("synthetic") {
            SourceGroundingStatus::SyntheticNoSourceAnchor
        } else {
            Self::source_grounding_status(&source_anchors)
        };

        if matches!(site_kind, Some("rust_call") | Some("c_call")) && node.is_none() {
            return Err(format!(
                "W1 source provenance: allocation '{allocation}' has a source allocation-site kind but no canonical node_id"
            ));
        }

        Ok(DiagnosticAllocationSite {
            abstract_alloc_id: allocation.to_string(),
            display: record.display.clone(),
            site: record.site.clone(),
            context: record.context.clone(),
            node,
            source_status,
            source_anchors,
        })
    }

    fn diagnostic_abstract_witness(
        &self,
        path: &[String],
        scope: AssessmentScope,
    ) -> Result<DiagnosticAbstractWitness, String> {
        let mut edges = Vec::new();
        for pair in path.windows(2) {
            let source = &pair[0];
            let destination = &pair[1];
            let successor_present = self
                .k
                .nodes
                .get(source)
                .is_some_and(|node| node.successors.iter().any(|succ| succ == destination));
            if !successor_present {
                return Err(format!(
                    "W1 diagnostic witness contains non-edge '{source}' -> '{destination}'"
                ));
            }

            let mut flows = self
                .k
                .typed_edges
                .iter()
                .filter(|edge| edge.source == *source && edge.destination == *destination)
                .map(|edge| edge.flow)
                .collect::<Vec<_>>();
            flows.sort();
            flows.dedup();
            if self.k.capabilities.contains("typed_edge_flow_v1") && flows.is_empty() {
                return Err(format!(
                    "W1 witness edge '{source}' -> '{destination}' is missing typed_edge_flow_v1 provenance"
                ));
            }
            if scope == AssessmentScope::NormalExecution && !flows.contains(&TypedEdgeFlow::Normal) {
                return Err(format!(
                    "W1 normal_execution witness edge '{source}' -> '{destination}' lacks a normal typed edge"
                ));
            }
            edges.push(DiagnosticWitnessEdge {
                source: source.clone(),
                destination: destination.clone(),
                flows,
                basis: if self.k.capabilities.contains("typed_edge_flow_v1") {
                    "typed_edge_flow_v1"
                } else {
                    "legacy_successor_relation"
                },
            });
        }
        Ok(DiagnosticAbstractWitness {
            model: "annotated_abstract_icfg",
            concrete_execution: false,
            nodes: path.to_vec(),
            edges,
        })
    }

    fn uncertainty_layer(reason: UncertaintyReason) -> &'static str {
        match reason {
            UncertaintyReason::PathJoin | UncertaintyReason::ControlFlowUnresolved => {
                "abstract_control_flow"
            }
            UncertaintyReason::QueryThreeValuedPropagation => "cqpl_semantics",
            UncertaintyReason::UnresolvedContract
            | UncertaintyReason::ExternalEffect
            | UncertaintyReason::UnresolvedEscape
            | UncertaintyReason::GlobalTopEffect
            | UncertaintyReason::HigherOrderUnresolved
            | UncertaintyReason::PanicLifecycleUnresolved => "producer_or_contract",
            _ => "abstract_domain",
        }
    }

    fn assessment_scope_name(scope: AssessmentScope) -> &'static str {
        match scope {
            AssessmentScope::AllExecution => "all_execution",
            AssessmentScope::NormalExecution => "normal_execution",
        }
    }

    fn diagnostic_certificate_for_finding(
        &self,
        finding: &AllocationObligationFinding,
        scope: AssessmentScope,
        reason_frontier: &[UncertaintyReason],
    ) -> Result<QueryWitnessCertificate, String> {
        let allocation = self.diagnostic_allocation_site(&finding.allocation)?;
        let witness_entry_node = finding
            .origin_node
            .as_deref()
            .or_else(|| finding.witness_path.first().map(String::as_str));
        let witness_entry = witness_entry_node.map(|node_id| {
            self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::WitnessEntry,
                node_id,
                &[],
                false,
            )
        });

        let mut events = Vec::new();
        if let Some(node) = finding.handoff_node.as_deref() {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::OwnershipHandoff,
                node,
                &[],
                true,
            ));
        }
        if let Some(node) = finding.return_node.as_deref() {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::NormalReturn,
                node,
                &[],
                true,
            ));
        }
        if let Some(node) = finding.first_drop_node.as_deref() {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::FirstDeallocation,
                node,
                &[EventKind::Drop],
                true,
            ));
        }
        if let Some(node) = finding.second_drop_node.as_deref() {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::SecondDeallocation,
                node,
                &[EventKind::Drop],
                true,
            ));
        }
        if let Some(node) = finding.use_node.as_deref() {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::UseAfterDeallocation,
                node,
                &[EventKind::Use, EventKind::Read, EventKind::Write],
                false,
            ));
        }
        if let Some(node) = finding.mismatch_node.as_deref() {
            if finding.kind == AllocationObligationFindingKind::AllocatorFamilyConsumerMismatch {
                events.push(self.source_grounded_event(
                    &finding.allocation,
                    DiagnosticEventRole::AllocatorFamilyConsumer,
                    node,
                    &[],
                    true,
                ));
            } else {
                events.push(self.source_grounded_event(
                    &finding.allocation,
                    DiagnosticEventRole::MismatchDeallocation,
                    node,
                    &[EventKind::Drop],
                    true,
                ));
            }
        }
        if finding.kind == AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop {
            if let Some(node) = finding.witness_path.last() {
                events.push(self.source_grounded_event(
                    &finding.allocation,
                    DiagnosticEventRole::ModeledFreedStateBarrier,
                    node,
                    &[EventKind::Drop],
                    true,
                ));
            }
        }
        for node in &finding.non_returning_discharge_nodes {
            events.push(self.source_grounded_event(
                &finding.allocation,
                DiagnosticEventRole::NonReturningDischarge,
                node,
                &[EventKind::Drop],
                true,
            ));
        }

        Ok(QueryWitnessCertificate {
            schema: QUERY_WITNESS_CERTIFICATE_VERSION,
            query: None,
            direction: finding.kind.query_direction(),
            finding_kind: finding.kind,
            strength: finding.strength,
            query_result: finding.query_result,
            assessment_scope: Self::assessment_scope_name(scope),
            allocation,
            witness_entry,
            events,
            abstract_witness: self.diagnostic_abstract_witness(&finding.witness_path, scope)?,
            uncertainty_frontier: reason_frontier
                .iter()
                .copied()
                .map(|reason| DiagnosticUncertaintyFrontierEntry {
                    reason,
                    layer: Self::uncertainty_layer(reason),
                })
                .collect(),
            evidence: finding.evidence.clone(),
            summary: finding.summary.clone(),
        })
    }

    fn diagnostic_certificates(
        &self,
        findings: &[AllocationObligationFinding],
        scope: AssessmentScope,
        reason_frontier: &[UncertaintyReason],
    ) -> Result<Vec<QueryWitnessCertificate>, String> {
        if !self.k.capabilities.contains("source_provenance_v1") {
            return Ok(Vec::new());
        }
        findings
            .iter()
            .map(|finding| self.diagnostic_certificate_for_finding(finding, scope, reason_frontier))
            .collect()
    }

    fn attach_boundary_evidence(&self, finding: &mut AllocationObligationFinding) {
        let mut nodes: BTreeSet<String> = finding.witness_path.iter().cloned().collect();
        for node in [
            finding.origin_node.as_ref(),
            finding.handoff_node.as_ref(),
            finding.return_node.as_ref(),
            finding.first_drop_node.as_ref(),
            finding.second_drop_node.as_ref(),
            finding.use_node.as_ref(),
            finding.mismatch_node.as_ref(),
        ].into_iter().flatten() {
            nodes.insert(node.clone());
        }
        let mut ffi = Vec::new();
        let mut ext = Vec::new();
        for node in nodes {
            for record in self.k.ffi_argument_identity_at(&node) {
                if record.allocations.iter().any(|a| a == &finding.allocation) {
                    ffi.push(record.clone());
                }
            }
            if let Some(record) = self.k.external_deallocation_effect_at(&node) {
                ext.push(record.clone());
            }
        }
        ffi.sort_by(|a, b| (&a.node, a.arg_index).cmp(&(&b.node, b.arg_index)));
        ffi.dedup_by(|a, b| a.node == b.node && a.arg_index == b.arg_index);
        ext.sort_by(|a, b| a.node.cmp(&b.node));
        ext.dedup_by(|a, b| a.node == b.node);
        finding.ffi_argument_identity = ffi;
        finding.external_effects = ext;
    }

    fn assessment_from_findings_for_document(
        &self,
        document: &QueryDocument,
        result: Truth,
        findings: &[AllocationObligationFinding],
    ) -> QueryResultAssessment {
        let mut assessment = self.assessment_from_findings(result, findings);
        if result == Truth::Unknown && document.assessment_scope == AssessmentScope::NormalExecution {
            assessment.basis.push("assessment_scope:normal_execution".to_string());
            assessment.basis.push("capability:typed_edge_flow_v1".to_string());
            assessment.basis.sort();
            assessment.basis.dedup();
            assessment.caveats.push(
                "directional assessment is restricted to typed normal edges; CQPL truth remains evaluated on the complete transition relation including unwind edges",
            );
        }
        assessment
    }

    fn assessment_from_findings(
        &self,
        result: Truth,
        findings: &[AllocationObligationFinding],
    ) -> QueryResultAssessment {
        let mut basis = BTreeSet::new();
        basis.insert("cqpl_three_valued_model_check_v1".to_string());
        for finding in findings {
            basis.insert(format!("finding:{}", finding.kind.as_str()));
            basis.insert(format!("finding_strength:{}", finding.strength.as_str()));
            for evidence in &finding.evidence {
                basis.insert(format!("evidence:{}", evidence.as_str()));
            }
            for contract in &finding.contracts {
                if let Some(b) = &contract.contract.basis {
                    basis.insert(format!("contract_basis:{b}"));
                }
            }
            for disposition in &finding.dispositions {
                basis.insert(format!("disposition_basis:{}", disposition.basis));
            }
            for record in &finding.ffi_argument_identity {
                basis.insert(format!("ffi_identity_basis:{}", record.basis));
                basis.insert(format!("formal_mapping_basis:{}", record.formal_mapping_basis));
                if !record.svf_may_points_to.is_empty() {
                    if let Some(b) = &record.svf_points_to_basis {
                        basis.insert(format!("pta_basis:{b}"));
                    }
                }
            }
            for record in &finding.external_effects {
                basis.insert(format!("external_effect_basis:{}", record.basis));
                for corroborating in &record.corroborating_bases {
                    basis.insert(format!(
                        "external_effect_corroborating_basis:{corroborating}"
                    ));
                }
            }
        }
        match result {
            Truth::True => QueryResultAssessment {
                schema: QUERY_RESULT_ASSESSMENT_VERSION,
                result: result.as_str(),
                subresult: QuerySubresult::Tt,
                direction: QueryEvidenceDirection::True,
                strength: QueryResultStrength::AbstractEstablished,
                basis: basis.into_iter().collect(),
                caveats: vec!["established in the annotated abstract Kripke model; not by itself a concrete-execution proof"],
            },
            Truth::False => QueryResultAssessment {
                schema: QUERY_RESULT_ASSESSMENT_VERSION,
                result: result.as_str(),
                subresult: QuerySubresult::Ff,
                direction: QueryEvidenceDirection::False,
                strength: QueryResultStrength::AbstractEstablished,
                basis: basis.into_iter().collect(),
                caveats: vec!["refuted within the modeled predicates and current abstraction"],
            },
            Truth::Unknown => {
                let positive_findings = findings
                    .iter()
                    .filter(|finding| finding.kind.query_direction() == QueryEvidenceDirection::True)
                    .collect::<Vec<_>>();
                let negative_findings = findings
                    .iter()
                    .filter(|finding| finding.kind.query_direction() == QueryEvidenceDirection::False)
                    .collect::<Vec<_>>();
                let directional_findings = positive_findings
                    .iter()
                    .chain(negative_findings.iter())
                    .copied()
                    .collect::<Vec<_>>();
                let strength = if directional_findings
                    .iter()
                    .any(|f| f.strength == AllocationObligationFindingStrength::StrongAbstractEvidence)
                {
                    QueryResultStrength::StrongAbstractEvidence
                } else if !directional_findings.is_empty() {
                    QueryResultStrength::ObservationalCandidate
                } else {
                    QueryResultStrength::Unresolved
                };
                if positive_findings.is_empty() && negative_findings.is_empty() {
                    QueryResultAssessment {
                        schema: QUERY_RESULT_ASSESSMENT_VERSION,
                        result: result.as_str(),
                        subresult: QuerySubresult::UnkUnoriented,
                        direction: QueryEvidenceDirection::None,
                        strength,
                        basis: basis.into_iter().collect(),
                        caveats: vec![
                            "no directional supporting or refuting finding is certified beyond the uncertainty frontier",
                            "non-directional unresolved-contract candidates do not orient UNKNOWN toward true or false",
                        ],
                    }
                } else if !positive_findings.is_empty() && negative_findings.is_empty() {
                    QueryResultAssessment {
                        schema: QUERY_RESULT_ASSESSMENT_VERSION,
                        result: result.as_str(),
                        subresult: QuerySubresult::UnkTrue,
                        direction: QueryEvidenceDirection::True,
                        strength,
                        basis: basis.into_iter().collect(),
                        caveats: vec![
                            "positive supporting evidence does not promote UNKNOWN to true",
                            "MAY evidence is never promoted to MUST",
                        ],
                    }
                } else if positive_findings.is_empty() && !negative_findings.is_empty() {
                    QueryResultAssessment {
                        schema: QUERY_RESULT_ASSESSMENT_VERSION,
                        result: result.as_str(),
                        subresult: QuerySubresult::UnkFalse,
                        direction: QueryEvidenceDirection::False,
                        strength,
                        basis: basis.into_iter().collect(),
                        caveats: if negative_findings.iter().all(|finding| {
                            finding.kind == AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop
                        }) {
                            vec![
                                "refuting diagnostic evidence does not promote UNKNOWN to false",
                                "producer-certified compatible deallocation barriers remain MAY evidence rather than MUST deallocation facts",
                            ]
                        } else {
                            vec![
                                "refuting diagnostic evidence does not promote UNKNOWN to false",
                                "negative orientation is relative to the typed normal-edge projection and does not convert MAY events into MUST facts",
                            ]
                        },
                    }
                } else {
                    QueryResultAssessment {
                        schema: QUERY_RESULT_ASSESSMENT_VERSION,
                        result: result.as_str(),
                        subresult: QuerySubresult::UnkMixed,
                        direction: QueryEvidenceDirection::Mixed,
                        strength,
                        basis: basis.into_iter().collect(),
                        caveats: vec![
                            "conflicting positive and refuting diagnostic evidence leaves CQPL truth UNKNOWN",
                            "MAY evidence is never promoted to MUST",
                        ],
                    }
                }
            }
        }
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
        if depth > self.truth_state_count().saturating_mul(4).max(64) {
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
                let successors = self.truth_successors(node);
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
        let path = self.bfs_to_truth(start, |n| phi_v.get(n).copied() == Some(target), |n| {
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
            if target == Truth::Unknown && self.path_has_mixed_truth_successor_truth(&z, &path_nodes) {
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
            if let Some(path_nodes) = self.bfs_to_truth(start, |n| phi_v.get(n).copied() == Some(Truth::Unknown), |n| {
                z.get(n).copied().unwrap_or(Truth::False) != Truth::False
            }) {
                let end = path_nodes.last().unwrap().clone();
                let mut traces = self.explain_state(phi, env, &end, Truth::Unknown, limit, depth + 1, seen)?;
                for witness in &mut traces {
                    prepend_path(witness, &path_nodes, "globally", target);
                    if quantifier == PathQuantifier::ForAll { witness.complete_dependency_trace = false; }
                    if self.path_has_mixed_truth_successor_truth(&z, &path_nodes) {
                        push_reason(witness, UncertaintyReason::PathJoin);
                    }
                }
                return Ok(traces);
            }
        }

        // For tt (or a conservative fallback), return a lasso/maximal prefix in
        // the non-refuted subgraph.  This is an abstract-model witness only.
        let path_nodes = self.maximal_prefix_truth(start, |n| {
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
        if let Some(path_nodes) = self.bfs_to_truth(start, |n| rhs_v.get(n).copied() == Some(target), |n| {
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
            if let Some(path_nodes) = self.bfs_to_truth(start, |n| lhs_v.get(n).copied() == Some(Truth::Unknown), |n| {
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

        let node = self.temporal.nodes.get(node_id)
            .ok_or_else(|| format!("explainability: missing truth-model node '{node_id}'"))?;
        match binding {
            Binding::ProgramVar(var) => {
                let value = node.post.value_of(var);
                detail.insert("program_var".into(), var.clone());
                detail.insert("post_value".into(), format!("{:?}", value));
                if value == CellValue::Top {
                    reasons.insert(UncertaintyReason::AbstractTopState);
                }
                if self.temporal.aliases_at(node_id, var).len() > 1 {
                    reasons.insert(UncertaintyReason::AliasJoin);
                }
                if identity_component_is_merged(node, Some(var), None) {
                    reasons.insert(UncertaintyReason::AbstractComponentMerge);
                }
            }
            Binding::Allocation(allocation) => {
                detail.insert("allocation".into(), allocation.clone());
                if predicate == MayPredicate::RepeatDrop {
                    let matching = self.temporal.panic_lifecycle
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
                            self.temporal.panic_lifecycle.coverage_at(node_id)
                        ));
                    } else if self.temporal.panic_lifecycle.coverage_at(node_id)
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
                let aliases = self.temporal.aliases_at(node_id, var);
                detail.insert("alias_component_size".into(), aliases.len().to_string());
                if truth == Truth::Unknown && aliases.len() > 1 {
                    reasons.insert(UncertaintyReason::AliasJoin);
                }
            }
            Binding::Allocation(allocation) => {
                detail.insert("allocation".into(), allocation.clone());
                let node = self.temporal.nodes.get(node_id)
                    .ok_or_else(|| format!("explainability: missing truth-model node '{node_id}'"))?;
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

    fn allocation_obligation_findings(&self, result: Truth, scope: AssessmentScope) -> Vec<AllocationObligationFinding> {
        if !self.k.capabilities.contains("allocation_disposition_v1")
            || !self.k.capabilities.contains("mir_semantic_labels_v1")
        {
            return Vec::new();
        }

        let mut findings = Vec::new();
        let reachable = self.reachable_nodes_in_scope(scope);
        for (handoff_node, node) in &self.k.nodes {
            if !reachable.contains(handoff_node) {
                continue;
            }
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
                let path = self.bfs_to_in_scope(scope,
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
                        if !reachable.contains(node_id) {
                            return None;
                        }
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
                            && !self.can_reach_normal_return_in_scope(scope, node_id)
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
                    .allocator_contract_witness(&allocation)
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
                    ffi_argument_identity: Vec::new(),
                    external_effects: Vec::new(),
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

    /// Positive diagnostic orientation for nullable bodyless C allocator origins.
    ///
    /// This is deliberately narrower than the generic allocation-state query:
    /// only an explicit `c_call` allocation whose producer contract is exactly
    /// `c_malloc` / `malloc|calloc|strdup|realloc` / `c` is eligible.  Until
    /// `external_contract_allocations_v1` provides a first-class proof-carrying
    /// discriminator, W1 Rust source provenance at the allocation event is used
    /// as a fail-closed proxy for the contract-materialized bodyless case.  A
    /// represented C/LLVM allocation therefore cannot enter this finding.
    ///
    /// The legacy TOP state is not itself treated as positive evidence; the
    /// explicit allocation event and allocator contract are required.  Because
    /// TOP also includes allocator failure/null, this diagnostic additionally
    /// fails closed whenever any modeled discharge/escape for the same abstract
    /// allocation is reachable in scope.  Without branch-sensitive correlation
    /// between allocation existence and the null/non-null guard, a path that
    /// merely avoids that discharge is not sufficient positive leak evidence.
    /// The routine never changes CQPL truth and also fails closed on static-site
    /// recurrence.
    fn external_allocation_leak_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown
            || !self.k.capabilities.contains("allocation_state_v1")
            || !self.k.capabilities.contains("allocation_contracts_v1")
            || !self.k.capabilities.contains("source_provenance_v1")
        {
            return Vec::new();
        }

        let reachable = self.reachable_nodes_in_scope(scope);
        let mut findings = Vec::new();

        for allocation in self.k.allocation_ids() {
            if self.k.allocation_site_kind(allocation) != Some("c_call") {
                continue;
            }
            let Some(record) = self.k.allocations.get(allocation) else { continue; };
            let Some(contract) = record.allocator_contract.as_ref() else { continue; };
            let site_allocator = record
                .site
                .as_ref()
                .and_then(|site| site.get("allocator"))
                .and_then(serde_json::Value::as_str);
            if contract.family != "c_malloc"
                || contract.language != "c"
                || !matches!(contract.operation.as_str(), "malloc" | "calloc" | "strdup" | "realloc")
                || site_allocator != Some(contract.operation.as_str())
            {
                continue;
            }

            let Some(origin) = self.k.allocation_site_node(allocation) else { continue; };
            if !reachable.contains(origin)
                || !self.node_has_allocation_event(origin, allocation, EventKind::Alloc)
            {
                continue;
            }

            // Temporary fail-closed bodyless discriminator.  A Phase-B-minimal
            // materialized allocation is anchored at the Rust MIR external-call
            // site; represented C/LLVM allocator bodies are anchored in C.  Do
            // not infer bodylessness from `site.kind == c_call` alone.
            let Some(origin_provenance) = self.k.source_provenance_at(origin) else { continue; };
            if origin_provenance.language != "rust" {
                continue;
            }

            // One static site can summarize multiple dynamic objects.  Until a
            // recurrence-sensitive positive leak certificate is defined, do not
            // orient such cases from the site abstraction alone.
            if self.node_can_recur_in_scope(scope, origin) {
                continue;
            }

            // Nullable TOP does not correlate the abstract allocation identity
            // with the non-null branch.  If any modeled discharge or escape of
            // this same abstract allocation is reachable, an alternative path
            // avoiding it may simply be the allocator-failure/null branch.  In
            // that case positive orientation would be unsound; fail closed.
            if self
                .bfs_to_in_scope(
                    scope,
                    origin,
                    |candidate| {
                        candidate != origin
                            && self.blocks_open_manual_obligation(candidate, allocation)
                    },
                    |_| true,
                )
                .is_some()
            {
                continue;
            }

            let path = self.bfs_to_in_scope(
                scope,
                origin,
                |candidate| self.is_normal_return_node(candidate),
                |candidate| !self.blocks_open_manual_obligation(candidate, allocation),
            );
            let Some(witness_path) = path else { continue; };
            let Some(return_node) = witness_path.last().cloned() else { continue; };

            let has_intervening_call = witness_path
                .iter()
                .skip(1)
                .take(witness_path.len().saturating_sub(2))
                .any(|node_id| {
                    self.k.nodes.get(node_id).is_some_and(|node| {
                        node.semantic_labels.iter().any(|label| label == "term:call")
                    })
                });

            let strength = if has_intervening_call {
                AllocationObligationFindingStrength::ObservationalCandidate
            } else {
                AllocationObligationFindingStrength::StrongAbstractEvidence
            };

            let mut evidence = vec![
                AllocationObligationEvidence::MayAllocationEventObserved,
                AllocationObligationEvidence::KnownAllocatorFamily,
                AllocationObligationEvidence::NormalReturnReachable,
                AllocationObligationEvidence::NoModeledDischargeOnWitnessPath,
            ];
            if !has_intervening_call {
                evidence.push(AllocationObligationEvidence::NoInterveningCallAfterAllocation);
            }

            let summary = match strength {
                AllocationObligationFindingStrength::StrongAbstractEvidence => format!(
                    "The abstract model contains a normal-return path from an explicit bodyless C {} allocation origin on which allocation {} has no modeled deallocation, obligation discharge, or escape. This is strong abstract evidence of a leak on normal completion; the nullable TOP allocation state remains MAY evidence and CQPL truth stays {}.",
                    contract.operation,
                    allocation,
                    result.as_str(),
                ),
                AllocationObligationFindingStrength::ObservationalCandidate => format!(
                    "The abstract model contains a normal-return path from an explicit bodyless C {} allocation origin on which allocation {} has no modeled deallocation, but the path contains an intervening call whose effects may be outside the current disposition vocabulary. This is positive observational evidence only; CQPL truth remains {}.",
                    contract.operation,
                    allocation,
                    result.as_str(),
                ),
            };

            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation,
                strength,
                allocation: allocation.clone(),
                query_result: result.as_str(),
                witness_path,
                evidence,
                origin_node: Some(origin.to_string()),
                handoff_node: None,
                return_node: Some(return_node),
                first_drop_node: None,
                second_drop_node: None,
                use_node: None,
                mismatch_node: None,
                allocator_family: Some(contract.family.clone()),
                deallocator_family: None,
                contracts: self.allocator_contract_witness(allocation).into_iter().collect(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary,
            });
        }

        findings.sort_by(|a, b| {
            (&a.allocation, &a.origin_node, &a.return_node)
                .cmp(&(&b.allocation, &b.origin_node, &b.return_node))
        });
        findings
    }

    /// allocation-state leak query.
    ///
    /// Scientific contract:
    /// - this routine is invoked only for an already-UNKNOWN query;
    /// - it never changes CQPL truth;
    /// - exact allocation origins are refuted only when every in-scope suffix
    ///   crosses a producer-certified compatible deallocation barrier;
    /// - nullable/TOP allocation origins additionally require an AGE1
    ///   `allocation_existence_guards_v1` certificate. The null successor is
    ///   excluded because the represented allocation does not concretely exist
    ///   there; the non-null successor must be closed by a compatible discharge;
    /// - allocation-site recurrence, unresolved external effects, mismatched
    ///   deallocators, true escape, and unreclaimed-obligation dispositions all
    ///   fail closed.
    fn leak_state_refuting_findings(&self, result: Truth, scope: AssessmentScope) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown || !self.k.capabilities.contains("allocation_state_v1") {
            return Vec::new();
        }

        let reachable = self.reachable_nodes_in_scope(scope);
        let candidates = self
            .k
            .allocation_ids()
            .filter(|allocation| {
                self.k.nodes.keys().filter(|node_id| reachable.contains(*node_id)).any(|node_id| {
                    self.k.allocation_may_hold(
                        node_id.as_str(),
                        allocation.as_str(),
                        MayPredicate::Alloc,
                    ) != Truth::False
                })
            })
            .cloned()
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return Vec::new();
        }

        let mut findings = Vec::new();
        for allocation in &candidates {
            let origins = self
                .k
                .nodes
                .keys()
                .filter(|node_id| reachable.contains(*node_id))
                .filter(|node_id| {
                    self.node_has_allocation_event(
                        node_id.as_str(),
                        allocation.as_str(),
                        EventKind::Alloc,
                    )
                })
                .cloned()
                .collect::<Vec<_>>();

            if origins.is_empty() {
                return Vec::new();
            }

            // One static allocation site may summarize several dynamic objects.
            // Negative leak orientation is not iteration-sensitive yet.
            if origins
                .iter()
                .any(|origin| self.node_can_recur_in_scope(scope, origin.as_str()))
            {
                return Vec::new();
            }

            let mut representative_path = Vec::new();
            let mut used_existence_guard = false;
            for origin in &origins {
                let origin_value = self
                    .k
                    .nodes
                    .get(origin)
                    .and_then(|node| node.allocation_post.as_ref())
                    .map(|post| post.value_of(allocation.as_str()))
                    .unwrap_or(CellValue::Bottom);

                let covered = match origin_value {
                    CellValue::Alloc => {
                        !self.negative_leak_suffix_can_escape(
                            scope,
                            origin,
                            allocation,
                            None,
                        )
                    }
                    CellValue::Top => {
                        let guard = self
                            .k
                            .allocation_existence_guards_for(allocation)
                            .filter(|guard| reachable.contains(&guard.predicate_call_node))
                            .find(|guard| {
                                self.nullable_origin_is_closed_by_guard(
                                    scope,
                                    origin,
                                    allocation,
                                    guard,
                                )
                            });
                        if guard.is_some() {
                            used_existence_guard = true;
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                if !covered {
                    return Vec::new();
                }

                if representative_path.is_empty() {
                    representative_path = self
                        .bfs_to_in_scope(
                            scope,
                            origin,
                            |candidate| {
                                self.is_compatible_deallocation_barrier(candidate, allocation)
                                    || self.legacy_exact_freed_barrier(candidate, allocation)
                            },
                            |candidate| {
                                candidate == origin
                                    || !self.node_blocks_negative_leak_orientation(candidate, allocation)
                                    || self.is_compatible_deallocation_barrier(candidate, allocation)
                                    || self.legacy_exact_freed_barrier(candidate, allocation)
                            },
                        )
                        .unwrap_or_else(|| vec![origin.clone()]);
                }
            }

            let mut evidence = vec![
                AllocationObligationEvidence::AllAllocationCandidatesCovered,
                AllocationObligationEvidence::AllAllocationOriginsCovered,
                AllocationObligationEvidence::NoDropFreeTerminalSuffix,
                AllocationObligationEvidence::NoDropFreeCyclicSuffix,
                AllocationObligationEvidence::CompatibleDeallocationBarrier,
            ];
            if used_existence_guard {
                evidence.push(AllocationObligationEvidence::ProducerCertifiedExistenceGuard);
            }

            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop,
                strength: AllocationObligationFindingStrength::ObservationalCandidate,
                allocation: allocation.clone(),
                query_result: result.as_str(),
                witness_path: representative_path,
                evidence,
                origin_node: origins.first().cloned(),
                handoff_node: None,
                return_node: None,
                first_drop_node: None,
                second_drop_node: None,
                use_node: None,
                mismatch_node: None,
                allocator_family: self
                    .k
                    .allocations
                    .get(allocation)
                    .and_then(|record| record.allocator_contract.as_ref())
                    .map(|contract| contract.family.clone()),
                deallocator_family: None,
                contracts: self.allocator_contract_witness(allocation).into_iter().collect(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary: format!(
                    "For allocation {allocation}, every concretely existing allocation candidate in the normal-execution projection is closed by a producer-certified compatible deallocation. Nullable allocation-failure branches are excluded only when allocation_existence_guards_v1 certifies the exact raw-pointer is_null split. This is negative observational evidence only; CQPL truth remains {}.",
                    result.as_str(),
                ),
            });
        }

        findings
    }

    fn legacy_exact_freed_barrier(&self, node_id: &str, allocation: &str) -> bool {
        !self.k.capabilities.contains("allocation_contracts_v1")
            && self.is_exact_freed_state(node_id, allocation)
    }

    fn is_exact_freed_state(&self, node_id: &str, allocation: &str) -> bool {
        self.k.nodes.get(node_id).is_some_and(|node| {
            node.allocation_post.as_ref().is_some_and(|post| {
                post.value_of(allocation) == CellValue::Freed
            })
        })
    }

    fn is_compatible_deallocation_barrier(&self, node_id: &str, allocation: &str) -> bool {
        let Some(allocator_family) = self
            .k
            .allocations
            .get(allocation)
            .and_then(|record| record.allocator_contract.as_ref())
            .map(|contract| contract.family.as_str())
        else {
            return false;
        };
        self.k.nodes.get(node_id).is_some_and(|node| {
            node.allocation_labels.iter().any(|label| {
                label.allocation == allocation
                    && label.predicate == EventKind::Drop
                    && label.deallocator_contract.as_ref().is_some_and(|contract| {
                        contract.family == allocator_family
                            && contract.basis.as_deref().is_some_and(|basis| basis != "unresolved")
                    })
            })
        })
    }

    fn node_blocks_negative_leak_orientation(&self, node_id: &str, allocation: &str) -> bool {
        if self.k.has_unmodeled_reallocation_boundary(node_id, allocation) {
            // RBF1 is a proof firewall only. Until the conditional realloc
            // success/failure semantics are modeled allocation-centrically,
            // a later free of the old pointer cannot certify that the original
            // obligation was validly discharged on every branch.
            return true;
        }
        if self.k.external_deallocation_effect_at(node_id).is_some_and(|record| {
            record.status == crate::kripke::ExternalDeallocationEffectStatus::Unresolved
        }) {
            return true;
        }

        let Some(node) = self.k.nodes.get(node_id) else {
            return true;
        };

        let has_drop = node.allocation_labels.iter().any(|label| {
            label.allocation == allocation && label.predicate == EventKind::Drop
        });
        if has_drop && !self.is_compatible_deallocation_barrier(node_id, allocation) {
            // A mismatched or unresolved deallocator is UB evidence, not a
            // certificate that the allocator obligation was correctly closed.
            return true;
        }

        node.allocation_disposition.iter().any(|record| {
            if record.allocation != allocation {
                return false;
            }
            match record.obligation_effect {
                AllocationObligationEffect::PreserveManualObligation
                | AllocationObligationEffect::RestoreRaiiObligation
                | AllocationObligationEffect::NoPointeeLifecycleEffect => false,
                AllocationObligationEffect::PreservePersistentObligation
                | AllocationObligationEffect::PreserveUnreclaimedObligation
                | AllocationObligationEffect::MayEscapeToCaller => true,
                AllocationObligationEffect::MayDischarge => {
                    !self.is_compatible_deallocation_barrier(node_id, allocation)
                }
            }
        })
    }

    fn nullable_origin_is_closed_by_guard(
        &self,
        scope: AssessmentScope,
        origin: &str,
        allocation: &str,
        guard: &crate::kripke::AllocationExistenceGuardRecord,
    ) -> bool {
        // First prove that all in-scope paths from the allocation origin either
        // reach this exact certified is_null predicate or already cross a
        // compatible discharge. A terminal/cycle/escape before the predicate
        // would make the existence-conditioned argument incomplete.
        if self.negative_leak_suffix_can_escape(
            scope,
            origin,
            allocation,
            Some(&guard.predicate_call_node),
        ) {
            return false;
        }

        // On the null successor this allocation does not concretely exist by
        // the producer certificate. Only the non-null continuation carries the
        // allocation obligation and therefore must be closed before completion.
        self.k.nodes.contains_key(&guard.non_null_successor)
            && !self.negative_leak_suffix_can_escape(
                scope,
                &guard.non_null_successor,
                allocation,
                None,
            )
    }

    /// Return true when the suffix can reach a maximal terminal/cycle or unsafe
    /// ownership boundary without crossing a compatible deallocation barrier.
    /// `stop_at` is a proof barrier used only to establish that a certified
    /// existence predicate dominates the relevant nullable origin paths.
    fn negative_leak_suffix_can_escape(
        &self,
        scope: AssessmentScope,
        start: &str,
        allocation: &str,
        stop_at: Option<&str>,
    ) -> bool {
        if stop_at == Some(start) {
            return false;
        }
        // A producer-certified compatible deallocation, or the legacy exact
        // FREED-state barrier when allocation contracts are unavailable, closes
        // the suffix before generic blocking rules are considered.  This order
        // preserves the pre-AGE1 exact-state negative proof while modern
        // contract-carrying graphs still reject mismatched/unresolved drops
        // because `legacy_exact_freed_barrier` is disabled there.
        if self.is_compatible_deallocation_barrier(start, allocation)
            || self.legacy_exact_freed_barrier(start, allocation)
        {
            return false;
        }
        if self.node_blocks_negative_leak_orientation(start, allocation) {
            return true;
        }

        let mut color: BTreeMap<String, u8> = BTreeMap::new();
        self.negative_leak_dfs_can_escape(
            scope,
            start,
            allocation,
            stop_at,
            &mut color,
        )
    }

    fn negative_leak_dfs_can_escape(
        &self,
        scope: AssessmentScope,
        node_id: &str,
        allocation: &str,
        stop_at: Option<&str>,
        color: &mut BTreeMap<String, u8>,
    ) -> bool {
        if stop_at == Some(node_id) {
            return false;
        }
        if self.is_compatible_deallocation_barrier(node_id, allocation)
            || self.legacy_exact_freed_barrier(node_id, allocation)
        {
            return false;
        }
        if self.node_blocks_negative_leak_orientation(node_id, allocation) {
            return true;
        }

        match color.get(node_id).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }

        if !self.k.nodes.contains_key(node_id) {
            return true;
        }
        let successors = self.successors_in_scope(scope, node_id);
        if successors.is_empty() {
            return true;
        }

        color.insert(node_id.to_string(), 1);
        for successor in &successors {
            if self.negative_leak_dfs_can_escape(
                scope,
                successor,
                allocation,
                stop_at,
                color,
            ) {
                return true;
            }
        }
        color.insert(node_id.to_string(), 2);
        false
    }

    fn node_can_recur_in_scope(&self, scope: AssessmentScope, node_id: &str) -> bool {
        self.successors_in_scope(scope, node_id).into_iter().any(|successor| {
            self.bfs_to_in_scope(scope, &successor, |candidate| candidate == node_id, |_| true)
                .is_some()
        })
    }


    fn conditional_reallocation_uaf_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown || !self.k.capabilities.contains("conditional_reallocations_v1") {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for record in &self.k.conditional_reallocations {
            let Some(path) = self.bfs_to_in_scope(
                scope,
                &record.success_successor,
                |node| self.node_has_use_event(node, &record.source_allocation),
                |_| true,
            ) else { continue; };
            let Some(use_node) = path.last().cloned() else { continue; };
            let mut witness_path = vec![record.reallocation_node.clone(), record.outcome_predicate_call_node.clone(), record.outcome_switch_node.clone()];
            witness_path.extend(path);
            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::SuccessfulReallocationThenOldUse,
                strength: AllocationObligationFindingStrength::StrongAbstractEvidence,
                allocation: record.source_allocation.clone(),
                query_result: result.as_str(),
                witness_path,
                evidence: vec![
                    AllocationObligationEvidence::ProducerCertifiedConditionalReallocation,
                    AllocationObligationEvidence::PositiveNonzeroReallocationSize,
                    AllocationObligationEvidence::ReallocationSuccessInvalidatesSource,
                    AllocationObligationEvidence::MayUseObserved,
                    AllocationObligationEvidence::OrderedDropBeforeUse,
                ],
                origin_node: Some(record.source_existence_predicate_call_node.clone()),
                handoff_node: None,
                return_node: None,
                first_drop_node: None,
                second_drop_node: None,
                use_node: Some(use_node),
                mismatch_node: None,
                allocator_family: Some("c_malloc".into()),
                deallocator_family: Some("c_malloc".into()),
                contracts: Vec::new(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary: format!(
                    "The producer-certified conditional realloc relation proves that successful realloc invalidates old allocation {} before a later MAY use/read/write through the old identity. CQPL truth remains {} because the allocation identity is MAY.",
                    record.source_allocation,
                    result.as_str(),
                ),
            });
        }
        findings
    }

    fn conditional_reallocation_double_free_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown || !self.k.capabilities.contains("conditional_reallocations_v1") {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for record in &self.k.conditional_reallocations {
            let Some(path) = self.bfs_to_in_scope(
                scope,
                &record.success_successor,
                |node| self.node_has_allocation_event(node, &record.source_allocation, EventKind::Drop),
                |_| true,
            ) else { continue; };
            let Some(second_drop) = path.last().cloned() else { continue; };
            let mut witness_path = vec![record.reallocation_node.clone(), record.outcome_predicate_call_node.clone(), record.outcome_switch_node.clone()];
            witness_path.extend(path);
            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::SuccessfulReallocationThenOldDrop,
                strength: AllocationObligationFindingStrength::StrongAbstractEvidence,
                allocation: record.source_allocation.clone(),
                query_result: result.as_str(),
                witness_path,
                evidence: vec![
                    AllocationObligationEvidence::ProducerCertifiedConditionalReallocation,
                    AllocationObligationEvidence::PositiveNonzeroReallocationSize,
                    AllocationObligationEvidence::ReallocationSuccessInvalidatesSource,
                    AllocationObligationEvidence::MayDeallocationObserved,
                    AllocationObligationEvidence::TwoOrderedDropsObserved,
                ],
                origin_node: Some(record.source_existence_predicate_call_node.clone()),
                handoff_node: None,
                return_node: None,
                first_drop_node: None,
                second_drop_node: Some(second_drop),
                use_node: None,
                mismatch_node: None,
                allocator_family: Some("c_malloc".into()),
                deallocator_family: Some("c_malloc".into()),
                contracts: Vec::new(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary: format!(
                    "The producer-certified successful realloc already invalidates old allocation {}; a later explicit MAY drop of that old identity is strong abstract evidence of a repeated deallocation. CQPL truth remains {} because the allocation identity is MAY.",
                    record.source_allocation,
                    result.as_str(),
                ),
            });
        }
        findings
    }

    fn result_reallocation_obligation_can_escape(
        &self,
        scope: AssessmentScope,
        start: &str,
        closing_nodes: &BTreeSet<String>,
    ) -> bool {
        fn dfs(
            checker: &ModelChecker<'_>,
            scope: AssessmentScope,
            node: &str,
            closing_nodes: &BTreeSet<String>,
            active: &mut BTreeSet<String>,
            memo: &mut BTreeMap<String, bool>,
        ) -> bool {
            if closing_nodes.contains(node) {
                return false;
            }
            if let Some(value) = memo.get(node) {
                return *value;
            }
            if !active.insert(node.to_string()) {
                return true;
            }
            let successors = checker.successors_in_scope(scope, node);
            let escapes = successors.is_empty()
                || successors.iter().any(|successor| {
                    dfs(checker, scope, successor, closing_nodes, active, memo)
                });
            active.remove(node);
            memo.insert(node.to_string(), escapes);
            escapes
        }
        dfs(self, scope, start, closing_nodes, &mut BTreeSet::new(), &mut BTreeMap::new())
    }

    fn conditional_reallocation_representative_closure_path(
        &self,
        scope: AssessmentScope,
        record: &crate::kripke::ConditionalReallocationRecord,
        closing_nodes: &BTreeSet<String>,
    ) -> Option<Vec<String>> {
        fn append_segment(path: &mut Vec<String>, segment: Vec<String>) -> Option<()> {
            if segment.is_empty() {
                return None;
            }
            if path.is_empty() {
                path.extend(segment);
                return Some(());
            }
            if path.last() != segment.first() {
                return None;
            }
            path.extend(segment.into_iter().skip(1));
            Some(())
        }

        // A diagnostic witness must be one concrete CFG path.  The universal
        // CR1 proof covers both realloc outcomes, but serializing the failure
        // and success successors consecutively would fabricate a non-edge.
        // Use the certified success outcome as a deterministic representative
        // path; branch-universal coverage remains carried by the evidence set.
        let to_realloc = self.bfs_to_in_scope(
            scope,
            &record.source_existence_predicate_call_node,
            |node| node == record.reallocation_node.as_str(),
            |_| true,
        )?;
        let to_outcome_predicate = self.bfs_to_in_scope(
            scope,
            &record.reallocation_node,
            |node| node == record.outcome_predicate_call_node.as_str(),
            |_| true,
        )?;
        let to_outcome_switch = self.bfs_to_in_scope(
            scope,
            &record.outcome_predicate_call_node,
            |node| node == record.outcome_switch_node.as_str(),
            |_| true,
        )?;
        if !self
            .successors_in_scope(scope, &record.outcome_switch_node)
            .iter()
            .any(|successor| successor == &record.success_successor)
        {
            return None;
        }
        let success_to_close = self.bfs_to_in_scope(
            scope,
            &record.success_successor,
            |node| closing_nodes.contains(node),
            |_| true,
        )?;

        let mut path = Vec::new();
        append_segment(&mut path, to_realloc)?;
        append_segment(&mut path, to_outcome_predicate)?;
        append_segment(&mut path, to_outcome_switch)?;
        append_segment(
            &mut path,
            vec![record.outcome_switch_node.clone(), record.success_successor.clone()],
        )?;
        append_segment(&mut path, success_to_close)?;
        Some(path)
    }

    fn conditional_reallocation_leak_refuting_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown || !self.k.capabilities.contains("conditional_reallocations_v1") {
            return Vec::new();
        }
        let reachable = self.reachable_nodes_in_scope(scope);
        let candidates = self
            .k
            .allocation_ids()
            .filter(|allocation| {
                self.k.nodes.keys().filter(|node| reachable.contains(*node)).any(|node| {
                    self.k.allocation_may_hold(node, allocation, MayPredicate::Alloc) != Truth::False
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Vec::new();
        }

        // The leak query is existential over allocations, so negative
        // orientation is sound only when *every* candidate allocation is
        // covered. CR1-v1 deliberately fails closed rather than combining a
        // conditional proof for one allocation with unrelated generic facts.
        let mut findings = Vec::new();
        for allocation in &candidates {
            let matching = self
                .k
                .conditional_reallocations_for(allocation)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Vec::new();
            }
            let record = matching[0];
            if self.node_can_recur_in_scope(scope, &record.reallocation_node) {
                return Vec::new();
            }
            if self.negative_leak_suffix_can_escape(
                scope,
                &record.failure_successor,
                &record.source_allocation,
                None,
            ) {
                return Vec::new();
            }
            let closing_nodes = record
                .result_deallocations
                .iter()
                .map(|d| d.node.clone())
                .collect::<BTreeSet<_>>();
            if closing_nodes.is_empty()
                || self.result_reallocation_obligation_can_escape(
                    scope,
                    &record.success_successor,
                    &closing_nodes,
                )
            {
                return Vec::new();
            }
            let Some(witness_path) = self.conditional_reallocation_representative_closure_path(
                scope,
                record,
                &closing_nodes,
            ) else {
                return Vec::new();
            };
            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::ConditionalReallocationObligationsClosed,
                strength: AllocationObligationFindingStrength::StrongAbstractEvidence,
                allocation: record.source_allocation.clone(),
                query_result: result.as_str(),
                witness_path,
                evidence: vec![
                    AllocationObligationEvidence::AllAllocationCandidatesCovered,
                    AllocationObligationEvidence::AllAllocationOriginsCovered,
                    AllocationObligationEvidence::ProducerCertifiedExistenceGuard,
                    AllocationObligationEvidence::ProducerCertifiedConditionalReallocation,
                    AllocationObligationEvidence::PositiveNonzeroReallocationSize,
                    AllocationObligationEvidence::ReallocationFailurePreservesSource,
                    AllocationObligationEvidence::ReallocationSuccessInvalidatesSource,
                    AllocationObligationEvidence::ReallocationResultObligationTransferred,
                    AllocationObligationEvidence::CompatibleDeallocationBarrier,
                    AllocationObligationEvidence::CompatibleResultDeallocationBarrier,
                ],
                origin_node: Some(record.source_existence_predicate_call_node.clone()),
                handoff_node: None,
                return_node: None,
                first_drop_node: None,
                second_drop_node: None,
                use_node: None,
                mismatch_node: None,
                allocator_family: Some("c_malloc".into()),
                deallocator_family: Some("c_malloc".into()),
                contracts: Vec::new(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary: format!(
                    "All producer-certified realloc outcomes close their corresponding obligation for {}: failure preserves and subsequently frees the old object, while success invalidates the old object and every certified result path reaches compatible free(q).",
                    record.source_allocation,
                ),
            });
        }
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
                let witness_entry = origin_path.first().map(String::as_str).unwrap_or(first_drop);
                let explicit_origin_reaches_entry = self
                    .explicit_allocation_origin_reaches(allocation, witness_entry);

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
                    let strength = if explicit_origin_reaches_entry {
                        AllocationObligationFindingStrength::StrongAbstractEvidence
                    } else {
                        AllocationObligationFindingStrength::ObservationalCandidate
                    };
                    let mut evidence = vec![origin_evidence];
                    if explicit_origin_reaches_entry
                        && origin_evidence != AllocationObligationEvidence::MayAllocationEventObserved
                    {
                        // Keep the shortest diagnostic witness entry while also
                        // recording the stronger, independently established
                        // canonical allocation-origin evidence.  W1 certificates
                        // already expose these as distinct concepts.
                        evidence.push(AllocationObligationEvidence::MayAllocationEventObserved);
                    }
                    evidence.extend([
                        AllocationObligationEvidence::MayDeallocationObserved,
                        AllocationObligationEvidence::MayUseObserved,
                        AllocationObligationEvidence::OrderedDropBeforeUse,
                        AllocationObligationEvidence::NoReallocationBetweenEvents,
                    ]);
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
                    if let Some(contract) = self.allocator_contract_witness(allocation) {
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
                        ffi_argument_identity: Vec::new(),
                        external_effects: Vec::new(),
                        non_returning_discharge_nodes: Vec::new(),
                        summary,
                    });
                    break;
                }
            }
        }
        findings
    }

    fn double_free_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        let mut findings = Vec::new();
        for allocation in self.k.allocations.keys() {
            for first_drop in self.k.nodes.keys().filter(|node_id| {
                self.node_has_allocation_event(node_id, allocation, EventKind::Drop)
            }) {
                let Some((origin_path, origin_evidence)) =
                    self.origin_path_to_in_scope(scope, allocation, first_drop)
                else {
                    continue;
                };

                let Some(suffix) =
                    self.repeated_drop_suffix_in_scope(scope, first_drop, allocation)
                else {
                    continue;
                };
                let Some(second_drop) = suffix.last().cloned() else {
                    continue;
                };

                let mut witness_path = origin_path.clone();
                witness_path.extend(suffix);
                let strength = if origin_evidence
                    == AllocationObligationEvidence::MayAllocationEventObserved
                {
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
                if let Some(contract) = self.allocator_contract_witness(allocation) {
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
                if let Some(witness) = self.disposition_witness_before_in_scope(
                    scope,
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
                    ffi_argument_identity: Vec::new(),
                    external_effects: Vec::new(),
                    non_returning_discharge_nodes: Vec::new(),
                    summary,
                });
            }
        }
        findings
    }

    /// Negative diagnostic for the exact allocation-state double-free query on
    /// the typed normal-edge projection.  This never changes CQPL truth.
    ///
    /// A false-oriented finding is emitted only after all represented allocation
    /// candidates and all explicit allocation origins are covered, every possible
    /// first deallocation reachable after an origin has been enumerated, and no
    /// such first deallocation has a normal-edge suffix reaching a second drop
    /// before re-allocation of the same AbstractAllocId.  Site recurrence and
    /// unresolved/escaping deallocation effects fail closed.
    fn double_free_state_refuting_findings(
        &self,
        result: Truth,
        scope: AssessmentScope,
    ) -> Vec<AllocationObligationFinding> {
        if result != Truth::Unknown
            || scope != AssessmentScope::NormalExecution
            || !self.k.capabilities.contains("allocation_state_v1")
            || !self.k.capabilities.contains("typed_edge_flow_v1")
        {
            return Vec::new();
        }

        let reachable = self.reachable_nodes_in_scope(scope);
        let candidates = self
            .k
            .allocation_ids()
            .filter(|allocation| {
                self.k
                    .nodes
                    .keys()
                    .filter(|node_id| reachable.contains(*node_id))
                    .any(|node_id| {
                        self.k.allocation_may_hold(
                            node_id.as_str(),
                            allocation.as_str(),
                            MayPredicate::Alloc,
                        ) != Truth::False
                    })
            })
            .cloned()
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            return Vec::new();
        }

        let mut findings = Vec::new();
        for allocation in &candidates {
            // CR1 inserts a checker-local successful-realloc deallocation into
            // CQPL truth. The producer Kripke intentionally does not contain
            // that synthetic event, so the legacy negative refuter cannot use
            // producer-only drops to certify absence of a repeated drop.
            if self.k.conditional_reallocations_for(allocation).next().is_some() {
                return Vec::new();
            }
            let origins = self
                .k
                .nodes
                .keys()
                .filter(|node_id| reachable.contains(*node_id))
                .filter(|node_id| {
                    self.node_has_allocation_event(
                        node_id.as_str(),
                        allocation.as_str(),
                        EventKind::Alloc,
                    ) && self.k.nodes.get(*node_id).is_some_and(|node| {
                        node.allocation_post.as_ref().is_some_and(|post| {
                            post.value_of(allocation.as_str()) == CellValue::Alloc
                        })
                    })
                })
                .cloned()
                .collect::<Vec<_>>();

            // State-only MAY membership is insufficient for a universal
            // refuting certificate: require producer-observed allocation origins.
            if origins.is_empty() {
                return Vec::new();
            }

            // One AbstractAllocId reused by a normal-edge cycle can summarize
            // multiple concrete instances; a negative certificate must not merge
            // those instances silently.
            if origins
                .iter()
                .any(|origin| self.node_can_recur_in_scope(scope, origin.as_str()))
            {
                return Vec::new();
            }

            let mut first_drops = BTreeSet::new();
            let mut representative_path = Vec::new();
            let mut representative_drop = None;

            for origin in &origins {
                let reachable_after_origin =
                    self.reachable_after_one_step_in_scope(scope, origin.as_str());

                // Unknown external deallocation or an escaping/may-discharge
                // disposition can hide an additional deallocation from this
                // event vocabulary.  Fail closed rather than claim absence.
                if reachable_after_origin.iter().any(|node_id| {
                    self.node_blocks_negative_double_free_orientation(
                        node_id,
                        allocation.as_str(),
                    )
                }) {
                    return Vec::new();
                }

                for node_id in &reachable_after_origin {
                    if self.node_has_allocation_event(
                        node_id,
                        allocation.as_str(),
                        EventKind::Drop,
                    ) {
                        first_drops.insert(node_id.clone());
                    }
                }
            }

            for first_drop in &first_drops {
                if self
                    .repeated_drop_suffix_in_scope(scope, first_drop, allocation.as_str())
                    .is_some()
                {
                    // Positive and negative findings must never be certified for
                    // the same exact normal-execution double-free shape.
                    return Vec::new();
                }

                // The no-reallocation suffix itself must also be free of
                // unresolved deallocation/escape effects.
                if self
                    .reachable_without_reallocation_after_drop(
                        scope,
                        first_drop,
                        allocation.as_str(),
                    )
                    .iter()
                    .any(|node_id| {
                        self.node_blocks_negative_double_free_orientation(
                            node_id,
                            allocation.as_str(),
                        )
                    })
                {
                    return Vec::new();
                }
            }

            if let Some(first_drop) = first_drops.iter().next() {
                for origin in &origins {
                    let Some(path) = self.bfs_to_in_scope(
                        scope,
                        origin.as_str(),
                        |candidate| candidate == first_drop.as_str(),
                        |_| true,
                    ) else {
                        continue;
                    };
                    if path.len() >= 2 {
                        representative_path = path;
                        representative_drop = Some(first_drop.clone());
                        break;
                    }
                }
            }
            if representative_path.is_empty() {
                representative_path = vec![origins[0].clone()];
            }

            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind: AllocationObligationFindingKind::AllCandidateDropSuffixesExcludeRepeatedDrop,
                strength: AllocationObligationFindingStrength::ObservationalCandidate,
                allocation: allocation.clone(),
                query_result: result.as_str(),
                witness_path: representative_path,
                evidence: vec![
                    AllocationObligationEvidence::AllAllocationCandidatesCovered,
                    AllocationObligationEvidence::AllAllocationOriginsCovered,
                    AllocationObligationEvidence::AllFirstDeallocationCandidatesCovered,
                    AllocationObligationEvidence::NoRepeatedDropBeforeReallocation,
                    AllocationObligationEvidence::NoUnresolvedDeallocationEffectOnNormalProjection,
                ],
                origin_node: origins.first().cloned(),
                handoff_node: None,
                return_node: None,
                first_drop_node: representative_drop,
                second_drop_node: None,
                use_node: None,
                mismatch_node: None,
                allocator_family: None,
                deallocator_family: None,
                contracts: Vec::new(),
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary: format!(
                    "For allocation {allocation}, every explicit allocation origin and every first deallocation candidate in the typed normal-edge projection is covered, and no first drop has a normal suffix reaching a second drop before re-allocation of the same AbstractAllocId. This is negative observational evidence only; CQPL truth remains {} because MAY events are not promoted to MUST facts.",
                    result.as_str(),
                ),
            });
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
                    if let Some(contract) = self.allocator_contract_witness(allocation) {
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
                        ffi_argument_identity: Vec::new(),
                        external_effects: Vec::new(),
                        non_returning_discharge_nodes: Vec::new(),
                        summary,
                    });
                }
            }
        }
        // RBF2: realloc's allocptr is a family-constrained consumer.  This is
        // not a deallocation event and therefore must not be represented as a
        // synthetic drop.  A v2 record says only that the call requires the
        // c_malloc family for this source allocation; the source's own
        // allocator contract remains authoritative.
        for boundary in &self.k.reallocation_boundaries {
            if boundary.basis != "rust_foreign_decl_c_realloc_allocptr_family_v2" {
                continue;
            }
            let Some(source) = self.k.allocations.get(&boundary.source_allocation) else {
                continue;
            };
            let allocator_family = source
                .allocator_contract
                .as_ref()
                .map(|contract| contract.family.clone())
                .unwrap_or_else(|| "unknown".into());
            let allocator_known = allocator_family != "unknown";
            if allocator_known && allocator_family == boundary.family {
                continue;
            }
            let Some((witness_path, origin_evidence)) =
                self.origin_path_to(&boundary.source_allocation, &boundary.node)
            else {
                continue;
            };
            let (kind, strength) = if allocator_known {
                (
                    AllocationObligationFindingKind::AllocatorFamilyConsumerMismatch,
                    AllocationObligationFindingStrength::StrongAbstractEvidence,
                )
            } else {
                (
                    AllocationObligationFindingKind::UnresolvedAllocatorContractCandidate,
                    AllocationObligationFindingStrength::ObservationalCandidate,
                )
            };
            let mut evidence = vec![
                origin_evidence,
                AllocationObligationEvidence::KnownRequiredAllocatorFamily,
                AllocationObligationEvidence::ProducerCertifiedAllocatorConsumerContract,
            ];
            if allocator_known {
                evidence.push(AllocationObligationEvidence::KnownAllocatorFamily);
                evidence.push(AllocationObligationEvidence::AllocatorFamiliesDiffer);
            } else {
                evidence.push(AllocationObligationEvidence::AllocatorFamilyUnresolved);
            }
            let mut contracts = Vec::new();
            if let Some(contract) = self.allocator_contract_witness(&boundary.source_allocation) {
                contracts.push(contract);
            }
            let summary = if allocator_known {
                format!(
                    "The abstract model associates allocation {} with allocator family '{}' and reaches producer-certified realloc consumer {} whose allocptr contract requires family '{}'. The families differ, which is strong abstract evidence of allocator-family mismatch at the realloc boundary; no deallocation is invented and CQPL truth remains {} because allocation identity is MAY.",
                    boundary.source_allocation, allocator_family, boundary.node, boundary.family, result.as_str(),
                )
            } else {
                format!(
                    "Producer-certified realloc consumer {} requires allocator family '{}' for allocation {}, but the source allocator family is unresolved. This is an observational candidate only; CQPL truth remains {}.",
                    boundary.node, boundary.family, boundary.source_allocation, result.as_str(),
                )
            };
            findings.push(AllocationObligationFinding {
                taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
                kind,
                strength,
                allocation: boundary.source_allocation.clone(),
                query_result: result.as_str(),
                witness_path,
                evidence,
                origin_node: self.k.allocation_site_node(&boundary.source_allocation).map(str::to_string),
                handoff_node: None,
                return_node: None,
                first_drop_node: None,
                second_drop_node: None,
                use_node: None,
                mismatch_node: Some(boundary.node.clone()),
                allocator_family: Some(allocator_family),
                deallocator_family: None,
                contracts,
                dispositions: Vec::new(),
                ffi_argument_identity: Vec::new(),
                external_effects: Vec::new(),
                non_returning_discharge_nodes: Vec::new(),
                summary,
            });
        }

        findings
    }

    fn allocator_contract_witness(
        &self,
        allocation: &str,
    ) -> Option<AllocationContractWitness> {
        let contract = self.k.allocations.get(allocation)?.allocator_contract.clone()?;
        Some(AllocationContractWitness {
            schema: ALLOCATION_CONTRACT_WITNESS_VERSION,
            role: AllocationContractWitnessRole::AllocatorOrigin,
            provenance: AllocationContractWitnessProvenance::LegacyV1AllocatorSummary,
            // W1 correction: allocator-origin provenance is anchored only at
            // the canonical AllocationSiteId node.  A diagnostic BFS entry is
            // not an allocation origin and must never be serialized as one.
            node: self.k.allocation_site_node(allocation).map(str::to_string),
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
    fn disposition_witness_before_in_scope(
        &self,
        scope: AssessmentScope,
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
                let Some(path) = self.bfs_to_in_scope(
                    scope,
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

    fn origin_path_to_in_scope(
        &self,
        scope: AssessmentScope,
        allocation: &str,
        target: &str,
    ) -> Option<(Vec<String>, AllocationObligationEvidence)> {
        let mut candidates = Vec::new();
        for node_id in self.k.nodes.keys() {
            let Some(evidence) = self.allocation_origin_evidence(node_id, allocation) else {
                continue;
            };
            let Some(path) = self.bfs_to_in_scope(
                scope,
                node_id,
                |candidate| candidate == target,
                |_| true,
            ) else {
                continue;
            };
            candidates.push((path, evidence));
        }
        candidates.sort_by(|a, b| {
            (a.0.len(), &a.0, a.1.as_str()).cmp(&(b.0.len(), &b.0, b.1.as_str()))
        });
        candidates.into_iter().next()
    }

    fn repeated_drop_suffix_in_scope(
        &self,
        scope: AssessmentScope,
        first_drop: &str,
        allocation: &str,
    ) -> Option<Vec<String>> {
        for successor in self.successors_in_scope(scope, first_drop) {
            let Some(path) = self.bfs_to_in_scope(
                scope,
                &successor,
                |candidate| {
                    self.node_has_allocation_event(candidate, allocation, EventKind::Drop)
                },
                |candidate| {
                    self.node_has_allocation_event(candidate, allocation, EventKind::Drop)
                        || !self.node_has_allocation_event(candidate, allocation, EventKind::Alloc)
                },
            ) else {
                continue;
            };
            return Some(path);
        }
        None
    }

    fn reachable_after_one_step_in_scope(
        &self,
        scope: AssessmentScope,
        start: &str,
    ) -> BTreeSet<String> {
        let mut reachable = BTreeSet::new();
        let mut queue = VecDeque::from(self.successors_in_scope(scope, start));
        while let Some(node) = queue.pop_front() {
            if !reachable.insert(node.clone()) {
                continue;
            }
            for successor in self.successors_in_scope(scope, &node) {
                if !reachable.contains(&successor) {
                    queue.push_back(successor);
                }
            }
        }
        reachable
    }

    fn reachable_without_reallocation_after_drop(
        &self,
        scope: AssessmentScope,
        first_drop: &str,
        allocation: &str,
    ) -> BTreeSet<String> {
        let mut reachable = BTreeSet::new();
        let mut queue = VecDeque::from(self.successors_in_scope(scope, first_drop));
        while let Some(node) = queue.pop_front() {
            if self.node_has_allocation_event(&node, allocation, EventKind::Alloc) {
                continue;
            }
            if !reachable.insert(node.clone()) {
                continue;
            }
            for successor in self.successors_in_scope(scope, &node) {
                if !reachable.contains(&successor) {
                    queue.push_back(successor);
                }
            }
        }
        reachable
    }

    fn node_blocks_negative_double_free_orientation(
        &self,
        node_id: &str,
        allocation: &str,
    ) -> bool {
        if self
            .k
            .external_deallocation_effect_at(node_id)
            .is_some_and(|record| {
                record.status == crate::kripke::ExternalDeallocationEffectStatus::Unresolved
            })
        {
            return true;
        }

        let Some(node) = self.k.nodes.get(node_id) else {
            return true;
        };
        node.allocation_disposition.iter().any(|record| {
            if record.allocation != allocation {
                return false;
            }
            match record.obligation_effect {
                AllocationObligationEffect::MayEscapeToCaller => true,
                AllocationObligationEffect::MayDischarge => !self.node_has_allocation_event(
                    node_id,
                    allocation,
                    EventKind::Drop,
                ),
                _ => false,
            }
        })
    }

    fn explicit_allocation_origin_reaches(&self, allocation: &str, target: &str) -> bool {
        self.k.nodes.keys().any(|origin| {
            if !self.node_has_allocation_event(origin, allocation, EventKind::Alloc) {
                return false;
            }
            self.bfs_to(
                origin,
                |candidate| candidate == target,
                |candidate| {
                    candidate == origin.as_str()
                        || candidate == target
                        || !self.node_has_allocation_event(candidate, allocation, EventKind::Alloc)
                },
            )
            .is_some()
        })
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

    fn can_reach_normal_return_in_scope(&self, scope: AssessmentScope, start: &str) -> bool {
        self.bfs_to_in_scope(scope, start, |node| self.is_normal_return_node(node), |_| true)
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

    fn reachable_nodes_in_scope(&self, scope: AssessmentScope) -> BTreeSet<String> {
        let mut reachable = BTreeSet::new();
        let mut queue = VecDeque::from([self.k.entry.clone()]);
        while let Some(node) = queue.pop_front() {
            if !reachable.insert(node.clone()) {
                continue;
            }
            for succ in self.successors_in_scope(scope, &node) {
                if !reachable.contains(&succ) {
                    queue.push_back(succ);
                }
            }
        }
        reachable
    }

    fn successors_in_scope(&self, scope: AssessmentScope, node: &str) -> Vec<String> {
        match scope {
            AssessmentScope::AllExecution => self.successors(node),
            AssessmentScope::NormalExecution => self.k.typed_edges.iter()
                .filter(|edge| edge.source == node && edge.flow == TypedEdgeFlow::Normal)
                .map(|edge| edge.destination.clone())
                .collect(),
        }
    }

    fn bfs_to_in_scope<Goal, Allowed>(
        &self,
        scope: AssessmentScope,
        start: &str,
        goal: Goal,
        allowed: Allowed,
    ) -> Option<Vec<String>>
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
            for succ in self.successors_in_scope(scope, &node) {
                if !parent.contains_key(&succ) && allowed(&succ) {
                    parent.insert(succ.clone(), Some(node.clone()));
                    queue.push_back(succ);
                }
            }
        }
        None
    }

    fn bfs_to_truth<Goal, Allowed>(&self, start: &str, goal: Goal, allowed: Allowed) -> Option<Vec<String>>
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
            for succ in self.truth_successors(&node) {
                if allowed(&succ) && !parent.contains_key(&succ) {
                    parent.insert(succ.clone(), Some(node.clone()));
                    queue.push_back(succ);
                }
            }
        }
        None
    }

    fn maximal_prefix_truth<Allowed>(&self, start: &str, allowed: Allowed) -> Vec<String>
    where
        Allowed: Fn(&str) -> bool,
    {
        let mut path = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = start.to_string();
        while allowed(&current) {
            path.push(current.clone());
            if !seen.insert(current.clone()) { break; }
            let Some(next) = self.truth_successors(&current).into_iter().find(|s| allowed(s)) else { break; };
            current = next;
        }
        path
    }

    fn path_has_mixed_truth_successor_truth(&self, z: &BTreeMap<String, Truth>, path: &[String]) -> bool {
        path.iter().any(|node| {
            let values: BTreeSet<_> = self.truth_successors(node).into_iter()
                .filter_map(|s| z.get(&s).copied())
                .collect();
            values.len() > 1
        })
    }

    /// Original producer successors.  Diagnostics, source-grounded findings,
    /// and typed-edge certificates intentionally stay on this relation.
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

/// Exact Gate L1 scope matcher for the canonical allocation-state leak query:
///
/// `exists_alloc a. EF (alloc(a) && EX EG !drop(a))`
///
/// The conjunction order is intentionally accepted in either direction, but
/// no event-label variant or logically different formula is matched.
fn formula_is_allocation_state_leak_shape(formula: &StateFormula) -> bool {
    fn is_alloc_atom(formula: &StateFormula, logic_var: &str) -> bool {
        matches!(
            formula,
            StateFormula::May {
                predicate: MayPredicate::Alloc,
                logic_var: var,
            } if var == logic_var
        )
    }

    fn is_ex_eg_not_drop(formula: &StateFormula, logic_var: &str) -> bool {
        let StateFormula::Path {
            quantifier: PathQuantifier::Exists,
            formula: PathFormula::Next(next),
        } = formula
        else {
            return false;
        };
        let StateFormula::Path {
            quantifier: PathQuantifier::Exists,
            formula: PathFormula::Globally(globally),
        } = next.as_ref()
        else {
            return false;
        };
        let StateFormula::Not(inner) = globally.as_ref() else {
            return false;
        };
        matches!(
            inner.as_ref(),
            StateFormula::May {
                predicate: MayPredicate::Drop,
                logic_var: var,
            } if var == logic_var
        )
    }

    let StateFormula::ExistsAlloc { logic_var, body } = formula else {
        return false;
    };
    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Eventually(eventually),
    } = body.as_ref()
    else {
        return false;
    };
    let StateFormula::And(left, right) = eventually.as_ref() else {
        return false;
    };

    (is_alloc_atom(left, logic_var) && is_ex_eg_not_drop(right, logic_var))
        || (is_alloc_atom(right, logic_var) && is_ex_eg_not_drop(left, logic_var))
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

fn formula_is_allocation_state_double_free_shape(formula: &StateFormula) -> bool {
    let StateFormula::ExistsAlloc { logic_var, body } = formula else {
        return false;
    };
    let var = logic_var.as_str();

    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Eventually(outer),
    } = body.as_ref()
    else {
        return false;
    };

    let StateFormula::And(alloc_state, after_alloc) = outer.as_ref() else {
        return false;
    };
    if !matches!(
        alloc_state.as_ref(),
        StateFormula::May {
            predicate: MayPredicate::Alloc,
            logic_var,
        } if logic_var.as_str() == var
    ) {
        return false;
    }

    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Next(after_alloc_next),
    } = after_alloc.as_ref()
    else {
        return false;
    };
    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Eventually(first_drop_body),
    } = after_alloc_next.as_ref()
    else {
        return false;
    };

    let StateFormula::And(first_drop, after_first_drop) = first_drop_body.as_ref() else {
        return false;
    };
    if !matches!(
        first_drop.as_ref(),
        StateFormula::Label {
            predicate: LabelPredicate::Drop,
            logic_var,
        } if logic_var.as_str() == var
    ) {
        return false;
    }

    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Next(after_drop_next),
    } = after_first_drop.as_ref()
    else {
        return false;
    };
    let StateFormula::Path {
        quantifier: PathQuantifier::Exists,
        formula: PathFormula::Until(no_realloc, second_drop),
    } = after_drop_next.as_ref()
    else {
        return false;
    };

    let StateFormula::Not(no_realloc_inner) = no_realloc.as_ref() else {
        return false;
    };
    let no_realloc_matches = matches!(
        no_realloc_inner.as_ref(),
        StateFormula::Label {
            predicate: LabelPredicate::Alloc,
            logic_var,
        } if logic_var.as_str() == var
    );
    let second_drop_matches = matches!(
        second_drop.as_ref(),
        StateFormula::Label {
            predicate: LabelPredicate::Drop,
            logic_var,
        } if logic_var.as_str() == var
    );

    no_realloc_matches && second_drop_matches
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
    use crate::ast::{AssessmentScope, PathFormula, PathQuantifier, QueryDocument, StateFormula};
    use crate::kripke::{
        AbstractAllocation, AbstractAllocationCell, AbstractAllocationMemoryAnnotation, AbstractMemoryAnnotation, AllocationContract, AllocationDispositionRecord, AllocationEventLabel, AllocationEventSourceRecord, AnnotatedIcfg, AnnotatedNode,
        Kripke, NodeIdentityAnnotation, NodeSourceProvenance, ParsedSourceSpan, ProgramLanguage, ProgramVariable, TypedEdgeFlow, TypedEdgeRecord,
    };
    use crate::parser::parse_query_document;
    use std::collections::BTreeSet;

    fn one_allocation_graph() -> Kripke {
        Kripke::from_annotated_icfg(AnnotatedIcfg {
            external_negative_evidence: vec![],
            external_return_relations: vec![],
            external_return_call_bindings: vec![],
            external_deallocation_effects: vec![],
            external_formal_memory_effects: vec![],
            schema_version: 2,
            entry: "b0".into(),
            capabilities: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ffi_argument_identity: vec![],
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


    fn minimal_directional_finding() -> AllocationObligationFinding {
        AllocationObligationFinding {
            taxonomy: "allocation_obligation_finding_v1",
            kind: AllocationObligationFindingKind::DropThenUseWithoutReallocation,
            strength: AllocationObligationFindingStrength::ObservationalCandidate,
            allocation: "A".into(),
            query_result: "unk",
            witness_path: vec!["b0".into()],
            evidence: vec![AllocationObligationEvidence::MayDeallocationObserved],
            origin_node: None,
            handoff_node: None,
            return_node: None,
            first_drop_node: Some("b0".into()),
            second_drop_node: None,
            use_node: Some("b0".into()),
            mismatch_node: None,
            allocator_family: None,
            deallocator_family: None,
            contracts: vec![],
            dispositions: vec![],
            ffi_argument_identity: vec![],
            external_effects: vec![],
            non_returning_discharge_nodes: vec![],
            summary: "test directional finding".into(),
        }
    }

    fn minimal_refuting_finding() -> AllocationObligationFinding {
        AllocationObligationFinding {
            taxonomy: MEMORY_ERROR_DIAGNOSTICS_VERSION,
            kind: AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop,
            strength: AllocationObligationFindingStrength::ObservationalCandidate,
            allocation: "A".into(),
            query_result: "unk",
            witness_path: vec!["b0".into(), "b1".into()],
            evidence: vec![
                AllocationObligationEvidence::AllAllocationCandidatesCovered,
                AllocationObligationEvidence::ModeledFreedStateBarrier,
            ],
            origin_node: Some("b0".into()),
            handoff_node: None,
            return_node: None,
            first_drop_node: None,
            second_drop_node: None,
            use_node: None,
            mismatch_node: None,
            allocator_family: None,
            deallocator_family: None,
            contracts: vec![],
            dispositions: vec![],
            ffi_argument_identity: vec![],
            external_effects: vec![],
            non_returning_discharge_nodes: vec![],
            summary: "test refuting finding".into(),
        }
    }

    #[test]
    fn cstring_evidence_wire_name_matches_assessment_basis_token() {
        for evidence in [
            AllocationObligationEvidence::ProducerCertifiedCStringIntoRaw,
            AllocationObligationEvidence::ProducerCertifiedCStringFromRaw,
        ] {
            let serialized = serde_json::to_string(&evidence).unwrap();
            assert_eq!(serialized.trim_matches('"'), evidence.as_str());
        }
    }

    #[test]
    fn assessment_cites_pta_basis_only_for_nonempty_memberships() {
        let k = one_allocation_graph();
        let checker = ModelChecker::new(&k);
        let mut finding = minimal_directional_finding();
        finding.ffi_argument_identity.push(FfiArgumentIdentityRecord {
            node: "dummyCall::x".into(),
            callee: "f".into(),
            callsite: "rust::main::bb0".into(),
            arg_index: 0,
            actual_variable: "rust::x".into(),
            formal_variable: "c::p".into(),
            allocations: vec!["A".into()],
            certainty: "may_abstract".into(),
            basis: "crema_bmulti_actual_formal_identity_v1".into(),
            formal_mapping_basis: "svf_formal_arg_index_v1".into(),
            svf_may_points_to: vec![],
            svf_points_to_basis: Some("svf_andersen_wave_diff_may_v1".into()),
        });

        let empty = checker.assessment_from_findings(Truth::Unknown, &[finding.clone()]);
        assert!(!empty
            .basis
            .iter()
            .any(|b| b == "pta_basis:svf_andersen_wave_diff_may_v1"));

        finding.ffi_argument_identity[0].svf_may_points_to = vec![6];
        let nonempty = checker.assessment_from_findings(Truth::Unknown, &[finding]);
        assert!(nonempty
            .basis
            .iter()
            .any(|b| b == "pta_basis:svf_andersen_wave_diff_may_v1"));
    }

    #[test]
    fn assessment_surfaces_external_effect_corroboration_separately() {
        let k = one_allocation_graph();
        let checker = ModelChecker::new(&k);
        let mut finding = minimal_directional_finding();
        finding.external_effects.push(ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "wrapper".into(),
            status: crate::kripke::ExternalDeallocationEffectStatus::ObservedMayDeallocate,
            basis: "structural_c_free_v1".into(),
            corroborating_bases: vec![
                "llvm16_tli_direct_callee_allockind_deallocation_v1".into(),
            ],
        });

        let assessment = checker.assessment_from_findings(Truth::Unknown, &[finding]);
        assert!(assessment
            .basis
            .iter()
            .any(|b| b == "external_effect_basis:structural_c_free_v1"));
        assert!(assessment.basis.iter().any(|b| {
            b == "external_effect_corroborating_basis:llvm16_tli_direct_callee_allockind_deallocation_v1"
        }));
    }

    #[test]
    fn dual_assessment_maps_negative_only_and_mixed_evidence_without_changing_truth() {
        let k = one_allocation_graph();
        let checker = ModelChecker::new(&k);

        let negative = checker.assessment_from_findings(
            Truth::Unknown,
            &[minimal_refuting_finding()],
        );
        assert_eq!(negative.result, "unk");
        assert_eq!(negative.subresult, QuerySubresult::UnkFalse);
        assert_eq!(negative.direction, QueryEvidenceDirection::False);
        assert_eq!(negative.strength, QueryResultStrength::ObservationalCandidate);

        let mixed = checker.assessment_from_findings(
            Truth::Unknown,
            &[minimal_directional_finding(), minimal_refuting_finding()],
        );
        assert_eq!(mixed.result, "unk");
        assert_eq!(mixed.subresult, QuerySubresult::UnkMixed);
        assert_eq!(mixed.direction, QueryEvidenceDirection::Mixed);
        assert_eq!(mixed.strength, QueryResultStrength::ObservationalCandidate);
    }

    fn canonical_leak_state_document() -> QueryDocument {
        parse_query_document(
            "requires allocation_state_v1;\nexists_alloc a. EF (alloc(a) && EX EG !drop(a))",
        )
        .unwrap()
    }

    fn normal_scoped_leak_state_document() -> QueryDocument {
        parse_query_document(
            "requires allocation_state_v1;\nrequires typed_edge_flow_v1;\nassessment_scope normal_execution;\nexists_alloc a. EF (alloc(a) && EX EG !drop(a))",
        )
        .unwrap()
    }

    fn all_execution_double_free_state_document() -> QueryDocument {
        parse_query_document(
            "requires allocation_state_v1;\nexists_alloc a. EF (alloc(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]))",
        )
        .unwrap()
    }

    fn normal_scoped_double_free_state_document() -> QueryDocument {
        parse_query_document(
            "requires allocation_state_v1;\nrequires typed_edge_flow_v1;\nassessment_scope normal_execution;\nexists_alloc a. EF (alloc(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]))",
        )
        .unwrap()
    }

    fn double_free_unwind_only_graph() -> Kripke {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.capabilities.insert("typed_edge_flow_v1".into());
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.successors = vec!["drop1".into()];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell {
                    allocation: "A".into(),
                    value: CellValue::Alloc,
                }],
            });
        }
        k.nodes.insert(
            "drop1".into(),
            AnnotatedNode {
                id: "drop1".into(),
                successors: vec!["ret".into(), "cleanup_drop2".into()],
                labels: vec![],
                semantic_labels: vec!["term:drop".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.nodes.insert(
            "ret".into(),
            AnnotatedNode {
                id: "ret".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:return".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.nodes.insert(
            "cleanup_drop2".into(),
            AnnotatedNode {
                id: "cleanup_drop2".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:drop".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.typed_edges = vec![
            TypedEdgeRecord {
                source: "b0".into(),
                destination: "drop1".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "drop1".into(),
                destination: "ret".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Drop return".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "drop1".into(),
                destination: "cleanup_drop2".into(),
                flow: TypedEdgeFlow::Unwind,
                label: Some("Drop unwind".into()),
                source_label: None,
                destination_label: None,
            },
        ];
        k
    }

    fn clean_linear_allocation_graph() -> Kripke {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.successors = vec!["b1".into()];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell {
                    allocation: "A".into(),
                    value: CellValue::Alloc,
                }],
            });
        }
        k.nodes.insert(
            "b1".into(),
            AnnotatedNode {
                id: "b1".into(),
                successors: vec!["b2".into()],
                labels: vec![],
                semantic_labels: vec!["term:drop".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.nodes.insert(
            "b2".into(),
            AnnotatedNode {
                id: "b2".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:return".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k
    }

    fn bodyless_c_malloc_leak_graph(intervening_call: bool) -> Kripke {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_state_v1".into());
        k.capabilities.insert("allocation_contracts_v1".into());
        k.capabilities.insert("typed_edge_flow_v1".into());
        k.capabilities.insert("source_provenance_v1".into());
        k.source_provenance.insert(
            "b0".into(),
            NodeSourceProvenance {
                language: "rust".into(),
                anchors: vec![],
                allocation_events: vec![],
            },
        );

        {
            let allocation = k.allocations.get_mut("A").unwrap();
            allocation.site = Some(serde_json::json!({
                "kind": "c_call",
                "node_id": "b0",
                "allocator": "malloc"
            }));
            allocation.allocator_contract = Some(AllocationContract {
                family: "c_malloc".into(),
                operation: "malloc".into(),
                language: "c".into(),
                basis: None,
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            });
        }
        {
            let b0 = k.nodes.get_mut("b0").unwrap();
            b0.semantic_labels = vec!["term:call".into()];
            b0.successors = vec![if intervening_call { "call".into() } else { "ret".into() }];
            b0.allocation_post = Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell {
                    allocation: "A".into(),
                    value: CellValue::Top,
                }],
            });
        }

        if intervening_call {
            k.nodes.insert(
                "call".into(),
                AnnotatedNode {
                    id: "call".into(),
                    successors: vec!["ret".into()],
                    labels: vec![],
                    semantic_labels: vec!["term:call".into()],
                    allocation_labels: vec![],
                    allocation_disposition: vec![],
                    identity: Some(NodeIdentityAnnotation::default()),
                    event_identity: Some(NodeIdentityAnnotation::default()),
                    allocation_post: Some(AbstractAllocationMemoryAnnotation {
                        cells: vec![AbstractAllocationCell {
                            allocation: "A".into(),
                            value: CellValue::Top,
                        }],
                    }),
                    pre: AbstractMemoryAnnotation::default(),
                    post: AbstractMemoryAnnotation::default(),
                },
            );
        }

        k.nodes.insert(
            "ret".into(),
            AnnotatedNode {
                id: "ret".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:return".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Top,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );

        let mut edges = vec![TypedEdgeRecord {
            source: "b0".into(),
            destination: if intervening_call { "call".into() } else { "ret".into() },
            flow: TypedEdgeFlow::Normal,
            label: Some("Call return".into()),
            source_label: None,
            destination_label: None,
        }];
        if intervening_call {
            edges.push(TypedEdgeRecord {
                source: "call".into(),
                destination: "ret".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()),
                source_label: None,
                destination_label: None,
            });
        }
        k.typed_edges = edges;
        k
    }

    #[test]
    fn bodyless_c_malloc_top_leak_orients_unknown_true_without_changing_truth() {
        let k = bodyless_c_malloc_leak_graph(true);
        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);

        let assessment = checker.assess_document(&doc, truth);
        assert_eq!(assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(assessment.strength, QueryResultStrength::ObservationalCandidate);
        assert!(assessment.basis.iter().any(|basis| {
            basis == "finding:normal_return_unreleased_external_allocation"
        }));

        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        let finding = report
            .supporting_findings
            .iter()
            .find(|finding| {
                finding.kind
                    == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
            })
            .expect("bodyless C allocation leak finding");
        assert_eq!(finding.strength, AllocationObligationFindingStrength::ObservationalCandidate);
        assert_eq!(finding.origin_node.as_deref(), Some("b0"));
        assert_eq!(finding.return_node.as_deref(), Some("ret"));
        assert!(finding
            .evidence
            .contains(&AllocationObligationEvidence::MayAllocationEventObserved));
        assert!(!finding
            .evidence
            .contains(&AllocationObligationEvidence::NoInterveningCallAfterAllocation));
    }

    #[test]
    fn bodyless_c_strdup_top_leak_uses_same_malloc_family_obligation_without_alias_claim() {
        let mut k = bodyless_c_malloc_leak_graph(true);
        let allocation = k.allocations.get_mut("A").unwrap();
        allocation.site = Some(serde_json::json!({
            "kind": "c_call",
            "node_id": "b0",
            "allocator": "strdup"
        }));
        allocation.allocator_contract = Some(AllocationContract {
            family: "c_malloc".into(),
            operation: "strdup".into(),
            language: "c".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        });

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::ObservationalCandidate);
        let finding = report
            .supporting_findings
            .iter()
            .find(|finding| {
                finding.kind
                    == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
            })
            .expect("bodyless strdup leak finding");
        assert_eq!(finding.origin_node.as_deref(), Some("b0"));
        assert_eq!(finding.allocator_family.as_deref(), Some("c_malloc"));
        assert!(finding.contracts.iter().any(|w| {
            w.contract.family == "c_malloc" && w.contract.operation == "strdup"
        }));
    }

    #[test]
    fn bodyless_c_realloc_null_top_leak_uses_fresh_malloc_family_obligation_without_old_source() {
        let mut k = bodyless_c_malloc_leak_graph(false);
        let allocation = k.allocations.get_mut("A").unwrap();
        allocation.site = Some(serde_json::json!({
            "kind": "c_call",
            "node_id": "b0",
            "allocator": "realloc"
        }));
        allocation.allocator_contract = Some(AllocationContract {
            family: "c_malloc".into(),
            operation: "realloc".into(),
            language: "c".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        });
        k.reallocation_boundaries.clear();

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::StrongAbstractEvidence);
        let finding = report
            .supporting_findings
            .iter()
            .find(|finding| {
                finding.kind
                    == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
            })
            .expect("bodyless realloc(NULL, n) leak finding");
        assert_eq!(finding.origin_node.as_deref(), Some("b0"));
        assert_eq!(finding.allocator_family.as_deref(), Some("c_malloc"));
        assert!(finding.contracts.iter().any(|w| {
            w.contract.family == "c_malloc" && w.contract.operation == "realloc"
        }));
        assert!(k.reallocation_boundaries.is_empty(),
            "realloc(NULL,n) fresh allocation must not manufacture an old-source boundary");
    }

    #[test]
    fn bodyless_c_malloc_direct_normal_return_is_strong_assessment_evidence_only() {
        let k = bodyless_c_malloc_leak_graph(false);
        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);

        let assessment = checker.assess_document(&doc, truth);
        assert_eq!(assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(assessment.strength, QueryResultStrength::StrongAbstractEvidence);

        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        let finding = report
            .supporting_findings
            .iter()
            .find(|finding| {
                finding.kind
                    == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
            })
            .expect("direct-return C allocation leak finding");
        assert!(finding
            .evidence
            .contains(&AllocationObligationEvidence::NoInterveningCallAfterAllocation));
    }

    fn bodyless_nullable_malloc_with_guarded_discharge_graph() -> Kripke {
        let mut k = bodyless_c_malloc_leak_graph(false);

        k.nodes.get_mut("b0").unwrap().successors = vec!["guard".into()];
        k.nodes.insert(
            "guard".into(),
            AnnotatedNode {
                id: "guard".into(),
                successors: vec!["ret_null".into(), "drop".into()],
                labels: vec![],
                semantic_labels: vec!["term:switch_int".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Top,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.nodes.insert(
            "drop".into(),
            AnnotatedNode {
                id: "drop".into(),
                successors: vec!["ret_freed".into()],
                labels: vec![],
                semantic_labels: vec!["term:call".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        for ret in ["ret_null", "ret_freed"] {
            k.nodes.insert(
                ret.into(),
                AnnotatedNode {
                    id: ret.into(),
                    successors: vec![],
                    labels: vec![],
                    semantic_labels: vec!["term:return".into()],
                    allocation_labels: vec![],
                    allocation_disposition: vec![],
                    identity: Some(NodeIdentityAnnotation::default()),
                    event_identity: Some(NodeIdentityAnnotation::default()),
                    allocation_post: Some(AbstractAllocationMemoryAnnotation {
                        cells: vec![AbstractAllocationCell {
                            allocation: "A".into(),
                            value: if ret == "ret_freed" {
                                CellValue::Freed
                            } else {
                                CellValue::Top
                            },
                        }],
                    }),
                    pre: AbstractMemoryAnnotation::default(),
                    post: AbstractMemoryAnnotation::default(),
                },
            );
        }
        k.nodes.remove("ret");
        k.typed_edges = vec![
            TypedEdgeRecord {
                source: "b0".into(),
                destination: "guard".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "guard".into(),
                destination: "ret_null".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Switch null".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "guard".into(),
                destination: "drop".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Switch non-null".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "drop".into(),
                destination: "ret_freed".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()),
                source_label: None,
                destination_label: None,
            },
        ];
        k
    }

    #[test]
    fn represented_c_malloc_origin_does_not_trigger_bodyless_leak_finding() {
        let mut k = bodyless_c_malloc_leak_graph(false);
        k.source_provenance.get_mut("b0").unwrap().language = "c".into();

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);

        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert!(!report.supporting_findings.iter().any(|finding| {
            finding.kind
                == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
        }));
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
    }

    #[test]
    fn bodyless_nullable_malloc_with_reachable_discharge_does_not_orient_leak_true() {
        let k = bodyless_nullable_malloc_with_guarded_discharge_graph();
        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);

        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert!(!report.supporting_findings.iter().any(|finding| {
            finding.kind
                == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
        }));
        assert_ne!(report.assessment.subresult, QuerySubresult::UnkTrue);
    }

    #[test]
    fn nullable_malloc_existence_guard_plus_compatible_drop_orients_unknown_false() {
        let mut k = bodyless_nullable_malloc_with_guarded_discharge_graph();
        k.nodes.get_mut("drop").unwrap().allocation_labels[0].deallocator_contract =
            Some(AllocationContract {
                family: "c_malloc".into(),
                operation: "free".into(),
                language: "c".into(),
                basis: Some("structural_c_free_v1".into()),
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            });
        k.allocation_existence_guards.push(crate::kripke::AllocationExistenceGuardRecord {
            allocation: "A".into(),
            producer_call_node: "b0".into(),
            producer_call_canonical_node: None,
            predicate_call_node: "b0".into(),
            switch_node: "guard".into(),
            tested_variable: "rust::main::Local(_1)".into(),
            predicate_result_variable: "rust::main::Local(_2)".into(),
            null_successor: "ret_null".into(),
            non_null_successor: "drop".into(),
            callee_def_path: "core::ptr::mut_ptr::<impl *mut u8>::is_null".into(),
            allocation_return_basis: "rust_foreign_decl_c_malloc_contract_v1".into(),
            basis: "rust_raw_pointer_is_null_switch_v1".into(),
        });

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkFalse);
        assert!(report.refuting_findings.iter().any(|finding| {
            finding.kind == AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop
                && finding.evidence.contains(&AllocationObligationEvidence::ProducerCertifiedExistenceGuard)
                && finding.evidence.contains(&AllocationObligationEvidence::CompatibleDeallocationBarrier)
        }));
    }

    #[test]
    fn conditional_reallocation_leak_witness_is_one_real_cfg_path() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("typed_edge_flow_v1".into());
        k.nodes.get_mut("b0").unwrap().successors = vec!["src_switch".into()];

        for (id, successors) in [
            ("src_switch", vec!["realloc"]),
            ("realloc", vec!["outcome_pred"]),
            ("outcome_pred", vec!["outcome_switch"]),
            ("outcome_switch", vec!["failure", "success"]),
            ("failure", vec![]),
            ("success", vec!["free_q"]),
            ("free_q", vec![]),
        ] {
            k.nodes.insert(
                id.into(),
                AnnotatedNode {
                    id: id.into(),
                    successors: successors.into_iter().map(str::to_string).collect(),
                    labels: vec![],
                    semantic_labels: vec![],
                    allocation_labels: vec![],
                    allocation_disposition: vec![],
                    identity: Some(NodeIdentityAnnotation::default()),
                    event_identity: Some(NodeIdentityAnnotation::default()),
                    allocation_post: None,
                    pre: AbstractMemoryAnnotation::default(),
                    post: AbstractMemoryAnnotation::default(),
                },
            );
        }

        k.typed_edges = [
            ("b0", "src_switch"),
            ("src_switch", "realloc"),
            ("realloc", "outcome_pred"),
            ("outcome_pred", "outcome_switch"),
            ("outcome_switch", "failure"),
            ("outcome_switch", "success"),
            ("success", "free_q"),
        ]
        .into_iter()
        .map(|(source, destination)| TypedEdgeRecord {
            source: source.into(),
            destination: destination.into(),
            flow: TypedEdgeFlow::Normal,
            label: Some("test".into()),
            source_label: None,
            destination_label: None,
        })
        .collect();

        let record = crate::kripke::ConditionalReallocationRecord {
            source_allocation: "A".into(),
            reallocation_node: "realloc".into(),
            source_variable: "rust::main::_1".into(),
            result_variable: "rust::main::_3".into(),
            source_existence_predicate_call_node: "b0".into(),
            outcome_predicate_call_node: "outcome_pred".into(),
            outcome_argument_variable: Some("rust::main::_3".into()),
            outcome_predicate_result_variable: "rust::main::_4".into(),
            outcome_switch_node: "outcome_switch".into(),
            failure_successor: "failure".into(),
            success_successor: "success".into(),
            reallocation_callee_def_path: "crate::realloc".into(),
            outcome_callee_def_path: "core::ptr::mut_ptr::<impl *mut u8>::is_null".into(),
            family: "c_malloc".into(),
            operation: "realloc".into(),
            certainty: "may_abstract".into(),
            size_semantics: "positive_nonzero_constant".into(),
            status: "conditional_guarded".into(),
            basis: "rust_foreign_decl_c_realloc_is_null_switch_v1".into(),
            outcome_correlation_basis: Some("direct_cfg_edge_realloc_to_is_null_v1".into()),
            outcome_value_flow_basis: Some("rust_mir_direct_result_operand_v1".into()),
            result_deallocations: vec![],
        };
        let checker = ModelChecker::new(&k);
        let closing = BTreeSet::from(["free_q".to_string()]);
        let path = checker
            .conditional_reallocation_representative_closure_path(
                AssessmentScope::NormalExecution,
                &record,
                &closing,
            )
            .expect("representative CR1 path");

        assert_eq!(
            path,
            vec![
                "b0",
                "src_switch",
                "realloc",
                "outcome_pred",
                "outcome_switch",
                "success",
                "free_q",
            ]
        );
        checker
            .diagnostic_abstract_witness(&path, AssessmentScope::NormalExecution)
            .expect("W1 must accept every adjacent pair as a real CFG edge");
        assert!(!path.contains(&"failure".to_string()));
    }

    #[test]
    fn unresolved_reallocation_boundary_blocks_negative_leak_orientation() {
        let mut k = bodyless_nullable_malloc_with_guarded_discharge_graph();
        k.nodes.get_mut("drop").unwrap().allocation_labels[0].deallocator_contract =
            Some(AllocationContract {
                family: "c_malloc".into(),
                operation: "free".into(),
                language: "c".into(),
                basis: Some("structural_c_free_v1".into()),
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            });
        k.allocation_existence_guards.push(crate::kripke::AllocationExistenceGuardRecord {
            allocation: "A".into(),
            producer_call_node: "b0".into(),
            producer_call_canonical_node: None,
            predicate_call_node: "b0".into(),
            switch_node: "guard".into(),
            tested_variable: "rust::main::Local(_1)".into(),
            predicate_result_variable: "rust::main::Local(_2)".into(),
            null_successor: "ret_null".into(),
            non_null_successor: "realloc".into(),
            callee_def_path: "core::ptr::mut_ptr::<impl *mut u8>::is_null".into(),
            allocation_return_basis: "rust_foreign_decl_c_malloc_contract_v1".into(),
            basis: "rust_raw_pointer_is_null_switch_v1".into(),
        });
        k.nodes.get_mut("guard").unwrap().successors = vec!["ret_null".into(), "realloc".into()];
        k.nodes.insert(
            "realloc".into(),
            AnnotatedNode {
                id: "realloc".into(),
                successors: vec!["drop".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![],
                allocation_disposition: vec![], identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation { cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Top }] }),
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
        );
        k.typed_edges.retain(|edge| !(edge.source == "guard" && edge.destination == "drop"));
        k.typed_edges.push(TypedEdgeRecord {
            source: "guard".into(), destination: "realloc".into(), flow: TypedEdgeFlow::Normal,
            label: Some("Switch non-null".into()), source_label: None, destination_label: None,
        });
        k.typed_edges.push(TypedEdgeRecord {
            source: "realloc".into(), destination: "drop".into(), flow: TypedEdgeFlow::Normal,
            label: Some("Call return".into()), source_label: None, destination_label: None,
        });
        k.reallocation_boundaries.push(crate::kripke::ReallocationBoundaryRecord {
            node: "realloc".into(), source_allocation: "A".into(),
            source_variable: "rust::main::Local(_1)".into(), result_variable: "rust::main::Local(_3)".into(),
            family: "c_malloc".into(), operation: "realloc".into(), certainty: "may_abstract".into(),
            status: "conditional_unmodeled".into(), basis: "rust_foreign_decl_c_realloc_boundary_v1".into(),
        });

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
        assert!(report.refuting_findings.is_empty());
    }

    #[test]
    fn modern_mismatched_drop_with_exact_freed_state_does_not_use_legacy_barrier() {
        let mut k = bodyless_nullable_malloc_with_guarded_discharge_graph();
        // Modern contract-carrying graph: a mismatched deallocator may leave
        // allocation_post=FREED as an abstract effect, but that must not count
        // as a valid negative leak certificate.
        k.nodes.get_mut("drop").unwrap().allocation_labels[0].deallocator_contract =
            Some(AllocationContract {
                family: "rust_global".into(),
                operation: "dealloc".into(),
                language: "rust".into(),
                basis: Some("rust_global_dealloc_v2".into()),
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            });
        k.allocation_existence_guards.push(crate::kripke::AllocationExistenceGuardRecord {
            allocation: "A".into(),
            producer_call_node: "b0".into(),
            producer_call_canonical_node: None,
            predicate_call_node: "b0".into(),
            switch_node: "guard".into(),
            tested_variable: "rust::main::Local(_1)".into(),
            predicate_result_variable: "rust::main::Local(_2)".into(),
            null_successor: "ret_null".into(),
            non_null_successor: "drop".into(),
            callee_def_path: "core::ptr::mut_ptr::<impl *mut u8>::is_null".into(),
            allocation_return_basis: "rust_foreign_decl_c_malloc_contract_v1".into(),
            basis: "rust_raw_pointer_is_null_switch_v1".into(),
        });

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
        assert!(report.refuting_findings.is_empty());
    }

    #[test]
    fn external_c_call_without_malloc_family_alloc_return_contract_does_not_orient_leak() {
        let mut k = bodyless_c_malloc_leak_graph(false);
        k.allocations.get_mut("A").unwrap().allocator_contract.as_mut().unwrap().operation =
            "realloc".into();

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert!(!report.supporting_findings.iter().any(|finding| {
            finding.kind
                == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
        }));
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
    }

    #[test]
    fn external_allocation_leak_orientation_fails_closed_on_site_reuse() {
        let mut k = bodyless_c_malloc_leak_graph(false);
        k.nodes.get_mut("b0").unwrap().successors = vec!["b0".into(), "ret".into()];
        k.typed_edges.push(TypedEdgeRecord {
            source: "b0".into(),
            destination: "b0".into(),
            flow: TypedEdgeFlow::Normal,
            label: Some("Loop".into()),
            source_label: None,
            destination_label: None,
        });

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert!(!report.supporting_findings.iter().any(|finding| {
            finding.kind
                == AllocationObligationFindingKind::NormalReturnUnreleasedExternalAllocation
        }));
    }

    #[test]
    fn l1_clean_linear_state_leak_unknown_is_oriented_false_only() {
        let k = clean_linear_allocation_graph();
        let checker = ModelChecker::new(&k);
        let doc = canonical_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);

        let assessment = checker.assess_document(&doc, truth);
        assert_eq!(assessment.result, "unk");
        assert_eq!(assessment.subresult, QuerySubresult::UnkFalse);
        assert_eq!(assessment.direction, QueryEvidenceDirection::False);
        assert_eq!(assessment.strength, QueryResultStrength::ObservationalCandidate);
        assert!(assessment
            .basis
            .iter()
            .any(|basis| basis == "finding:all_candidate_suffixes_cross_modeled_drop"));

        let report = checker.explain_document(&doc, &Env::new(), 4).unwrap();
        assert_eq!(report.result, "unk");
        assert!(report.supporting_findings.is_empty());
        assert_eq!(report.refuting_findings.len(), 1);
        assert_eq!(
            report.refuting_findings[0].kind,
            AllocationObligationFindingKind::AllCandidateSuffixesCrossModeledDrop,
        );
    }

    #[test]
    fn normal_execution_scope_filters_only_unwind_edges_for_assessment_not_truth() {
        let mut k = clean_linear_allocation_graph();
        k.capabilities.insert("typed_edge_flow_v1".into());
        k.nodes.get_mut("b0").unwrap().successors.push("unwind".into());
        k.nodes.insert(
            "unwind".into(),
            AnnotatedNode {
                id: "unwind".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:unwind_resume".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Top,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.typed_edges = vec![
            TypedEdgeRecord {
                source: "b0".into(), destination: "b1".into(), flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()), source_label: None, destination_label: None,
            },
            TypedEdgeRecord {
                source: "b0".into(), destination: "unwind".into(), flow: TypedEdgeFlow::Unwind,
                label: Some("Call unwind".into()), source_label: None, destination_label: None,
            },
            TypedEdgeRecord {
                source: "b1".into(), destination: "b2".into(), flow: TypedEdgeFlow::Normal,
                label: Some("Drop return".into()), source_label: None, destination_label: None,
            },
        ];

        let checker = ModelChecker::new(&k);
        let all_doc = canonical_leak_state_document();
        let normal_doc = normal_scoped_leak_state_document();
        let all_truth = checker.evaluate_document(&all_doc, &Env::new()).unwrap();
        let normal_truth = checker.evaluate_document(&normal_doc, &Env::new()).unwrap();
        assert_eq!(all_truth, Truth::Unknown);
        assert_eq!(normal_truth, all_truth, "assessment scope must not change CQPL truth");

        let all_assessment = checker.assess_document(&all_doc, all_truth);
        assert_ne!(all_assessment.subresult, QuerySubresult::UnkFalse);

        let normal_assessment = checker.assess_document(&normal_doc, normal_truth);
        assert_eq!(normal_assessment.subresult, QuerySubresult::UnkFalse);
        assert!(normal_assessment.basis.iter().any(|b| b == "assessment_scope:normal_execution"));
        assert!(normal_assessment.caveats.iter().any(|c| c.contains("complete transition relation")));
    }

    #[test]
    fn df_negative_orientation_is_restricted_to_exact_allocation_state_shape() {
        let doc = parse_query_document(
            "requires allocation_state_v1;\nrequires typed_edge_flow_v1;\nassessment_scope normal_execution;\nexists_alloc a. EF (alloc_l(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U drop_l(a)]))",
        )
        .unwrap();
        assert!(!formula_is_allocation_state_double_free_shape(&doc.formula));
    }

    #[test]
    fn df_normal_execution_excludes_unwind_only_second_drop_without_changing_truth() {
        let k = double_free_unwind_only_graph();
        let checker = ModelChecker::new(&k);
        let all_doc = all_execution_double_free_state_document();
        let normal_doc = normal_scoped_double_free_state_document();

        assert!(formula_is_allocation_state_double_free_shape(&normal_doc.formula));
        let all_truth = checker.evaluate_document(&all_doc, &Env::new()).unwrap();
        let normal_truth = checker.evaluate_document(&normal_doc, &Env::new()).unwrap();
        assert_eq!(all_truth, Truth::Unknown);
        assert_eq!(normal_truth, all_truth, "assessment scope must not change CQPL truth");

        let all_assessment = checker.assess_document(&all_doc, all_truth);
        assert_eq!(all_assessment.subresult, QuerySubresult::UnkTrue);

        let normal_report = checker.explain_document(&normal_doc, &Env::new(), 8).unwrap();
        assert_eq!(normal_report.result, "unk");
        assert_eq!(normal_report.assessment.subresult, QuerySubresult::UnkFalse);
        assert_eq!(normal_report.assessment.direction, QueryEvidenceDirection::False);
        assert!(normal_report.supporting_findings.is_empty());
        assert_eq!(normal_report.refuting_findings.len(), 1);
        let finding = &normal_report.refuting_findings[0];
        assert_eq!(
            finding.kind,
            AllocationObligationFindingKind::AllCandidateDropSuffixesExcludeRepeatedDrop,
        );
        assert_eq!(finding.first_drop_node.as_deref(), Some("drop1"));
        assert!(finding.evidence.contains(
            &AllocationObligationEvidence::AllFirstDeallocationCandidatesCovered,
        ));
        assert!(finding.evidence.contains(
            &AllocationObligationEvidence::NoRepeatedDropBeforeReallocation,
        ));
        assert!(normal_report.assessment.basis.iter().any(|basis| {
            basis == "assessment_scope:normal_execution"
        }));
    }

    #[test]
    fn df_normal_execution_keeps_real_normal_second_drop_positive() {
        let mut k = double_free_unwind_only_graph();
        k.nodes.get_mut("drop1").unwrap().successors = vec!["cleanup_drop2".into()];
        k.typed_edges = vec![
            TypedEdgeRecord {
                source: "b0".into(),
                destination: "drop1".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call return".into()),
                source_label: None,
                destination_label: None,
            },
            TypedEdgeRecord {
                source: "drop1".into(),
                destination: "cleanup_drop2".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Drop return".into()),
                source_label: None,
                destination_label: None,
            },
        ];

        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_double_free_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let report = checker.explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert!(report.refuting_findings.is_empty());
        let finding = report
            .supporting_findings
            .iter()
            .find(|finding| {
                finding.kind == AllocationObligationFindingKind::RepeatedDropWithoutReallocation
            })
            .expect("normal second-drop witness");
        assert_eq!(finding.witness_path, vec!["b0", "drop1", "cleanup_drop2"]);
    }

    #[test]
    fn df_all_execution_preserves_one_finding_per_first_drop() {
        let mut k = double_free_unwind_only_graph();
        k.nodes.get_mut("drop1").unwrap().successors = vec!["drop2".into()];
        k.nodes.insert(
            "drop2".into(),
            AnnotatedNode {
                id: "drop2".into(),
                successors: vec!["drop3".into()],
                labels: vec![],
                semantic_labels: vec!["term:drop".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );
        k.nodes.insert(
            "drop3".into(),
            AnnotatedNode {
                id: "drop3".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:drop".into()],
                allocation_labels: vec![AllocationEventLabel {
                    predicate: EventKind::Drop,
                    allocation: "A".into(),
                    certainty: AllocationEventCertainty::MayAbstract,
                    deallocator_contract: None,
                }],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Freed,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );

        let checker = ModelChecker::new(&k);
        let doc = all_execution_double_free_state_document();
        let report = checker.explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        let repeated = report
            .supporting_findings
            .iter()
            .filter(|finding| {
                finding.kind == AllocationObligationFindingKind::RepeatedDropWithoutReallocation
            })
            .collect::<Vec<_>>();
        assert_eq!(repeated.len(), 2, "legacy all-execution diagnostics keep one witness per first-drop candidate");
        assert_eq!(repeated[0].first_drop_node.as_deref(), Some("drop1"));
        assert_eq!(repeated[0].second_drop_node.as_deref(), Some("drop2"));
        assert_eq!(repeated[1].first_drop_node.as_deref(), Some("drop2"));
        assert_eq!(repeated[1].second_drop_node.as_deref(), Some("drop3"));
    }

    #[test]
    fn df_normal_execution_refutation_fails_closed_on_unresolved_deallocation_effect() {
        let mut k = double_free_unwind_only_graph();
        k.external_deallocation_effects.insert(
            "ret".into(),
            ExternalDeallocationEffectRecord {
                node: "ret".into(),
                callee: "opaque".into(),
                status: crate::kripke::ExternalDeallocationEffectStatus::Unresolved,
                basis: "unresolved".into(),
                corroborating_bases: vec![],
            },
        );
        let checker = ModelChecker::new(&k);
        let doc = normal_scoped_double_free_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let assessment = checker.assess_document(&doc, truth);
        assert_eq!(assessment.subresult, QuerySubresult::UnkUnoriented);
        assert!(checker.refuting_findings_for_document(&doc, truth).is_empty());
    }

    #[test]
    fn l1_drop_or_leak_branch_never_orients_unknown_false() {
        let mut k = clean_linear_allocation_graph();
        k.nodes.get_mut("b0").unwrap().successors.push("leak".into());
        k.nodes.insert(
            "leak".into(),
            AnnotatedNode {
                id: "leak".into(),
                successors: vec![],
                labels: vec![],
                semantic_labels: vec!["term:return".into()],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: Some(NodeIdentityAnnotation::default()),
                event_identity: Some(NodeIdentityAnnotation::default()),
                allocation_post: Some(AbstractAllocationMemoryAnnotation {
                    cells: vec![AbstractAllocationCell {
                        allocation: "A".into(),
                        value: CellValue::Alloc,
                    }],
                }),
                pre: AbstractMemoryAnnotation::default(),
                post: AbstractMemoryAnnotation::default(),
            },
        );

        let checker = ModelChecker::new(&k);
        let doc = canonical_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        let assessment = checker.assess_document(&doc, truth);
        assert_ne!(assessment.subresult, QuerySubresult::UnkFalse);
        assert!(checker.refuting_findings_for_document(&doc, truth).is_empty());
    }

    #[test]
    fn l1_allocation_site_reuse_cycle_fails_closed() {
        let mut k = clean_linear_allocation_graph();
        k.nodes.get_mut("b1").unwrap().successors = vec!["b0".into()];

        let checker = ModelChecker::new(&k);
        let doc = canonical_leak_state_document();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert_eq!(truth, Truth::Unknown);
        assert!(checker.refuting_findings_for_document(&doc, truth).is_empty());
    }

    #[test]
    fn l1_event_leak_query_is_out_of_scope() {
        let k = clean_linear_allocation_graph();
        let checker = ModelChecker::new(&k);
        let doc = parse_query_document(
            "exists_alloc a. EF (alloc_l(a) && EX EG !drop_l(a))",
        )
        .unwrap();
        let truth = checker.evaluate_document(&doc, &Env::new()).unwrap();
        assert!(checker.refuting_findings_for_document(&doc, truth).is_empty());
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
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::None);
        assert_eq!(report.assessment.strength, QueryResultStrength::Unresolved);
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
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::StrongAbstractEvidence);
        assert!(report.assessment.basis.iter().any(|b| b == "finding:normal_return_open_manual_obligation"));
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

        let findings = ModelChecker::new(&k).allocation_obligation_findings(Truth::Unknown, AssessmentScope::AllExecution);
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
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::StrongAbstractEvidence);
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
    fn uaf_strength_uses_explicit_origin_even_with_shorter_state_witness_entry() {
        let mut k = one_allocation_graph();
        k.nodes.get_mut("b0").unwrap().successors = vec!["state".into()];
        k.nodes.insert("state".into(), AnnotatedNode {
            id: "state".into(), successors: vec!["drop1".into()], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![], allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
            }),
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("drop1".into(), AnnotatedNode {
            id: "drop1".into(), successors: vec!["use1".into()], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None,
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

        let findings = ModelChecker::new(&k).use_after_free_findings(Truth::Unknown);
        let finding = findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::DropThenUseWithoutReallocation)
            .expect("UAF finding");
        assert_eq!(finding.origin_node.as_deref(), Some("state"));
        assert_eq!(finding.witness_path, vec!["state", "drop1", "use1"]);
        assert_eq!(finding.strength, AllocationObligationFindingStrength::StrongAbstractEvidence);
        assert!(finding.evidence.contains(&AllocationObligationEvidence::AllocationStateIncludesAllocated));
        assert!(finding.evidence.contains(&AllocationObligationEvidence::MayAllocationEventObserved));
    }

    #[test]
    fn w1_certificate_separates_canonical_allocation_site_from_shorter_witness_entry() {
        fn anchor(kind: &str, line: u32, statement_index: Option<usize>) -> SourceAnchor {
            SourceAnchor {
                kind: kind.into(),
                statement_index,
                raw_span: format!("/tmp/project/src/main.rs:{line}:5: {line}:14 (#1)"),
                parsed_span: Some(ParsedSourceSpan {
                    file: "/tmp/project/src/main.rs".into(),
                    start_line: line,
                    start_column: 5,
                    end_line: line,
                    end_column: 14,
                }),
                basis: "rustc_mir_source_info_v1".into(),
            }
        }

        let mut k = one_allocation_graph();
        k.capabilities.insert("source_provenance_v1".into());
        k.allocations.get_mut("A").unwrap().site = Some(serde_json::json!({
            "kind": "rust_call",
            "node_id": "b0",
            "callee": "alloc::boxed::Box::<i32>::new"
        }));
        k.nodes.get_mut("b0").unwrap().successors = vec!["state".into()];
        k.nodes.get_mut("b0").unwrap().allocation_post = Some(AbstractAllocationMemoryAnnotation {
            cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
        });
        k.nodes.insert("state".into(), AnnotatedNode {
            id: "state".into(), successors: vec!["drop1".into()], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![], allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()), event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: Some(AbstractAllocationMemoryAnnotation {
                cells: vec![AbstractAllocationCell { allocation: "A".into(), value: CellValue::Alloc }],
            }),
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        });
        k.nodes.insert("drop1".into(), AnnotatedNode {
            id: "drop1".into(), successors: vec!["use1".into()], labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![AllocationEventLabel {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None,
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

        let alloc_anchor = anchor("mir_terminator", 10, None);
        let state_anchor = anchor("mir_statement", 11, Some(0));
        let drop_anchor = anchor("mir_terminator", 12, None);
        let use_anchor = anchor("mir_statement", 13, Some(0));
        k.source_provenance.insert("b0".into(), NodeSourceProvenance {
            language: "rust".into(),
            anchors: vec![alloc_anchor.clone()],
            allocation_events: vec![AllocationEventSourceRecord {
                predicate: EventKind::Alloc, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                anchors: vec![alloc_anchor.clone()],
            }],
        });
        k.source_provenance.insert("state".into(), NodeSourceProvenance {
            language: "rust".into(), anchors: vec![state_anchor.clone()], allocation_events: vec![],
        });
        k.source_provenance.insert("drop1".into(), NodeSourceProvenance {
            language: "rust".into(),
            anchors: vec![drop_anchor.clone()],
            allocation_events: vec![AllocationEventSourceRecord {
                predicate: EventKind::Drop, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                anchors: vec![drop_anchor.clone()],
            }],
        });
        k.source_provenance.insert("use1".into(), NodeSourceProvenance {
            language: "rust".into(),
            anchors: vec![use_anchor.clone()],
            allocation_events: vec![AllocationEventSourceRecord {
                predicate: EventKind::Read, allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                anchors: vec![use_anchor.clone()],
            }],
        });

        let doc = parse_query_document(
            "exists_alloc a. EF (alloc_l(a) && EX EF (drop_l(a) && EX E[(!alloc_l(a)) U use_l(a)]))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::DropThenUseWithoutReallocation)
            .expect("UAF supporting finding");
        assert_eq!(finding.origin_node.as_deref(), Some("state"), "fixture must expose the legacy witness-entry ambiguity");

        let certificate = report.diagnostic_certificates.iter()
            .find(|c| c.finding_kind == AllocationObligationFindingKind::DropThenUseWithoutReallocation)
            .expect("source-grounded certificate");
        assert_eq!(certificate.schema, QUERY_WITNESS_CERTIFICATE_VERSION);
        assert_eq!(certificate.allocation.node.as_deref(), Some("b0"));
        assert_eq!(certificate.allocation.source_status, SourceGroundingStatus::Grounded);
        assert_eq!(certificate.allocation.source_anchors, vec![alloc_anchor]);
        assert_eq!(certificate.witness_entry.as_ref().map(|e| e.node.as_str()), Some("state"));
        assert!(!certificate.abstract_witness.concrete_execution);
        assert_eq!(certificate.abstract_witness.nodes, vec!["state", "drop1", "use1"]);
        assert!(certificate.events.iter().any(|event| {
            event.role == DiagnosticEventRole::FirstDeallocation
                && event.node == "drop1"
                && event.source_anchors == vec![drop_anchor.clone()]
        }));
        assert!(certificate.events.iter().any(|event| {
            event.role == DiagnosticEventRole::UseAfterDeallocation
                && event.node == "use1"
                && event.source_anchors == vec![use_anchor.clone()]
        }));
        assert!(report.diagnostics.source_provenance_capability_present);
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
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::StrongAbstractEvidence);
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
    fn realloc_family_consumer_mismatch_is_strong_without_inventing_drop() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_contracts_v1".into());
        k.capabilities.insert("reallocation_boundaries_v1".into());
        k.capabilities.insert("reallocation_boundaries_v2".into());
        k.capabilities.insert("source_provenance_v1".into());
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "rust_global".into(),
            operation: "box_new".into(),
            language: "rust".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["realloc".into()];
        k.nodes.insert("realloc".into(), AnnotatedNode {
            id: "realloc".into(),
            successors: vec![],
            labels: vec![],
            semantic_labels: vec!["term:call".into()],
            allocation_labels: vec![],
            allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.reallocation_boundaries.push(crate::kripke::ReallocationBoundaryRecord {
            node: "realloc".into(),
            source_allocation: "A".into(),
            source_variable: "rust::main::Local(_1)".into(),
            result_variable: "rust::main::Local(_2)".into(),
            family: "c_malloc".into(),
            operation: "realloc".into(),
            certainty: "may_abstract".into(),
            status: "conditional_unmodeled".into(),
            basis: "rust_foreign_decl_c_realloc_allocptr_family_v2".into(),
        });

        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF (alloc_l(a) && EX EF allocator_mismatch_l(a))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkTrue);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::True);
        assert_eq!(report.assessment.strength, QueryResultStrength::StrongAbstractEvidence);
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::AllocatorFamilyConsumerMismatch)
            .expect("realloc allocator-family consumer finding");
        assert_eq!(finding.mismatch_node.as_deref(), Some("realloc"));
        assert_eq!(finding.allocator_family.as_deref(), Some("rust_global"));
        assert!(finding.deallocator_family.is_none());
        assert!(finding.evidence.contains(&AllocationObligationEvidence::KnownRequiredAllocatorFamily));
        assert!(finding.evidence.contains(&AllocationObligationEvidence::ProducerCertifiedAllocatorConsumerContract));
        assert!(finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamiliesDiffer));
        assert!(k.nodes["realloc"].allocation_labels.is_empty(), "consumer mismatch must not synthesize a drop");
        let certificate = report.diagnostic_certificates.iter()
            .find(|c| c.finding_kind == AllocationObligationFindingKind::AllocatorFamilyConsumerMismatch)
            .expect("consumer mismatch certificate");
        assert!(certificate.events.iter().any(|event| {
            event.role == DiagnosticEventRole::AllocatorFamilyConsumer && event.node == "realloc"
        }));
    }

    #[test]
    fn realloc_family_consumer_with_unknown_source_family_is_unoriented() {
        let mut k = one_allocation_graph();
        k.capabilities.insert("allocation_contracts_v1".into());
        k.capabilities.insert("reallocation_boundaries_v1".into());
        k.capabilities.insert("reallocation_boundaries_v2".into());
        k.allocations.get_mut("A").unwrap().allocator_contract = Some(AllocationContract {
            family: "unknown".into(),
            operation: "unknown".into(),
            language: "unknown".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        });
        k.nodes.get_mut("b0").unwrap().successors = vec!["realloc".into()];
        k.nodes.insert("realloc".into(), AnnotatedNode {
            id: "realloc".into(),
            successors: vec![],
            labels: vec![],
            semantic_labels: vec!["term:call".into()],
            allocation_labels: vec![],
            allocation_disposition: vec![],
            identity: Some(NodeIdentityAnnotation::default()),
            event_identity: Some(NodeIdentityAnnotation::default()),
            allocation_post: None,
            pre: AbstractMemoryAnnotation::default(),
            post: AbstractMemoryAnnotation::default(),
        });
        k.reallocation_boundaries.push(crate::kripke::ReallocationBoundaryRecord {
            node: "realloc".into(),
            source_allocation: "A".into(),
            source_variable: "rust::main::Local(_1)".into(),
            result_variable: "rust::main::Local(_2)".into(),
            family: "c_malloc".into(),
            operation: "realloc".into(),
            certainty: "may_abstract".into(),
            status: "conditional_unmodeled".into(),
            basis: "rust_foreign_decl_c_realloc_allocptr_family_v2".into(),
        });

        let doc = parse_query_document(
            "requires allocation_contracts_v1; exists_alloc a. EF (alloc_l(a) && EX EF allocator_mismatch_l(a))"
        ).unwrap();
        let report = ModelChecker::new(&k).explain_document(&doc, &Env::new(), 8).unwrap();
        assert_eq!(report.result, "unk");
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::None);
        assert_eq!(report.assessment.strength, QueryResultStrength::Unresolved);
        let finding = report.supporting_findings.iter()
            .find(|f| f.kind == AllocationObligationFindingKind::UnresolvedAllocatorContractCandidate)
            .expect("unresolved realloc family-consumer candidate");
        assert!(finding.evidence.contains(&AllocationObligationEvidence::KnownRequiredAllocatorFamily));
        assert!(finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamilyUnresolved));
        assert!(!finding.evidence.contains(&AllocationObligationEvidence::AllocatorFamiliesDiffer));
        assert!(finding.deallocator_family.is_none());
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
        assert_eq!(report.result, "unk");
        assert_eq!(report.assessment.subresult, QuerySubresult::UnkUnoriented);
        assert_eq!(report.assessment.direction, QueryEvidenceDirection::None);
        assert_eq!(report.assessment.strength, QueryResultStrength::Unresolved);
        assert!(report.assessment.caveats.iter().any(|c| c.contains("non-directional unresolved-contract")));
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
