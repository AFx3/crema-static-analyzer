#[warn(non_snake_case)]
use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, HashMap};


// -----------------------
// CANONICAL PROGRAM / ALLOCATION IDENTITY
// -----------------------

/// Globally scoped program-variable identity used by the interprocedural
/// allocation-identity analysis.  This is deliberately distinct from the
/// historical `Name = String` representation used by the legacy detector.
///
/// Scientific invariant:
/// two MIR locals with the same `_N` index but different function scopes are
/// different `ProgramVarId`s.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgramVarId {
    Rust {
        function: String,
        local: u32,
    },
    C {
        function: String,
        var_id: usize,
        callsite: Option<String>,
    },
    Synthetic {
        scope: String,
        name: String,
    },
}

impl ProgramVarId {
    /// Parse `_3`, `Local(_3)` or `Local(_3) [mutable]` in the supplied
    /// function scope.  Returns None for non-MIR names.
    pub fn rust(function: impl Into<String>, raw: &str) -> Option<Self> {
        let local = parse_mir_local_index(raw)?;
        Some(Self::Rust {
            function: function.into(),
            local,
        })
    }

    pub fn c(
        function: impl Into<String>,
        var_id: usize,
        callsite: Option<String>,
    ) -> Self {
        Self::C {
            function: function.into(),
            var_id,
            callsite,
        }
    }

    /// Stable human-readable spelling for diagnostics / JSON boundaries.
    pub fn canonical_string(&self) -> String {
        match self {
            Self::Rust { function, local } => {
                format!("rust::{function}::Local(_{local})")
            }
            Self::C {
                function,
                var_id,
                callsite,
            } => match callsite {
                Some(callsite) => {
                    format!("c::{function}::svf({var_id})@{callsite}")
                }
                None => format!("c::{function}::svf({var_id})"),
            },
            Self::Synthetic { scope, name } => {
                format!("synthetic::{scope}::{name}")
            }
        }
    }
}

/// Extract the numeric MIR local index without assigning any function scope.
pub fn parse_mir_local_index(raw: &str) -> Option<u32> {
    let raw = raw.trim();

    let start = if let Some(pos) = raw.find("Local(_") {
        pos + "Local(_".len()
    } else if let Some(pos) = raw.find('_') {
        pos + 1
    } else {
        return None;
    };

    let digits: String = raw[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Finite abstract allocation-site identity.
///
/// This is an identity in the chosen abstraction, not a claim that one static
/// allocation site corresponds to exactly one concrete runtime object.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AbstractAllocId {
    pub site: AllocationSiteId,
    /// Bounded abstract context.  Phase 6A stores the representation and
    /// validates its semantics; subsequent phases populate it from matched
    /// callsites (initially a 1-callsite string).
    pub context: Vec<String>,
}

impl AbstractAllocId {
    pub fn new(site: AllocationSiteId, context: Vec<String>) -> Self {
        Self { site, context }
    }

    pub fn canonical_string(&self) -> String {
        let site = match &self.site {
            AllocationSiteId::RustCall {
                node_id,
                callee,
            } => format!("rust-call:{node_id}:{callee}"),
            AllocationSiteId::CCall {
                node_id,
                allocator,
            } => format!("c-call:{node_id}:{allocator}"),
            AllocationSiteId::Synthetic { scope, label } => {
                format!("synthetic:{scope}:{label}")
            }
        };

        if self.context.is_empty() {
            site
        } else {
            format!("{site}::ctx[{}]", self.context.join("->"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AllocationSiteId {
    RustCall {
        node_id: String,
        callee: String,
    },
    CCall {
        node_id: String,
        allocator: String,
    },
    Synthetic {
        scope: String,
        label: String,
    },
}

/// Canonical MIR place identity.  `base` is globally scoped and projections
/// are explicit rather than encoded into ad-hoc strings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlaceId {
    pub base: ProgramVarId,
    pub projection: Vec<PlaceProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceProjection {
    Deref,
    Field { index: u32 },
    Index { local: ProgramVarId },
    Opaque { text: String },
}

/// F6C1A operational metadata only. Definition/instance identities are obtained
/// from rustc, never reconstructed from the diagnostic ICFG node spelling.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RustDropNodeIdentity {
    pub definition_id: String,
    pub instance_id: Option<String>,
    pub basic_block: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RustDropOwnerPlace {
    Supported { place: PlaceId },
    Unresolved { reason: RustDropOwnerPlaceUnresolved },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RustDropOwnerPlaceUnresolved {
    MissingLocalDeclaration,
    UnsupportedProjection { position: usize, projection_kind: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustDropOwnerPlaceRecord {
    pub drop_identity: RustDropNodeIdentity,
    /// Explicit binding to the existing producer function/ICFG key. This is
    /// copied at construction, not parsed to recover structural identity.
    pub function_key: String,
    pub node_id: String,
    pub original_local: u32,
    pub owner: RustDropOwnerPlace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustDropOwnerPlaceMetadataV1 {
    pub schema_version: u32,
    pub records: Vec<RustDropOwnerPlaceRecord>,
}

impl RustDropOwnerPlaceMetadataV1 {
    pub fn new(mut records: Vec<RustDropOwnerPlaceRecord>) -> Result<Self, String> {
        records.sort_by(|a, b| a.drop_identity.cmp(&b.drop_identity));
        let mut nodes = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        for record in &records {
            if !nodes.insert(record.node_id.clone()) || !keys.insert(record.drop_identity.clone()) {
                return Err("duplicate Rust Drop owner metadata".into());
            }
            if let RustDropOwnerPlace::Supported { place } = &record.owner {
                if place.base != (ProgramVarId::Rust {
                    function: record.function_key.clone(), local: record.original_local,
                }) {
                    return Err("Rust Drop owner metadata base mismatch".into());
                }
            }
        }
        Ok(Self { schema_version: 1, records })
    }

    /// Exact coverage of represented MIR Drop nodes. Unrepresented extraction
    /// records are excluded; a missing record is an error, never guessed from
    /// dropped_value or labels. The table is deliberately not a field of the
    /// GlobalICFGOrdered type consumed by CQPL/identity.
    pub fn for_graph(&self, graph: &GlobalICFGOrdered) -> Result<Self, String> {
        let by_node: BTreeMap<_, _> = self.records.iter().map(|r| (r.node_id.as_str(), r)).collect();
        let mut selected = Vec::new();
        for (node_id, node) in &graph.ordered_nodes {
            if let GlobalICFGNode::Mir(bb) = node {
                if matches!(bb.terminator, Some(MirTerminator::Drop { .. })) {
                    let record = by_node.get(node_id.as_str())
                        .ok_or_else(|| format!("missing structural Rust Drop owner metadata: {node_id}"))?;
                    if record.drop_identity.basic_block != bb.block_id {
                        return Err("Rust Drop owner metadata block mismatch".into());
                    }
                    selected.push((*record).clone());
                }
            }
        }
        Self::new(selected)
    }
}

/// Function metadata needed for callsite-matched interprocedural analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustFunctionMetadata {
    pub name: String,
    pub arg_count: usize,
    pub entry_node: String,
    pub return_nodes: Vec<String>,
}

/// Explicit Rust->Rust callsite metadata.
///
/// Phase 6A records this information in the ICFG without changing legacy
/// fixed-point semantics.  Phase 6B consumes it to replace the global
/// `call_stack.pop()` protocol with matched actual/formal and return bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustCallMetadata {
    pub caller_function: String,
    pub call_node: String,
    pub callee_function: String,
    pub dummy_call_node: String,
    pub dummy_ret_node: String,
    pub arguments: Vec<MirCallArgument>,
    pub return_place: String,
    pub return_node: String,
    pub is_closure: bool,
}

/// Internal higher-order activation policy already decided by the MIR producer.
/// Moved from icfg.rs so stable activation provenance reuses the same enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HigherOrderCallSemantics {
    LazyCarrier,
    RepeatedCallbacks,
    SpawnMayOnce,
    ConditionalCallbackOnce,
    Unknown,
}

/// Existing callsite/proxy identity, with no parsing of opaque node IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustActivationIdentity {
    pub caller_function: String,
    pub call_node: String,
    pub dummy_call_node: String,
    pub callee_function: String,
}

impl From<&RustCallMetadata> for RustActivationIdentity {
    fn from(call: &RustCallMetadata) -> Self {
        Self { caller_function: call.caller_function.clone(), call_node: call.call_node.clone(),
            dummy_call_node: call.dummy_call_node.clone(), callee_function: call.callee_function.clone() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustActivationKind { Direct, SynchronousCallback, DetachedCallback, Unresolved }

/// Stable projection of MIR UnwindAction. Existing MirTerminator stores its
/// normalized action as a string; rustc's BB-indexed enum is not serde wire data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RustActivationUnwindAction {
    Cleanup { target: String }, Continue, Terminate, Unreachable,
}

impl RustActivationUnwindAction {
    pub fn from_mir_action(action: &str) -> Result<Self, String> {
        match action {
            "continue" => Ok(Self::Continue), "terminate" => Ok(Self::Terminate),
            "unreachable" => Ok(Self::Unreachable),
            _ => {
                let target = action.strip_prefix("cleanup(").and_then(|s| s.strip_suffix(')'));
                match target {
                    Some(bb) if bb.strip_prefix("bb").is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) =>
                        Ok(Self::Cleanup { target: bb.into() }),
                    _ => Err("unsupported structured MIR unwind action".into()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RustActivationMetadataSource {
    MirCall, MirDrop,
    HigherOrderPolicy { policy: HigherOrderCallSemantics },
    ExecutionActivation { call_key: String },
    MissingPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustActivationUnwindEdge {
    pub source_node: String,
    pub destination_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustActivationUnresolvedReason {
    MissingCallbackPolicy, MissingCallbackUnwindEvidence, UnsupportedMirUnwindAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RustActivationUnwindSemantics {
    Direct { action: RustActivationUnwindAction },
    CertifiedCallback { action: RustActivationUnwindAction, unwind_edges: Vec<RustActivationUnwindEdge> },
    DetachedNoCallerUnwind,
    Unresolved { reason: RustActivationUnresolvedReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustActivationUnwindMetadata {
    pub activation: RustActivationIdentity,
    pub activation_kind: RustActivationKind,
    pub source: RustActivationMetadataSource,
    pub unwind: RustActivationUnwindSemantics,
}

impl RustActivationUnwindMetadata {
    pub fn direct(call: &RustCallMetadata, action: &str, is_drop: bool) -> Self {
        let unwind = match RustActivationUnwindAction::from_mir_action(action) {
            Ok(action) => RustActivationUnwindSemantics::Direct { action },
            Err(_) => RustActivationUnwindSemantics::Unresolved {
                reason: RustActivationUnresolvedReason::UnsupportedMirUnwindAction },
        };
        Self { activation: call.into(), activation_kind: RustActivationKind::Direct,
            source: if is_drop { RustActivationMetadataSource::MirDrop } else { RustActivationMetadataSource::MirCall }, unwind }
    }

    /// The provider's MIR unwind is deliberately not an input to this factory.
    pub fn callback(call: &RustCallMetadata, policy: Option<HigherOrderCallSemantics>) -> Self {
        let (activation_kind, unwind) = match policy {
            Some(HigherOrderCallSemantics::SpawnMayOnce) =>
                (RustActivationKind::DetachedCallback, RustActivationUnwindSemantics::DetachedNoCallerUnwind),
            Some(HigherOrderCallSemantics::RepeatedCallbacks | HigherOrderCallSemantics::ConditionalCallbackOnce) =>
                (RustActivationKind::SynchronousCallback, RustActivationUnwindSemantics::Unresolved {
                    reason: RustActivationUnresolvedReason::MissingCallbackUnwindEvidence }),
            _ => (RustActivationKind::Unresolved, RustActivationUnwindSemantics::Unresolved {
                reason: RustActivationUnresolvedReason::MissingCallbackPolicy }),
        };
        Self { activation: call.into(), activation_kind,
            source: policy.map(|policy| RustActivationMetadataSource::HigherOrderPolicy { policy })
                .unwrap_or(RustActivationMetadataSource::MissingPolicy), unwind }
    }
}

/// Operational producer metadata only. Serialized beside the existing raw
/// ICFG fields across the rustc-wrapper boundary; never added to CQPL export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustActivationUnwindMetadataV1 {
    pub schema_version: u32,
    pub activations: Vec<RustActivationUnwindMetadata>,
}

impl RustActivationUnwindMetadataV1 {
    pub fn new(records: impl IntoIterator<Item = RustActivationUnwindMetadata>) -> Result<Self, String> {
        let mut by_activation = BTreeMap::new();
        for record in records {
            let coherent = matches!((&record.activation_kind, &record.unwind),
                (RustActivationKind::Direct, RustActivationUnwindSemantics::Direct { .. })
                | (RustActivationKind::Direct, RustActivationUnwindSemantics::Unresolved {
                    reason: RustActivationUnresolvedReason::UnsupportedMirUnwindAction })
                | (RustActivationKind::SynchronousCallback, RustActivationUnwindSemantics::CertifiedCallback { .. })
                | (RustActivationKind::SynchronousCallback, RustActivationUnwindSemantics::Unresolved {
                    reason: RustActivationUnresolvedReason::MissingCallbackUnwindEvidence })
                | (RustActivationKind::DetachedCallback, RustActivationUnwindSemantics::DetachedNoCallerUnwind)
                | (RustActivationKind::Unresolved, RustActivationUnwindSemantics::Unresolved {
                    reason: RustActivationUnresolvedReason::MissingCallbackPolicy }));
            if !coherent { return Err("activation kind conflicts with unwind semantics".into()); }
            let source_kind = match &record.source {
                RustActivationMetadataSource::MirCall | RustActivationMetadataSource::MirDrop => RustActivationKind::Direct,
                RustActivationMetadataSource::HigherOrderPolicy { policy } => match policy {
                    HigherOrderCallSemantics::SpawnMayOnce => RustActivationKind::DetachedCallback,
                    HigherOrderCallSemantics::RepeatedCallbacks | HigherOrderCallSemantics::ConditionalCallbackOnce =>
                        RustActivationKind::SynchronousCallback,
                    _ => RustActivationKind::Unresolved,
                },
                RustActivationMetadataSource::MissingPolicy => RustActivationKind::Unresolved,
                RustActivationMetadataSource::ExecutionActivation { call_key } => {
                    if call_key.is_empty() { return Err("missing producer activation key".into()); }
                    record.activation_kind
                }
            };
            if source_kind != record.activation_kind { return Err("activation kind conflicts with producer policy".into()); }
            if [&record.activation.caller_function, &record.activation.call_node,
                &record.activation.dummy_call_node, &record.activation.callee_function].iter().any(|s| s.is_empty()) {
                return Err("empty activation identity".into());
            }
            if let RustActivationUnwindSemantics::CertifiedCallback { action, unwind_edges } = &record.unwind {
                if !matches!(record.source, RustActivationMetadataSource::ExecutionActivation { .. }) {
                    return Err("callback unwind certification requires activation relation evidence".into());
                }
                if unwind_edges.is_empty() || !unwind_edges.windows(2).all(|pair| pair[0] < pair[1])
                    || matches!(action, RustActivationUnwindAction::Unreachable) {
                    return Err("invalid certified callback unwind relations".into());
                }
            }
            if let Some(previous) = by_activation.insert(record.activation.clone(), record.clone()) {
                if previous != record { return Err("conflicting activation unwind metadata".into()); }
            }
        }
        Ok(Self { schema_version: 1, activations: by_activation.into_values().collect() })
    }

    pub fn validate_calls(&self, calls: &[RustCallMetadata]) -> Result<(), String> {
        let expected: BTreeMap<_, _> = calls.iter().map(|c| (RustActivationIdentity::from(c), c)).collect();
        if expected.len() != calls.len() || expected.len() != self.activations.len()
            || self.activations.iter().any(|r| !expected.contains_key(&r.activation)) {
            return Err("activation metadata is not one-to-one with rust_calls".into());
        }
        Ok(())
    }

    /// Explicit operational importer for later producer consumers. Legacy
    /// GlobalICFGOrdered serde intentionally ignores the additive metadata.
    pub fn from_operational_json(raw: &serde_json::Value) -> Result<Option<Self>, String> {
        if raw.get("dependency_body_ingestion_v1")
            .and_then(|dep| dep.get("context_execution_graph_v1")).is_some_and(|value| value.is_object()) {
            let graph: GlobalICFGOrdered = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            return crate::execution_graph::rust_activation_unwind_metadata_from_execution_graph(&graph);
        }
        let Some(value) = raw.get("rust_activation_unwind_metadata_v1") else { return Ok(None); };
        let table: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if table.schema_version != 1 { return Err("unsupported internal activation metadata version".into()); }
        let canonical = Self::new(table.activations.clone())?;
        if table.activations.iter().any(|r| matches!(r.source, RustActivationMetadataSource::ExecutionActivation { .. })) {
            return Err("execution activation evidence requires the existing execution ledger".into());
        }
        if table != canonical { return Err("activation metadata must be canonical and unique".into()); }
        let calls: Vec<RustCallMetadata> = serde_json::from_value(raw.get("rust_calls").cloned()
            .ok_or("missing rust_calls for activation metadata")?).map_err(|e| e.to_string())?;
        table.validate_calls(&calls)?;
        Ok(Some(table))
    }
}

// -----------------------
// RUST MIR
// -----------------------

/// P3-R1P producer-certified evidence that a MIR rvalue extracts the data
/// pointer of an already-tracked owned Box value through rustc-certified
/// representation-preserving field projections.
///
/// The consumer MUST match only `kind` and the structured source local/place.
/// Diagnostic strings are audit provenance and are never proof.  In
/// particular, no consumer may recognize `Box`, `Unique`, `NonNull`, `_6`, or
/// `drop_in_place` from rendered text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustPlaceRelationEvidenceKind {
    /// `CopyForDeref` reads the raw data pointer reached from rustc's
    /// `owned_box` lang item, then only through field-0 `repr(transparent)`
    /// wrappers, ending in a raw pointer to the same Box element type.
    OwnedBoxDataPointerCopyForDeref,
    /// &*p / &raw *p preserves the pointer referent, not the pointer slot.
    PointerReborrow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustPlaceRelationEvidence {
    pub kind: RustPlaceRelationEvidenceKind,
    /// MIR local at the base of the producer-certified source Place.
    pub source_local: u32,
    /// Direct destination local receiving the certified raw pointer.
    pub destination_local: u32,
    /// Exact supported source projection.  P3-R1P currently emits only
    /// `Deref` followed by field projections certified by rustc.
    pub source_projection: Vec<PlaceProjection>,
    /// Audit-only canonical DefPath after structural lang-item matching.
    pub owner_def_path: String,
    /// Audit-only rendered types. Consumers MUST NOT infer semantics from them.
    pub source_type_diagnostic: String,
    pub result_type_diagnostic: String,
    /// Number of transparent wrapper fields after the Box payload field.
    pub transparent_wrapper_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirStatement {
    pub source_info: SourceInfoData,  
    pub kind: String,             
    pub details: String,          
    pub place: Option<String>,
    pub is_mutable: Option<bool>, // if the place is mutable
    pub rvalue: Option<String>,
    /// Optional typed producer evidence for the P3-R1P Place/resource gate.
    /// Old artifacts deserialize with `None`; when absent, identity transfer
    /// retains the committed R10 behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_place_relation_v1: Option<RustPlaceRelationEvidence>,
}
// new struct to represent a call argument and its mutability
/// Rustc-certified std ManuallyDrop payload borrow. The identity domain erases
/// ManuallyDrop::new's transparent shell; projection within T remains intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManuallyDropBorrowEvidence {
    pub receiver_local: u32,
    pub destination_local: u32,
    pub owner_def_path: String,
    pub trait_def_path: String,
    pub payload_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirCallArgument {
    pub arg: String,
    // if the argument comes from a place, this flag indicates whether that place is mutable.
    // (for non-place arguments, this will be None)
    pub is_mutable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfoData {
    pub span: String, 
    pub scope: String, 
}

/// v6N producer-evidence object for refining a generic MIR `Drop` to the Rust global
/// allocator family.  The producer may emit this only from rustc semantic type
/// identity; consumers must never reconstruct it from pretty-printed type text.
///
/// Normative library contracts:
/// - Box memory layout: https://doc.rust-lang.org/std/boxed/index.html#memory-layout
/// - Vec memory layout: https://doc.rust-lang.org/std/vec/index.html#memory-layout
///
/// rustc identity mechanism:
/// - diagnostic items: https://rustc-dev-guide.rust-lang.org/diagnostics/diagnostic-items.html
/// - `owned_box` / `global_alloc_ty` lang items are exposed by LanguageItems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustDropAllocatorEvidenceKind {
    BoxGlobal,
    VecGlobal,
    /// `CString` stores its owned bytes in `Box<[u8]>`; the canonical
    /// `cstring_type` diagnostic item therefore has a Global-backed drop.
    CStringGlobal,
    /// `String` owns its byte buffer through the standard library's Global-
    /// backed `Vec<u8>` representation on the pinned toolchain.
    StringGlobal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustDropAllocatorEvidence {
    pub kind: RustDropAllocatorEvidenceKind,
    /// Diagnostic provenance only.  The classifier has already matched the
    /// rustc DefId structurally; the checker does not infer from these strings.
    pub owner_def_path: String,
    pub allocator_def_path: String,
}

/// Producer-side evidence for an explicit Rust deallocation call.
/// This proof object is emitted while rustc semantic identity and argument
/// types are still available; exporters/checkers must not reconstruct it from
/// pretty MIR text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustCallDeallocatorEvidenceKind {
    /// Public `alloc::alloc::dealloc` API.
    GlobalDeallocApi,
    /// Bcontract-DROP1: exact `core::mem::drop` whose moved argument is
    /// `Box<T, Global>`.  The producer proves both the Box owner identity and
    /// the Global allocator identity before emitting this kind.
    MemDropOwnedBoxGlobal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustCallDeallocatorEvidence {
    pub kind: RustCallDeallocatorEvidenceKind,
    /// Audit-only canonical DefPath after structural producer classification.
    pub callee_def_path: String,
    /// DROP1 typed-call provenance.  These fields are never interpreted as
    /// proof by consumers; they are serialized only after the producer has
    /// matched rustc identities structurally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_def_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocator_def_path: Option<String>,
}

/// v6S-r1 producer-certified ownership/disposition evidence.
///
/// This evidence is observational: it records API/type facts while rustc DefId
/// and argument types are still available.  It does not change the Phase-5
/// CellValue transfer semantics or the truth value of historical CQPL queries.
/// Consumers must not reconstruct these kinds from pretty-printed MIR strings.
///
/// Official Rust semantics used by the producer:
/// - `Box::into_raw` consumes the Box and leaves cleanup responsibility to the
///   caller: https://doc.rust-lang.org/std/boxed/struct.Box.html#method.into_raw
/// - `Box::from_raw` reconstructs Box ownership; the Box destructor will drop T
///   and free the allocation: https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw
/// - `CString::into_raw` transfers ownership to the caller and explicitly must
///   be paired with `CString::from_raw`, not C `free`:
///   https://doc.rust-lang.org/std/ffi/struct.CString.html#method.into_raw
/// - `CString::from_raw` retakes ownership of a pointer previously returned by
///   `CString::into_raw`:
///   https://doc.rust-lang.org/std/ffi/struct.CString.html#method.from_raw
/// - the official `alloc` source marks `CString` with rustc diagnostic item
///   `cstring_type` and stores its buffer as `Box<[u8]>`:
///   https://doc.rust-lang.org/src/alloc/ffi/c_str.rs.html
/// - dropping a raw pointer has no effect on the lifecycle of the pointee:
///   https://doc.rust-lang.org/reference/types/pointer.html#raw-pointers-const-and-mut
/// - `mem::forget` skips the destructor of the consumed value:
///   https://doc.rust-lang.org/std/mem/fn.forget.html
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustAllocationDispositionEvidenceKind {
    BoxIntoRaw,
    BoxFromRaw,
    BoxLeak,
    // Canonical B1.1 wire names. Use explicit Serde variant renames rather
    // than deriving protocol spelling from the Rust identifier.
    // https://serde.rs/variant-attrs.html
    #[serde(rename = "cstring_into_raw")]
    CStringIntoRaw,
    #[serde(rename = "cstring_from_raw")]
    CStringFromRaw,
    MemForgetOwnedBox,
    MemDropRawPointer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustAllocationDispositionEvidence {
    pub kind: RustAllocationDispositionEvidenceKind,
    /// Audit-only canonical DefPath.  Classification is completed before this
    /// string is serialized.
    pub callee_def_path: String,
    /// Exact rustc owner type when the evidence is tied to Box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_def_path: Option<String>,
}

/// AP1 producer-certified standard-library AtomicPtr operation.
///
/// The producer classifies these calls while rustc DefId/type identity is
/// available.  Consumers must not infer AtomicPtr semantics from pretty MIR
/// strings.  The closed operation set intentionally covers only operations
/// whose pointer-value effect is specified directly by the public API on the
/// pinned toolchain.
///
/// Official Rust semantics:
/// - `AtomicPtr::new(p)` creates an atomic pointer initialized with `p`;
/// - `load` returns the currently stored pointer without modifying it;
/// - `store` replaces the stored pointer;
/// - `swap` stores a new pointer and returns the previous pointer;
/// - `compare_exchange`/`compare_exchange_weak` return the previous pointer in
///   either Result variant, and write `new` only on success;
/// - `compare_and_swap` (deprecated but present on the pinned toolchain)
///   returns the previous pointer and conditionally writes `new`; and
/// - `into_inner` consumes the atomic and returns its contained pointer.
///
/// https://doc.rust-lang.org/std/sync/atomic/struct.AtomicPtr.html
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustAtomicPtrCallEvidenceKind {
    New,
    Load,
    Store,
    Swap,
    CompareExchange,
    CompareExchangeWeak,
    CompareAndSwap,
    IntoInner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustAtomicPtrCallEvidence {
    pub kind: RustAtomicPtrCallEvidenceKind,
    /// Audit-only canonical DefPath.  Semantic classification is already
    /// complete when this string is serialized.
    pub callee_def_path: String,
    /// Exact canonical owner DefPath proved by the producer.
    pub owner_def_path: String,
}

/// v6P-r1b producer-certified evidence for narrowly supported higher-order APIs.
/// Consumers must not reconstruct this classification from pretty-printed MIR
/// or from `tcx.def_path_str` rendering of inherent impl items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustHigherOrderCallEvidenceKind {
    /// `Option::map`: callback is invoked iff the receiver is `Some`.
    OptionMap,
    /// `Option::map_or`: callback is invoked iff the receiver is `Some`.
    OptionMapOr,
    /// `Option::map_or_else`: exactly one of the two callbacks is invoked.
    OptionMapOrElse,
    /// `Option::and_then`: callback is invoked iff the receiver is `Some`.
    OptionAndThen,
    /// `Option::filter`: predicate is invoked iff the receiver is `Some`.
    OptionFilter,
    /// `Option::inspect`: callback is invoked iff the receiver is `Some`.
    OptionInspect,
    /// `Option::or_else`: callback is invoked iff the receiver is `None`.
    OptionOrElse,
    /// `Option::unwrap_or_else`: callback is invoked iff the receiver is `None`.
    OptionUnwrapOrElse,
    /// `Option::ok_or_else`: callback is invoked iff the receiver is `None`.
    OptionOkOrElse,
    /// `Option::is_some_and`: predicate is invoked iff the receiver is `Some`.
    OptionIsSomeAnd,
    /// `Option::is_none_or`: predicate is invoked iff the receiver is `Some`.
    OptionIsNoneOr,
    /// `Result::map`: callback is invoked iff the receiver is `Ok`.
    ResultMap,
    /// `Result::map_err`: callback is invoked iff the receiver is `Err`.
    ResultMapErr,
    /// `Result::map_or`: callback is invoked iff the receiver is `Ok`.
    ResultMapOr,
    /// `Result::map_or_else`: exactly one of the Err/Ok callbacks is invoked.
    ResultMapOrElse,
    /// `Result::and_then`: callback is invoked iff the receiver is `Ok`.
    ResultAndThen,
    /// `Result::or_else`: callback is invoked iff the receiver is `Err`.
    ResultOrElse,
    /// `Result::unwrap_or_else`: callback is invoked iff the receiver is `Err`.
    ResultUnwrapOrElse,
    /// `Result::inspect`: callback is invoked iff the receiver is `Ok`.
    ResultInspect,
    /// `Result::inspect_err`: callback is invoked iff the receiver is `Err`.
    ResultInspectErr,
    /// `Result::is_ok_and`: predicate is invoked iff the receiver is `Ok`.
    ResultIsOkAnd,
    /// `Result::is_err_and`: predicate is invoked iff the receiver is `Err`.
    ResultIsErrAnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustHigherOrderCallEvidence {
    pub kind: RustHigherOrderCallEvidenceKind,
    /// Audit-only exact rustc DefPath of the associated function.
    pub callee_def_path: String,
    /// Audit-only exact rustc DefPath of the self ADT certified by the producer.
    pub owner_def_path: String,
    /// MIR argument positions that are semantically callback-bearing for the
    /// certified API.  This is producer evidence, not reconstructed by the
    /// ICFG consumer.  `serde(default)` preserves historical artifact loading.
    #[serde(default)]
    pub callback_argument_indices: Vec<usize>,
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MirTerminator {
    Goto { details: String, source_info: String, target: String }, // 1
    SwitchInt { details: String, source_info: String, targets: Vec<String>, discr: String, otherwise: Option<String> }, //2
    UnwindResume { details: String, source_info: String }, //3
    /// Pinned rustc cleanup terminator: execution terminates rather than returning.
    #[serde(alias = "Terminate")]
    UnwindTerminate { details: String, source_info: String, reason: String },
    Return { details: String, source_info: String }, //5
    //4 Abort {details: String, source_info: String}, //4 -- it's a  CALL ALLA process::abort() Not KIND TERMINATOR as 1.86
    Unreachable { details: String, source_info: String }, //6
    Drop {
        details: String,
        source_info: String,
        return_target: String,
        unwind_target: String,
        dropped_value: String,
        is_mutable: bool,
        /// v6N: optional rustc-structural evidence for the allocator family
        /// used by this owning drop. Historical ICFG JSON remains readable.
        #[serde(default)]
        deallocator_evidence: Option<RustDropAllocatorEvidence>,
    }, //7
    //Call { details: String, source_info: String, function_called: String, arguments: Vec<String>, return_place: String, return_target: Option<String>, unwind_target: String }, //8
    // --- modified Call variant: note that arguments is now a Vec<MirCallArgument> ---
    Call {
        details: String,
        source_info: String,
        /// Human-readable rustc MIR spelling.  This remains diagnostic-only;
        /// control-flow resolution uses `callee_def_path` below.
        function_called: String,
        /// Canonical rustc DefPath for a constant FnDef call target, when one
        /// exists.  Unlike `function_called`, this excludes monomorphization
        /// pretty-print noise and is therefore suitable for matching local MIR
        /// bodies deterministically.
        #[serde(default)]
        callee_def_path: Option<String>,
        /// v6N-r1a: producer-side semantic evidence for supported explicit
        /// deallocator APIs. Historical ICFG JSON remains readable.
        #[serde(default)]
        deallocator_evidence: Option<RustCallDeallocatorEvidence>,
        /// v6S-r1: producer-certified ownership/disposition observation.
        /// This is deliberately separate from deallocation evidence: e.g.
        /// `drop(*mut T)` is a raw-pointer value drop and does not free T.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allocation_disposition_evidence: Option<RustAllocationDispositionEvidence>,
        /// AP1: producer-certified AtomicPtr pointer-storage semantics.
        /// Historical MIR/ICFG JSON remains readable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        atomic_ptr_evidence: Option<RustAtomicPtrCallEvidence>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manually_drop_borrow: Option<ManuallyDropBorrowEvidence>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        non_reclaiming_arguments: Vec<usize>,
        /// v6P-r1b: producer-certified structural evidence for supported
        /// higher-order API contracts. Historical MIR JSON remains readable.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        higher_order_evidence: Option<RustHigherOrderCallEvidence>,
        /// Whether rustc reports the FnDef itself as local to the analyzed
        /// crate.  A local target without a resolved MIR body is a fail-closed
        /// ICFG construction error in schema-v2 mode.
        #[serde(default)]
        callee_is_local: bool,
        /// Canonical local closure DefPaths occurring anywhere in the types of
        /// the call operands.  This is extracted structurally from rustc types,
        /// not from `{closure@...}` text embedded in Debug output.
        #[serde(default)]
        callback_def_paths: Vec<String>,
        /// v6L concrete monomorphized local targets observed by propagating
        /// rustc `Instance`s from the selected concrete entry. Sorted and
        /// deduplicated; multiple entries intentionally denote MAY fanout.
        #[serde(default)]
        resolved_instance_callees: Vec<String>,
        /// True when this generic callsite was visited in at least one reachable
        /// concrete caller Instance.
        #[serde(default)]
        instance_dispatch_observed: bool,
        /// At least one reachable concrete instantiation resolves to external
        /// code. Its summary/continuation branch must be retained.
        #[serde(default)]
        instance_dispatch_external: bool,
        /// At least one reachable concrete instantiation could not be resolved.
        /// Schema-v2 CQPL export must remain fail-closed in this case.
        #[serde(default)]
        instance_dispatch_unresolved: bool,
        arguments: Vec<MirCallArgument>,
        return_place: String,
        return_target: Option<String>,
        unwind_target: String,
    },
    /// Explicit tail call.  r1d preserves typed producer evidence and then
    /// fail-closes schema-v2 control flow until caller-pop semantics are modeled.
    TailCall {
        details: String,
        source_info: String,
        function_called: String,
        #[serde(default)]
        callee_def_path: Option<String>,
        #[serde(default)]
        callee_is_local: bool,
        arguments: Vec<MirCallArgument>,
    },
    Assert { details: String, source_info: String, return_target: String, unwind_target: String, cond: String, expected: bool, msg: String }, //9
    /// Coroutine suspension.  `resume_target` and `drop_target` are MAY
    /// continuations used by the conservative ICFG abstraction.
    Yield {
        details: String,
        source_info: String,
        resume_target: String,
        drop_target: Option<String>,
        resume_arg: String,
        value: String,
    },
    /// Semantically a return from coroutine drop glue in the pinned rustc MIR.
    CoroutineDrop { details: String, source_info: String },
    /// Borrow-checking-only terminators whose runtime semantics is one real goto.
    FalseEdge { details: String, source_info: String, real_target: String, imaginary_target: String },
    FalseUnwind { details: String, source_info: String, real_target: String, unwind_target: String },
    InlineAsm {
        details: String,
        source_info: String,
        template: Vec<String>,
        operands: Vec<String>,
        options: String,
        line_spans: Vec<String>,
        #[serde(default)]
        targets: Vec<String>,
        unwind_target: Option<String>,
    }, //10
    /// Deliberately fail-closed for pinned variants whose interprocedural
    /// semantics are not yet represented (notably TailCall).
    Unhandled { details: String, source_info: String },        
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirBasicBlock {
    pub block_id: usize,
    pub statements: Vec<MirStatement>,
    pub terminator: Option<MirTerminator>,
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirRepresentation {
    pub functions: BTreeMap<String, Vec<MirBasicBlock>>, // deterministic function name -> basic blocks
}
//whole mir as a mapping from function names to their corresponding basic blocks.

// -----------------------
// C LLVM IR 
// -----------------------

//(actually the svf icfg after Andersen's pta readen a json file)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmJsonNode {
    pub node_id: usize,
    pub node_type: bool,
    pub info: String,
    pub node_kind_string: String,
    pub node_kind: usize,
    pub node_source_loc: String,
    pub function_name: Option<String>,
    pub basic_block: Option<usize>,
    pub basic_block_name: Option<String>,
    pub basic_block_info: Option<String>,
    pub svf_statements: Vec<SvfStatement>,
    pub incoming_edges: Vec<LlvmEdge>,
    pub outgoing_edges: Vec<LlvmEdge>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvfPhiOperand {
    pub op_var_id: usize,
    /// ICFG node associated with this incoming Phi operand in the legacy
    /// SVF exporter schema.  It is not required for provenance propagation,
    /// but retaining it makes deserialization lossless for existing artifacts.
    pub icfg_node: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvfStatement {
    pub stmt_id: usize,
    pub stmt_type: String,
    pub stmt_info: String,
    pub edge_id: Option<usize>,
    pub pta_edge: Option<bool>,
    pub lhs_var_id: Option<usize>,
    pub rhs_var_id: Option<usize>,
    pub res_var_id: Option<usize>,
    pub operand_var_ids: Option<Vec<usize>>,
    /// Backward-compatible representation emitted by the pre-Phase-5
    /// `svf-ex.cpp` for PhiStmt.
    pub operand_vars: Option<Vec<SvfPhiOperand>>,
    pub call_inst: Option<String>,
    pub is_conditional: Option<bool>,
    pub condition_var_id: Option<usize>,
    pub successors: Option<Vec<BranchSuccessor>>,
}

impl SvfStatement {
    /// Canonical SSA result variable for statements whose producer schemas
    /// historically used either `lhs_var_id` or `res_var_id`.
    ///
    /// The Phase-5 consumer must accept both because existing SVF artifacts
    /// encode PhiStmt results as `res_var_id`, while the canonical exporter
    /// added by Phase 5 also emits `lhs_var_id`.
    pub fn result_var_id(&self) -> Option<usize> {
        self.lhs_var_id.or(self.res_var_id)
    }

    /// Canonical ordered list of operand VarIDs across both supported SVF
    /// JSON schemas. The canonical `operand_var_ids` field has precedence;
    /// `operand_vars[*].op_var_id` is a backward-compatibility fallback only.
    /// Duplicate IDs are removed without changing first-seen order.
    pub fn normalized_operand_var_ids(&self) -> Vec<usize> {
        let mut out = Vec::new();

        if let Some(ids) = &self.operand_var_ids {
            for id in ids {
                if !out.contains(id) {
                    out.push(*id);
                }
            }
            return out;
        }

        if let Some(operands) = &self.operand_vars {
            for operand in operands {
                if !out.contains(&operand.op_var_id) {
                    out.push(operand.op_var_id);
                }
            }
        }

        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSuccessor {
    pub successor_id: usize, // ID of successor node
    pub condition_value: Option<i64>, // Condition value (if applicable)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlvmEdge {
    pub source: usize,
    pub destination: usize,
    pub edge_type: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmMemoryAccessV1 {
    pub argmem: String,
    pub inaccessiblemem: String,
    pub other: String,
    #[serde(default)]
    pub encoded: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFormalEffectsSnapshotV1 {
    pub index: usize,
    #[serde(default)]
    pub pointer_typed: bool,
    #[serde(default)]
    pub nofree: bool,
    #[serde(default)]
    pub nocapture: bool,
    #[serde(default)]
    pub returned: bool,
    #[serde(default)]
    pub readnone: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub writeonly: bool,
    #[serde(default)]
    pub allocptr: bool,
    #[serde(default)]
    pub allocalign: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlvmAllocSizeV1 {
    pub element_size_arg: usize,
    #[serde(default)]
    pub num_elements_arg: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFunctionEffectsSnapshotV1 {
    #[serde(default)]
    pub nofree: bool,
    #[serde(default)]
    pub nosync: bool,
    #[serde(default)]
    pub willreturn: bool,
    #[serde(default)]
    pub nobuiltin: bool,
    #[serde(default)]
    pub optnone: bool,
    #[serde(default)]
    pub memory_explicit: bool,
    pub memory: LlvmMemoryAccessV1,
    #[serde(default)]
    pub alloc_kind: Vec<String>,
    #[serde(default)]
    pub alloc_family: Option<String>,
    #[serde(default)]
    pub return_noalias: bool,
    #[serde(default)]
    pub alloc_size: Option<LlvmAllocSizeV1>,
    #[serde(default)]
    pub formals: Vec<LlvmFormalEffectsSnapshotV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmFunctionEffectsRecordV1 {
    pub name: String,
    pub is_declaration: bool,
    pub origin_explicit: String,
    pub explicit: LlvmFunctionEffectsSnapshotV1,
    #[serde(default)]
    pub tli_recognized: bool,
    #[serde(default)]
    pub tli_libfunc: Option<String>,
    pub origin_inferred: String,
    pub tli_inferred: LlvmFunctionEffectsSnapshotV1,
    #[serde(default)]
    pub tli_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmCallsiteEffectsRecordV1 {
    pub caller: String,
    pub ordinal: usize,
    pub direct: bool,
    #[serde(default)]
    pub callee: Option<String>,
    #[serde(default)]
    pub callsite_memory_explicit: bool,
    pub effective_memory: LlvmMemoryAccessV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmEffectsModuleV1 {
    pub input: String,
    pub target_triple: String,
    pub input_ir_verified: bool,
    pub tli_clone_verified: bool,
    #[serde(default)]
    pub functions: Vec<LlvmFunctionEffectsRecordV1>,
    #[serde(default)]
    pub callsites_explicit: Vec<LlvmCallsiteEffectsRecordV1>,
    #[serde(default)]
    pub callsites_tli_inferred: Vec<LlvmCallsiteEffectsRecordV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LlvmMemoryEffectsArtifactV1 {
    pub schema: String,
    pub llvm_version: String,
    pub explicit_basis: String,
    pub tli_basis: String,
    #[serde(default)]
    pub modules: Vec<LlvmEffectsModuleV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfFormalPointsToV1 {
    pub formal_index: usize,
    pub svf_var_id: usize,
    #[serde(default)]
    pub points_to: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfFunctionPointsToV1 {
    pub function: String,
    #[serde(default)]
    pub formals: Vec<SvfFormalPointsToV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SvfSolvedPointsToArtifactV1 {
    pub schema: String,
    pub analysis: String,
    pub semantics: String,
    pub formal_mapping_schema: String,
    #[serde(default)]
    pub functions: Vec<SvfFunctionPointsToV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmJson {
    pub nodes: Vec<LlvmJsonNode>, // list of nodes in the JSON structure
    pub edges: Vec<LlvmEdge>, // list of edges in the JSON structure
    /// Bmulti producer certificate: SVF formal ArgVal VarIDs in declaration
    /// order. Historical SVF artifacts omit this field and remain readable.
    #[serde(default)]
    pub formal_param_var_ids: Vec<usize>,
    #[serde(default)]
    pub formal_param_mapping_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlvmFunction {
    pub function_name: String,
    //pub basic_blocks: Vec<LlvmBasicBlock>,
    pub nodes: Vec<LlvmJsonNode>,
    /// Certified positional formal parameters copied from the per-function
    /// SVF artifact. Empty means legacy artifact / no Bmulti certificate.
    #[serde(default)]
    pub formal_param_var_ids: Vec<usize>,
    #[serde(default)]
    pub formal_param_mapping_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlvmRepresentation {
    pub functions: HashMap<String, LlvmFunction>,
    pub global_edges: Vec<LlvmEdge>,
    #[serde(default)]
    pub llvm_memory_effects: Option<LlvmMemoryEffectsArtifactV1>,
    #[serde(default)]
    pub svf_solved_points_to: Option<SvfSolvedPointsToArtifactV1>,
}

// -----------------------
// GLOBAL ICFG 
// -----------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcfgEdge {
    pub source: String,
    pub destination: String,
    pub label: Option<String>,
    pub source_label: Option<String>,
    pub destination_label: Option<String>,
}

// a node in the global ICFG can be either a MIR basic block, an LLVM node, or a dummy node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", content = "node_data")]
pub enum GlobalICFGNode {
    Llvm(LlvmJsonNode),
    Mir(MirBasicBlock),
    DummyCall(DummyNode),
    DummyRet(DummyNode),
    /// Explicit maximal terminal state (e.g. rustc UnwindAction::Terminate).
    /// v6K requires every ICFG edge endpoint to be present in the node domain.
    Terminal(TerminalNode),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalNode {
    pub reason: String,
}

// a dummy node with extra fields
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DummyArgumentBinding {
    /// Zero-based source-language argument position.
    pub arg_index: usize,
    /// MIR actual argument spelling at this exact Rust callsite.
    pub mir_var: String,
    /// Callsite-scoped SVF formal VarID spelling (`<id>@rust::<...>::bbN`).
    pub llvm_var: String,
    /// EFX1/Bpta-R1: solved Andersen points-to set for this formal. MAY only.
    #[serde(default)]
    pub svf_may_points_to: Vec<usize>,
    /// Exact producer basis for the MAY set. None on historical artifacts.
    #[serde(default)]
    pub svf_points_to_basis: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DummyNode {
    pub dummy_node_name: String,
    pub incoming_edge: String,
    pub outgoing_edge: String,
    pub id: String,
    /// Legacy first-argument bridge retained for historical artifacts.
    pub mir_var: Option<String>,
    pub llvm_var: Option<String>,
    /// Bmulti positional bridge. Non-empty only when the SVF producer emitted
    /// an exact formal-parameter certificate matching MIR arity.
    #[serde(default)]
    pub argument_bindings: Vec<DummyArgumentBinding>,
    pub is_internal: Option<bool>,
}

//  The overall global ICFG has a mapping from node IDs (strings) to optional node data
/// (if the node is found and is not a leaf, otherwise `None`), and a list of ICFG edges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalICFG {
    pub icfg_nodes: HashMap<String, Option<GlobalICFGNode>>,
    pub icfg_edges: Vec<IcfgEdge>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalICFGOrdered {
    pub ordered_nodes: Vec<(String, GlobalICFGNode)>,
    pub icfg_edges: Vec<IcfgEdge>,
    /// EFX1 evidence is carried through the global ICFG as immutable metadata.
    /// CQPL export may consume it only through capability-gated derivations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llvm_memory_effects: Option<LlvmMemoryEffectsArtifactV1>,
    /// Bpta-R1 solved Andersen points-to evidence. Membership is MAY only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svf_solved_points_to: Option<SvfSolvedPointsToArtifactV1>,
    /// Phase-6 interprocedural identity metadata.  `serde(default)` keeps
    /// historical frozen ICFG JSON files readable.
    #[serde(default)]
    pub rust_functions: BTreeMap<String, RustFunctionMetadata>,
    #[serde(default)]
    pub rust_calls: Vec<RustCallMetadata>,
    /// Producer evidence for opt-in dependency-body ingestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_body_ingestion_v1: Option<DependencyBodyIngestionExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyBodyIngestionExport {
    pub capability: String,
    pub target_triple: String,
    pub crate_correlations: Vec<serde_json::Value>,
    pub package_scope: Vec<serde_json::Value>,
    pub body_statuses: Vec<serde_json::Value>,
    pub exact_call_bindings: Vec<serde_json::Value>,
    /// Exact compiler-derived semantic call identities joined to the existing
    /// ICFG MIR call nodes by the producer while both views share one rustc
    /// session. Call coverage consumers must use this relation, never display
    /// names, as the bridge key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icfg_callsite_links: Vec<serde_json::Value>,
    /// DEP1-P2 control-flow provenance. This references the existing call/body
    /// producers and the ordinary global ICFG nodes/edges; it carries no
    /// resource identity semantics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icfg_stitching_v1: Option<serde_json::Value>,
    /// Context-explicit execution-state provenance.  This is present only on
    /// DEP1-enabled output after the producer has replaced the flat Rust
    /// call/return adjacency with its finite execution graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_execution_graph_v1: Option<serde_json::Value>,
}

/// Canonical compiler body/code provenance.  `concrete_instance_id` is the
/// persisted rustc-derived Instance identity; basic blocks are shared across
/// execution contexts and are never reacquired for each caller.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CanonicalCodeNodeIdentity {
    pub concrete_instance_id: String,
    pub basic_block: u32,
}

/// One ordered activation context over canonical code.  Static COV1 call keys
/// are the context elements; order is semantically significant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ExecutionStateIdentity {
    pub code: CanonicalCodeNodeIdentity,
    pub context: Vec<String>,
}

/// Centralized deterministic serialization for context-qualified Rust MIR
/// nodes.  Length-prefixing plus hexadecimal encoding is injective and does
/// not depend on display names, paths, or session-local compiler indices.
pub fn execution_state_node_id(identity: &ExecutionStateIdentity) -> String {
    fn append_field(out: &mut Vec<u8>, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u64).to_be_bytes());
        out.extend_from_slice(value);
    }
    let mut bytes = b"crema-execution-state-v1".to_vec();
    append_field(&mut bytes, identity.code.concrete_instance_id.as_bytes());
    bytes.extend_from_slice(&(identity.context.len() as u64).to_be_bytes());
    for call_key in &identity.context {
        append_field(&mut bytes, call_key.as_bytes());
    }
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("rust::exec-v1::{hex}::bb{}", identity.code.basic_block)
}

#[cfg(test)]
mod execution_state_identity_tests {
    use super::*;

    fn state(context: &[&str]) -> ExecutionStateIdentity {
        ExecutionStateIdentity {
            code: CanonicalCodeNodeIdentity {
                concrete_instance_id: "instance-hash-v1:0123456789abcdef".into(),
                basic_block: 7,
            },
            context: context.iter().map(|part| (*part).to_string()).collect(),
        }
    }

    #[test]
    fn contexts_are_ordered_and_do_not_duplicate_canonical_code() {
        let a = state(&["call-A"]);
        let b = state(&["call-B"]);
        assert_eq!(a.code, b.code);
        assert_ne!(execution_state_node_id(&a), execution_state_node_id(&b));
        assert_ne!(execution_state_node_id(&state(&["outer", "inner"])),
                   execution_state_node_id(&state(&["inner", "outer"])));
        assert!(execution_state_node_id(&a).ends_with("::bb7"));
    }
}
#[cfg(test)]
mod rust_activation_unwind_metadata_tests {
    use super::*;
    use serde_json::json;

    fn call(index: usize) -> RustCallMetadata {
        RustCallMetadata { caller_function: "caller".into(), call_node: format!("site{index}"),
            dummy_call_node: format!("proxy{index}"), callee_function: "callee".into(),
            dummy_ret_node: format!("return{index}"), arguments: vec![], return_place: "_0".into(),
            return_node: format!("continuation{index}"), is_closure: false }
    }
    fn graph() -> GlobalICFGOrdered {
        GlobalICFGOrdered { ordered_nodes: vec![], icfg_edges: vec![], llvm_memory_effects: None,
            svf_solved_points_to: None, rust_functions: BTreeMap::new(), rust_calls: vec![call(0)],
            dependency_body_ingestion_v1: None }
    }

    #[test]
    fn rust_activation_unwind_metadata_direct_cleanup() {
        let r = RustActivationUnwindMetadata::direct(&call(0), "cleanup(bb7)", false);
        assert_eq!(r.activation_kind, RustActivationKind::Direct);
        assert_eq!(r.source, RustActivationMetadataSource::MirCall);
        assert_eq!(r.unwind, RustActivationUnwindSemantics::Direct {
            action: RustActivationUnwindAction::Cleanup { target: "bb7".into() } });
    }
    #[test]
    fn rust_activation_unwind_metadata_direct_continue() {
        assert_eq!(RustActivationUnwindMetadata::direct(&call(0), "continue", false).unwind,
            RustActivationUnwindSemantics::Direct { action: RustActivationUnwindAction::Continue });
    }
    #[test]
    fn rust_activation_unwind_metadata_direct_terminate() {
        assert_eq!(RustActivationUnwindMetadata::direct(&call(0), "terminate", false).unwind,
            RustActivationUnwindSemantics::Direct { action: RustActivationUnwindAction::Terminate });
    }
    #[test]
    fn rust_activation_unwind_metadata_direct_unreachable() {
        assert_eq!(RustActivationUnwindMetadata::direct(&call(0), "unreachable", false).unwind,
            RustActivationUnwindSemantics::Direct { action: RustActivationUnwindAction::Unreachable });
    }
    #[test]
    fn rust_activation_unwind_metadata_custom_drop() {
        let a = RustActivationUnwindMetadata::direct(&call(0), "cleanup(bb3)", true);
        let b = RustActivationUnwindMetadata::direct(&call(1), "terminate", true);
        assert_eq!(a.source, RustActivationMetadataSource::MirDrop);
        assert_ne!(a.activation, b.activation); assert_ne!(a.unwind, b.unwind);
    }
    #[test]
    fn rust_activation_unwind_metadata_synchronous_callback_without_relations() {
        let r = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::RepeatedCallbacks));
        assert_eq!(r.activation_kind, RustActivationKind::SynchronousCallback);
        assert_eq!(r.unwind, RustActivationUnwindSemantics::Unresolved {
            reason: RustActivationUnresolvedReason::MissingCallbackUnwindEvidence });
    }
    #[test]
    fn rust_activation_unwind_metadata_detached_callback() {
        let r = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::SpawnMayOnce));
        assert_eq!(r.activation_kind, RustActivationKind::DetachedCallback);
        assert_eq!(r.unwind, RustActivationUnwindSemantics::DetachedNoCallerUnwind);
    }
    #[test]
    fn rust_activation_unwind_metadata_provider_collision() {
        let provider = json!({"kind":"Call", "details":"", "source_info":"", "function_called":"opaque",
            "arguments":[], "return_place":"_0", "return_target":"bb1", "unwind_target":"continue"});
        let before: MirTerminator = serde_json::from_value(provider.clone()).unwrap();
        let synchronous = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::RepeatedCallbacks));
        let detached = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::SpawnMayOnce));
        assert_ne!(synchronous.activation_kind, detached.activation_kind);
        assert_ne!(synchronous.unwind, detached.unwind);
        // Neither callback factory takes or consults the provider MIR action.
        assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(
            serde_json::from_value::<MirTerminator>(provider).unwrap()).unwrap());
    }
    #[test]
    fn rust_activation_unwind_metadata_missing_callback_policy() {
        for policy in [None, Some(HigherOrderCallSemantics::Unknown), Some(HigherOrderCallSemantics::LazyCarrier)] {
            let r = RustActivationUnwindMetadata::callback(&call(0), policy);
            assert_eq!(r.activation_kind, RustActivationKind::Unresolved);
            assert_eq!(r.unwind, RustActivationUnwindSemantics::Unresolved {
                reason: RustActivationUnresolvedReason::MissingCallbackPolicy });
        }
    }
    #[test]
    fn rust_activation_unwind_metadata_shared_callee_separate_activations() {
        let a = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::RepeatedCallbacks));
        let b = RustActivationUnwindMetadata::callback(&call(1), Some(HigherOrderCallSemantics::SpawnMayOnce));
        let table = RustActivationUnwindMetadataV1::new([a.clone(), b.clone()]).unwrap();
        assert_eq!(table.activations, vec![a, b]);
        assert_eq!(table.activations[0].activation.callee_function, table.activations[1].activation.callee_function);
    }
    #[test]
    fn rust_activation_unwind_metadata_deterministic() {
        let a = RustActivationUnwindMetadata::direct(&call(0), "continue", false);
        let b = RustActivationUnwindMetadata::direct(&call(1), "terminate", true);
        let first = RustActivationUnwindMetadataV1::new([a.clone(), b.clone(), a.clone()]).unwrap();
        let second = RustActivationUnwindMetadataV1::new([b, a]).unwrap();
        assert_eq!(serde_json::to_string(&first).unwrap(), serde_json::to_string(&second).unwrap());
    }
    #[test]
    fn rust_activation_unwind_metadata_no_cqpl_artifact_effect() {
        let original = serde_json::to_value(graph()).unwrap();
        let mut operational = original.clone();
        operational["rust_activation_unwind_metadata_v1"] = serde_json::to_value(
            RustActivationUnwindMetadataV1::new([RustActivationUnwindMetadata::direct(&call(0), "continue", false)]).unwrap()).unwrap();
        assert!(RustActivationUnwindMetadataV1::from_operational_json(&operational).unwrap().is_some());
        // All inputs to the frozen CQPL exporter are identical after crossing
        // the operational serde boundary; new provenance is not part of that model.
        let consumed: GlobalICFGOrdered = serde_json::from_value(operational).unwrap();
        assert_eq!(serde_json::to_value(consumed).unwrap(), original);
    }
    #[test]
    fn rust_activation_unwind_metadata_existing_rust_calls_preserved() {
        let calls = vec![call(0), call(1)];
        let original = serde_json::to_value(&calls).unwrap();
        let table = RustActivationUnwindMetadataV1::new(calls.iter().map(|c|
            RustActivationUnwindMetadata::direct(c, "unreachable", false))).unwrap();
        let raw = json!({"rust_calls":calls, "rust_activation_unwind_metadata_v1":table});
        let loaded = RustActivationUnwindMetadataV1::from_operational_json(&raw).unwrap().unwrap();
        assert_eq!(loaded.activations.len(), 2);
        assert_eq!(raw["rust_calls"], original);
    }
    #[test]
    fn rust_activation_unwind_metadata_ambiguous_or_malformed_fail_closed() {
        let a = RustActivationUnwindMetadata::direct(&call(0), "continue", false);
        let b = RustActivationUnwindMetadata::direct(&call(0), "terminate", false);
        assert!(RustActivationUnwindMetadataV1::new([a,b]).is_err());
        let mut inconsistent = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::Unknown));
        inconsistent.activation_kind = RustActivationKind::DetachedCallback;
        inconsistent.unwind = RustActivationUnwindSemantics::DetachedNoCallerUnwind;
        assert!(RustActivationUnwindMetadataV1::new([inconsistent]).is_err());
        let mut policy_only = RustActivationUnwindMetadata::callback(&call(0), Some(HigherOrderCallSemantics::RepeatedCallbacks));
        policy_only.unwind = RustActivationUnwindSemantics::CertifiedCallback {
            action: RustActivationUnwindAction::Continue,
            unwind_edges: vec![RustActivationUnwindEdge { source_node: "exit".into(), destination_node: "proxy".into() }] };
        assert!(RustActivationUnwindMetadataV1::new([policy_only]).is_err());
        let r = RustActivationUnwindMetadata::direct(&call(0), "cleanup(garbage)", false);
        assert_eq!(r.unwind, RustActivationUnwindSemantics::Unresolved {
            reason: RustActivationUnresolvedReason::UnsupportedMirUnwindAction });
    }
}
