use crate::ast::{LabelPredicate, MayPredicate, StructuralLabelKind};
use crate::truth::Truth;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CellValue {
    Bottom,
    Boxtimes,
    Alloc,
    Freed,
    Mb,
    Immb,
    Mv,
    Top,
}

impl CellValue {
    /// Exact Phase-5 CellValue order:
    /// BOTTOM <= all; ALLOC <= MB/IMMB/MV; all <= TOP.
    pub fn leq(self, other: Self) -> bool {
        use CellValue::*;
        match (self, other) {
            (x, y) if x == y => true,
            (Bottom, _) => true,
            (_, Top) => true,
            (Alloc, Mb | Immb | Mv) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramLanguage {
    Rust,
    C,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramVariable {
    /// Globally unique cross-language identifier used in CQPL environments.
    pub id: String,
    pub language: ProgramLanguage,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Alloc,
    Drop,
    Read,
    Write,
    Use,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLabel {
    pub predicate: EventKind,
    pub variable: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypedEdgeFlow {
    Normal,
    Unwind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TypedEdgeRecord {
    pub source: String,
    pub destination: String,
    pub flow: TypedEdgeFlow,
    pub label: Option<String>,
    pub source_label: Option<String>,
    pub destination_label: Option<String>,
}

/// W1 diagnostic-only source grounding.  These records are parsed as a side
/// overlay so the historical AnnotatedNode serde boundary stays unchanged.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParsedSourceSpan {
    pub file: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceAnchor {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_index: Option<usize>,
    pub raw_span: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parsed_span: Option<ParsedSourceSpan>,
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationEventSourceRecord {
    pub predicate: EventKind,
    pub allocation: String,
    pub certainty: AllocationEventCertainty,
    #[serde(default)]
    pub anchors: Vec<SourceAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSourceProvenance {
    pub language: String,
    #[serde(default)]
    pub anchors: Vec<SourceAnchor>,
    #[serde(default)]
    pub allocation_events: Vec<AllocationEventSourceRecord>,
}

pub type SourceProvenanceOverlay = BTreeMap<String, NodeSourceProvenance>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationEventCertainty {
    /// Schema-v2 allocation-event facts are derived only from MAY identity
    /// information.  A singleton abstract target is not a concrete MUST fact.
    MayAbstract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationContract {
    pub family: String,
    pub operation: String,
    pub language: String,
    /// allocation_contracts_v2 proof basis for deallocator contracts.
    /// The checker validates this closed vocabulary; it never derives family
    /// from diagnostic provenance strings.
    #[serde(default)]
    pub basis: Option<String>,
    #[serde(default)]
    pub owner_def_path: Option<String>,
    #[serde(default)]
    pub allocator_def_path: Option<String>,
    #[serde(default)]
    pub callee_def_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalDeallocationEffectStatus {
    CertifiedAbsent,
    ObservedMayDeallocate,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDeallocationEffectRecord {
    pub node: String,
    pub callee: String,
    pub status: ExternalDeallocationEffectStatus,
    pub basis: String,
    #[serde(default)]
    pub corroborating_bases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FfiArgumentIdentityRecord {
    pub node: String,
    pub callee: String,
    pub callsite: String,
    pub arg_index: usize,
    pub actual_variable: String,
    pub formal_variable: String,
    #[serde(default)]
    pub allocations: Vec<String>,
    pub certainty: String,
    pub basis: String,
    pub formal_mapping_basis: String,
    #[serde(default)]
    pub svf_may_points_to: Vec<usize>,
    #[serde(default)]
    pub svf_points_to_basis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmMemoryAccessEvidenceV1 {
    pub argmem: String,
    pub inaccessiblemem: String,
    pub other: String,
    #[serde(default)]
    pub encoded: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFormalEffectsEvidenceV1 {
    pub index: usize,
    #[serde(default)] pub pointer_typed: bool,
    #[serde(default)] pub nofree: bool,
    #[serde(default)] pub nocapture: bool,
    #[serde(default)] pub returned: bool,
    #[serde(default)] pub readnone: bool,
    #[serde(default)] pub readonly: bool,
    #[serde(default)] pub writeonly: bool,
    #[serde(default)] pub allocptr: bool,
    #[serde(default)] pub allocalign: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlvmAllocSizeEvidenceV1 {
    pub element_size_arg: usize,
    #[serde(default)]
    pub num_elements_arg: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFunctionEffectsEvidenceV1 {
    #[serde(default)] pub nofree: bool,
    #[serde(default)] pub nosync: bool,
    #[serde(default)] pub willreturn: bool,
    #[serde(default)] pub nobuiltin: bool,
    #[serde(default)] pub optnone: bool,
    #[serde(default)] pub memory_explicit: bool,
    pub memory: LlvmMemoryAccessEvidenceV1,
    #[serde(default)] pub alloc_kind: Vec<String>,
    #[serde(default)] pub alloc_family: Option<String>,
    #[serde(default)] pub return_noalias: bool,
    #[serde(default)] pub alloc_size: Option<LlvmAllocSizeEvidenceV1>,
    #[serde(default)] pub formals: Vec<LlvmFormalEffectsEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFunctionEffectsRecordEvidenceV1 {
    pub name: String,
    pub is_declaration: bool,
    pub origin_explicit: String,
    pub explicit: LlvmFunctionEffectsEvidenceV1,
    #[serde(default)] pub tli_recognized: bool,
    #[serde(default)] pub tli_libfunc: Option<String>,
    pub origin_inferred: String,
    pub tli_inferred: LlvmFunctionEffectsEvidenceV1,
    #[serde(default)] pub tli_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmCallsiteEffectsEvidenceV1 {
    pub caller: String,
    pub ordinal: usize,
    pub direct: bool,
    #[serde(default)] pub callee: Option<String>,
    #[serde(default)] pub callsite_memory_explicit: bool,
    pub effective_memory: LlvmMemoryAccessEvidenceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmEffectsModuleEvidenceV1 {
    pub input: String,
    pub target_triple: String,
    pub input_ir_verified: bool,
    pub tli_clone_verified: bool,
    #[serde(default)] pub functions: Vec<LlvmFunctionEffectsRecordEvidenceV1>,
    #[serde(default)] pub callsites_explicit: Vec<LlvmCallsiteEffectsEvidenceV1>,
    #[serde(default)] pub callsites_tli_inferred: Vec<LlvmCallsiteEffectsEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmMemoryEffectsEvidenceV1 {
    pub schema: String,
    pub llvm_version: String,
    pub explicit_basis: String,
    pub tli_basis: String,
    #[serde(default)] pub modules: Vec<LlvmEffectsModuleEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfFormalPointsToEvidenceV1 {
    pub formal_index: usize,
    pub svf_var_id: usize,
    #[serde(default)] pub points_to: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfFunctionPointsToEvidenceV1 {
    pub function: String,
    #[serde(default)] pub formals: Vec<SvfFormalPointsToEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfSolvedPointsToEvidenceV1 {
    pub schema: String,
    pub analysis: String,
    pub semantics: String,
    pub formal_mapping_schema: String,
    #[serde(default)] pub functions: Vec<SvfFunctionPointsToEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationEventLabel {
    pub predicate: EventKind,
    pub allocation: String,
    pub certainty: AllocationEventCertainty,
    #[serde(default)]
    pub deallocator_contract: Option<AllocationContract>,
}

/// Satellite panic/unwind lifecycle evidence keyed by AbstractAllocId.
///
/// Stage A3.7-v1 exports positive MAY facts only. Capability v2 adds an
/// explicit producer-coverage frontier: matching MAY evidence is `unk`;
/// absence is `ff` only under complete coverage and `unk` when unresolved.
/// `tt` remains unavailable for this MAY-only predicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanicLifecycleRecord {
    pub allocation: String,
    pub certainty: AllocationEventCertainty,
    #[serde(default)]
    pub may_own: bool,
    #[serde(default)]
    pub may_partial_drop: bool,
    #[serde(default)]
    pub may_stale_owner: bool,
    #[serde(default)]
    pub may_committed: bool,
    #[serde(default)]
    pub may_complete: bool,
}

impl PanicLifecycleRecord {
    pub fn may_repeat_drop(&self) -> bool {
        self.may_own && self.may_partial_drop && self.may_stale_owner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanicLifecycleCoverage {
    Complete,
    Unresolved,
}

#[derive(Debug, Clone, Default)]
pub struct PanicLifecycleOverlay {
    records: BTreeMap<String, Vec<PanicLifecycleRecord>>,
    coverage: BTreeMap<String, PanicLifecycleCoverage>,
}

impl std::ops::Deref for PanicLifecycleOverlay {
    type Target = BTreeMap<String, Vec<PanicLifecycleRecord>>;

    fn deref(&self) -> &Self::Target {
        &self.records
    }
}

impl std::ops::DerefMut for PanicLifecycleOverlay {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.records
    }
}

impl<'a> IntoIterator for &'a PanicLifecycleOverlay {
    type Item = (&'a String, &'a Vec<PanicLifecycleRecord>);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Vec<PanicLifecycleRecord>>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}

impl PanicLifecycleOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn coverage_at(&self, node_id: &str) -> Option<PanicLifecycleCoverage> {
        self.coverage.get(node_id).copied()
    }

    pub fn set_coverage(
        &mut self,
        node_id: impl Into<String>,
        coverage: PanicLifecycleCoverage,
    ) -> Option<PanicLifecycleCoverage> {
        self.coverage.insert(node_id.into(), coverage)
    }

    pub fn coverage_is_empty(&self) -> bool {
        self.coverage.is_empty()
    }
}

/// Parse node-local lifecycle records and, for capability v2, the explicit
/// producer coverage frontier without changing the frozen AnnotatedNode serde
/// boundary.
pub fn panic_lifecycle_overlay_from_json(root: &serde_json::Value) -> Result<PanicLifecycleOverlay, String> {
    let mut overlay = PanicLifecycleOverlay::new();
    let Some(nodes) = root.get("nodes").and_then(serde_json::Value::as_array) else {
        return Ok(overlay);
    };
    for (index, node) in nodes.iter().enumerate() {
        let Some(object) = node.as_object() else { continue; };
        let Some(node_id) = object.get("id").and_then(serde_json::Value::as_str) else { continue; };
        if let Some(raw_records) = object.get("panic_lifecycle") {
            let records: Vec<PanicLifecycleRecord> = serde_json::from_value(raw_records.clone())
                .map_err(|err| format!("invalid nodes[{index}].panic_lifecycle: {err}"))?;
            if overlay.insert(node_id.to_string(), records).is_some() {
                return Err(format!("duplicate node id '{node_id}' while parsing panic_lifecycle overlay"));
            }
        }
        if let Some(raw_coverage) = object.get("panic_lifecycle_coverage") {
            let coverage: PanicLifecycleCoverage = serde_json::from_value(raw_coverage.clone())
                .map_err(|err| format!("invalid nodes[{index}].panic_lifecycle_coverage: {err}"))?;
            if overlay.set_coverage(node_id.to_string(), coverage).is_some() {
                return Err(format!("duplicate node id '{node_id}' while parsing panic lifecycle coverage"));
            }
        }
    }
    Ok(overlay)
}

/// Parse the A/R2 typed canonical edge payload without changing the frozen
/// AnnotatedIcfg serde struct used by legacy/unit-test constructors.
pub fn typed_edge_overlay_from_json(
    root: &serde_json::Value,
) -> Result<Option<Vec<TypedEdgeRecord>>, String> {
    let Some(raw_edges) = root.get("typed_edges") else {
        return Ok(None);
    };
    let edges: Vec<TypedEdgeRecord> = serde_json::from_value(raw_edges.clone())
        .map_err(|err| format!("invalid typed_edges payload: {err}"))?;
    Ok(Some(edges))
}

/// Parse W1 node-local source provenance without adding the field to the frozen
/// AnnotatedNode serde type used by legacy constructors/tests.
pub fn source_provenance_overlay_from_json(
    root: &serde_json::Value,
) -> Result<SourceProvenanceOverlay, String> {
    let mut overlay = SourceProvenanceOverlay::new();
    let Some(nodes) = root.get("nodes").and_then(serde_json::Value::as_array) else {
        return Ok(overlay);
    };
    for (index, node) in nodes.iter().enumerate() {
        let Some(object) = node.as_object() else { continue; };
        let Some(node_id) = object.get("id").and_then(serde_json::Value::as_str) else { continue; };
        let Some(raw) = object.get("source_provenance") else { continue; };
        let record: NodeSourceProvenance = serde_json::from_value(raw.clone())
            .map_err(|err| format!("invalid nodes[{index}].source_provenance: {err}"))?;
        if overlay.insert(node_id.to_string(), record).is_some() {
            return Err(format!("duplicate node id '{node_id}' while parsing source provenance"));
        }
    }
    Ok(overlay)
}

fn validate_source_anchor(anchor: &SourceAnchor, where_: &str) -> Result<(), String> {
    if anchor.raw_span.is_empty() {
        return Err(format!("{where_}.raw_span must be non-empty"));
    }
    match (anchor.kind.as_str(), anchor.basis.as_str(), anchor.statement_index) {
        ("mir_statement", "rustc_mir_source_info_v1", Some(_)) => {}
        ("mir_terminator", "rustc_mir_source_info_v1", None) => {}
        ("llvm_node", "svf_llvm_node_source_loc_v1", None) => {}
        _ => {
            return Err(format!(
                "{where_} has unsupported source anchor tuple kind='{}' basis='{}' statement_index={:?}",
                anchor.kind, anchor.basis, anchor.statement_index
            ));
        }
    }
    if let Some(span) = &anchor.parsed_span {
        if span.file.is_empty()
            || span.start_line == 0
            || span.start_column == 0
            || span.end_line == 0
            || span.end_column == 0
            || span.end_line < span.start_line
            || (span.end_line == span.start_line && span.end_column < span.start_column)
        {
            return Err(format!("{where_}.parsed_span is not a valid non-empty source interval"));
        }
        if anchor.basis != "rustc_mir_source_info_v1" {
            return Err(format!("{where_}.parsed_span is currently supported only for rustc MIR anchors"));
        }
    }
    Ok(())
}

fn validate_source_provenance_overlay(
    overlay: &SourceProvenanceOverlay,
    nodes: &BTreeMap<String, AnnotatedNode>,
    allocations: &BTreeMap<String, AbstractAllocation>,
) -> Result<(), String> {
    for node_id in nodes.keys() {
        if !overlay.contains_key(node_id) {
            return Err(format!(
                "artifact declares source_provenance_v1 but node '{node_id}' is missing source_provenance"
            ));
        }
    }
    for (node_id, provenance) in overlay {
        let node = nodes.get(node_id).ok_or_else(|| {
            format!("source_provenance_v1 references unknown node '{node_id}'")
        })?;
        if !matches!(provenance.language.as_str(), "rust" | "c" | "synthetic") {
            return Err(format!(
                "node '{node_id}' source_provenance has unsupported language '{}'",
                provenance.language
            ));
        }
        let mut node_anchors = BTreeSet::new();
        for (index, anchor) in provenance.anchors.iter().enumerate() {
            validate_source_anchor(anchor, &format!("nodes['{node_id}'].source_provenance.anchors[{index}]"))?;
            if !node_anchors.insert(anchor.clone()) {
                return Err(format!("node '{node_id}' source_provenance contains a duplicate anchor"));
            }
        }
        let mut event_keys = BTreeSet::new();
        for (index, record) in provenance.allocation_events.iter().enumerate() {
            if !allocations.contains_key(&record.allocation) {
                return Err(format!(
                    "node '{node_id}' source_provenance allocation_events[{index}] references undeclared allocation '{}'",
                    record.allocation
                ));
            }
            if record.certainty != AllocationEventCertainty::MayAbstract {
                return Err(format!(
                    "node '{node_id}' source_provenance allocation event must remain may_abstract"
                ));
            }
            if !node.allocation_labels.iter().any(|label| {
                label.allocation == record.allocation
                    && label.predicate == record.predicate
                    && label.certainty == record.certainty
            }) {
                return Err(format!(
                    "node '{node_id}' source_provenance event {:?}('{}') has no matching allocation_label",
                    record.predicate, record.allocation
                ));
            }
            let predicate_key = match record.predicate {
                EventKind::Alloc => "alloc",
                EventKind::Drop => "drop",
                EventKind::Read => "read",
                EventKind::Write => "write",
                EventKind::Use => "use",
            };
            if !event_keys.insert((predicate_key, record.allocation.clone())) {
                return Err(format!(
                    "node '{node_id}' source_provenance contains duplicate event provenance for {:?}('{}')",
                    record.predicate, record.allocation
                ));
            }
            let mut seen = BTreeSet::new();
            for (anchor_index, anchor) in record.anchors.iter().enumerate() {
                validate_source_anchor(
                    anchor,
                    &format!(
                        "nodes['{node_id}'].source_provenance.allocation_events[{index}].anchors[{anchor_index}]"
                    ),
                )?;
                if !seen.insert(anchor.clone()) {
                    return Err(format!(
                        "node '{node_id}' source_provenance event contains a duplicate anchor"
                    ));
                }
                if !node_anchors.contains(anchor) {
                    return Err(format!(
                        "node '{node_id}' source_provenance event anchor is not present in the node anchor set"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn expected_typed_edge_flow(label: Option<&str>) -> TypedEdgeFlow {
    match label {
        Some("Call unwind")
        | Some("Drop unwind")
        | Some("Assert unwind")
        | Some("InlineAsm unwind")
        | Some("Rust unwind propagate")
        | Some("Rust drop unwind propagate") => TypedEdgeFlow::Unwind,
        _ => TypedEdgeFlow::Normal,
    }
}

fn validate_typed_edge_relation(
    edges: &[TypedEdgeRecord],
    nodes: &BTreeMap<String, AnnotatedNode>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut projected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in edges {
        if edge.source.is_empty() || edge.destination.is_empty() {
            return Err("typed_edge_flow_v1 contains an empty edge endpoint".into());
        }
        if !nodes.contains_key(&edge.source) || !nodes.contains_key(&edge.destination) {
            return Err(format!(
                "typed_edge_flow_v1 edge '{}' -> '{}' is not closed over the node domain",
                edge.source, edge.destination
            ));
        }
        if !seen.insert(edge.clone()) {
            return Err(format!(
                "typed_edge_flow_v1 contains duplicate canonical edge '{}' -> '{}' ({:?})",
                edge.source, edge.destination, edge.flow
            ));
        }
        let expected = expected_typed_edge_flow(edge.label.as_deref());
        if edge.flow != expected {
            return Err(format!(
                "typed_edge_flow_v1 flow/label mismatch on '{}' -> '{}': label={:?} flow={:?} expected={:?}",
                edge.source, edge.destination, edge.label, edge.flow, expected
            ));
        }
        projected
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
    }

    for (node_id, node) in nodes {
        let legacy: BTreeSet<String> = node.successors.iter().cloned().collect();
        let typed = projected.get(node_id).cloned().unwrap_or_default();
        if legacy != typed {
            return Err(format!(
                "typed_edge_flow_v1 projection mismatch at '{node_id}': successors={legacy:?} typed={typed:?}"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationDispositionKind {
    BoxIntoRaw,
    BoxFromRaw,
    BoxLeak,
    // Pin the versioned wire vocabulary explicitly. Serde supports per-variant
    // rename attributes (<https://serde.rs/variant-attrs.html>); do not let
    // Rust identifier case-conversion define an artifact protocol.
    #[serde(rename = "cstring_into_raw")]
    CStringIntoRaw,
    #[serde(rename = "cstring_from_raw")]
    CStringFromRaw,
    MemForgetOwnedBox,
    RawPointerDropNoop,
    ReturnEscape,
    MayDeallocate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationObligationEffect {
    PreserveManualObligation,
    RestoreRaiiObligation,
    PreservePersistentObligation,
    PreserveUnreclaimedObligation,
    NoPointeeLifecycleEffect,
    MayEscapeToCaller,
    MayDischarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocationDispositionRecord {
    pub allocation: String,
    pub kind: AllocationDispositionKind,
    pub certainty: AllocationEventCertainty,
    pub obligation_effect: AllocationObligationEffect,
    pub basis: String,
    #[serde(default)]
    pub source_variable: Option<String>,
    #[serde(default)]
    pub target_variable: Option<String>,
    #[serde(default)]
    pub callee_def_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstractAllocation {
    pub id: String,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub site: Option<serde_json::Value>,
    #[serde(default)]
    pub context: Vec<String>,
    /// Present when `allocation_contracts_v1` is declared by the artifact.
    #[serde(default)]
    pub allocator_contract: Option<AllocationContract>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityPointsToRecord {
    pub variable: String,
    #[serde(default)]
    pub allocations: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentityAnnotation {
    #[serde(default)]
    pub points_to: Vec<IdentityPointsToRecord>,
    // Stack-reference records are exported for auditability by CREMA v2 but
    // are not needed by the checker after allocation_labels are materialized.
    #[serde(default)]
    pub stack_refs: Vec<serde_json::Value>,
}

/// One implementation-level AbstractMemory component. All variables in
/// `aliases` denote the same abstract allocation at this program point and
/// therefore share one CellValue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractCell {
    pub aliases: Vec<String>,
    pub value: CellValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractMemoryAnnotation {
    #[serde(default)]
    pub cells: Vec<AbstractCell>,
}

impl AbstractMemoryAnnotation {
    pub fn value_of(&self, var: &str) -> CellValue {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.value)
            .unwrap_or(CellValue::Bottom)
    }

    pub fn aliases_of(&self, var: &str) -> BTreeSet<String> {
        self.cells
            .iter()
            .find(|cell| cell.aliases.iter().any(|v| v == var))
            .map(|cell| cell.aliases.iter().cloned().collect())
            .unwrap_or_else(|| BTreeSet::from([var.to_string()]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractAllocationCell {
    pub allocation: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbstractAllocationMemoryAnnotation {
    #[serde(default)]
    pub cells: Vec<AbstractAllocationCell>,
}

impl AbstractAllocationMemoryAnnotation {
    pub fn value_of(&self, allocation: &str) -> CellValue {
        self.cells
            .iter()
            .find(|cell| cell.allocation == allocation)
            .map(|cell| cell.value)
            .unwrap_or(CellValue::Bottom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedNode {
    pub id: String,
    #[serde(default)]
    pub successors: Vec<String>,
    #[serde(default)]
    pub labels: Vec<EventLabel>,
    /// v6P block-level structural MIR labels (stmt:/rvalue:/term:).
    #[serde(default)]
    pub semantic_labels: Vec<String>,
    #[serde(default)]
    pub allocation_labels: Vec<AllocationEventLabel>,
    #[serde(default)]
    pub allocation_disposition: Vec<AllocationDispositionRecord>,
    #[serde(default)]
    pub identity: Option<NodeIdentityAnnotation>,
    #[serde(default)]
    pub event_identity: Option<NodeIdentityAnnotation>,
    #[serde(default)]
    pub allocation_post: Option<AbstractAllocationMemoryAnnotation>,
    #[serde(default)]
    pub pre: AbstractMemoryAnnotation,
    #[serde(default)]
    pub post: AbstractMemoryAnnotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedIcfg {
    pub schema_version: u32,
    pub entry: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Legacy/program-variable quantifier domain. It intentionally includes Rust and C variables.
    pub variables: Vec<ProgramVariable>,
    /// Canonical abstract-allocation quantifier domain introduced by schema v2.
    #[serde(default)]
    pub allocations: Vec<AbstractAllocation>,
    /// Bcontract-ND1 external-call deallocation-effect evidence. Absence is
    /// never interpreted as a negative certificate.
    #[serde(default)]
    pub external_deallocation_effects: Vec<ExternalDeallocationEffectRecord>,
    #[serde(default)]
    pub llvm_memory_effects: Option<LlvmMemoryEffectsEvidenceV1>,
    #[serde(default)]
    pub svf_solved_points_to: Option<SvfSolvedPointsToEvidenceV1>,
    #[serde(default)]
    pub ffi_argument_identity: Vec<FfiArgumentIdentityRecord>,
    pub nodes: Vec<AnnotatedNode>,
}

#[derive(Debug, Clone)]
pub struct Kripke {
    pub schema_version: u32,
    pub entry: String,
    pub capabilities: BTreeSet<String>,
    pub variables: BTreeMap<String, ProgramVariable>,
    pub allocations: BTreeMap<String, AbstractAllocation>,
    pub nodes: BTreeMap<String, AnnotatedNode>,
    /// W1 read-only source provenance, capability-gated and never consulted by
    /// the temporal semantics or truth evaluation.
    pub source_provenance: SourceProvenanceOverlay,
    /// A/R2 exact typed copy of the canonical CREMA edge relation. CQPL
    /// temporal semantics still traverse `AnnotatedNode::successors` in R2.
    pub typed_edges: Vec<TypedEdgeRecord>,
    pub external_deallocation_effects: BTreeMap<String, ExternalDeallocationEffectRecord>,
    pub ffi_argument_identity: BTreeMap<(String, usize), FfiArgumentIdentityRecord>,
    pub llvm_memory_effects: Option<LlvmMemoryEffectsEvidenceV1>,
    pub svf_solved_points_to: Option<SvfSolvedPointsToEvidenceV1>,
    pub panic_lifecycle: PanicLifecycleOverlay,
}

fn rust_function_scope(node_id: &str) -> Option<&str> {
    if let Some((scope, bb)) = node_id.rsplit_once("::bb") {
        if scope.starts_with("rust::") && !bb.is_empty() && bb.chars().all(|c| c.is_ascii_digit()) {
            return Some(scope);
        }
    }
    // v6K.4 explicit unwind-terminal nodes are part of the same Rust
    // function's intra projection and represent a maximal path endpoint.
    if let Some(scope) = node_id.strip_suffix("::terminate") {
        if scope.starts_with("rust::") {
            return Some(scope);
        }
    }
    None
}

fn valid_structural_label(label: &str) -> bool {
    let Some((kind, name)) = label.split_once(':') else { return false; };
    if !matches!(kind, "stmt" | "rvalue" | "term") || name.is_empty() {
        return false;
    }
    name.chars().all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn validate_allocation_disposition_record(
    record: &AllocationDispositionRecord,
    allow_cstring_v2: bool,
) -> Result<(), String> {
    use AllocationDispositionKind as K;
    use AllocationObligationEffect as E;

    let expected = match record.kind {
        K::BoxIntoRaw => (E::PreserveManualObligation, "rustc_box_into_raw_v1", true),
        K::BoxFromRaw => (E::RestoreRaiiObligation, "rustc_box_from_raw_v1", true),
        K::BoxLeak => (E::PreservePersistentObligation, "rustc_box_leak_v1", true),
        K::CStringIntoRaw if allow_cstring_v2 => {
            (E::PreserveManualObligation, "rustc_cstring_into_raw_v1", true)
        }
        K::CStringFromRaw if allow_cstring_v2 => {
            (E::RestoreRaiiObligation, "rustc_cstring_from_raw_v1", true)
        }
        K::CStringIntoRaw | K::CStringFromRaw => {
            return Err(format!(
                "allocation disposition {:?} requires artifact capability allocation_disposition_v2",
                record.kind
            ));
        }
        K::MemForgetOwnedBox => (E::PreserveUnreclaimedObligation, "rustc_mem_forget_owned_box_v1", true),
        K::RawPointerDropNoop => (E::NoPointeeLifecycleEffect, "rustc_mem_drop_raw_pointer_v1", true),
        K::ReturnEscape => (E::MayEscapeToCaller, "rust_return_identity_v1", false),
        K::MayDeallocate => (E::MayDischarge, "allocation_drop_label_v1", false),
    };
    if record.obligation_effect != expected.0 || record.basis != expected.1 {
        return Err(format!(
            "allocation_disposition_v1 invalid tuple for {:?}: effect={:?} basis={}",
            record.kind, record.obligation_effect, record.basis
        ));
    }
    if expected.2 && record.callee_def_path.as_deref().map_or(true, str::is_empty) {
        return Err(format!(
            "allocation_disposition_v1 {:?} requires producer callee_def_path provenance",
            record.kind
        ));
    }
    if !expected.2 && record.callee_def_path.is_some() {
        return Err(format!(
            "allocation_disposition_v1 {:?} must not invent call provenance",
            record.kind
        ));
    }
    Ok(())
}

fn efx1_valid_access(access: &str) -> bool {
    matches!(access, "none" | "read" | "write" | "readwrite")
}

fn validate_embedded_memory(memory: &LlvmMemoryAccessEvidenceV1, where_: &str) -> Result<(), String> {
    if memory.encoded > 63 {
        return Err(format!("{where_}: invalid LLVM16 MemoryEffects encoding {}", memory.encoded));
    }
    for (location, access) in [
        ("argmem", memory.argmem.as_str()),
        ("inaccessiblemem", memory.inaccessiblemem.as_str()),
        ("other", memory.other.as_str()),
    ] {
        if !efx1_valid_access(access) {
            return Err(format!("{where_}: invalid {location} memory access '{access}'"));
        }
    }
    Ok(())
}

fn validate_embedded_snapshot(snapshot: &LlvmFunctionEffectsEvidenceV1, where_: &str) -> Result<(), String> {
    validate_embedded_memory(&snapshot.memory, &format!("{where_}.memory"))?;
    let valid_alloc_kinds = ["alloc", "realloc", "free", "uninitialized", "zeroed", "aligned"];
    let mut kinds = BTreeSet::new();
    for kind in &snapshot.alloc_kind {
        if !valid_alloc_kinds.contains(&kind.as_str()) || !kinds.insert(kind.as_str()) {
            return Err(format!("{where_}: invalid/duplicate alloc kind '{kind}'"));
        }
    }
    let mut returned = 0usize;
    for (expected, formal) in snapshot.formals.iter().enumerate() {
        if formal.index != expected {
            return Err(format!("{where_}: formal index {} != declaration position {expected}", formal.index));
        }
        if formal.returned { returned += 1; }
        if (formal.nofree || formal.nocapture || formal.readnone || formal.readonly || formal.writeonly || formal.allocptr)
            && !formal.pointer_typed
        {
            return Err(format!("{where_}: pointer effect attached to non-pointer formal {expected}"));
        }
    }
    if returned > 1 {
        return Err(format!("{where_}: returned appears on more than one formal"));
    }
    if let Some(alloc_size) = snapshot.alloc_size.as_ref() {
        if alloc_size.element_size_arg >= snapshot.formals.len() {
            return Err(format!("{where_}: alloc_size element_size_arg out of range"));
        }
        if alloc_size.num_elements_arg.is_some_and(|index| index >= snapshot.formals.len()) {
            return Err(format!("{where_}: alloc_size num_elements_arg out of range"));
        }
    }
    Ok(())
}

fn validate_embedded_llvm_effects(evidence: &LlvmMemoryEffectsEvidenceV1) -> Result<(), String> {
    if evidence.schema != "llvm_memory_effects_v1"
        || evidence.llvm_version != "16.0.4"
        || evidence.explicit_basis != "llvm16_explicit_input_ir_v1"
        || evidence.tli_basis != "llvm16_tli_libfunc_attrs_v1"
    {
        return Err("invalid embedded llvm_memory_effects_v1 header/provenance".into());
    }
    if evidence.modules.is_empty() {
        return Err("embedded llvm_memory_effects_v1 contains no modules".into());
    }
    for (mi, module) in evidence.modules.iter().enumerate() {
        if module.input.is_empty() || !module.input_ir_verified || !module.tli_clone_verified {
            return Err(format!("llvm_memory_effects modules[{mi}] missing input or LLVM verifier certificate"));
        }
        let mut names = BTreeSet::new();
        for (fi, function) in module.functions.iter().enumerate() {
            let where_ = format!("llvm_memory_effects modules[{mi}].functions[{fi}]({})", function.name);
            if function.name.is_empty() || !names.insert(function.name.as_str()) {
                return Err(format!("{where_}: missing/duplicate function"));
            }
            if function.origin_explicit != "explicit_input_ir" || function.origin_inferred != "llvm_tli_inferred" {
                return Err(format!("{where_}: invalid origin"));
            }
            if function.tli_recognized != function.tli_libfunc.as_ref().is_some_and(|name| !name.is_empty()) {
                return Err(format!("{where_}: tli_recognized/tli_libfunc mismatch"));
            }
            validate_embedded_snapshot(&function.explicit, &format!("{where_}.explicit"))?;
            validate_embedded_snapshot(&function.tli_inferred, &format!("{where_}.tli_inferred"))?;
            let delta = function.explicit != function.tli_inferred;
            if delta != function.tli_changed {
                return Err(format!("{where_}: tli_changed disagrees with structural delta"));
            }
            if function.tli_changed
                && (!function.tli_recognized || !function.is_declaration || function.explicit.nobuiltin || function.explicit.optnone)
            {
                return Err(format!("{where_}: inadmissible TLI mutation"));
            }
        }
        if module.callsites_explicit.len() != module.callsites_tli_inferred.len() {
            return Err(format!("llvm_memory_effects modules[{mi}]: TLI clone changed callsite cardinality"));
        }
        for (ci, (before, after)) in module.callsites_explicit.iter().zip(&module.callsites_tli_inferred).enumerate() {
            if before.caller != after.caller || before.ordinal != after.ordinal || before.direct != after.direct || before.callee != after.callee {
                return Err(format!("llvm_memory_effects modules[{mi}].callsites[{ci}]: identity changed under TLI clone"));
            }
            validate_embedded_memory(&before.effective_memory, &format!("llvm_memory_effects modules[{mi}].callsites[{ci}].explicit"))?;
            validate_embedded_memory(&after.effective_memory, &format!("llvm_memory_effects modules[{mi}].callsites[{ci}].inferred"))?;
        }
    }
    Ok(())
}

fn validate_embedded_svf_pts(evidence: &SvfSolvedPointsToEvidenceV1) -> Result<(), String> {
    if evidence.schema != "svf_solved_points_to_v1"
        || evidence.analysis != "AndersenWaveDiff"
        || evidence.semantics != "may"
        || evidence.formal_mapping_schema != "svf_formal_arg_index_v1"
    {
        return Err("invalid embedded svf_solved_points_to_v1 header".into());
    }
    let mut functions = BTreeSet::new();
    for function in &evidence.functions {
        if function.function.is_empty() || !functions.insert(function.function.as_str()) {
            return Err("embedded svf_solved_points_to_v1 has missing/duplicate function".into());
        }
        let mut vars = BTreeSet::new();
        for (expected, formal) in function.formals.iter().enumerate() {
            if formal.formal_index != expected {
                return Err(format!("{}: non-contiguous formal index", function.function));
            }
            if !vars.insert(formal.svf_var_id) {
                return Err(format!("{}: duplicate formal SVF VarID", function.function));
            }
            if !formal.points_to.windows(2).all(|w| w[0] < w[1]) {
                return Err(format!("{} formal {expected}: points-to set is not sorted unique", function.function));
            }
        }
    }
    Ok(())
}

impl Kripke {
    pub fn from_annotated_icfg(input: AnnotatedIcfg) -> Result<Self, String> {
        Self::from_annotated_icfg_with_overlays(
            input,
            PanicLifecycleOverlay::new(),
            None,
        )
    }

    pub fn from_annotated_icfg_with_panic_lifecycle(
        input: AnnotatedIcfg,
        panic_lifecycle: PanicLifecycleOverlay,
    ) -> Result<Self, String> {
        Self::from_annotated_icfg_with_overlays(input, panic_lifecycle, None)
    }

    pub fn from_annotated_icfg_with_overlays(
        input: AnnotatedIcfg,
        panic_lifecycle: PanicLifecycleOverlay,
        typed_edges: Option<Vec<TypedEdgeRecord>>,
    ) -> Result<Self, String> {
        Self::from_annotated_icfg_with_all_overlays(
            input,
            panic_lifecycle,
            typed_edges,
            SourceProvenanceOverlay::new(),
        )
    }

    pub fn from_annotated_icfg_with_all_overlays(
        input: AnnotatedIcfg,
        panic_lifecycle: PanicLifecycleOverlay,
        typed_edges: Option<Vec<TypedEdgeRecord>>,
        source_provenance: SourceProvenanceOverlay,
    ) -> Result<Self, String> {
        if !matches!(input.schema_version, 1 | 2) {
            return Err(format!(
                "unsupported annotated ICFG schema_version {}; expected 1 or 2",
                input.schema_version
            ));
        }
        let schema_version = input.schema_version;
        let llvm_memory_effects = input.llvm_memory_effects.clone();
        let svf_solved_points_to = input.svf_solved_points_to.clone();
        let capabilities: BTreeSet<String> = input.capabilities.iter().cloned().collect();
        let has_allocation_contracts = capabilities.contains("allocation_contracts_v1");
        let has_allocation_contracts_v2 = capabilities.contains("allocation_contracts_v2");
        let has_allocation_contracts_v3 = capabilities.contains("allocation_contracts_v3");
        let has_allocation_state = capabilities.contains("allocation_state_v1");
        let has_allocation_disposition = capabilities.contains("allocation_disposition_v1");
        let has_allocation_disposition_v2 = capabilities.contains("allocation_disposition_v2");
        let has_external_deallocation_effects = capabilities.contains("external_deallocation_effects_v1");
        let has_llvm_memory_effects = capabilities.contains("llvm_memory_effects_v1");
        let has_svf_solved_points_to = capabilities.contains("svf_solved_points_to_v1");
        let has_ffi_argument_identity = capabilities.contains("ffi_argument_identity_v1");
        let has_typed_edge_flow = capabilities.contains("typed_edge_flow_v1");
        let has_source_provenance = capabilities.contains("source_provenance_v1");
        if has_typed_edge_flow != typed_edges.is_some() {
            return Err("typed_edge_flow_v1 capability and typed_edges payload must appear together".into());
        }
        if has_typed_edge_flow && schema_version != 2 {
            return Err("typed_edge_flow_v1 requires annotated ICFG schema v2".into());
        }
        if has_source_provenance && schema_version != 2 {
            return Err("source_provenance_v1 requires annotated ICFG schema v2".into());
        }
        if has_source_provenance != !source_provenance.is_empty() {
            return Err("source_provenance_v1 capability and node provenance overlay must appear together".into());
        }
        if has_ffi_argument_identity != !input.ffi_argument_identity.is_empty() {
            return Err("ffi_argument_identity_v1 capability and evidence records must appear together".into());
        }
        if has_llvm_memory_effects != input.llvm_memory_effects.is_some() {
            return Err("llvm_memory_effects_v1 capability and embedded evidence must appear together".into());
        }
        if has_svf_solved_points_to != input.svf_solved_points_to.is_some() {
            return Err("svf_solved_points_to_v1 capability and embedded evidence must appear together".into());
        }
        if let Some(evidence) = input.llvm_memory_effects.as_ref() {
            validate_embedded_llvm_effects(evidence)?;
        }
        if let Some(evidence) = input.svf_solved_points_to.as_ref() {
            validate_embedded_svf_pts(evidence)?;
        }
        if has_allocation_contracts && schema_version != 2 {
            return Err("allocation_contracts_v1 requires annotated ICFG schema v2".into());
        }
        if has_allocation_contracts_v2 && schema_version != 2 {
            return Err("allocation_contracts_v2 requires annotated ICFG schema v2".into());
        }
        if has_allocation_contracts_v2 && !has_allocation_contracts {
            return Err("allocation_contracts_v2 is a refinement of allocation_contracts_v1 and requires both artifact capabilities".into());
        }
        if has_allocation_contracts_v3 && schema_version != 2 {
            return Err("allocation_contracts_v3 requires annotated ICFG schema v2".into());
        }
        if has_allocation_contracts_v3 && (!has_allocation_contracts || !has_allocation_contracts_v2) {
            return Err("allocation_contracts_v3 refines allocation_contracts_v2 and requires allocation_contracts_v1 + allocation_contracts_v2 + allocation_contracts_v3".into());
        }
        if has_allocation_state && schema_version != 2 {
            return Err("allocation_state_v1 requires annotated ICFG schema v2".into());
        }
        if has_allocation_disposition && schema_version != 2 {
            return Err("allocation_disposition_v1 requires annotated ICFG schema v2".into());
        }
        if has_allocation_disposition_v2 && schema_version != 2 {
            return Err("allocation_disposition_v2 requires annotated ICFG schema v2".into());
        }
        if has_allocation_disposition_v2 && !has_allocation_disposition {
            return Err(
                "allocation_disposition_v2 refines allocation_disposition_v1 and requires both artifact capabilities"
                    .into(),
            );
        }
        if has_external_deallocation_effects && schema_version != 2 {
            return Err("external_deallocation_effects_v1 requires annotated ICFG schema v2".into());
        }
        if has_llvm_memory_effects && schema_version != 2 {
            return Err("llvm_memory_effects_v1 requires annotated ICFG schema v2".into());
        }
        if has_svf_solved_points_to && schema_version != 2 {
            return Err("svf_solved_points_to_v1 requires annotated ICFG schema v2".into());
        }
        if has_ffi_argument_identity && schema_version != 2 {
            return Err("ffi_argument_identity_v1 requires annotated ICFG schema v2".into());
        }
        let has_mir_semantic_labels = capabilities.contains("mir_semantic_labels_v1");
        if has_mir_semantic_labels && schema_version != 2 {
            return Err("mir_semantic_labels_v1 requires annotated ICFG schema v2".into());
        }
        let has_mir_semantics_v2 = capabilities.contains("mir_semantics_v2");
        if has_mir_semantics_v2 && !has_mir_semantic_labels {
            return Err("mir_semantics_v2 requires mir_semantic_labels_v1 so the active transfer profile is auditable".into());
        }
        if capabilities.contains("panic_unwind_lifecycle_v1")
            && (schema_version != 2 || !has_mir_semantics_v2)
        {
            return Err("panic_unwind_lifecycle_v1 requires schema v2 and mir_semantics_v2".into());
        }
        let has_panic_lifecycle_state = capabilities.contains("panic_lifecycle_state_v1");
        let has_panic_lifecycle_state_v2 = capabilities.contains("panic_lifecycle_state_v2");
        if has_panic_lifecycle_state
            && (schema_version != 2 || !capabilities.contains("panic_unwind_lifecycle_v1"))
        {
            return Err("panic_lifecycle_state_v1 requires schema v2 and panic_unwind_lifecycle_v1".into());
        }
        if has_panic_lifecycle_state_v2 && !has_panic_lifecycle_state {
            return Err("panic_lifecycle_state_v2 refines panic_lifecycle_state_v1; the artifact must declare both capabilities".into());
        }

        let mut variables = BTreeMap::new();
        for v in input.variables {
            if variables.insert(v.id.clone(), v).is_some() {
                return Err("duplicate program-variable id in annotated ICFG".into());
            }
        }
        if variables.is_empty() {
            return Err("annotated ICFG contains no program variables".into());
        }

        let mut allocations = BTreeMap::new();
        for allocation in input.allocations {
            if has_allocation_contracts && allocation.allocator_contract.is_none() {
                return Err(format!(
                    "artifact declares allocation_contracts_v1 but allocation '{}' is missing allocator_contract",
                    allocation.id
                ));
            }
            if let Some(contract) = allocation.allocator_contract.as_ref() {
                validate_contract(contract, "allocator_contract")?;
            }
            if allocation.id.is_empty() {
                return Err("empty abstract-allocation id in annotated ICFG".into());
            }
            if allocations.insert(allocation.id.clone(), allocation).is_some() {
                return Err("duplicate abstract-allocation id in annotated ICFG".into());
            }
        }
        let mut nodes = BTreeMap::new();
        for n in input.nodes {
            if !has_mir_semantic_labels && !n.semantic_labels.is_empty() {
                return Err(format!("node '{}' contains structural MIR labels without capability mir_semantic_labels_v1", n.id));
            }
            for label in &n.semantic_labels {
                if !valid_structural_label(label) {
                    return Err(format!("node '{}' contains invalid structural MIR label '{}'", n.id, label));
                }
            }
            validate_memory(&n.id, "pre", &n.pre, &variables)?;
            validate_memory(&n.id, "post", &n.post, &variables)?;
            if has_allocation_state {
                let allocation_post = n.allocation_post.as_ref().ok_or_else(|| {
                    format!(
                        "artifact declares allocation_state_v1 but node '{}' is missing allocation_post",
                        n.id
                    )
                })?;
                validate_allocation_memory(&n.id, allocation_post, &allocations)?;
            } else if let Some(allocation_post) = n.allocation_post.as_ref() {
                validate_allocation_memory(&n.id, allocation_post, &allocations)?;
            }
            if nodes.insert(n.id.clone(), n).is_some() {
                return Err("duplicate node id in annotated ICFG".into());
            }
        }
        if !nodes.contains_key(&input.entry) {
            return Err(format!("entry node '{}' is not present", input.entry));
        }

        for node in nodes.values() {
            for succ in &node.successors {
                if !nodes.contains_key(succ) {
                    return Err(format!("node '{}' references missing successor '{}'", node.id, succ));
                }
            }
            for label in &node.labels {
                if !variables.contains_key(&label.variable) {
                    return Err(format!("node '{}' label references undeclared variable '{}'", node.id, label.variable));
                }
            }
            for label in &node.allocation_labels {
                if !allocations.contains_key(&label.allocation) {
                    return Err(format!(
                        "node '{}' allocation label references undeclared allocation '{}'",
                        node.id, label.allocation
                    ));
                }
                if has_allocation_contracts && label.predicate == EventKind::Drop
                    && label.deallocator_contract.is_none()
                {
                    return Err(format!(
                        "artifact declares allocation_contracts_v1 but node '{}' drop label for '{}' is missing deallocator_contract",
                        node.id, label.allocation
                    ));
                }
                if let Some(contract) = label.deallocator_contract.as_ref() {
                    validate_contract(contract, "deallocator_contract")?;
                    if has_allocation_contracts_v2 && label.predicate == EventKind::Drop {
                        validate_v2_deallocator_contract(
                            contract,
                            "deallocator_contract",
                            has_allocation_contracts_v3,
                        )?;
                    }
                }
            }
            if !has_allocation_disposition && !node.allocation_disposition.is_empty() {
                return Err(format!(
                    "node '{}' contains allocation disposition records without capability allocation_disposition_v1",
                    node.id
                ));
            }
            for record in &node.allocation_disposition {
                if !allocations.contains_key(&record.allocation) {
                    return Err(format!(
                        "node '{}' disposition record references undeclared allocation '{}'",
                        node.id, record.allocation
                    ));
                }
                validate_allocation_disposition_record(record, has_allocation_disposition_v2)?;
                for variable in [record.source_variable.as_ref(), record.target_variable.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    if !variables.contains_key(variable) {
                        return Err(format!(
                            "node '{}' disposition record references undeclared variable '{}'",
                            node.id, variable
                        ));
                    }
                }
            }

            for (kind, identity) in [
                ("identity", node.identity.as_ref()),
                ("event_identity", node.event_identity.as_ref()),
            ] {
                if let Some(identity) = identity {
                    for relation in &identity.points_to {
                        if !variables.contains_key(&relation.variable) {
                            return Err(format!(
                                "node '{}' {kind} relation references undeclared variable '{}'",
                                node.id, relation.variable
                            ));
                        }
                        for allocation in &relation.allocations {
                            if !allocations.contains_key(allocation) {
                                return Err(format!(
                                    "node '{}' {kind} relation references undeclared allocation '{}'",
                                    node.id, allocation
                                ));
                            }
                        }
                    }
                }
            }
        }

        if has_source_provenance {
            validate_source_provenance_overlay(&source_provenance, &nodes, &allocations)?;
        } else if !source_provenance.is_empty() {
            return Err("source provenance records require capability source_provenance_v1".into());
        }

        if !has_external_deallocation_effects && !input.external_deallocation_effects.is_empty() {
            return Err("external deallocation-effect records require capability external_deallocation_effects_v1".into());
        }
        let typed_edges = typed_edges.unwrap_or_default();
        if has_typed_edge_flow {
            validate_typed_edge_relation(&typed_edges, &nodes)?;
        }

        let mut external_deallocation_effects = BTreeMap::new();
        for record in input.external_deallocation_effects {
            if !nodes.contains_key(&record.node) {
                return Err(format!("external deallocation-effect record references unknown node '{}'", record.node));
            }
            if !record.node.starts_with("dummyCall::") {
                return Err(format!("external deallocation-effect record must reference a dummyCall node, got '{}'", record.node));
            }
            if record.callee.is_empty() {
                return Err("external deallocation-effect record has empty callee".into());
            }
            validate_external_deallocation_effect(&record, has_llvm_memory_effects)?;
            if external_deallocation_effects.insert(record.node.clone(), record).is_some() {
                return Err("duplicate external deallocation-effect record for node".into());
            }
        }

        let mut ffi_argument_identity = BTreeMap::new();
        for record in input.ffi_argument_identity {
            if !nodes.contains_key(&record.node) {
                return Err(format!("ffi argument-identity record references unknown node '{}'", record.node));
            }
            if !record.node.starts_with("dummyCall::") {
                return Err(format!("ffi argument-identity record must reference a dummyCall node, got '{}'", record.node));
            }
            if record.callee.is_empty() || record.callsite.is_empty() {
                return Err("ffi argument-identity record requires non-empty callee/callsite".into());
            }
            if !variables.contains_key(&record.actual_variable) {
                return Err(format!("ffi argument-identity actual variable is undeclared: '{}'", record.actual_variable));
            }
            if !variables.contains_key(&record.formal_variable) {
                return Err(format!("ffi argument-identity formal variable is undeclared: '{}'", record.formal_variable));
            }
            if record.certainty != "may_abstract"
                || record.basis != "crema_bmulti_actual_formal_identity_v1"
                || record.formal_mapping_basis != "svf_formal_arg_index_v1"
            {
                return Err(format!(
                    "ffi argument-identity invalid proof tuple at '{}' arg {}",
                    record.node, record.arg_index
                ));
            }
            if record.allocations.is_empty() {
                return Err(format!(
                    "ffi argument-identity record must carry at least one MAY allocation at '{}' arg {}",
                    record.node, record.arg_index
                ));
            }
            let mut seen_allocations = BTreeSet::new();
            for allocation in &record.allocations {
                if !allocations.contains_key(allocation) {
                    return Err(format!("ffi argument-identity record references undeclared allocation '{}'", allocation));
                }
                if !seen_allocations.insert(allocation.clone()) {
                    return Err(format!("ffi argument-identity record has duplicate allocation '{}'", allocation));
                }
            }
            let mut pts = record.svf_may_points_to.clone();
            pts.sort_unstable();
            pts.dedup();
            if pts != record.svf_may_points_to {
                return Err(format!("ffi argument-identity SVF MAY set must be sorted/unique at '{}' arg {}", record.node, record.arg_index));
            }
            if !record.svf_may_points_to.is_empty()
                && record.svf_points_to_basis.as_deref() != Some("svf_andersen_wave_diff_may_v1")
            {
                return Err(format!("ffi argument-identity nonempty SVF MAY set lacks Andersen basis at '{}' arg {}", record.node, record.arg_index));
            }
            if record.svf_points_to_basis.as_deref().is_some_and(|b| b != "svf_andersen_wave_diff_may_v1") {
                return Err(format!("ffi argument-identity has unsupported SVF points-to basis at '{}' arg {}", record.node, record.arg_index));
            }
            let key = (record.node.clone(), record.arg_index);
            if ffi_argument_identity.insert(key, record).is_some() {
                return Err("duplicate ffi argument-identity record for node/arg_index".into());
            }
        }

        if has_panic_lifecycle_state {
            for node_id in nodes.keys() {
                if !panic_lifecycle.contains_key(node_id) {
                    return Err(format!(
                        "artifact declares panic_lifecycle_state_v1 but node '{node_id}' is missing panic_lifecycle"
                    ));
                }
            }
        } else if !panic_lifecycle.is_empty() {
            return Err("panic lifecycle records require capability panic_lifecycle_state_v1".into());
        }
        if has_panic_lifecycle_state_v2 {
            for node_id in nodes.keys() {
                if panic_lifecycle.coverage_at(node_id).is_none() {
                    return Err(format!(
                        "artifact declares panic_lifecycle_state_v2 but node '{node_id}' is missing panic_lifecycle_coverage"
                    ));
                }
            }
        } else if !panic_lifecycle.coverage_is_empty() {
            return Err("panic lifecycle coverage requires capability panic_lifecycle_state_v2".into());
        }
        for (node_id, records) in &panic_lifecycle {
            if !nodes.contains_key(node_id) {
                return Err(format!("panic lifecycle overlay references unknown node '{node_id}'"));
            }
            let mut seen = BTreeSet::new();
            for record in records {
                if !allocations.contains_key(&record.allocation) {
                    return Err(format!(
                        "node '{node_id}' panic lifecycle record references undeclared allocation '{}'",
                        record.allocation
                    ));
                }
                if !seen.insert(record.allocation.clone()) {
                    return Err(format!(
                        "node '{node_id}' has duplicate panic lifecycle record for allocation '{}'",
                        record.allocation
                    ));
                }
            }
        }

        Ok(Self {
            schema_version, entry: input.entry, capabilities, variables, allocations, nodes,
            source_provenance, typed_edges, external_deallocation_effects, ffi_argument_identity,
            llvm_memory_effects, svf_solved_points_to, panic_lifecycle,
        })
    }

    /// Resolve a node id or Rust function name to exactly one Kripke entry.
    /// Ambiguous suffixes are rejected rather than resolved by map order.
    pub fn resolve_entry(&self, requested: &str) -> Result<String, String> {
        if self.nodes.contains_key(requested) {
            return Ok(requested.to_string());
        }

        let normalized = requested
            .strip_prefix("rust::")
            .unwrap_or(requested)
            .trim_end_matches("::bb0");
        let mut candidates = BTreeSet::new();
        for id in self.nodes.keys() {
            let Some(scope) = rust_function_scope(id) else { continue; };
            if !id.ends_with("::bb0") {
                continue;
            }
            let bare_scope = scope.strip_prefix("rust::").unwrap_or(scope);
            if bare_scope == normalized || bare_scope.ends_with(&format!("::{normalized}")) {
                candidates.insert(id.clone());
            }
        }

        match candidates.len() {
            0 => Err(format!("no Kripke node/function matches entry '{requested}'")),
            1 => Ok(candidates.into_iter().next().unwrap()),
            _ => Err(format!(
                "ambiguous Kripke entry '{requested}'; candidates: {}",
                candidates.into_iter().collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// Return a deterministic entry-scoped Kripke projection.
    ///
    /// Whole-program mode retains every node reachable from the requested
    /// entry.  `intra=true` additionally restricts traversal to MIR basic
    /// blocks of the entry's Rust function; an interprocedural edge is a hard
    /// boundary and no synthetic call bypass is introduced.  This is an
    /// explicit *scoped* model-checking mode, not a replacement for the v6I
    /// whole-program no-refutation theorem.
    pub fn project_from_entry(&self, requested: &str, intra: bool) -> Result<Self, String> {
        let entry = self.resolve_entry(requested)?;
        let intra_scope = if intra {
            Some(
                rust_function_scope(&entry)
                    .ok_or_else(|| format!("--intra requires a Rust MIR entry, got '{entry}'"))?
                    .to_string(),
            )
        } else {
            None
        };

        let mut retained = BTreeSet::new();
        let mut worklist = std::collections::VecDeque::new();
        retained.insert(entry.clone());
        worklist.push_back(entry.clone());

        while let Some(id) = worklist.pop_front() {
            let Some(node) = self.nodes.get(&id) else { continue; };
            let mut successors = node.successors.clone();
            successors.sort();
            successors.dedup();
            for succ in successors {
                if let Some(scope) = intra_scope.as_deref() {
                    if rust_function_scope(&succ) != Some(scope) {
                        continue;
                    }
                }
                if retained.insert(succ.clone()) {
                    worklist.push_back(succ);
                }
            }
        }

        let mut nodes = BTreeMap::new();
        for id in &retained {
            let mut node = self.nodes.get(id).unwrap().clone();
            node.successors.retain(|succ| retained.contains(succ));
            node.successors.sort();
            node.successors.dedup();
            nodes.insert(id.clone(), node);
        }

        let mut used_variables = BTreeSet::new();
        let mut used_allocations = BTreeSet::new();
        for node in nodes.values() {
            used_variables.extend(node.labels.iter().map(|l| l.variable.clone()));
            used_allocations.extend(node.allocation_labels.iter().map(|l| l.allocation.clone()));
            for memory in [&node.pre, &node.post] {
                for cell in &memory.cells {
                    used_variables.extend(cell.aliases.iter().cloned());
                }
            }
            for identity in [node.identity.as_ref(), node.event_identity.as_ref()].into_iter().flatten() {
                for relation in &identity.points_to {
                    used_variables.insert(relation.variable.clone());
                    used_allocations.extend(relation.allocations.iter().cloned());
                }
            }
        }

        let mut panic_lifecycle = PanicLifecycleOverlay::new();
        for id in &retained {
            if let Some(records) = self.panic_lifecycle.get(id) {
                used_allocations.extend(records.iter().map(|record| record.allocation.clone()));
                panic_lifecycle.insert(id.clone(), records.clone());
            }
            if let Some(coverage) = self.panic_lifecycle.coverage_at(id) {
                panic_lifecycle.set_coverage(id.clone(), coverage);
            }
        }

        let variables = self
            .variables
            .iter()
            .filter(|(id, _)| used_variables.contains(*id))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        let allocations = self
            .allocations
            .iter()
            .filter(|(id, _)| used_allocations.contains(*id))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();

        let source_provenance = self
            .source_provenance
            .iter()
            .filter(|(node, _)| retained.contains(*node))
            .map(|(node, record)| (node.clone(), record.clone()))
            .collect();

        let external_deallocation_effects = self
            .external_deallocation_effects
            .iter()
            .filter(|(node, _)| retained.contains(*node))
            .map(|(node, record)| (node.clone(), record.clone()))
            .collect();

        Ok(Self {
            schema_version: self.schema_version,
            entry,
            capabilities: self.capabilities.clone(),
            variables,
            allocations,
            nodes,
            source_provenance,
            typed_edges: self
                .typed_edges
                .iter()
                .filter(|edge| retained.contains(&edge.source) && retained.contains(&edge.destination))
                .cloned()
                .collect(),
            external_deallocation_effects,
            ffi_argument_identity: self
                .ffi_argument_identity
                .iter()
                .filter(|((node, _), _)| retained.contains(node))
                .map(|(key, record)| (key.clone(), record.clone()))
                .collect(),
            llvm_memory_effects: self.llvm_memory_effects.clone(),
            svf_solved_points_to: self.svf_solved_points_to.clone(),
            panic_lifecycle,
        })
    }

    pub fn variable_ids(&self) -> impl Iterator<Item = &String> { self.variables.keys() }
    pub fn allocation_ids(&self) -> impl Iterator<Item = &String> { self.allocations.keys() }

    pub fn source_provenance_at(&self, node_id: &str) -> Option<&NodeSourceProvenance> {
        self.source_provenance.get(node_id)
    }

    pub fn allocation_event_source_anchors(
        &self,
        node_id: &str,
        allocation: &str,
        predicate: EventKind,
    ) -> Vec<SourceAnchor> {
        let Some(provenance) = self.source_provenance.get(node_id) else {
            return Vec::new();
        };
        let mut anchors = provenance
            .allocation_events
            .iter()
            .filter(|record| record.allocation == allocation && record.predicate == predicate)
            .flat_map(|record| record.anchors.iter().cloned())
            .collect::<Vec<_>>();
        anchors.sort();
        anchors.dedup();
        anchors
    }

    /// Canonical producer allocation-site node, derived only from the
    /// serialized AllocationSiteId object.  This is intentionally distinct
    /// from an explanation algorithm's witness-entry/origin node.
    pub fn allocation_site_node(&self, allocation: &str) -> Option<&str> {
        self.allocations
            .get(allocation)?
            .site
            .as_ref()?
            .get("node_id")?
            .as_str()
    }

    pub fn allocation_site_kind(&self, allocation: &str) -> Option<&str> {
        self.allocations
            .get(allocation)?
            .site
            .as_ref()?
            .get("kind")?
            .as_str()
    }

    pub fn external_deallocation_effect_at(
        &self,
        node_id: &str,
    ) -> Option<&ExternalDeallocationEffectRecord> {
        self.external_deallocation_effects.get(node_id)
    }

    pub fn ffi_argument_identity_at(
        &self,
        node_id: &str,
    ) -> impl Iterator<Item = &FfiArgumentIdentityRecord> {
        self.ffi_argument_identity
            .range((node_id.to_string(), 0)..=(node_id.to_string(), usize::MAX))
            .map(|(_, record)| record)
    }

    /// Alias component at this concrete program point in the abstract Kripke.
    /// Labels are associated with execution of the block, so both pre and post
    /// alias evidence is conservatively visible to label matching.
    pub fn structural_label_hold(&self, node_id: &str, kind: StructuralLabelKind, name: &str) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let prefix = match kind {
            StructuralLabelKind::Statement => "stmt",
            StructuralLabelKind::Rvalue => "rvalue",
            StructuralLabelKind::Terminator => "term",
        };
        let expected = format!("{prefix}:{name}");
        if node.semantic_labels.iter().any(|label| label == &expected) { Truth::True } else { Truth::False }
    }

    pub fn aliases_at(&self, node_id: &str, var: &str) -> BTreeSet<String> {
        let Some(node) = self.nodes.get(node_id) else {
            return BTreeSet::from([var.to_string()]);
        };
        let mut aliases = node.pre.aliases_of(var);
        aliases.extend(node.post.aliases_of(var));
        aliases.insert(var.to_string());
        aliases
    }

    /// TaintMayHold from the theory. The implementation-level alias grouping is
    /// already encoded inside Pi#_post; no extra global alias closure is added.
    pub fn may_hold(&self, node_id: &str, var: &str, p: MayPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let atom = match p {
            MayPredicate::Alloc => CellValue::Alloc,
            MayPredicate::Drop => CellValue::Freed,
            MayPredicate::OwnForg => CellValue::Mv,
            MayPredicate::RepeatDrop => return Truth::False,
        };
        if atom.leq(node.post.value_of(var)) { Truth::Unknown } else { Truth::False }
    }

    /// Exact syntactic label predicate, lifted only through the MAY-alias
    /// component available at this node (pre/post), including Rust<->C aliases.
    pub fn label_hold(&self, node_id: &str, var: &str, p: LabelPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let aliases = self.aliases_at(node_id, var);
        let holds = node.labels.iter().any(|label| {
            if !aliases.contains(&label.variable) { return false; }
            match p {
                LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
                LabelPredicate::Drop => label.predicate == EventKind::Drop,
                LabelPredicate::Read => label.predicate == EventKind::Read,
                LabelPredicate::Write => label.predicate == EventKind::Write,
                LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
                LabelPredicate::AllocatorMismatch => false,
            }
        });
        if holds { Truth::True } else { Truth::False }
    }

    /// Allocation-centric MAY state predicate provided by allocation_state_v1.
    /// Positive MAY membership yields `unk`, exactly like ProgramVar state
    /// predicates; exclusion yields `ff`.
    pub fn allocation_may_hold(&self, node_id: &str, allocation: &str, p: MayPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        if p == MayPredicate::RepeatDrop {
            let has_may_witness = self.panic_lifecycle
                .get(node_id)
                .into_iter()
                .flatten()
                .any(|record| {
                    record.allocation == allocation
                        && record.certainty == AllocationEventCertainty::MayAbstract
                        && record.may_repeat_drop()
                });
            if has_may_witness {
                return Truth::Unknown;
            }
            if self.capabilities.contains("panic_lifecycle_state_v2")
                && self.panic_lifecycle.coverage_at(node_id) == Some(PanicLifecycleCoverage::Unresolved)
            {
                return Truth::Unknown;
            }
            return Truth::False;
        }
        let Some(post) = node.allocation_post.as_ref() else { return Truth::False; };
        let atom = match p {
            MayPredicate::Alloc => CellValue::Alloc,
            MayPredicate::Drop => CellValue::Freed,
            MayPredicate::OwnForg => CellValue::Mv,
            MayPredicate::RepeatDrop => unreachable!("handled above"),
        };
        if atom.leq(post.value_of(allocation)) { Truth::Unknown } else { Truth::False }
    }

    /// Allocation-centric event predicate introduced by schema v2.
    ///
    /// Schema v2 contains only `may_abstract` labels.  Matching a positive
    /// allocation-event atom therefore yields `unk`; absence yields `ff`.
    /// `tt` is intentionally unavailable until an explicit MUST relation and
    /// its concretization theorem are implemented in a later schema version.
    pub fn allocation_label_hold(&self, node_id: &str, allocation: &str, p: LabelPredicate) -> Truth {
        let Some(node) = self.nodes.get(node_id) else { return Truth::False; };
        let mut acc = Truth::False;
        for label in &node.allocation_labels {
            if label.allocation != allocation {
                continue;
            }
            let matches = match p {
                LabelPredicate::Alloc => label.predicate == EventKind::Alloc,
                LabelPredicate::Drop => label.predicate == EventKind::Drop,
                LabelPredicate::Read => label.predicate == EventKind::Read,
                LabelPredicate::Write => label.predicate == EventKind::Write,
                LabelPredicate::Use => matches!(label.predicate, EventKind::Use | EventKind::Read | EventKind::Write),
                LabelPredicate::AllocatorMismatch => {
                    if label.predicate != EventKind::Drop {
                        false
                    } else {
                        let allocator = self.allocations
                            .get(allocation)
                            .and_then(|a| a.allocator_contract.as_ref())
                            .map(|c| c.family.as_str())
                            .unwrap_or("unknown");
                        let deallocator = label.deallocator_contract.as_ref()
                            .map(|c| c.family.as_str())
                            .unwrap_or("unknown");
                        allocator == "unknown" || deallocator == "unknown" || allocator != deallocator
                    }
                },
            };
            if !matches {
                continue;
            }
            let value = match label.certainty {
                AllocationEventCertainty::MayAbstract => Truth::Unknown,
            };
            acc = acc.join(value);
        }
        acc
    }
}

fn validate_external_deallocation_effect(
    record: &ExternalDeallocationEffectRecord,
    has_llvm_memory_effects: bool,
) -> Result<(), String> {
    let llvm_basis = matches!(
        record.basis.as_str(),
        "llvm16_explicit_nofree_v1"
            | "llvm16_tli_nofree_v1"
            | "llvm16_explicit_nonmodifying_memory_v1"
            | "llvm16_tli_nonmodifying_memory_v1"
            | "llvm16_explicit_allockind_deallocation_v1"
            | "llvm16_tli_allockind_deallocation_v1"
            | "llvm16_explicit_direct_callee_allockind_deallocation_v1"
            | "llvm16_tli_direct_callee_allockind_deallocation_v1"
    );
    if llvm_basis && !has_llvm_memory_effects {
        return Err(format!(
            "external deallocation-effect basis '{}' requires artifact capability llvm_memory_effects_v1",
            record.basis
        ));
    }

    let ok = match record.status {
        ExternalDeallocationEffectStatus::CertifiedAbsent => matches!(
            record.basis.as_str(),
            "svf_leaf_no_call_deallocation_v1"
                | "llvm16_explicit_nofree_v1"
                | "llvm16_tli_nofree_v1"
                | "llvm16_explicit_nonmodifying_memory_v1"
                | "llvm16_tli_nonmodifying_memory_v1"
            ),
        ExternalDeallocationEffectStatus::ObservedMayDeallocate => matches!(
            record.basis.as_str(),
            "structural_c_free_v1"
                | "llvm16_explicit_allockind_deallocation_v1"
                | "llvm16_tli_allockind_deallocation_v1"
                | "llvm16_explicit_direct_callee_allockind_deallocation_v1"
                | "llvm16_tli_direct_callee_allockind_deallocation_v1"
            ),
        ExternalDeallocationEffectStatus::Unresolved => matches!(
            record.basis.as_str(),
            "svf_call_effect_unresolved_v1" | "svf_body_unavailable_v1"
        ),
    };
    if !ok {
        return Err(format!(
            "invalid external deallocation-effect tuple: status={:?} basis={}",
            record.status, record.basis
        ));
    }

    let mut previous: Option<&str> = None;
    let mut seen = BTreeSet::new();
    for corroborating in &record.corroborating_bases {
        if corroborating == &record.basis {
            return Err("external deallocation-effect corroborating basis duplicates primary basis".into());
        }
        if !seen.insert(corroborating.as_str()) {
            return Err("external deallocation-effect has duplicate corroborating basis".into());
        }
        if previous.is_some_and(|p| p >= corroborating.as_str()) {
            return Err("external deallocation-effect corroborating bases must be sorted unique".into());
        }
        previous = Some(corroborating.as_str());

        let llvm_corroborating = matches!(
            corroborating.as_str(),
            "llvm16_explicit_nofree_v1"
                | "llvm16_tli_nofree_v1"
                | "llvm16_explicit_nonmodifying_memory_v1"
                | "llvm16_tli_nonmodifying_memory_v1"
                | "llvm16_explicit_allockind_deallocation_v1"
                | "llvm16_tli_allockind_deallocation_v1"
                | "llvm16_explicit_direct_callee_allockind_deallocation_v1"
                | "llvm16_tli_direct_callee_allockind_deallocation_v1"
        );
        if !llvm_corroborating {
            return Err(format!(
                "unsupported external deallocation-effect corroborating basis '{}'",
                corroborating
            ));
        }
        if !has_llvm_memory_effects {
            return Err(format!(
                "external deallocation-effect corroborating basis '{}' requires artifact capability llvm_memory_effects_v1",
                corroborating
            ));
        }
        let compatible = match record.status {
            ExternalDeallocationEffectStatus::CertifiedAbsent => matches!(
                corroborating.as_str(),
                "llvm16_explicit_nofree_v1"
                    | "llvm16_tli_nofree_v1"
                    | "llvm16_explicit_nonmodifying_memory_v1"
                    | "llvm16_tli_nonmodifying_memory_v1"
            ),
            ExternalDeallocationEffectStatus::ObservedMayDeallocate => matches!(
                corroborating.as_str(),
                "llvm16_explicit_allockind_deallocation_v1"
                    | "llvm16_tli_allockind_deallocation_v1"
                    | "llvm16_explicit_direct_callee_allockind_deallocation_v1"
                    | "llvm16_tli_direct_callee_allockind_deallocation_v1"
            ),
            ExternalDeallocationEffectStatus::Unresolved => false,
        };
        if !compatible {
            return Err(format!(
                "external deallocation-effect corroborating basis '{}' is incompatible with status {:?}",
                corroborating, record.status
            ));
        }
    }
    Ok(())
}

fn validate_allocation_memory(
    node_id: &str,
    memory: &AbstractAllocationMemoryAnnotation,
    allocations: &BTreeMap<String, AbstractAllocation>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for cell in &memory.cells {
        if !allocations.contains_key(&cell.allocation) {
            return Err(format!(
                "node '{}' allocation_post references undeclared allocation '{}'",
                node_id, cell.allocation
            ));
        }
        if !seen.insert(cell.allocation.clone()) {
            return Err(format!(
                "node '{}' allocation_post contains duplicate allocation '{}'",
                node_id, cell.allocation
            ));
        }
    }
    Ok(())
}

fn validate_contract(contract: &AllocationContract, field: &str) -> Result<(), String> {
    if !matches!(contract.family.as_str(), "rust_global" | "c_malloc" | "unknown") {
        return Err(format!("{field} has unsupported family '{}'", contract.family));
    }
    if contract.operation.is_empty() {
        return Err(format!("{field} has empty operation"));
    }
    if !matches!(contract.language.as_str(), "rust" | "c" | "unknown") {
        return Err(format!("{field} has unsupported language '{}'", contract.language));
    }
    Ok(())
}


/// allocation_contracts_v2 closed producer-evidence basis vocabulary.
///
/// The producer is solely responsible for proving the basis.  The checker only
/// validates that a claimed basis is compatible with the serialized family and
/// operation; it never parses owner/allocator DefPath strings to infer a family.
fn validate_v2_deallocator_contract(
    contract: &AllocationContract,
    field: &str,
    allow_v3: bool,
) -> Result<(), String> {
    let basis = contract.basis.as_deref().ok_or_else(|| {
        format!("{field} requires proof field 'basis' under allocation_contracts_v2")
    })?;

    match basis {
        "rust_box_global_drop" | "rust_vec_global_drop" | "rust_cstring_global_drop" => {
            if contract.family != "rust_global" || contract.operation != "drop" || contract.language != "rust" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=rust_global operation=drop language=rust"
                ));
            }
            if contract.owner_def_path.as_deref().map_or(true, str::is_empty)
                || contract.allocator_def_path.as_deref().map_or(true, str::is_empty)
            {
                return Err(format!(
                    "{field} basis '{basis}' requires diagnostic owner_def_path and allocator_def_path"
                ));
            }
            if contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept explicit-call provenance"
                ));
            }
        }
        "rust_global_dealloc_api" => {
            if contract.family != "rust_global" || contract.operation != "dealloc" || contract.language != "rust" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=rust_global operation=dealloc language=rust"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept typed-drop provenance fields"
                ));
            }
            if contract.callee_def_path.as_deref().map_or(true, str::is_empty) {
                return Err(format!(
                    "{field} basis '{basis}' requires producer audit field callee_def_path"
                ));
            }
        }
        "rust_mem_drop_owned_box_global_v1" => {
            if !allow_v3 {
                return Err(format!(
                    "{field} basis 'rust_mem_drop_owned_box_global_v1' requires artifact capability allocation_contracts_v3"
                ));
            }
            if contract.family != "rust_global" || contract.operation != "drop" || contract.language != "rust" {
                return Err(format!(
                    "{field} basis 'rust_mem_drop_owned_box_global_v1' requires family=rust_global operation=drop language=rust"
                ));
            }
            if contract.owner_def_path.as_deref().map_or(true, str::is_empty)
                || contract.allocator_def_path.as_deref().map_or(true, str::is_empty)
                || contract.callee_def_path.as_deref().map_or(true, str::is_empty)
            {
                return Err(format!(
                    "{field} basis 'rust_mem_drop_owned_box_global_v1' requires producer audit fields owner_def_path, allocator_def_path, and callee_def_path"
                ));
            }
        }
        "structural_c_free_v1" => {
            if contract.family != "c_malloc" || contract.operation != "free" || contract.language != "c" {
                return Err(format!(
                    "{field} basis '{basis}' requires family=c_malloc operation=free language=c"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() || contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} basis '{basis}' does not accept Rust provenance fields"
                ));
            }
        }
        "unresolved" => {
            if contract.family != "unknown" {
                return Err(format!(
                    "{field} basis 'unresolved' must remain family=unknown"
                ));
            }
            if contract.owner_def_path.is_some() || contract.allocator_def_path.is_some() || contract.callee_def_path.is_some() {
                return Err(format!(
                    "{field} unresolved contracts must not carry provenance fields"
                ));
            }
        }
        other => return Err(format!("{field} has unsupported allocation_contracts_v2 basis '{other}'")),
    }
    Ok(())
}

fn validate_memory(
    node_id: &str,
    which: &str,
    mem: &AbstractMemoryAnnotation,
    variables: &BTreeMap<String, ProgramVariable>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for cell in &mem.cells {
        if cell.aliases.is_empty() {
            return Err(format!("node '{node_id}' {which} contains an empty alias component"));
        }
        for v in &cell.aliases {
            if !variables.contains_key(v) {
                return Err(format!("node '{node_id}' {which} references undeclared variable '{v}'"));
            }
            if !seen.insert(v.clone()) {
                return Err(format!("node '{node_id}' {which}: variable '{v}' appears in more than one abstract allocation"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem(aliases: &[&str], value: CellValue) -> AbstractMemoryAnnotation {
        AbstractMemoryAnnotation { cells: vec![AbstractCell {
            aliases: aliases.iter().map(|x| x.to_string()).collect(), value
        }] }
    }

    fn valid_efx1_memory(readwrite: bool) -> LlvmMemoryAccessEvidenceV1 {
        LlvmMemoryAccessEvidenceV1 {
            argmem: if readwrite { "readwrite" } else { "none" }.into(),
            inaccessiblemem: if readwrite { "readwrite" } else { "none" }.into(),
            other: "none".into(),
            encoded: 0,
        }
    }

    fn valid_efx1_evidence() -> LlvmMemoryEffectsEvidenceV1 {
        let explicit = LlvmFunctionEffectsEvidenceV1 {
            memory: valid_efx1_memory(true),
            formals: vec![LlvmFormalEffectsEvidenceV1 {
                index: 0,
                pointer_typed: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let inferred = LlvmFunctionEffectsEvidenceV1 {
            willreturn: true,
            memory_explicit: true,
            memory: valid_efx1_memory(true),
            alloc_kind: vec!["free".into()],
            alloc_family: Some("malloc".into()),
            formals: vec![LlvmFormalEffectsEvidenceV1 {
                index: 0,
                pointer_typed: true,
                nocapture: true,
                allocptr: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        LlvmMemoryEffectsEvidenceV1 {
            schema: "llvm_memory_effects_v1".into(),
            llvm_version: "16.0.4".into(),
            explicit_basis: "llvm16_explicit_input_ir_v1".into(),
            tli_basis: "llvm16_tli_libfunc_attrs_v1".into(),
            modules: vec![LlvmEffectsModuleEvidenceV1 {
                input: "fixture.ll".into(),
                target_triple: "x86_64-pc-linux-gnu".into(),
                input_ir_verified: true,
                tli_clone_verified: true,
                functions: vec![LlvmFunctionEffectsRecordEvidenceV1 {
                    name: "free".into(),
                    is_declaration: true,
                    origin_explicit: "explicit_input_ir".into(),
                    explicit,
                    tli_recognized: true,
                    tli_libfunc: Some("free".into()),
                    origin_inferred: "llvm_tli_inferred".into(),
                    tli_inferred: inferred,
                    tli_changed: true,
                }],
                callsites_explicit: vec![],
                callsites_tli_inferred: vec![],
            }],
        }
    }

    fn valid_pts_evidence() -> SvfSolvedPointsToEvidenceV1 {
        SvfSolvedPointsToEvidenceV1 {
            schema: "svf_solved_points_to_v1".into(),
            analysis: "AndersenWaveDiff".into(),
            semantics: "may".into(),
            formal_mapping_schema: "svf_formal_arg_index_v1".into(),
            functions: vec![],
        }
    }

    fn base() -> AnnotatedIcfg {
        AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ffi_argument_identity: vec![],
            entry: "b0".into(),
            variables: vec![
                ProgramVariable { id: "rust::x".into(), language: ProgramLanguage::Rust, display: None, function: None },
                ProgramVariable { id: "c::p".into(), language: ProgramLanguage::C, display: None, function: None },
            ],
            allocations: vec![],
            external_deallocation_effects: vec![],
            nodes: vec![AnnotatedNode {
                id: "b0".into(), successors: vec![],
                labels: vec![EventLabel { predicate: EventKind::Drop, variable: "c::p".into() }],
                semantic_labels: vec![],
                allocation_labels: vec![],
                allocation_disposition: vec![],
                identity: None,
                event_identity: None,
                    allocation_post: None,
                pre: mem(&["rust::x", "c::p"], CellValue::Alloc),
                post: mem(&["rust::x", "c::p"], CellValue::Top),
            }],
        }
    }

    #[test]
    fn w1_source_provenance_overlay_parses_structured_event_anchors() {
        let raw = serde_json::json!({
            "nodes": [{
                "id": "b0",
                "source_provenance": {
                    "language": "rust",
                    "anchors": [{
                        "kind": "mir_terminator",
                        "raw_span": "/tmp/project/src/main.rs:8:5: 8:14 (#1)",
                        "parsed_span": {
                            "file": "/tmp/project/src/main.rs",
                            "start_line": 8,
                            "start_column": 5,
                            "end_line": 8,
                            "end_column": 14
                        },
                        "basis": "rustc_mir_source_info_v1"
                    }],
                    "allocation_events": [{
                        "predicate": "drop",
                        "allocation": "A",
                        "certainty": "may_abstract",
                        "anchors": [{
                            "kind": "mir_terminator",
                            "raw_span": "/tmp/project/src/main.rs:8:5: 8:14 (#1)",
                            "parsed_span": {
                                "file": "/tmp/project/src/main.rs",
                                "start_line": 8,
                                "start_column": 5,
                                "end_line": 8,
                                "end_column": 14
                            },
                            "basis": "rustc_mir_source_info_v1"
                        }]
                    }]
                }
            }]
        });
        let overlay = source_provenance_overlay_from_json(&raw).unwrap();
        let provenance = overlay.get("b0").unwrap();
        assert_eq!(provenance.language, "rust");
        assert_eq!(provenance.anchors.len(), 1);
        assert_eq!(provenance.allocation_events.len(), 1);
        assert_eq!(provenance.allocation_events[0].predicate, EventKind::Drop);
        assert_eq!(provenance.allocation_events[0].certainty, AllocationEventCertainty::MayAbstract);
    }

    #[test]
    fn w1_source_provenance_is_additive_and_must_match_existing_allocation_events() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["source_provenance_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: Some(serde_json::json!({
                "kind": "rust_call",
                "node_id": "b0",
                "callee": "alloc::boxed::Box::<i32>::new"
            })),
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop,
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: None,
        }];
        let anchor = SourceAnchor {
            kind: "mir_terminator".into(),
            statement_index: None,
            raw_span: "/tmp/project/src/main.rs:8:5: 8:14 (#1)".into(),
            parsed_span: Some(ParsedSourceSpan {
                file: "/tmp/project/src/main.rs".into(),
                start_line: 8,
                start_column: 5,
                end_line: 8,
                end_column: 14,
            }),
            basis: "rustc_mir_source_info_v1".into(),
        };
        let mut overlay = SourceProvenanceOverlay::new();
        overlay.insert("b0".into(), NodeSourceProvenance {
            language: "rust".into(),
            anchors: vec![anchor.clone()],
            allocation_events: vec![AllocationEventSourceRecord {
                predicate: EventKind::Drop,
                allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                anchors: vec![anchor.clone()],
            }],
        });

        let k = Kripke::from_annotated_icfg_with_all_overlays(
            input.clone(),
            PanicLifecycleOverlay::new(),
            None,
            overlay.clone(),
        ).unwrap();
        assert_eq!(
            k.allocation_event_source_anchors("b0", "A", EventKind::Drop),
            vec![anchor]
        );

        let mut bad = overlay;
        bad.get_mut("b0").unwrap().allocation_events[0].predicate = EventKind::Alloc;
        let err = Kripke::from_annotated_icfg_with_all_overlays(
            input,
            PanicLifecycleOverlay::new(),
            None,
            bad,
        ).unwrap_err();
        assert!(err.contains("has no matching allocation_label"), "unexpected error: {err}");
    }

    #[test]
    fn w1_source_anchor_wire_format_matches_producer_optional_field_policy() {
        let anchor = SourceAnchor {
            kind: "mir_terminator".into(),
            statement_index: None,
            raw_span: "/tmp/project/src/main.rs:8:5: 8:14 (#1)".into(),
            parsed_span: Some(ParsedSourceSpan {
                file: "/tmp/project/src/main.rs".into(),
                start_line: 8,
                start_column: 5,
                end_line: 8,
                end_column: 14,
            }),
            basis: "rustc_mir_source_info_v1".into(),
        };
        let value = serde_json::to_value(&anchor).unwrap();
        assert!(value.get("statement_index").is_none());
        assert!(value.get("parsed_span").is_some());

        let mut without_parsed = anchor;
        without_parsed.parsed_span = None;
        let value = serde_json::to_value(&without_parsed).unwrap();
        assert!(value.get("statement_index").is_none());
        assert!(value.get("parsed_span").is_none());
    }

    #[test]
    fn w1_source_provenance_capability_and_payload_are_atomic() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["source_provenance_v1".into()];
        let err = Kripke::from_annotated_icfg_with_all_overlays(
            input,
            PanicLifecycleOverlay::new(),
            None,
            SourceProvenanceOverlay::new(),
        ).unwrap_err();
        assert!(err.contains("capability and node provenance overlay"), "unexpected error: {err}");
    }

    #[test]
    fn cross_language_alias_lifts_labels_and_post_state() {
        let k = Kripke::from_annotated_icfg(base()).unwrap();
        assert_eq!(k.label_hold("b0", "rust::x", LabelPredicate::Drop), Truth::True);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Alloc), Truth::Unknown);
        assert_eq!(k.may_hold("b0", "rust::x", MayPredicate::Drop), Truth::Unknown);
    }

    #[test]
    fn rejects_overlapping_abstract_components_at_one_program_point() {
        let mut input = base();
        input.nodes[0].post.cells.push(AbstractCell { aliases: vec!["rust::x".into()], value: CellValue::Freed });
        assert!(Kripke::from_annotated_icfg(input).is_err());
    }

    #[test]
    fn schema_v2_rejects_reserved_exact_abstract_certainty() {
        let raw = r#"{"predicate":"drop","allocation":"A","certainty":"exact_abstract"}"#;
        assert!(serde_json::from_str::<AllocationEventLabel>(raw).is_err());
    }


    #[test]
    fn v6s_allocation_disposition_accepts_certified_raw_pointer_drop_noop() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_disposition_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_disposition = vec![AllocationDispositionRecord {
            allocation: "A".into(),
            kind: AllocationDispositionKind::RawPointerDropNoop,
            certainty: AllocationEventCertainty::MayAbstract,
            obligation_effect: AllocationObligationEffect::NoPointeeLifecycleEffect,
            basis: "rustc_mem_drop_raw_pointer_v1".into(),
            source_variable: Some("rust::x".into()),
            target_variable: None,
            callee_def_path: Some("core::mem::drop".into()),
        }];
        Kripke::from_annotated_icfg(input).unwrap();
    }

    #[test]
    fn b1_1_disposition_v2_wire_names_are_exact_and_closed() {
        assert_eq!(
            serde_json::to_string(&AllocationDispositionKind::CStringIntoRaw).unwrap(),
            "\"cstring_into_raw\""
        );
        assert_eq!(
            serde_json::to_string(&AllocationDispositionKind::CStringFromRaw).unwrap(),
            "\"cstring_from_raw\""
        );
        assert_eq!(
            serde_json::from_str::<AllocationDispositionKind>("\"cstring_into_raw\"").unwrap(),
            AllocationDispositionKind::CStringIntoRaw
        );
        assert_eq!(
            serde_json::from_str::<AllocationDispositionKind>("\"cstring_from_raw\"").unwrap(),
            AllocationDispositionKind::CStringFromRaw
        );
        assert!(serde_json::from_str::<AllocationDispositionKind>("\"c_string_into_raw\"").is_err());
        assert!(serde_json::from_str::<AllocationDispositionKind>("\"c_string_from_raw\"").is_err());
    }

    #[test]
    fn b1_1_r1_allocation_disposition_accepts_cstring_handoff_and_reclaim() {
        let mut input = base();
        // The disposition validator deliberately requires every source/target
        // variable to be declared in the annotated-ICFG variable universe.
        // Keep the fixture faithful to producer output instead of relying on
        // synthetic undeclared temporaries.
        input.variables.extend([
            ProgramVariable {
                id: "rust::ret".into(),
                language: ProgramLanguage::Rust,
                display: None,
                function: None,
            },
            ProgramVariable {
                id: "rust::owner".into(),
                language: ProgramLanguage::Rust,
                display: None,
                function: None,
            },
        ]);
        input.schema_version = 2;
        input.capabilities = vec!["allocation_disposition_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_disposition = vec![
            AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::CStringIntoRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::PreserveManualObligation,
                basis: "rustc_cstring_into_raw_v1".into(),
                source_variable: Some("rust::x".into()),
                target_variable: Some("rust::ret".into()),
                callee_def_path: Some("alloc::ffi::c_str::CString::into_raw".into()),
            },
            AllocationDispositionRecord {
                allocation: "A".into(),
                kind: AllocationDispositionKind::CStringFromRaw,
                certainty: AllocationEventCertainty::MayAbstract,
                obligation_effect: AllocationObligationEffect::RestoreRaiiObligation,
                basis: "rustc_cstring_from_raw_v1".into(),
                source_variable: Some("rust::ret".into()),
                target_variable: Some("rust::owner".into()),
                callee_def_path: Some("alloc::ffi::c_str::CString::from_raw".into()),
            },
        ];
        let err = Kripke::from_annotated_icfg(input.clone()).unwrap_err();
        assert!(
            err.contains("requires artifact capability allocation_disposition_v2"),
            "unexpected error: {err}"
        );
        input.capabilities.push("allocation_disposition_v2".into());
        Kripke::from_annotated_icfg(input).unwrap();
    }

    #[test]
    fn v6s_allocation_disposition_rejects_wrong_raw_pointer_drop_effect() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_disposition_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        input.nodes[0].allocation_disposition = vec![AllocationDispositionRecord {
            allocation: "A".into(),
            kind: AllocationDispositionKind::RawPointerDropNoop,
            certainty: AllocationEventCertainty::MayAbstract,
            obligation_effect: AllocationObligationEffect::MayDischarge,
            basis: "rustc_mem_drop_raw_pointer_v1".into(),
            source_variable: Some("rust::x".into()),
            target_variable: None,
            callee_def_path: Some("core::mem::drop".into()),
        }];
        assert!(Kripke::from_annotated_icfg(input).is_err());
    }

    #[test]
    fn entry_projection_is_deterministic_and_reachable_only() {
        let mut input = base();
        input.entry = "rust::main::bb0".into();
        input.nodes = vec![
            AnnotatedNode {
                id: "rust::main::bb0".into(),
                successors: vec!["rust::main::bb1".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::main::bb1".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::dead::bb0".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
        ];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", false).unwrap();
        assert_eq!(projected.entry, "rust::main::bb0");
        assert_eq!(projected.nodes.keys().cloned().collect::<Vec<_>>(), vec![
            "rust::main::bb0".to_string(),
            "rust::main::bb1".to_string(),
        ]);
    }

    #[test]
    fn intra_projection_stops_at_interprocedural_boundary() {
        let mut input = base();
        input.entry = "rust::main::bb0".into();
        input.nodes = vec![
            AnnotatedNode {
                id: "rust::main::bb0".into(),
                successors: vec!["dummyCall::rust::main::bb0".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "dummyCall::rust::main::bb0".into(),
                successors: vec!["rust::callee::bb0".into()],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
            AnnotatedNode {
                id: "rust::callee::bb0".into(),
                successors: vec![],
                labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None,
                    allocation_post: None,
                pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
            },
        ];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", true).unwrap();
        assert_eq!(projected.nodes.len(), 1);
        assert!(projected.nodes.contains_key("rust::main::bb0"));
        assert!(projected.nodes["rust::main::bb0"].successors.is_empty());
    }

    #[test]
    fn schema_v2_allocation_label_uses_three_valued_abstract_certainty() {
        let mut input = base();
        input.schema_version = 2;
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: None,
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop,
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: None,
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Drop), Truth::Unknown);
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Read), Truth::False);
    }


    #[test]
    fn entry_projection_prunes_unreachable_nodes_and_quantifier_domains() {
        let input = AnnotatedIcfg {
            schema_version: 2,
            capabilities: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ffi_argument_identity: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![
                ProgramVariable { id: "rust::main::Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: Some("main".into()) },
                ProgramVariable { id: "rust::dead::Local(_1)".into(), language: ProgramLanguage::Rust, display: None, function: Some("dead".into()) },
            ],
            allocations: vec![
                AbstractAllocation { id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None },
                AbstractAllocation { id: "DEAD".into(), display: None, site: None, context: vec![], allocator_contract: None },
            ],
            external_deallocation_effects: vec![],
            nodes: vec![
                AnnotatedNode {
                    id: "rust::main::bb0".into(), successors: vec!["rust::main::bb1".into()],
                    labels: vec![EventLabel { predicate: EventKind::Read, variable: "rust::main::Local(_1)".into() }],
                    semantic_labels: vec![],
                    allocation_labels: vec![AllocationEventLabel { predicate: EventKind::Alloc, allocation: "A".into(), certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None }],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
                AnnotatedNode {
                    id: "rust::main::bb1".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
                AnnotatedNode {
                    id: "rust::dead::bb0".into(), successors: vec![],
                    labels: vec![EventLabel { predicate: EventKind::Read, variable: "rust::dead::Local(_1)".into() }],
                    semantic_labels: vec![],
                    allocation_labels: vec![AllocationEventLabel { predicate: EventKind::Alloc, allocation: "DEAD".into(), certainty: AllocationEventCertainty::MayAbstract, deallocator_contract: None }],
                    allocation_disposition: vec![],
                    identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default(),
                },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", false).unwrap();
        assert_eq!(projected.entry, "rust::main::bb0");
        assert_eq!(projected.nodes.len(), 2);
        assert!(projected.nodes.contains_key("rust::main::bb1"));
        assert!(!projected.nodes.contains_key("rust::dead::bb0"));
        assert!(projected.variables.contains_key("rust::main::Local(_1)"));
        assert!(!projected.variables.contains_key("rust::dead::Local(_1)"));
        assert!(projected.allocations.contains_key("A"));
        assert!(!projected.allocations.contains_key("DEAD"));
    }

    #[test]
    fn intra_projection_preserves_same_function_unwind_terminal() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ffi_argument_identity: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![ProgramVariable { id: "rust::x".into(), language: ProgramLanguage::Rust, display: None, function: Some("main".into()) }],
            allocations: vec![],
            external_deallocation_effects: vec![],
            nodes: vec![
                AnnotatedNode { id: "rust::main::bb0".into(), successors: vec!["rust::main::terminate".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::main::terminate".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let projected = k.project_from_entry("main", true).unwrap();
        assert!(projected.nodes.contains_key("rust::main::bb0"));
        assert!(projected.nodes.contains_key("rust::main::terminate"));
        assert_eq!(projected.nodes["rust::main::bb0"].successors, vec!["rust::main::terminate".to_string()]);
        assert!(projected.nodes["rust::main::terminate"].successors.is_empty());
    }

    #[test]
    fn intra_projection_is_same_function_induced_subgraph_and_stops_at_call_boundary() {
        let input = AnnotatedIcfg {
            schema_version: 1,
            capabilities: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ffi_argument_identity: vec![],
            entry: "rust::main::bb0".into(),
            variables: vec![ProgramVariable { id: "v".into(), language: ProgramLanguage::Rust, display: None, function: None }],
            allocations: vec![],
            external_deallocation_effects: vec![],
            nodes: vec![
                AnnotatedNode { id: "rust::main::bb0".into(), successors: vec!["dummyCall::x".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "dummyCall::x".into(), successors: vec!["rust::callee::bb0".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::callee::bb0".into(), successors: vec!["rust::callee::bb1".into()], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
                AnnotatedNode { id: "rust::callee::bb1".into(), successors: vec![], labels: vec![], semantic_labels: vec![], allocation_labels: vec![], allocation_disposition: vec![], identity: None, event_identity: None, allocation_post: None, pre: Default::default(), post: Default::default() },
            ],
        };
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let caller = k.project_from_entry("main", true).unwrap();
        assert_eq!(caller.nodes.keys().cloned().collect::<Vec<_>>(), vec!["rust::main::bb0".to_string()]);
        assert!(caller.nodes["rust::main::bb0"].successors.is_empty());

        let callee = k.project_from_entry("callee", true).unwrap();
        assert_eq!(callee.nodes.len(), 2);
        assert!(callee.nodes.contains_key("rust::callee::bb1"));
    }

    #[test]
    fn allocation_contract_capability_mismatch_label_is_unknown_and_distinct_from_drop() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_contracts_v1".into()];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![],
            allocator_contract: Some(AllocationContract { family: "rust_global".into(), operation: "box_allocation".into(), language: "rust".into(), basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None }),
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop, allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: Some(AllocationContract { family: "c_malloc".into(), operation: "free".into(), language: "c".into(), basis: None, owner_def_path: None, allocator_def_path: None, callee_def_path: None }),
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::AllocatorMismatch), Truth::Unknown);
        assert_eq!(k.allocation_label_hold("b0", "A", LabelPredicate::Drop), Truth::Unknown);
    }


    fn v2_contract_test_input(deallocator: AllocationContract) -> AnnotatedIcfg {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "allocation_contracts_v1".into(),
            "allocation_contracts_v2".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(),
            display: None,
            site: None,
            context: vec![],
            allocator_contract: Some(AllocationContract {
                family: "rust_global".into(),
                operation: "box_allocation".into(),
                language: "rust".into(),
                basis: None,
                owner_def_path: None,
                allocator_def_path: None,
                callee_def_path: None,
            }),
        }];
        input.nodes[0].allocation_labels = vec![AllocationEventLabel {
            predicate: EventKind::Drop,
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            deallocator_contract: Some(deallocator),
        }];
        input
    }

    fn v3_contract_test_input(deallocator: AllocationContract) -> AnnotatedIcfg {
        let mut input = v2_contract_test_input(deallocator);
        input.capabilities.push("allocation_contracts_v3".into());
        input
    }

    #[test]
    fn allocation_contracts_v2_requires_v1_artifact_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_contracts_v2".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("requires both artifact capabilities"), "unexpected error: {err}");
    }

    #[test]
    fn bcontract_drop1_v3_requires_v1_and_v2_capabilities() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["allocation_contracts_v3".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("requires allocation_contracts_v1 + allocation_contracts_v2 + allocation_contracts_v3"), "unexpected error: {err}");
    }

    #[test]
    fn bcontract_drop1_v3_accepts_mem_drop_owned_box_provenance() {
        let contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_mem_drop_owned_box_global_v1".into()),
            owner_def_path: Some("opaque::box_owner".into()),
            allocator_def_path: Some("opaque::global_allocator".into()),
            callee_def_path: Some("opaque::core_mem_drop".into()),
        };
        Kripke::from_annotated_icfg(v3_contract_test_input(contract)).unwrap();
    }

    #[test]
    fn bcontract_drop1_basis_is_rejected_without_v3_capability() {
        let contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_mem_drop_owned_box_global_v1".into()),
            owner_def_path: Some("opaque::box_owner".into()),
            allocator_def_path: Some("opaque::global_allocator".into()),
            callee_def_path: Some("opaque::core_mem_drop".into()),
        };
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires artifact capability allocation_contracts_v3"), "unexpected error: {err}");
    }

    #[test]
    fn bcontract_drop1_v3_requires_all_three_provenance_fields() {
        let mut contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_mem_drop_owned_box_global_v1".into()),
            owner_def_path: Some("opaque::box_owner".into()),
            allocator_def_path: Some("opaque::global_allocator".into()),
            callee_def_path: Some("opaque::core_mem_drop".into()),
        };
        contract.allocator_def_path = None;
        let err = Kripke::from_annotated_icfg(v3_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires producer audit fields"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_typed_drop_accepts_provenance_without_interpreting_paths() {
        let contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_box_global_drop".into()),
            // Deliberately opaque diagnostic strings. Acceptance proves that
            // the checker validates the proof basis tuple and does not infer
            // family by parsing these strings.
            owner_def_path: Some("opaque::owner::diagnostic".into()),
            allocator_def_path: Some("opaque::allocator::diagnostic".into()),
            callee_def_path: None,
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap();
    }

    #[test]
    fn allocation_contracts_v2_typed_cstring_drop_accepts_producer_provenance() {
        let contract = AllocationContract {
            family: "rust_global".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_cstring_global_drop".into()),
            owner_def_path: Some("alloc::ffi::c_str::CString".into()),
            allocator_def_path: Some("alloc::alloc::Global".into()),
            callee_def_path: None,
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap();
    }

    #[test]
    fn allocation_contracts_v2_rejects_basis_family_mismatch() {
        let contract = AllocationContract {
            family: "c_malloc".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("rust_vec_global_drop".into()),
            owner_def_path: Some("opaque::owner".into()),
            allocator_def_path: Some("opaque::allocator".into()),
            callee_def_path: None,
        };
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires family=rust_global"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_global_dealloc_requires_producer_callee_provenance() {
        let mut contract = AllocationContract {
            family: "rust_global".into(),
            operation: "dealloc".into(),
            language: "rust".into(),
            basis: Some("rust_global_dealloc_api".into()),
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: Some("alloc::alloc::dealloc".into()),
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract.clone())).unwrap();
        contract.callee_def_path = None;
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires producer audit field callee_def_path"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_rejects_missing_basis() {
        let contract = AllocationContract {
            family: "unknown".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        };
        let err = Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap_err();
        assert!(err.contains("requires proof field 'basis'"), "unexpected error: {err}");
    }

    #[test]
    fn allocation_contracts_v2_accepts_fail_closed_unresolved_drop() {
        let contract = AllocationContract {
            family: "unknown".into(),
            operation: "drop".into(),
            language: "rust".into(),
            basis: Some("unresolved".into()),
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        };
        Kripke::from_annotated_icfg(v2_contract_test_input(contract)).unwrap();
    }

    #[test]
    fn panic_unwind_capability_requires_mir_v2() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["panic_unwind_lifecycle_v1".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("panic_unwind_lifecycle_v1"), "unexpected error: {err}");
    }

    #[test]
    fn panic_lifecycle_repeat_drop_may_is_unknown() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "mir_semantic_labels_v1".into(),
            "mir_semantics_v2".into(),
            "panic_unwind_lifecycle_v1".into(),
            "panic_lifecycle_state_v1".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        let mut overlay = PanicLifecycleOverlay::new();
        overlay.insert(
            "b0".to_string(),
            vec![PanicLifecycleRecord {
                allocation: "A".into(),
                certainty: AllocationEventCertainty::MayAbstract,
                may_own: true,
                may_partial_drop: true,
                may_stale_owner: true,
                may_committed: false,
                may_complete: false,
            }],
        );
        let k = Kripke::from_annotated_icfg_with_panic_lifecycle(input, overlay).unwrap();
        assert_eq!(k.allocation_may_hold("b0", "A", MayPredicate::RepeatDrop), Truth::Unknown);
    }

    #[test]
    fn panic_lifecycle_repeat_drop_absence_is_false_when_coverage_is_complete() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "mir_semantic_labels_v1".into(),
            "mir_semantics_v2".into(),
            "panic_unwind_lifecycle_v1".into(),
            "panic_lifecycle_state_v1".into(),
            "panic_lifecycle_state_v2".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        let mut overlay = PanicLifecycleOverlay::new();
        overlay.insert("b0".to_string(), Vec::new());
        overlay.set_coverage("b0", PanicLifecycleCoverage::Complete);
        let k = Kripke::from_annotated_icfg_with_panic_lifecycle(input, overlay).unwrap();
        assert_eq!(k.allocation_may_hold("b0", "A", MayPredicate::RepeatDrop), Truth::False);
    }

    #[test]
    fn panic_lifecycle_repeat_drop_unresolved_coverage_is_unknown() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "mir_semantic_labels_v1".into(),
            "mir_semantics_v2".into(),
            "panic_unwind_lifecycle_v1".into(),
            "panic_lifecycle_state_v1".into(),
            "panic_lifecycle_state_v2".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        let mut overlay = PanicLifecycleOverlay::new();
        overlay.insert("b0".to_string(), Vec::new());
        overlay.set_coverage("b0", PanicLifecycleCoverage::Unresolved);
        let k = Kripke::from_annotated_icfg_with_panic_lifecycle(input, overlay).unwrap();
        assert_eq!(k.allocation_may_hold("b0", "A", MayPredicate::RepeatDrop), Truth::Unknown);
    }

    #[test]
    fn entry_projection_preserves_reachable_panic_lifecycle_sidecar() {
        let mut input = base();
        input.schema_version = 2;
        input.entry = "rust::main::bb0".into();
        input.capabilities = vec![
            "mir_semantic_labels_v1".into(),
            "mir_semantics_v2".into(),
            "panic_unwind_lifecycle_v1".into(),
            "panic_lifecycle_state_v1".into(),
        ];
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        let empty_node = |id: &str, successors: Vec<String>| AnnotatedNode {
            id: id.into(), successors, labels: vec![], semantic_labels: vec![],
            allocation_labels: vec![], allocation_disposition: vec![], identity: None,
            event_identity: None, allocation_post: None,
            pre: AbstractMemoryAnnotation::default(), post: AbstractMemoryAnnotation::default(),
        };
        input.nodes = vec![
            empty_node("rust::main::bb0", vec!["rust::main::bb1".into()]),
            empty_node("rust::main::bb1", vec![]),
            empty_node("rust::dead::bb0", vec![]),
        ];
        let record = PanicLifecycleRecord {
            allocation: "A".into(),
            certainty: AllocationEventCertainty::MayAbstract,
            may_own: true, may_partial_drop: true, may_stale_owner: true,
            may_committed: false, may_complete: false,
        };
        let mut overlay = PanicLifecycleOverlay::new();
        overlay.insert("rust::main::bb0".to_string(), vec![record.clone()]);
        overlay.insert("rust::main::bb1".to_string(), vec![record]);
        overlay.insert("rust::dead::bb0".to_string(), Vec::new());
        let k = Kripke::from_annotated_icfg_with_panic_lifecycle(input, overlay).unwrap();
        let projected = k.project_from_entry("main", false).unwrap();
        assert!(projected.panic_lifecycle.contains_key("rust::main::bb0"));
        assert!(projected.panic_lifecycle.contains_key("rust::main::bb1"));
        assert!(!projected.panic_lifecycle.contains_key("rust::dead::bb0"));
        assert_eq!(
            projected.allocation_may_hold("rust::main::bb1", "A", MayPredicate::RepeatDrop),
            Truth::Unknown
        );
    }

    #[test]
    fn bcontract_nd1_accepts_closed_leaf_negative_certificate() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["external_deallocation_effects_v1".into()];
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "touch_second".into(),
            status: ExternalDeallocationEffectStatus::CertifiedAbsent,
            basis: "svf_leaf_no_call_deallocation_v1".into(),
            corroborating_bases: vec![],
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(
            k.external_deallocation_effect_at("dummyCall::x").unwrap().status,
            ExternalDeallocationEffectStatus::CertifiedAbsent
        );
    }

    #[test]
    fn r2_accepts_closed_ffi_argument_identity_certificate() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["ffi_argument_identity_v1".into()];
        input.entry = "dummyCall::x".into();
        input.nodes[0].id = "dummyCall::x".into();
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        input.ffi_argument_identity = vec![FfiArgumentIdentityRecord {
            node: "dummyCall::x".into(),
            callee: "c_free_i32".into(),
            callsite: "rust::main::bb3".into(),
            arg_index: 0,
            actual_variable: "rust::x".into(),
            formal_variable: "c::p".into(),
            allocations: vec!["A".into()],
            certainty: "may_abstract".into(),
            basis: "crema_bmulti_actual_formal_identity_v1".into(),
            formal_mapping_basis: "svf_formal_arg_index_v1".into(),
            svf_may_points_to: vec![],
            svf_points_to_basis: Some("svf_andersen_wave_diff_may_v1".into()),
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        let rows = k.ffi_argument_identity_at("dummyCall::x").collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].arg_index, 0);
        assert_eq!(rows[0].allocations, vec!["A"]);
    }

    #[test]
    fn r2_rejects_ffi_identity_capability_payload_mismatch() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["ffi_argument_identity_v1".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("ffi_argument_identity_v1 capability and evidence records"));
    }

    #[test]
    fn r2_rejects_nonempty_svf_may_set_without_andersen_basis() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["ffi_argument_identity_v1".into()];
        input.entry = "dummyCall::x".into();
        input.nodes[0].id = "dummyCall::x".into();
        input.allocations = vec![AbstractAllocation {
            id: "A".into(), display: None, site: None, context: vec![], allocator_contract: None,
        }];
        input.ffi_argument_identity = vec![FfiArgumentIdentityRecord {
            node: "dummyCall::x".into(), callee: "f".into(), callsite: "rust::main::bb0".into(),
            arg_index: 0, actual_variable: "rust::x".into(), formal_variable: "c::p".into(),
            allocations: vec!["A".into()], certainty: "may_abstract".into(),
            basis: "crema_bmulti_actual_formal_identity_v1".into(),
            formal_mapping_basis: "svf_formal_arg_index_v1".into(),
            svf_may_points_to: vec![6], svf_points_to_basis: None,
        }];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("nonempty SVF MAY set lacks Andersen basis"));
    }

    #[test]
    fn bcontract_nd1_rejects_invalid_negative_certificate_basis() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["external_deallocation_effects_v1".into()];
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "touch_second".into(),
            status: ExternalDeallocationEffectStatus::CertifiedAbsent,
            basis: "unresolved".into(),
            corroborating_bases: vec![],
        }];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("invalid external deallocation-effect tuple"));
    }

    #[test]
    fn efx1_external_effect_basis_requires_llvm_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["external_deallocation_effects_v1".into()];
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "free".into(),
            status: ExternalDeallocationEffectStatus::ObservedMayDeallocate,
            basis: "llvm16_tli_allockind_deallocation_v1".into(),
            corroborating_bases: vec![],
        }];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("requires artifact capability llvm_memory_effects_v1"));
    }

    #[test]
    fn efx1_capability_requires_embedded_payload() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["llvm_memory_effects_v1".into()];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("capability and embedded evidence must appear together"));
    }

    #[test]
    fn efx1_embedded_payload_requires_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.llvm_memory_effects = Some(valid_efx1_evidence());
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("capability and embedded evidence must appear together"));
    }

    #[test]
    fn bpta_capability_and_payload_are_atomic() {
        let mut missing = base();
        missing.schema_version = 2;
        missing.capabilities = vec!["svf_solved_points_to_v1".into()];
        assert!(Kripke::from_annotated_icfg(missing).unwrap_err().contains("capability and embedded evidence must appear together"));

        let mut naked = base();
        naked.schema_version = 2;
        naked.svf_solved_points_to = Some(valid_pts_evidence());
        assert!(Kripke::from_annotated_icfg(naked).unwrap_err().contains("capability and embedded evidence must appear together"));
    }

    #[test]
    fn efx1_external_effect_basis_is_accepted_with_llvm_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "external_deallocation_effects_v1".into(),
            "llvm_memory_effects_v1".into(),
        ];
        input.llvm_memory_effects = Some(valid_efx1_evidence());
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "free".into(),
            status: ExternalDeallocationEffectStatus::ObservedMayDeallocate,
            basis: "llvm16_tli_allockind_deallocation_v1".into(),
            corroborating_bases: vec![],
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(
            k.external_deallocation_effect_at("dummyCall::x").unwrap().status,
            ExternalDeallocationEffectStatus::ObservedMayDeallocate
        );
    }


    #[test]
    fn r2_external_effect_accepts_sorted_llvm_corroboration() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec![
            "external_deallocation_effects_v1".into(),
            "llvm_memory_effects_v1".into(),
        ];
        input.llvm_memory_effects = Some(valid_efx1_evidence());
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "wrapper".into(),
            status: ExternalDeallocationEffectStatus::ObservedMayDeallocate,
            basis: "structural_c_free_v1".into(),
            corroborating_bases: vec![
                "llvm16_tli_direct_callee_allockind_deallocation_v1".into(),
            ],
        }];
        let k = Kripke::from_annotated_icfg(input).unwrap();
        assert_eq!(
            k.external_deallocation_effect_at("dummyCall::x")
                .unwrap()
                .corroborating_bases,
            vec!["llvm16_tli_direct_callee_allockind_deallocation_v1"]
        );
    }

    #[test]
    fn r2_external_effect_rejects_corroboration_without_llvm_capability() {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities = vec!["external_deallocation_effects_v1".into()];
        input.nodes[0].id = "dummyCall::x".into();
        input.entry = "dummyCall::x".into();
        input.external_deallocation_effects = vec![ExternalDeallocationEffectRecord {
            node: "dummyCall::x".into(),
            callee: "wrapper".into(),
            status: ExternalDeallocationEffectStatus::ObservedMayDeallocate,
            basis: "structural_c_free_v1".into(),
            corroborating_bases: vec![
                "llvm16_tli_direct_callee_allockind_deallocation_v1".into(),
            ],
        }];
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("corroborating basis"));
        assert!(err.contains("requires artifact capability llvm_memory_effects_v1"));
    }

    fn typed_edge_test_input() -> AnnotatedIcfg {
        let mut input = base();
        input.schema_version = 2;
        input.capabilities.push("typed_edge_flow_v1".into());
        input.nodes[0].successors = vec!["b1".into()];
        let mut b1 = input.nodes[0].clone();
        b1.id = "b1".into();
        b1.successors.clear();
        b1.labels.clear();
        input.nodes.push(b1);
        input
    }

    #[test]
    fn typed_edge_flow_accepts_exact_legacy_projection_without_changing_traversal() {
        let input = typed_edge_test_input();
        let edges = vec![TypedEdgeRecord {
            source: "b0".into(),
            destination: "b1".into(),
            flow: TypedEdgeFlow::Unwind,
            label: Some("Call unwind".into()),
            source_label: Some("call".into()),
            destination_label: Some("cleanup".into()),
        }];
        let k = Kripke::from_annotated_icfg_with_overlays(
            input,
            PanicLifecycleOverlay::new(),
            Some(edges.clone()),
        )
        .unwrap();
        assert_eq!(k.typed_edges, edges);
        assert_eq!(k.nodes["b0"].successors, vec!["b1".to_string()]);
    }

    #[test]
    fn typed_edge_flow_rejects_projection_mismatch() {
        let input = typed_edge_test_input();
        let err = Kripke::from_annotated_icfg_with_overlays(
            input,
            PanicLifecycleOverlay::new(),
            Some(vec![]),
        )
        .unwrap_err();
        assert!(err.contains("projection mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn typed_edge_flow_rejects_flow_label_disagreement() {
        let input = typed_edge_test_input();
        let err = Kripke::from_annotated_icfg_with_overlays(
            input,
            PanicLifecycleOverlay::new(),
            Some(vec![TypedEdgeRecord {
                source: "b0".into(),
                destination: "b1".into(),
                flow: TypedEdgeFlow::Normal,
                label: Some("Call unwind".into()),
                source_label: None,
                destination_label: None,
            }]),
        )
        .unwrap_err();
        assert!(err.contains("flow/label mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn typed_edge_flow_requires_payload_when_capability_is_declared() {
        let input = typed_edge_test_input();
        let err = Kripke::from_annotated_icfg(input).unwrap_err();
        assert!(err.contains("capability and typed_edges payload"), "unexpected error: {err}");
    }

}
