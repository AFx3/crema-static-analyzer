use crate::abstract_domain::{AbstractMemory, AbstractState, CellValue, Name};
use crate::identity::{AllocationIdentityMemory, AllocationIdentityState};
use crate::mir_semantics::{mir_semantics_v2_enabled, semantic_labels_for_block};
use crate::panic_unwind::{edge_flow_kind, panic_unwind_lifecycle_v1_enabled, EdgeFlowKind};
use crate::panic_lifecycle_domain::{fixed_point_real_panic_lifecycle, PanicLifecycleMemory};
use crate::memory_events;
use crate::structs::{
    AbstractAllocId, AllocationSiteId, GlobalICFGNode, GlobalICFGOrdered, MirCallArgument, MirTerminator,
    PlaceId, PlaceProjection, ProgramVarId, RustDropAllocatorEvidenceKind,
    RustCallDeallocatorEvidenceKind, RustAllocationDispositionEvidenceKind, SvfStatement,
    LlvmMemoryEffectsArtifactV1, LlvmFunctionEffectsRecordV1, LlvmFunctionEffectsSnapshotV1,
    SvfSolvedPointsToArtifactV1, RustActivationUnwindMetadataV1, RustActivationUnwindSemantics,
    RustActivationUnwindAction, RustActivationUnwindEdge, RustActivationIdentity, RustActivationKind,
    RustActivationMetadataSource, RustActivationUnresolvedReason,
};
use crate::utils::load_ffi_functions;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use once_cell::sync::Lazy;

/// Exact callsite pairing owned by the Rust producer metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RustCallReturnRelationV1 {
    caller_function: String,
    call_node: String,
    dummy_call_node: String,
    callee_function: String,
    callee_entry_node: String,
    callee_return_nodes: Vec<String>,
    dummy_ret_node: String,
    continuation_node: String,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RustCallUnwindSemanticsV1 {
    Cleanup, Continue, Terminate, Unreachable, CertifiedCallback,
    DetachedNoCallerUnwind, Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RustCallUnwindActionV1 {
    Cleanup { target: String }, Continue, Terminate, Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RustCallUnwindReasonV1 {
    MissingCallbackPolicy, MissingCallbackUnwindEvidence, UnsupportedMirUnwindAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RustCallUnwindRelationV1 {
    caller_function: String,
    call_node: String,
    dummy_call_node: String,
    callee_function: String,
    callee_entry_node: String,
    activation_kind: RustActivationKind,
    activation_source: RustActivationMetadataSource,
    unwind_semantics: RustCallUnwindSemanticsV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    unwind_action: Option<RustCallUnwindActionV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved_reason: Option<RustCallUnwindReasonV1>,
    unwind_edges: Vec<RustActivationUnwindEdge>,
    basis: &'static str,
}

fn rust_call_unwind_wire_action(action: &RustActivationUnwindAction) -> RustCallUnwindActionV1 {
    match action {
        RustActivationUnwindAction::Cleanup { target } => RustCallUnwindActionV1::Cleanup { target: target.clone() },
        RustActivationUnwindAction::Continue => RustCallUnwindActionV1::Continue,
        RustActivationUnwindAction::Terminate => RustCallUnwindActionV1::Terminate,
        RustActivationUnwindAction::Unreachable => RustCallUnwindActionV1::Unreachable,
    }
}

/// F6B0 chooses the activation policy/destination before the graph is examined.
/// Canonical edges never choose a caller or resolve callback semantics.
fn rust_call_unwind_relation_records(
    icfg: &GlobalICFGOrdered,
    metadata: &RustActivationUnwindMetadataV1,
    returns: &[RustCallReturnRelationV1],
    typed: &[TypedEdgeRecord],
    node_ids: &BTreeSet<String>,
) -> Result<Vec<RustCallUnwindRelationV1>, Box<dyn Error>> {
    if metadata.schema_version != 1 { return Err("unsupported activation unwind metadata version".into()); }
    let mut by_activation = BTreeMap::new();
    for record in &metadata.activations {
        if by_activation.insert(record.activation.clone(), record).is_some() {
            return Err("duplicate F6B0 unwind activation".into());
        }
    }
    RustActivationUnwindMetadataV1::new(metadata.activations.clone())?;
    metadata.validate_calls(&icfg.rust_calls)?;
    let nodes: BTreeMap<_, _> = icfg.ordered_nodes.iter().map(|(id, node)| (id.as_str(), node)).collect();
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for call in &icfg.rust_calls {
        let key = RustActivationIdentity::from(call);
        if !seen.insert(key.clone()) { return Err("duplicate represented unwind activation".into()); }
        let record = by_activation.get(&key).ok_or("missing F6B0 unwind activation")?;
        let function = icfg.rust_functions.get(&call.callee_function).ok_or("missing unwind callee metadata")?;
        let matched: Vec<_> = returns.iter().filter(|r| r.caller_function == key.caller_function
            && r.call_node == key.call_node && r.dummy_call_node == key.dummy_call_node
            && r.callee_function == key.callee_function && r.callee_entry_node == function.entry_node).collect();
        if matched.len() != 1 { return Err("F2/unwind activation consistency mismatch".into()); }
        for node in [&call.call_node, &call.dummy_call_node, &function.entry_node] {
            if !node_ids.contains(node) { return Err("unwind activation references missing node".into()); }
        }
        let mut edges = BTreeSet::new();
        let mut reason = None;
        let (semantics, action) = match &record.unwind {
            RustActivationUnwindSemantics::Direct { action } => {
                // DEP1 exposes exact activation-specific pairs, including real
                // Continue proxies and terminal nodes. Reuse those rows only.
                if let RustActivationMetadataSource::ExecutionActivation { call_key } = &record.source {
                    let ledger = icfg.dependency_body_ingestion_v1.as_ref()
                        .and_then(|d| d.context_execution_graph_v1.as_ref())
                        .and_then(|g| g.get("represented_call_activations"))
                        .and_then(serde_json::Value::as_array).ok_or("missing activation unwind ledger")?;
                    let rows: Vec<_> = ledger.iter().filter(|r| r["call_key"].as_str() == Some(call_key.as_str())
                        && r["caller_call_state"].as_str() == Some(call.call_node.as_str())
                        && r["dummy_call_state"].as_str() == Some(call.dummy_call_node.as_str())
                        && r["callee_entry_state"].as_str() == Some(function.entry_node.as_str())).collect();
                    if rows.len() != 1 { return Err("ambiguous native unwind activation".into()); }
                    for relation in rows[0].get("unwind_return_relations").and_then(serde_json::Value::as_array)
                        .ok_or("missing native unwind relations")? {
                        let relation_action = RustActivationUnwindAction::from_mir_action(
                            relation["unwind_action"].as_str().ok_or("missing native unwind action")?)?;
                        if &relation_action != action { return Err("native unwind action conflicts with F6B0".into()); }
                        edges.insert(RustActivationUnwindEdge {
                            source_node: relation["callee_unwind_state"].as_str().ok_or("missing native unwind source")?.into(),
                            destination_node: relation["caller_unwind_state"].as_str().ok_or("missing native unwind destination")?.into(),
                        });
                    }
                } else if let RustActivationUnwindAction::Cleanup { target } = action {
                    // Standard F6B0 certifies a relative caller MIR BB. Resolve
                    // its registered node, never an edge-selected destination.
                    let destination = format!("rust::{}::{target}", call.caller_function);
                    if !node_ids.contains(&destination) { return Err("missing certified cleanup destination".into()); }
                    for edge in typed.iter().filter(|edge| edge.destination == destination) {
                        let owned = match nodes.get(edge.source.as_str()) {
                            Some(GlobalICFGNode::Mir(block)) => edge.source == format!(
                                "rust::{}::bb{}", call.callee_function, block.block_id),
                            _ => false,
                        };
                        if owned {
                            edges.insert(RustActivationUnwindEdge { source_node: edge.source.clone(), destination_node: destination.clone() });
                        }
                    }
                }
                let semantics = match action {
                    RustActivationUnwindAction::Cleanup { .. } => RustCallUnwindSemanticsV1::Cleanup,
                    RustActivationUnwindAction::Continue => RustCallUnwindSemanticsV1::Continue,
                    RustActivationUnwindAction::Terminate => RustCallUnwindSemanticsV1::Terminate,
                    RustActivationUnwindAction::Unreachable => RustCallUnwindSemanticsV1::Unreachable,
                };
                (semantics, Some(rust_call_unwind_wire_action(action)))
            }
            RustActivationUnwindSemantics::CertifiedCallback { action, unwind_edges } => {
                edges.extend(unwind_edges.iter().cloned());
                (RustCallUnwindSemanticsV1::CertifiedCallback, Some(rust_call_unwind_wire_action(action)))
            }
            RustActivationUnwindSemantics::DetachedNoCallerUnwind =>
                (RustCallUnwindSemanticsV1::DetachedNoCallerUnwind, None),
            RustActivationUnwindSemantics::Unresolved { reason: why } => {
                reason = Some(match why {
                    RustActivationUnresolvedReason::MissingCallbackPolicy => RustCallUnwindReasonV1::MissingCallbackPolicy,
                    RustActivationUnresolvedReason::MissingCallbackUnwindEvidence => RustCallUnwindReasonV1::MissingCallbackUnwindEvidence,
                    RustActivationUnresolvedReason::UnsupportedMirUnwindAction => RustCallUnwindReasonV1::UnsupportedMirUnwindAction,
                });
                (RustCallUnwindSemanticsV1::Unresolved, None)
            }
        };
        for edge in &edges {
            if !node_ids.contains(&edge.source_node) || !node_ids.contains(&edge.destination_node) {
                return Err("certified unwind edge references missing node".into());
            }
            if !typed.iter().any(|t| t.source == edge.source_node && t.destination == edge.destination_node && t.flow == "unwind") {
                return Err("certified unwind edge missing canonical Unwind flow".into());
            }
            if let Some(RustCallUnwindActionV1::Terminate) = &action {
                if !matches!(nodes.get(edge.destination_node.as_str()), Some(GlobalICFGNode::Terminal(_))) {
                    return Err("terminate relation targets a cleanup".into());
                }
            }
            if let Some(RustCallUnwindActionV1::Unreachable) = &action {
                return Err("unreachable activation acquired unwind edge".into());
            }
        }
        result.push(RustCallUnwindRelationV1 { caller_function: call.caller_function.clone(),
            call_node: call.call_node.clone(), dummy_call_node: call.dummy_call_node.clone(),
            callee_function: call.callee_function.clone(), callee_entry_node: function.entry_node.clone(),
            activation_kind: record.activation_kind, activation_source: record.source.clone(),
            unwind_semantics: semantics, unwind_action: action, unresolved_reason: reason,
            unwind_edges: edges.into_iter().collect(), basis: "rust_activation_unwind_metadata_v1" });
    }
    result.sort_by(|a,b| (&a.caller_function,&a.call_node,&a.dummy_call_node,&a.callee_function)
        .cmp(&(&b.caller_function,&b.call_node,&b.dummy_call_node,&b.callee_function)));
    Ok(result)
}

/// Read-only transport of facts already known at node/edge creation. Scope IDs
/// are opaque body-membership tokens, not function-name classifiers.
#[derive(Debug, Clone, Serialize, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RustUnwindScopeEvidenceV1 {
    pub schema_version: u32,
    pub node_owners: BTreeMap<String, String>,
    pub boundary_edges: Vec<RustActivationUnwindEdge>,
}

impl RustUnwindScopeEvidenceV1 {
    pub(crate) fn from_operational_json(raw: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(value) = raw.get("rust_unwind_scope_evidence_v1") else { return Ok(None); };
        let evidence: Self = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        if evidence.schema_version != 1 || evidence.node_owners.iter().any(|(n,o)| n.is_empty() || o.is_empty())
            || !evidence.boundary_edges.windows(2).all(|p| p[0] < p[1]) {
            return Err("invalid canonical unwind scope evidence".into());
        }
        Ok(Some(evidence))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RustUnwindEdgeScopeKindV1 { IntraActivation, RustActivationBoundary, NonRust, Unresolved }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RustUnwindEdgeScopeBasisV1 {
    RustActivationUnwindMetadataV1, CanonicalRustFunctionMembershipV1,
    NonRustUnwindScopeV1, UnresolvedUnwindScopeV1, ExecutionContextMembershipV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RustUnwindEdgeScopeV1 {
    source_node: String,
    destination_node: String,
    scope: RustUnwindEdgeScopeKindV1,
    /// Canonical set approved for shared edges certified for multiple callsites.
    #[serde(skip_serializing_if = "Option::is_none")]
    activation: Option<Vec<RustActivationIdentity>>,
    basis: RustUnwindEdgeScopeBasisV1,
}

fn rust_unwind_scope_native_evidence(icfg: &GlobalICFGOrdered) -> Result<Option<RustUnwindScopeEvidenceV1>, Box<dyn Error>> {
    let Some(graph) = icfg.dependency_body_ingestion_v1.as_ref().and_then(|d| d.context_execution_graph_v1.as_ref()) else {
        return Ok(None);
    };
    if graph.get("p2_control_flow_complete").and_then(serde_json::Value::as_bool) != Some(true) {
        return Ok(Some(RustUnwindScopeEvidenceV1 { schema_version:1, ..Default::default() }));
    }
    let mut owners = BTreeMap::new();
    for state in graph.get("states").and_then(serde_json::Value::as_array).ok_or("missing execution states")? {
        let node = state["execution_node"].as_str().ok_or("missing execution node")?;
        let instance = state["concrete_instance_id"].as_str().ok_or("missing owner instance")?;
        let context = state.get("context").and_then(serde_json::Value::as_array).ok_or("missing owner context")?;
        let owner = serde_json::to_string(&serde_json::json!({"instance":instance,"context":context}))?;
        if owners.insert(node.into(), owner).is_some() { return Err("duplicate execution owner".into()); }
    }
    let mut boundary = BTreeSet::new();
    for record in graph.get("represented_call_activations").and_then(serde_json::Value::as_array)
        .ok_or("missing activation ledger")? {
        for edge in record.get("unwind_return_relations").and_then(serde_json::Value::as_array)
            .ok_or("missing activation unwind relations")? {
            boundary.insert(RustActivationUnwindEdge {
                source_node:edge["callee_unwind_state"].as_str().ok_or("missing native source")?.into(),
                destination_node:edge["caller_unwind_state"].as_str().ok_or("missing native destination")?.into(),
            });
        }
    }
    Ok(Some(RustUnwindScopeEvidenceV1 { schema_version:1, node_owners:owners, boundary_edges:boundary.into_iter().collect() }))
}

fn rust_unwind_edge_scope_records(
    icfg: &GlobalICFGOrdered,
    typed: &[TypedEdgeRecord],
    relations: &[RustCallUnwindRelationV1],
    evidence: Option<&RustUnwindScopeEvidenceV1>,
) -> Result<Vec<RustUnwindEdgeScopeV1>, Box<dyn Error>> {
    let native = rust_unwind_scope_native_evidence(icfg)?;
    let evidence = native.as_ref().or(evidence);
    let nodes: BTreeMap<_,_> = icfg.ordered_nodes.iter().map(|(id,n)|(id.as_str(),n)).collect();
    let pairs: BTreeSet<_> = typed.iter().filter(|e|e.flow=="unwind")
        .map(|e|(e.source.clone(),e.destination.clone())).collect();
    let mut claims = BTreeMap::<_,BTreeSet<RustActivationIdentity>>::new();
    for r in relations {
        let key=RustActivationIdentity {caller_function:r.caller_function.clone(),call_node:r.call_node.clone(),
            dummy_call_node:r.dummy_call_node.clone(),callee_function:r.callee_function.clone()};
        for edge in &r.unwind_edges {
            if !pairs.contains(&(edge.source_node.clone(),edge.destination_node.clone())) {
                return Err("F6B1 scope claim lacks typed Unwind edge".into());
            }
            claims.entry((edge.source_node.clone(),edge.destination_node.clone())).or_default().insert(key.clone());
        }
    }
    let boundary: BTreeSet<_> = evidence.into_iter().flat_map(|e| &e.boundary_edges)
        .map(|e|(e.source_node.as_str(),e.destination_node.as_str())).collect();
    let mut result=Vec::new();
    for (source,destination) in &pairs {
        if !nodes.contains_key(source.as_str()) || !nodes.contains_key(destination.as_str()) {
            return Err("scope edge references missing canonical node".into());
        }
        let (scope,activation,basis)=if let Some(keys)=claims.get(&(source.clone(),destination.clone())) {
            (RustUnwindEdgeScopeKindV1::RustActivationBoundary,Some(keys.iter().cloned().collect()),
                RustUnwindEdgeScopeBasisV1::RustActivationUnwindMetadataV1)
        } else if boundary.contains(&(source.as_str(),destination.as_str())) {
            // Known boundary but no concrete F6B1 certificate; do not invent
            // an activation from same-owner or flat fan-out evidence.
            (RustUnwindEdgeScopeKindV1::Unresolved,None,RustUnwindEdgeScopeBasisV1::UnresolvedUnwindScopeV1)
        } else if evidence.is_some_and(|e| e.node_owners.get(source).is_some()
            && e.node_owners.get(source)==e.node_owners.get(destination)) {
            (RustUnwindEdgeScopeKindV1::IntraActivation,None,
                if native.is_some() { RustUnwindEdgeScopeBasisV1::ExecutionContextMembershipV1 }
                else { RustUnwindEdgeScopeBasisV1::CanonicalRustFunctionMembershipV1 })
        } else if matches!(nodes[source.as_str()],GlobalICFGNode::Llvm(_))
            && matches!(nodes[destination.as_str()],GlobalICFGNode::Llvm(_)) {
            (RustUnwindEdgeScopeKindV1::NonRust,None,RustUnwindEdgeScopeBasisV1::NonRustUnwindScopeV1)
        } else {
            (RustUnwindEdgeScopeKindV1::Unresolved,None,RustUnwindEdgeScopeBasisV1::UnresolvedUnwindScopeV1)
        };
        result.push(RustUnwindEdgeScopeV1 {source_node:source.clone(),destination_node:destination.clone(),scope,activation,basis});
    }
    validate_rust_unwind_edge_scopes(&result,&pairs,&claims)?;
    Ok(result)
}

fn validate_rust_unwind_edge_scopes(
    records: &[RustUnwindEdgeScopeV1],
    pairs: &BTreeSet<(String,String)>,
    claims: &BTreeMap<(String,String),BTreeSet<RustActivationIdentity>>,
) -> Result<(),Box<dyn Error>> {
    let mut seen=BTreeSet::new();
    for r in records {
        let pair=(r.source_node.clone(),r.destination_node.clone());
        if !pairs.contains(&pair) || !seen.insert(pair.clone()) {return Err("duplicate or foreign unwind scope record".into());}
        if r.scope==RustUnwindEdgeScopeKindV1::RustActivationBoundary {
            let keys=r.activation.as_ref().ok_or("missing boundary activation set")?;
            if keys.is_empty() || !keys.windows(2).all(|p|p[0]<p[1])
                || claims.get(&pair)!=Some(&keys.iter().cloned().collect()) {
                return Err("scope activation set conflicts with F6B1".into());
            }
        } else if r.activation.is_some() || claims.contains_key(&pair) {
            return Err("F6B1 concrete edge not classified as its activation boundary".into());
        }
    }
    if seen!=*pairs {return Err("incomplete unwind edge scope coverage".into());}
    Ok(())
}

/// Versioned, read-only boundary from CREMA to the standalone CQPL checker.
///
/// Scientific invariant: this module never mutates the ICFG, abstract state,
/// taint state, or legacy memory-error detector state. It only serializes
/// information already produced by CREMA plus syntactic labels obtained by
/// inspecting the existing ICFG nodes.
#[derive(Debug, Clone, Serialize)]
struct AnnotatedIcfg {
    /// DEP1-P2 execution-state provenance. Program successors remain the exact
    /// producer graph; this sidecar lets consumers map opaque node IDs back to
    /// one canonical MIR node and its ordered call context without parsing IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    context_execution_graph_v1: Option<serde_json::Value>,
    /// DEP1-COV1 diagnostic provenance only. The checker does not consume this
    /// ledger; each record references an existing semantic producer artifact.
    #[serde(skip_serializing_if = "Option::is_none")]
    call_semantic_coverage: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    call_semantic_coverage_complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_call_bindings: Option<Vec<ExternalCallBindingV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_library_effects: Option<Vec<ExternalLibraryEffectsV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_deallocation_call_provenance: Option<Vec<ExternalDeallocationCallProvenanceV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_negative_evidence: Option<Vec<ExternalNegativeEvidenceRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_return_relations: Option<Vec<ExternalReturnRelationRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_return_call_bindings: Option<Vec<ExternalReturnCallBinding>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_call_return_relations: Option<Vec<RustCallReturnRelationV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_call_unwind_relations: Option<Vec<RustCallUnwindRelationV1>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_unwind_edge_scopes: Option<Vec<RustUnwindEdgeScopeV1>>,
    schema_version: u32,
    entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<&'static str>>,
    /// A/R2 exact typed copy of the canonical CREMA ICFG edge relation.
    /// This payload is additive: legacy CQPL semantics still traverse only
    /// `nodes[*].successors` until a later capability explicitly consumes flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    typed_edges: Option<Vec<TypedEdgeRecord>>,
    /// OME1: nodes whose memory effect is intentionally opaque.  CQPL taints
    /// the event node and all reachable successors so memory predicates become
    /// UNKNOWN rather than inheriting optimistic state across the boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    opaque_memory_effects: Option<Vec<OpaqueMemoryEffectRecord>>,
    variables: Vec<ProgramVariable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocations: Option<Vec<AbstractAllocationRecord>>,
    /// Bcontract-ND1: producer-side negative/positive deallocation-effect
    /// certificates for external C call boundaries. Absence of a record is
    /// never interpreted as proof of absence.
    #[serde(skip_serializing_if = "Option::is_none")]
    external_deallocation_effects: Option<Vec<ExternalDeallocationEffectRecord>>,
    /// EFM2: proof-carrying semantic memory effects for bodyless external calls.
    /// These records classify formal argument roles only; actual Rust allocation
    /// identity remains entirely producer-side and is never borrowed from SVF.
    #[serde(skip_serializing_if = "Option::is_none")]
    external_formal_memory_effects: Option<Vec<ExternalFormalMemoryEffectRecord>>,
    /// EFX1 proof payload. This is the exact validated producer sidecar carried
    /// across the CREMA -> CQPL boundary, not merely a capability bit.
    #[serde(skip_serializing_if = "Option::is_none")]
    llvm_memory_effects: Option<LlvmMemoryEffectsArtifactV1>,
    /// Bpta-R1 solved Andersen evidence. Membership is MAY only.
    #[serde(skip_serializing_if = "Option::is_none")]
    svf_solved_points_to: Option<SvfSolvedPointsToArtifactV1>,
    /// R2 proof-carrying Rust -> C positional allocation-identity bridge.
    /// This serializes the already-computed Bmulti/identity relation; it does
    /// not create aliases or upgrade MAY evidence to MUST.
    #[serde(skip_serializing_if = "Option::is_none")]
    ffi_argument_identity: Option<Vec<FfiArgumentIdentityRecord>>,
    /// AGE1: proof-carrying correlation between a raw-pointer `is_null` test
    /// and the existence of one singleton abstract allocation identity.
    /// Additive diagnostic evidence only; CQPL truth never consumes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    allocation_existence_guards: Option<Vec<AllocationExistenceGuardRecord>>,
    /// RBF1: diagnostic-only proof boundary for a bodyless C realloc whose
    /// success/failure split is not yet modeled allocation-centrically.
    /// Consumers may fail closed across this boundary, but must not derive
    /// truth or a deallocation event from it unless CR1 is also present.
    #[serde(skip_serializing_if = "Option::is_none")]
    reallocation_boundaries: Option<Vec<ReallocationBoundaryRecord>>,
    /// CR1: proof-carrying conditional realloc outcome.  The payload certifies
    /// a non-null source object, a positive non-zero size, an exact raw-pointer
    /// `is_null` split of the realloc result, and direct compatible `free`
    /// calls on that result where present.  It is exported as a sidecar; CREMA's
    /// fixed point and allocation-state lattice remain unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    conditional_reallocations: Option<Vec<ConditionalReallocationRecord>>,
    nodes: Vec<AnnotatedNode>,
}

#[derive(Debug, Clone, Serialize)]
struct AbstractAllocationRecord {
    /// Opaque, injective serialization of AbstractAllocId used by CQPL bindings.
    id: String,
    /// Human-readable diagnostic only; not used as logical identity.
    display: String,
    site: AllocationSiteId,
    context: Vec<String>,
    /// Capability allocation_contracts_v1: semantic allocator contract.
    /// This is exporter-produced structure; the checker never infers it from IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    allocator_contract: Option<AllocationContract>,
}

#[derive(Debug, Clone, Serialize)]
struct ProgramVariable {
    id: String,
    language: &'static str,
}

/// ELE1 is metadata over existing proofs. Neither record is consumed by an
/// event or abstract-state producer.
#[derive(Debug, Clone, Serialize)]
struct ExternalCallBindingV1 {
    binding_id: String,
    node: String,
    rust_function_scope: String,
    callee: String,
    arity: usize,
    arguments: Vec<Option<String>>,
    result_variable: Option<String>,
    body_status: &'static str,
    basis: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct ExternalLibraryEffectsV1 {
    binding_id: String,
    effect_families: Vec<&'static str>,
    effect_counts: BTreeMap<&'static str, usize>,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct AllocationContract {
    family: &'static str,
    operation: &'static str,
    language: &'static str,
    /// allocation_contracts_v2 proof basis.  v6N-r1 requires this field on
    /// deallocator contracts only; allocator-origin classification remains the
    /// frozen v1 boundary and is not silently re-certified by this gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    basis: Option<&'static str>,
    /// Diagnostic provenance emitted only for rustc-structural typed drops.
    /// The checker validates the basis/family tuple but never infers from these
    /// strings.
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_def_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocator_def_path: Option<String>,
    /// Audit-only provenance for producer-classified explicit Rust calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    callee_def_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ExternalDeallocationEffectRecord {
    /// Dummy-call program point carrying the external-call boundary.
    node: String,
    /// Canonical C function name resolved by the existing Rust->SVF bridge.
    callee: String,
    /// Closed ND1 vocabulary: certified_absent | observed_may_deallocate | unresolved.
    status: &'static str,
    /// Primary evidence basis. Historical bases remain stable for reproducibility.
    basis: &'static str,
    /// Independent evidence that supports the same status without replacing the
    /// historical primary basis.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    corroborating_bases: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ExternalFormalMemoryEffectRecord {
    /// Rust MIR call node on which the existing CQPL read/write event is emitted.
    node: String,
    /// Exact external function symbol admitted by the closed semantic contract.
    callee: String,
    /// Versioned semantic family; this is not an SVF variable or function object.
    semantic_class: &'static str,
    /// Zero-based formal parameter position carrying the memory effect.
    formal_index: usize,
    /// Closed event vocabulary reused by CQPL truth: read | write.
    access: &'static str,
    /// Raw MIR local used by the node-level event label.
    event_variable: String,
    /// Function-scoped canonical Rust ProgramVarId for the same actual operand.
    actual_variable: String,
    /// Closed EFM2 extent vocabulary: bytes_from_formal | c_string_until_nul.
    extent_kind: &'static str,
    /// Byte-count formal for bytes_from_formal; absent for c_string_until_nul.
    #[serde(skip_serializing_if = "Option::is_none")]
    extent_argument_index: Option<usize>,
    /// Primary producer proof basis, frozen as a closed vocabulary.
    basis: &'static str,
    /// Frozen provider/documentation semantics from which the CREMA contract was derived.
    /// These are provenance references, not runtime-observed SVF/TLI facts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    semantic_sources: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct TypedEdgeRecord {
    source: String,
    destination: String,
    flow: &'static str,
    label: Option<String>,
    source_label: Option<String>,
    destination_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct OpaqueMemoryEffectRecord {
    node: String,
    effect: &'static str,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct FfiArgumentIdentityRecord {
    /// External DummyCall at which the positional Rust -> C binding occurs.
    node: String,
    /// Canonical C callee from the replicated LLVM entry edge.
    callee: String,
    /// Replicated Rust callsite used to scope the SVF formal ProgramVarId.
    callsite: String,
    /// Zero-based Bmulti-certified argument position.
    arg_index: usize,
    /// Canonical Rust actual ProgramVarId.
    actual_variable: String,
    /// Canonical callsite-scoped C formal ProgramVarId.
    formal_variable: String,
    /// MAY AbstractAllocIds proven equal on both sides of the positional bridge.
    allocations: Vec<String>,
    /// Closed certainty vocabulary. R2 does not introduce MUST identity.
    certainty: &'static str,
    /// Producer proof basis for the cross-language identity transfer.
    basis: &'static str,
    /// Positional certificate required by Bmulti.
    formal_mapping_basis: &'static str,
    /// Independent solved Andersen MAY set for this formal, when available.
    svf_may_points_to: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    svf_points_to_basis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct AllocationExistenceGuardRecord {
    allocation: String,
    producer_call_node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    producer_call_canonical_node: Option<String>,
    predicate_call_node: String,
    switch_node: String,
    tested_variable: String,
    predicate_result_variable: String,
    null_successor: String,
    non_null_successor: String,
    callee_def_path: String,
    allocation_return_basis: &'static str,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ReallocationBoundaryRecord {
    node: String,
    source_allocation: String,
    source_variable: String,
    result_variable: String,
    family: &'static str,
    operation: &'static str,
    certainty: &'static str,
    status: &'static str,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ConditionalReallocationResultDeallocationRecord {
    node: String,
    /// Logical realloc-result local whose allocation obligation is discharged.
    variable: String,
    /// MIR local actually passed to `free`. CR1-v2 keeps this distinct from
    /// `variable` because rustc may materialize a one-step copy temporary.
    argument_variable: String,
    callee_def_path: String,
    family: &'static str,
    operation: &'static str,
    basis: &'static str,
    /// CR1-v2 producer proof that the observed free argument is either the
    /// realloc-result local itself or one exact MIR `copy` of that local.
    argument_correlation_basis: &'static str,
    /// CR1-v2 producer proof: every CFG path from the certified success
    /// successor to this free(q) keeps the realloc result local unchanged and
    /// does not consume its allocation obligation first.
    value_flow_basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ConditionalReallocationRecord {
    source_allocation: String,
    reallocation_node: String,
    source_variable: String,
    result_variable: String,
    source_existence_predicate_call_node: String,
    outcome_predicate_call_node: String,
    /// MIR local actually consumed by the `is_null` call. It may be the
    /// realloc result itself or one exact rustc-generated copy temporary.
    outcome_argument_variable: String,
    outcome_predicate_result_variable: String,
    outcome_switch_node: String,
    failure_successor: String,
    success_successor: String,
    reallocation_callee_def_path: String,
    outcome_callee_def_path: String,
    family: &'static str,
    operation: &'static str,
    certainty: &'static str,
    size_semantics: &'static str,
    status: &'static str,
    basis: &'static str,
    /// CR1-v2 producer proof that q.is_null() is the immediate canonical CFG
    /// successor of the realloc call.
    outcome_correlation_basis: &'static str,
    /// CR1-v2 producer proof that the `is_null` receiver is either the realloc
    /// result local itself (without an intra-block redefinition) or one exact
    /// MIR `copy` temporary derived from that unchanged result local.
    outcome_value_flow_basis: &'static str,
    result_deallocations: Vec<ConditionalReallocationResultDeallocationRecord>,
}

impl AllocationContract {
    fn v1(family: &'static str, operation: &'static str, language: &'static str) -> Self {
        Self {
            family,
            operation,
            language,
            basis: None,
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        }
    }

    fn v2_deallocator(
        family: &'static str,
        operation: &'static str,
        language: &'static str,
        basis: &'static str,
    ) -> Self {
        Self {
            family,
            operation,
            language,
            basis: Some(basis),
            owner_def_path: None,
            allocator_def_path: None,
            callee_def_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AnnotatedNode {
    id: String,
    successors: Vec<String>,
    labels: Vec<EventLabel>,
    /// v6P structural MIR labels.  They are emitted only under the explicit
    /// `mir_semantic_labels_v1` capability and denote block-level presence,
    /// not an invented statement ordering inside the block.
    #[serde(skip_serializing_if = "Option::is_none")]
    semantic_labels: Option<Vec<String>>,
    /// W1 read-only source provenance.  This payload is diagnostic-only and is
    /// never consumed by CREMA abstract interpretation or CQPL truth semantics.
    /// Event anchors are tied to the same raw syntactic events used to build
    /// allocation_labels, so source reporting does not reconstruct semantics
    /// from node names or pretty-printed source text.
    #[serde(skip_serializing_if = "Option::is_none")]
    source_provenance: Option<NodeSourceProvenance>,
    /// Allocation-centric event labels derived from the canonical identity
    /// fixed point.  Present only in schema v2.
    #[serde(skip_serializing_if = "Option::is_none")]
    allocation_labels: Option<Vec<AllocationEventLabel>>,
    /// v6S-r1 allocation-disposition/escape provenance. These records are
    /// observational MAY facts and are not consumed by the frozen v6R query
    /// semantics. Present only with capability allocation_disposition_v1.
    #[serde(skip_serializing_if = "Option::is_none")]
    allocation_disposition: Option<Vec<AllocationDispositionRecord>>,
    /// A3.7 satellite panic/unwind lifecycle state. Under
    /// `panic_lifecycle_state_v1` this field is present on every schema-v2
    /// node, including as an empty array. Records are MAY-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    panic_lifecycle: Option<Vec<PanicLifecycleRecord>>,
    /// A3.7 lifecycle producer coverage. `complete` means absence of a MAY
    /// lifecycle witness may be refuted at this node; `unresolved` means the
    /// producer lost precision on at least one relevant operation along a path.
    /// Present only with capability `panic_lifecycle_state_v2`.
    #[serde(skip_serializing_if = "Option::is_none")]
    panic_lifecycle_coverage: Option<&'static str>,
    /// Auditable scoped post-state identity relation at this node. Present only in v2.
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<NodeIdentityAnnotation>,
    /// Auditable intra-node MAY summary used to resolve allocation-centric
    /// event labels. Present only in v2.
    #[serde(skip_serializing_if = "Option::is_none")]
    event_identity: Option<NodeIdentityAnnotation>,
    /// Capability allocation_state_v1: pointwise MAY projection of the existing
    /// ProgramVar post-state through the post allocation-identity relation.
    #[serde(skip_serializing_if = "Option::is_none")]
    allocation_post: Option<AbstractAllocationMemoryAnnotation>,
    /// CREMA Phase 5 stores one converged per-node state after applying the
    /// node transformer. It does not retain a separate stable Pi#_pre map.
    /// Do not fabricate one: v1/v2 export an explicit empty pre-memory.
    pre: AbstractMemoryAnnotation,
    post: AbstractMemoryAnnotation,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct AllocationEventLabel {
    predicate: &'static str,
    allocation: String,
    certainty: &'static str,
    /// Capability allocation_contracts_v1: semantic deallocator contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    deallocator_contract: Option<AllocationContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct PanicLifecycleRecord {
    allocation: String,
    certainty: &'static str,
    may_own: bool,
    may_partial_drop: bool,
    may_stale_owner: bool,
    may_committed: bool,
    may_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct AllocationDispositionRecord {
    allocation: String,
    /// Closed allocation-disposition vocabulary.  The historical v1 subset
    /// remains frozen; CString handoff/reclaim records require the additive
    /// allocation_disposition_v2 capability.
    kind: &'static str,
    /// Identity resolution is MAY in v6S-r1.  A singleton AbstractAllocId is
    /// not promoted to a concrete MUST fact.
    certainty: &'static str,
    /// Effect on the deallocation obligation, not on pointer-variable storage.
    obligation_effect: &'static str,
    /// Producer proof basis.  Consumers validate the closed kind/basis/effect
    /// tuple instead of inferring semantics from DefPath strings.
    basis: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_variable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_variable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callee_def_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct NodeIdentityAnnotation {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    access_bases: Vec<IdentityPointsToRecord>,
    points_to: Vec<IdentityPointsToRecord>,
    stack_refs: Vec<IdentityStackRefsRecord>,
    place_points_to: Vec<IdentityPlacePointsToRecord>,
    place_stack_refs: Vec<IdentityPlaceStackRefsRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct IdentityPointsToRecord {
    variable: String,
    allocations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct IdentityStackRefsRecord {
    variable: String,
    places: Vec<PlaceId>,
}

#[derive(Debug, Clone, Serialize)]
struct IdentityPlacePointsToRecord {
    place: PlaceId,
    allocations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct IdentityPlaceStackRefsRecord {
    place: PlaceId,
    targets: Vec<PlaceId>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct AbstractMemoryAnnotation {
    cells: Vec<AbstractCell>,
}

#[derive(Debug, Clone, Serialize)]
struct AbstractCell {
    aliases: Vec<String>,
    value: &'static str,
}

#[derive(Debug, Clone, Default, Serialize)]
struct AbstractAllocationMemoryAnnotation {
    cells: Vec<AbstractAllocationCell>,
}

#[derive(Debug, Clone, Serialize)]
struct AbstractAllocationCell {
    allocation: String,
    value: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct EventLabel {
    predicate: &'static str,
    variable: String,
}

/// W1 source-grounding payload.  These records are deliberately orthogonal to
/// the abstract state and event semantics: they explain where producer facts
/// came from, but they never create or strengthen a fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ParsedSourceSpan {
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct SourceAnchor {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    statement_index: Option<usize>,
    raw_span: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parsed_span: Option<ParsedSourceSpan>,
    basis: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct AllocationEventSourceRecord {
    predicate: &'static str,
    allocation: String,
    certainty: &'static str,
    anchors: Vec<SourceAnchor>,
}

#[derive(Debug, Clone, Serialize)]
struct NodeSourceProvenance {
    language: &'static str,
    anchors: Vec<SourceAnchor>,
    allocation_events: Vec<AllocationEventSourceRecord>,
}

type EventSourceMap = BTreeMap<EventLabel, BTreeSet<SourceAnchor>>;

static MIR_LOCAL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Local\(_[0-9]+\)|_[0-9]+").expect("valid MIR local regex"));
static MIR_DEREF_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\*\s*(Local\(_[0-9]+\)|_[0-9]+)").expect("valid MIR deref regex"));
static CLOSURE_FIELD_COPY_FOR_DEREF_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^deref_copy\s+\((?:\(\*_[0-9]+\)|_[0-9]+)\.([0-9]+):")
        .expect("valid closure CopyForDeref regex")
});
static DIRECT_DEREF_VALUE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:copy|move)\s+\(\*\s*(Local\(_[0-9]+\)|_[0-9]+)\)$")
        .expect("valid direct dereference value regex")
});
static LLVM_IR_LHS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"%([0-9]+)\s*=").expect("valid LLVM lhs regex"));
static LLVM_FREE_ARG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@free\([^%]*%([0-9]+)").expect("valid LLVM free regex"));
static RUST_SOURCE_SPAN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<file>.+):(?P<sl>[0-9]+):(?P<sc>[0-9]+):\s*(?P<el>[0-9]+):(?P<ec>[0-9]+)(?:\s+\(#[0-9]+\))?$")
        .expect("valid rust source span regex")
});

pub fn export_cqpl_annotated_icfg(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    entry: &str,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    export_cqpl_annotated_icfg_versioned(icfg, abs_state, None, None, None, None, entry, 1, output_path)
}

#[cfg(test)]
pub fn export_cqpl_annotated_icfg_with_identity(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    identity_state: &AllocationIdentityState,
    entry: &str,
    schema_version: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let fixture_metadata = RustActivationUnwindMetadataV1::new(icfg.rust_calls.iter().map(|call|
        crate::structs::RustActivationUnwindMetadata::callback(call, None)))?;
    export_cqpl_annotated_icfg_versioned(
        icfg,
        abs_state,
        Some(identity_state),
        Some(identity_state),
        Some(&fixture_metadata),
        None,
        entry,
        schema_version,
        output_path,
    )
}

pub fn export_cqpl_annotated_icfg_with_identity_and_disposition(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    identity_state: &AllocationIdentityState,
    disposition_identity_state: &AllocationIdentityState,
    activation_unwind_metadata: Option<&RustActivationUnwindMetadataV1>,
    unwind_scope_evidence: Option<&RustUnwindScopeEvidenceV1>,
    entry: &str,
    schema_version: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    export_cqpl_annotated_icfg_versioned(
        icfg,
        abs_state,
        Some(identity_state),
        Some(disposition_identity_state),
        activation_unwind_metadata,
        unwind_scope_evidence,
        entry,
        schema_version,
        output_path,
    )
}

fn export_cqpl_annotated_icfg_versioned(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    identity_state: Option<&AllocationIdentityState>,
    disposition_identity_state: Option<&AllocationIdentityState>,
    activation_unwind_metadata: Option<&RustActivationUnwindMetadataV1>,
    unwind_scope_evidence: Option<&RustUnwindScopeEvidenceV1>,
    entry: &str,
    schema_version: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    if !matches!(schema_version, 1 | 2) {
        return Err(format!("unsupported CQPL annotated ICFG schema version {schema_version}; expected 1 or 2").into());
    }
    if schema_version == 2 && (identity_state.is_none() || disposition_identity_state.is_none()) {
        return Err("CQPL annotated ICFG schema v2 requires legacy and disposition AllocationIdentityState views".into());
    }
    let node_ids: BTreeSet<String> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    if !node_ids.contains(entry) {
        return Err(format!("CQPL export entry node '{entry}' is not present in the GlobalICFG").into());
    }

    let mut successors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &icfg.icfg_edges {
        if !node_ids.contains(&edge.source) || !node_ids.contains(&edge.destination) {
            return Err(format!(
                "canonical ICFG edge relation is not closed over the node domain: '{}' -> '{}'",
                edge.source, edge.destination
            )
            .into());
        }
        successors
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
    }

    // v6K invariant: CREMA and CQPL consume exactly the same canonical ICFG
    // edge relation.  Internal Rust call/return edges must already be explicit
    // in `icfg_edges`; the exporter is no longer allowed to synthesize a second
    // relation.  Validate the metadata/edge agreement instead.
    let rust_call_return_relations = if schema_version == 2 {
        Some(rust_call_return_relation_records(icfg, &successors, &node_ids)?)
    } else {
        validate_canonical_internal_rust_edges(icfg, &successors)?;
        None
    };

    let typed_edges = if schema_version == 2 {
        let records = typed_edge_records(icfg);
        validate_typed_edge_projection(&records, &successors)?;
        Some(records)
    } else {
        None
    };

    let rust_call_unwind_relations = if schema_version == 2 {
        let empty = RustActivationUnwindMetadataV1::new(Vec::new())?;
        let metadata = match activation_unwind_metadata {
            Some(metadata) => metadata,
            None if icfg.rust_calls.is_empty() => &empty,
            None => return Err("missing F6B0 activation unwind metadata at exporter boundary".into()),
        };
        Some(rust_call_unwind_relation_records(icfg, metadata,
            rust_call_return_relations.as_ref().expect("v2 returns"),
            typed_edges.as_ref().expect("v2 typed edges"), &node_ids)?)
    } else { None };

    let rust_unwind_edge_scopes = if schema_version == 2 {
        Some(rust_unwind_edge_scope_records(icfg, typed_edges.as_ref().expect("v2 edges"),
            rust_call_unwind_relations.as_ref().expect("v2 unwind relations"), unwind_scope_evidence)?)
    } else { None };

    let reachable = reachable_from(entry, &successors);
    if schema_version == 2 {
        for edge in &icfg.icfg_edges {
            if reachable.contains(&edge.source)
                && edge
                    .label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("UNRESOLVED_"))
            {
                return Err(format!(
                    "schema-v2 fail-closed: reachable unresolved control-flow summary at '{}' -> '{}': {}",
                    edge.source,
                    edge.destination,
                    edge.label.as_deref().unwrap_or("UNRESOLVED")
                )
                .into());
            }
        }
    }

    // Read-only cross-node maps used only to attach C free/load/store labels to
    // the same scoped SVF identifiers already used by CREMA's abstract state.
    let llvm_names = LlvmNameResolver::build(icfg);
    let ffi_functions = load_ffi_functions("./ffi_functions.json").unwrap_or_default();
    let represented_c_functions = represented_c_function_names(icfg);

    // Closure bodies can materialize the same captured pointer into distinct MIR
    // temporaries at different uses. The legacy detector already reconstructs
    // this value-flow relation from CopyForDeref closure-field reads. The
    // abstract post-state, however, does not necessarily keep those temporaries
    // in one pointwise alias component. Build a read-only, closure-local MAY
    // relation for syntactic event labels so two drops/uses of the same captured
    // value remain the same CQPL program object. This does not alter Pi#_post.
    let closure_event_aliases = build_closure_event_aliases(icfg, abs_state);

    // A3.7 Stage 3: compute the real panic lifecycle satellite domain from
    // canonical MIR/ICFG events plus the existing allocation-identity state.
    // The capability is declared only when this producer is active.
    let panic_lifecycle_state = if schema_version == 2 && panic_unwind_lifecycle_v1_enabled() {
        Some(
            fixed_point_real_panic_lifecycle(
                icfg,
                identity_state.expect("checked schema-v2 identity state"),
                entry,
            )
            .map_err(|err| -> Box<dyn Error> { err.into() })?,
        )
    } else {
        None
    };

    let mut variable_ids = BTreeSet::new();
    let mut nodes = Vec::with_capacity(icfg.ordered_nodes.len());

    for (node_id, node) in &icfg.ordered_nodes {
        let post_mem = abs_state.get(node_id).unwrap_or_default();
        let post = memory_annotation(&post_mem);
        for cell in &post.cells {
            variable_ids.extend(cell.aliases.iter().cloned());
        }

        // Quantifier domain is intentionally larger than the sparse abstract
        // memory: include syntactically occurring Rust/C program variables too.
        variable_ids.extend(collect_program_variables(node_id, node));

        // Preserve the raw syntactic events for schema-v2 allocation binding.
        // The historical closure-event expansion remains v1 compatibility only;
        // allocation-centric labels are resolved through AllocationIdentityState
        // and therefore do not depend on that workaround.  W1 source provenance
        // is produced from the very same event-occurrence map, so reporting
        // cannot silently drift from the semantic event vocabulary.
        let event_mem = if schema_version == 2 {
            identity_state
                .and_then(|state| state.event_by_node.get(node_id))
                .cloned()
                .unwrap_or_default()
        } else {
            AllocationIdentityMemory::default()
        };
        let mut event_sources = event_sources_for_node(node_id, node, &llvm_names, &ffi_functions, &represented_c_functions);
        if schema_version == 2 {
            add_realloc_null_allocation_event_source(node_id, node, &event_mem, &mut event_sources);
        }
        let raw_labels: Vec<EventLabel> = event_sources.keys().cloned().collect();
        let allocation_labels = if schema_version == 2 {
            Some(allocation_labels_for_node(node_id, node, &raw_labels, &event_mem, &ffi_functions, schema_version))
        } else {
            None
        };
        let source_provenance = if schema_version == 2 {
            Some(node_source_provenance(
                node_id,
                node,
                &event_sources,
                &event_mem,
                allocation_labels.as_deref().unwrap_or(&[]),
            ))
        } else {
            None
        };
        if schema_version == 2
            && reachable.contains(node_id)
            && raw_labels.iter().any(|label| label.predicate == "alloc")
            && allocation_labels
                .as_ref()
                .is_some_and(|labels| labels.iter().all(|label| label.predicate != "alloc"))
        {
            return Err(format!(
                "schema-v2 fail-closed: reachable modeled alloc event at '{node_id}' has no AbstractAllocId/event_identity"
            )
            .into());
        }
        let (identity, event_identity, allocation_post, allocation_disposition) = if schema_version == 2 {
            let post_identity = identity_state
                .and_then(|state| state.by_node.get(node_id))
                .cloned()
                .unwrap_or_default();
            let event_identity_mem = identity_state
                .and_then(|state| state.event_by_node.get(node_id))
                .cloned()
                .unwrap_or_default();

            // Schema-v2 closure invariant: every ProgramVarId serialized by the
            // identity relation belongs to the declared program-variable domain.
            // Identity uses globally scoped canonical IDs, so keep those IDs
            // distinct from the legacy unscoped Name strings used by Pi#_post.
            variable_ids.extend(collect_identity_program_variables(&post_identity));
            variable_ids.extend(collect_identity_program_variables(&event_identity_mem));

            let disposition_event_identity = disposition_identity_state
                .and_then(|state| state.event_by_node.get(node_id))
                .cloned()
                .unwrap_or_default();
            let disposition_post_identity = disposition_identity_state
                .and_then(|state| state.by_node.get(node_id))
                .cloned()
                .unwrap_or_default();

            let disposition = allocation_disposition_for_node(
                node_id,
                node,
                &disposition_event_identity,
                &disposition_post_identity,
                allocation_labels.as_deref().unwrap_or(&[]),
            );

            // Schema closure for v6S observational provenance only.
            //
            // The legacy identity view deliberately remains frozen, but
            // allocation_disposition may refer to canonical source/target
            // variables observed only by the disposition identity view.
            // Declare exactly those referenced variables without merging the
            // disposition identity relation into legacy identity/event state.
            for record in &disposition {
                for variable in [
                    record.source_variable.as_ref(),
                    record.target_variable.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    variable_ids.insert(variable.clone());
                }
            }

            (
                Some(identity_annotation(&post_identity)),
                Some(identity_annotation(&event_identity_mem)),
                Some(allocation_memory_annotation(node_id, node, &post_mem, &post_identity)),
                Some(disposition),
            )
        } else {
            (None, None, None, None)
        };

        let mut labels = raw_labels;
        expand_closure_event_labels(node_id, &mut labels, &closure_event_aliases);
        for label in &labels {
            variable_ids.insert(label.variable.clone());
        }

        let semantic_labels = if mir_semantics_v2_enabled() {
            match node {
                GlobalICFGNode::Mir(block) => Some(semantic_labels_for_block(block)),
                _ => Some(Vec::new()),
            }
        } else {
            None
        };

        let lifecycle_memory = panic_lifecycle_state
            .as_ref()
            .map(|state| state.get(node_id));
        let panic_lifecycle = lifecycle_memory
            .as_ref()
            .map(panic_lifecycle_records);
        let panic_lifecycle_coverage = lifecycle_memory
            .as_ref()
            .map(|memory| memory.coverage().as_str());

        nodes.push(AnnotatedNode {
            id: node_id.clone(),
            successors: successors
                .get(node_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            labels,
            semantic_labels,
            source_provenance,
            allocation_labels,
            allocation_disposition,
            panic_lifecycle,
            panic_lifecycle_coverage,
            identity,
            event_identity,
            allocation_post,
            pre: AbstractMemoryAnnotation::default(),
            post,
        });
    }

    let allocation_existence_guards = if schema_version == 2 && mir_semantics_v2_enabled() {
        let records = allocation_existence_guard_records(
            icfg,
            identity_state.expect("checked schema-v2 identity state"),
        );
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    let reallocation_boundaries = if schema_version == 2 {
        let records = reallocation_boundary_records(
            icfg,
            identity_state.expect("checked schema-v2 identity state"),
            &ffi_functions,
        );
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    let conditional_reallocations = if schema_version == 2 && mir_semantics_v2_enabled() {
        let records = conditional_reallocation_records(
            icfg,
            &ffi_functions,
            reallocation_boundaries.as_deref().unwrap_or_default(),
            allocation_existence_guards.as_deref().unwrap_or_default(),
        );
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    let external_formal_memory_effects = if schema_version == 2 && mir_semantics_v2_enabled() {
        let records = external_formal_memory_effect_records(
            icfg,
            &ffi_functions,
            &represented_c_functions,
        );
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    // AGE1 protocol closure: every program variable referenced by the
    // diagnostic-only existence-guard sidecar must also be declared in the
    // top-level variable catalog.  The predicate-result temporary is often a
    // scalar MIR local and therefore need not occur in allocation identity or
    // event labels; omitting it would make an otherwise valid proof-carrying
    // record fail the checker boundary as an undeclared variable.
    close_variable_catalog_over_allocation_existence_guards(
        &mut variable_ids,
        allocation_existence_guards.as_deref(),
    );
    close_variable_catalog_over_reallocation_boundaries(
        &mut variable_ids,
        reallocation_boundaries.as_deref(),
    );
    close_variable_catalog_over_conditional_reallocations(
        &mut variable_ids,
        conditional_reallocations.as_deref(),
    );

    close_variable_catalog_over_external_formal_memory_effects(
        &mut variable_ids,
        external_formal_memory_effects.as_deref(),
    );

    let (return_records, return_bindings) = if schema_version == 2 {
        external_return_relation_records(icfg, &ffi_functions, &represented_c_functions)
    } else { (vec![], vec![]) };
    for record in &return_records {
        variable_ids.insert(record.result_variable.clone());
        if let Some(source) = &record.source_actual_variable { variable_ids.insert(source.clone()); }
    }
    let external_return_relations = if return_records.is_empty() { None } else { Some(return_records) };
    // ERR1's historical binding is replaced on the wire by ELE1 below. The
    // return records retain exactly their accepted semantic fields.
    let _return_bindings = return_bindings;
    let external_return_call_bindings = None;

    let negative_records = if schema_version == 2 {
        external_negative_evidence_records(icfg, &ffi_functions, &represented_c_functions)
    } else { vec![] };
    for record in &negative_records {
        variable_ids.extend(record.call_arguments.iter().flatten().cloned());
    }
    let external_negative_evidence = (!negative_records.is_empty()).then_some(negative_records);

    let dcp1_records = if schema_version == 2 {
        external_deallocation_call_provenance_records(
            icfg, &ffi_functions, &represented_c_functions, &variable_ids,
        )
    } else { vec![] };
    let external_deallocation_call_provenance = (!dcp1_records.is_empty()).then_some(dcp1_records);

    if variable_ids.is_empty() {
        return Err("CQPL annotated ICFG contains no program variables".into());
    }

    let variables: Vec<ProgramVariable> = variable_ids
        .into_iter()
        .map(|id| ProgramVariable {
            language: language_of(&id),
            id,
        })
        .collect();

    let opaque_memory_effects = if schema_version == 2 {
        let records = opaque_memory_effect_records(icfg, &reachable);
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    let mut allocations = if schema_version == 2 {
        Some(allocation_catalog(identity_state.expect("checked schema-v2 identity state"), schema_version))
    } else {
        None
    };
    // An opaque Rust boundary can allocate storage whose concrete site is not
    // present in an exact imported MIR body. Introduce one finite MAY
    // allocation-site identity per opaque boundary so
    // allocation-quantified CQPL formulas cannot refute hidden allocations by
    // ranging only over the pre-summary catalog.  Event/state truth for this
    // site is intentionally UNKNOWN through opaque_memory_effect_v1.
    if let (Some(opaque), Some(catalog)) = (opaque_memory_effects.as_ref(), allocations.as_mut()) {
        append_opaque_summary_allocations(catalog, opaque);
    }
    let external_deallocation_effects = if schema_version == 2 {
        Some(external_deallocation_effect_records(icfg))
    } else {
        None
    };
    let (external_call_bindings, external_library_effects) = if schema_version == 2 {
        external_library_effect_envelopes(
            icfg, &variables, allocations.as_deref().unwrap_or_default(),
            reallocation_boundaries.as_deref().unwrap_or_default(),
            external_deallocation_call_provenance.as_deref().unwrap_or_default(),
            external_formal_memory_effects.as_deref().unwrap_or_default(),
            external_return_relations.as_deref().unwrap_or_default(),
            external_negative_evidence.as_deref().unwrap_or_default(),
        )
    } else { (None, None) };
    let (call_semantic_coverage, call_semantic_coverage_complete) =
        if icfg.dependency_body_ingestion_v1.is_some() {
            let (records, complete) = dep1_call_semantic_coverage(
                icfg,
                external_call_bindings.as_deref().unwrap_or_default(),
                external_library_effects.as_deref().unwrap_or_default(),
                &reachable,
            ).expect("DEP1 export presence checked");
            (Some(records), Some(complete))
        } else { (None, None) };
    let ffi_argument_identity = if schema_version == 2 {
        let records = ffi_argument_identity_records(
            icfg,
            identity_state.expect("checked schema-v2 identity state"),
        )?;
        if records.is_empty() { None } else { Some(records) }
    } else {
        None
    };

    let has_external_return_relations = external_return_relations.is_some();
    let has_ene1 = external_negative_evidence.is_some();
    let has_dcp1 = external_deallocation_call_provenance.is_some();
    let has_ele1 = external_call_bindings.is_some();
    let output = AnnotatedIcfg {
        context_execution_graph_v1: icfg.dependency_body_ingestion_v1.as_ref()
            .and_then(|dep| dep.context_execution_graph_v1.clone()),
        call_semantic_coverage,
        call_semantic_coverage_complete,
        external_call_bindings,
        external_library_effects,
        external_deallocation_call_provenance,
        external_negative_evidence,
        external_return_relations,
        external_return_call_bindings,
        rust_call_return_relations,
        rust_call_unwind_relations,
        rust_unwind_edge_scopes,
        schema_version,
        entry: entry.to_string(),
        capabilities: if schema_version == 2 {
            let mut caps = vec![
                "typed_edge_flow_v1",
                // W1 diagnostic-only source grounding.  The checker consumes
                // this capability exclusively for explanation certificates.
                "source_provenance_v1",
                "allocation_contracts_v1",
                "allocation_contracts_v2",
                // Bcontract-DROP1: additive proof vocabulary for exact
                // core::mem::drop(Box<T, Global>) calls.  v2 remains frozen;
                // v3 explicitly gates the new call-site basis.
                "allocation_contracts_v3",
                // HT2-R2.4: String owner-boundary deallocation is an explicit
                // additive contract.  v4 is required whenever the producer
                // emits rust_string_global_drop; nested Vec implementation
                // drops for String-origin resources are coalesced below.
                "allocation_contracts_v4",
                "allocation_state_v1",
                "allocation_disposition_v1",
                // B1.1: CString ownership handoff/reclaim extends the frozen
                // v6S-r1 disposition vocabulary.  Keep v1 declared for
                // backward-compatible base semantics and advertise the
                // additive closed refinement explicitly as v2.
                "allocation_disposition_v2",
                // Additive MAY identity evidence through return-rooted fields.
                "return_projection_escape_v1",
                "rust_call_return_relation_v1",
                "rust_call_unwind_relation_v1",
                "rust_unwind_edge_scope_v1",
                // AP1: producer-certified, target-independent identity transport
                // through core::sync::atomic::AtomicPtr storage.  This capability
                // changes identity precision only; it adds no allocation, drop,
                // or CellValue semantics.
                "atomic_ptr_identity_summary_v1",
                // Bcontract-ND1 + EFX1: explicit external-call deallocation
                // evidence. Frozen leaf/structural behavior is preserved; a
                // bodyless external boundary may additionally close from a
                // capability-gated explicit/TLI LLVM16 contract.
                "external_deallocation_effects_v1",
            ];
            if external_formal_memory_effects.is_some() {
                caps.push("external_formal_memory_effects_v2");
            }
            if has_ene1 { caps.push("external_negative_evidence_v1"); }
            if has_dcp1 { caps.push("external_deallocation_call_provenance_v1"); }
            if has_ele1 { caps.push("external_library_effects_v1"); }
            if let Some(context_graph)=icfg.dependency_body_ingestion_v1.as_ref()
                .and_then(|dep| dep.context_execution_graph_v1.as_ref())
            {
                caps.push("context_explicit_icfg_v1");
                match context_graph.get("capability").and_then(serde_json::Value::as_str) {
                    Some("context_explicit_icfg_v3") => {
                        caps.push("context_explicit_icfg_v3");
                        caps.push("recursive_scc_local_k1_v1");
                    }
                    Some("context_explicit_icfg_v4") => {
                        caps.push("context_explicit_icfg_v4");
                        caps.push("dependency_region_local_k1_v1");
                        if context_graph.get("recursive_context_policy").and_then(serde_json::Value::as_str)
                            == Some("scc_local_call_string_k1")
                        {
                            caps.push("recursive_scc_local_k1_v1");
                        }
                    }
                    _ => {}
                }
            }
            if icfg.dependency_body_ingestion_v1.as_ref().is_some_and(|dep|
                dep.exact_call_bindings.iter().any(|call|
                    call.get("body_status").and_then(serde_json::Value::as_str)==Some("dynamic_drop_summary")
                        && call.get("dynamic_drop_summary_v1").and_then(|v|v.get("semantics"))
                            .and_then(serde_json::Value::as_str)==Some("opaque_dynamic_destructor_may_effect_v1")))
            {
                caps.push("dynamic_trait_drop_summary_v1");
            }
            if icfg.dependency_body_ingestion_v1.as_ref()
                .and_then(|dep|dep.context_execution_graph_v1.as_ref())
                .and_then(|ctx|ctx.get("opaque_higher_order_summaries"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|records|!records.is_empty())
            {
                caps.push("opaque_higher_order_callback_summary_v1");
            }
            if opaque_memory_effects.is_some() {
                caps.push("opaque_memory_effect_v1");
            }
            if has_external_return_relations {
                caps.push("external_return_relations_v1");
            }
            if icfg.llvm_memory_effects.is_some() {
                caps.push("llvm_memory_effects_v1");
            }
            if icfg.svf_solved_points_to.is_some() {
                caps.push("svf_solved_points_to_v1");
            }
            if ffi_argument_identity.is_some() {
                caps.push("ffi_argument_identity_v1");
            }
            if allocation_existence_guards.is_some() {
                caps.push("allocation_existence_guards_v1");
            }
            if let Some(records) = reallocation_boundaries.as_deref() {
                // RBF2 extends the existing payload rather than replacing it:
                // every non-empty payload remains an RBF1 surface, while the
                // v2 capability is advertised only when at least one record
                // uses the allocator-family-consumer basis.
                caps.push("reallocation_boundaries_v1");
                if records.iter().any(|record| {
                    record.basis == "rust_foreign_decl_c_realloc_allocptr_family_v2"
                }) {
                    caps.push("reallocation_boundaries_v2");
                }
            }
            if conditional_reallocations.is_some() {
                caps.push("conditional_reallocations_v1");
                caps.push("conditional_reallocations_v2");
            }
            if mir_semantics_v2_enabled() {
                caps.push("mir_semantic_labels_v1");
                caps.push("mir_semantics_v2");
            }
            if panic_unwind_lifecycle_v1_enabled() {
                caps.push("panic_unwind_lifecycle_v1");
                caps.push("panic_lifecycle_state_v1");
                caps.push("panic_lifecycle_state_v2");
            }
            Some(caps)
        } else {
            None
        },
        typed_edges,
        opaque_memory_effects,
        variables,
        allocations,
        external_deallocation_effects,
        external_formal_memory_effects,
        llvm_memory_effects: if schema_version == 2 { icfg.llvm_memory_effects.clone() } else { None },
        svf_solved_points_to: if schema_version == 2 { icfg.svf_solved_points_to.clone() } else { None },
        ffi_argument_identity,
        allocation_existence_guards,
        reallocation_boundaries,
        conditional_reallocations,
        nodes,
    };

    let file = File::create(output_path)?;
    serde_json::to_writer_pretty(file, &output)?;
    Ok(())
}

fn close_variable_catalog_over_allocation_existence_guards(
    variable_ids: &mut BTreeSet<Name>,
    guards: Option<&[AllocationExistenceGuardRecord]>,
) {
    let Some(guards) = guards else { return; };
    for guard in guards {
        variable_ids.insert(guard.tested_variable.clone());
        variable_ids.insert(guard.predicate_result_variable.clone());
    }
}

fn close_variable_catalog_over_reallocation_boundaries(
    variable_ids: &mut BTreeSet<Name>,
    boundaries: Option<&[ReallocationBoundaryRecord]>,
) {
    let Some(boundaries) = boundaries else { return; };
    for boundary in boundaries {
        variable_ids.insert(boundary.source_variable.clone());
        variable_ids.insert(boundary.result_variable.clone());
    }
}

fn close_variable_catalog_over_conditional_reallocations(
    variable_ids: &mut BTreeSet<Name>,
    records: Option<&[ConditionalReallocationRecord]>,
) {
    let Some(records) = records else { return; };
    for record in records {
        variable_ids.insert(record.source_variable.clone());
        variable_ids.insert(record.result_variable.clone());
        variable_ids.insert(record.outcome_argument_variable.clone());
        variable_ids.insert(record.outcome_predicate_result_variable.clone());
        for deallocation in &record.result_deallocations {
            variable_ids.insert(deallocation.variable.clone());
            variable_ids.insert(deallocation.argument_variable.clone());
        }
    }
}

fn close_variable_catalog_over_external_formal_memory_effects(
    variable_ids: &mut BTreeSet<Name>,
    records: Option<&[ExternalFormalMemoryEffectRecord]>,
) {
    let Some(records) = records else { return; };
    for record in records {
        variable_ids.insert(record.actual_variable.clone());
        variable_ids.insert(record.event_variable.clone());
    }
}

fn is_raw_pointer_is_null_def_path(path: &str) -> bool {
    let raw_pointer_impl = path.contains("::ptr::mut_ptr::<impl *mut ")
        || path.contains("::ptr::const_ptr::<impl *const ");
    raw_pointer_impl && path.ends_with(">::is_null")
}

fn canonical_rust_block_target(node_id: &str, raw_target: &str) -> Option<String> {
    let scope = mir_function_scope_from_node_id(node_id)?;
    let block = raw_target.trim();
    let digits = block.strip_prefix("bb")?;
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(format!("{scope}::{block}"))
}

fn parse_switch_target(raw: &str) -> Option<(u128, &str)> {
    let inner = raw.trim().strip_prefix('(')?.strip_suffix(')')?;
    let (value, block) = inner.split_once(',')?;
    let value = value.trim().parse::<u128>().ok()?;
    let block = block.trim();
    if !block.starts_with("bb") || !block[2..].chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some((value, block))
}

fn rust_call_producing_variable<'a>(
    icfg: &'a GlobalICFGOrdered,
    predicate_node_id: &str,
    variable: &ProgramVarId,
) -> Option<&'a str> {
    let scope = mir_function_scope_from_node_id(predicate_node_id)?;
    let function = scope.strip_prefix("rust::").unwrap_or(&scope);
    let mut producers = Vec::new();
    for (node_id, node) in &icfg.ordered_nodes {
        if mir_function_scope_from_node_id(node_id).as_deref() != Some(scope.as_str()) {
            continue;
        }
        let GlobalICFGNode::Mir(block) = node else { continue; };
        let Some(MirTerminator::Call { return_place, .. }) = block.terminator.as_ref() else { continue; };
        if ProgramVarId::rust(function, return_place).as_ref() == Some(variable) {
            producers.push(node_id.as_str());
        }
    }
    producers.sort_unstable();
    producers.dedup();
    if producers.len() == 1 { producers.into_iter().next() } else { None }
}

fn parse_svf_var_id(raw: &str) -> Option<usize> {
    raw.split('@')
        .next()?
        .trim()
        .trim_start_matches('%')
        .parse::<usize>()
        .ok()
}

fn llvm_function_from_global_node_id(node_id: &str) -> Option<&str> {
    node_id
        .strip_prefix("llvm::")?
        .split_once("::node")
        .map(|(function, _)| function)
}

fn exact_single_source_svf_flow(
    definitions: &BTreeMap<usize, Vec<(String, Vec<usize>)>>,
    current: usize,
    source: usize,
    visiting: &mut BTreeSet<usize>,
) -> bool {
    if current == source {
        return true;
    }
    if !visiting.insert(current) {
        return false;
    }
    let result = definitions.get(&current).is_some_and(|defs| {
        if defs.len() != 1 {
            return false;
        }
        let (kind, sources) = &defs[0];
        matches!(kind.as_str(), "AssignStmt" | "CopyStmt" | "PhiStmt")
            && sources.len() == 1
            && exact_single_source_svf_flow(definitions, sources[0], source, visiting)
    });
    visiting.remove(&current);
    result
}

/// Certify that a represented C wrapper returns exactly the value produced by
/// this allocation site.  The proof is deliberately narrower than the normal
/// MAY identity analysis: the C -> Rust DummyRet must target the tested MIR
/// local and the SVF return value must have one unique, single-source value-flow
/// chain back to the malloc/calloc result.  Any phi/select with multiple inputs,
/// ambiguous definition, or unsupported memory flow fails closed.
fn represented_c_allocator_exact_return(
    icfg: &GlobalICFGOrdered,
    allocation_site_node: &str,
    producer_call_node: &str,
    tested_variable: &ProgramVarId,
) -> bool {
    let Some(function) = llvm_function_from_global_node_id(allocation_site_node) else {
        return false;
    };
    let Some(callsite) = llvm_call_suffix_from_global_node_id(allocation_site_node) else {
        return false;
    };
    if callsite != producer_call_node {
        return false;
    }
    let Some(GlobalICFGNode::Llvm(alloc_node)) = icfg
        .ordered_nodes
        .iter()
        .find(|(id, _)| id == allocation_site_node)
        .map(|(_, node)| node)
    else {
        return false;
    };
    let mut allocation_results: Vec<usize> = alloc_node
        .svf_statements
        .iter()
        .filter(|stmt| stmt.stmt_type == "AddrStmt")
        .filter_map(SvfStatement::result_var_id)
        .collect();
    allocation_results.sort_unstable();
    allocation_results.dedup();
    if allocation_results.len() != 1 {
        return false;
    }
    let allocation_result = allocation_results[0];

    let Some(scope) = mir_function_scope_from_node_id(producer_call_node) else {
        return false;
    };
    let rust_function = scope.strip_prefix("rust::").unwrap_or(&scope);
    let mut return_vars = Vec::new();
    for (_, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::DummyRet(dummy) = node else { continue; };
        if dummy.is_internal.unwrap_or(false) {
            continue;
        }
        let Some(mir_var) = dummy.mir_var.as_deref() else { continue; };
        // The C/LLVM return relation is keyed by the pre-expansion MIR
        // callsite, while `tested_variable` is scoped by the exact execution
        // activation.  Once context states are introduced those function
        // namespaces intentionally differ; require the same MIR local index
        // here, with the exact activation checked by the caller.
        let same_local = match (ProgramVarId::rust(rust_function, mir_var), tested_variable) {
            (Some(ProgramVarId::Rust { local: a, .. }), ProgramVarId::Rust { local: b, .. }) => a == *b,
            _ => false,
        };
        if !same_local {
            continue;
        }
        if llvm_function_from_global_node_id(&dummy.incoming_edge) != Some(function)
            || llvm_call_suffix_from_global_node_id(&dummy.incoming_edge) != Some(callsite)
        {
            continue;
        }
        let Some(llvm_var) = dummy.llvm_var.as_deref().and_then(parse_svf_var_id) else {
            continue;
        };
        return_vars.push(llvm_var);
    }
    return_vars.sort_unstable();
    return_vars.dedup();
    if return_vars.len() != 1 {
        return false;
    }

    let mut definitions: BTreeMap<usize, Vec<(String, Vec<usize>)>> = BTreeMap::new();
    for (node_id, node) in &icfg.ordered_nodes {
        if llvm_function_from_global_node_id(node_id) != Some(function)
            || llvm_call_suffix_from_global_node_id(node_id) != Some(callsite)
        {
            continue;
        }
        let GlobalICFGNode::Llvm(llvm) = node else { continue; };
        for stmt in &llvm.svf_statements {
            let Some(result) = stmt.result_var_id() else { continue; };
            let mut sources = llvm_flow_sources(stmt);
            sources.sort_unstable();
            sources.dedup();
            definitions
                .entry(result)
                .or_default()
                .push((stmt.stmt_type.clone(), sources));
        }
    }

    exact_single_source_svf_flow(
        &definitions,
        return_vars[0],
        allocation_result,
        &mut BTreeSet::new(),
    )
}

fn c_malloc_origin_return_basis(
    icfg: &GlobalICFGOrdered,
    allocation: &AbstractAllocId,
    producer_call_node: &str,
    tested_variable: &ProgramVarId,
) -> Option<&'static str> {
    let AllocationSiteId::CCall { node_id, allocator } = &allocation.site else {
        return None;
    };
    if !matches!(allocator.as_str(), "malloc" | "calloc" | "realloc") {
        return None;
    }
    if node_id == producer_call_node {
        // Phase-B bodyless contract materialization: the Rust call destination
        // is the nullable allocator result itself.  RN1 is allowed here only
        // because a CCall{allocator=realloc} site can be materialized by the
        // identity producer solely after a MUST-null proof for formal 0.
        return Some("rust_foreign_decl_c_malloc_contract_v1");
    }
    if allocator == "realloc" {
        // Represented-C realloc remains outside AGE1: the current SVF return
        // proof does not carry the independent MUST-null source certificate.
        return None;
    }
    let producer_canonical_node = canonical_mir_node_id(icfg, producer_call_node)
        .unwrap_or(producer_call_node);
    if node_id.ends_with(&format!("::{producer_canonical_node}"))
        && represented_c_allocator_exact_return(
            icfg,
            node_id,
            producer_canonical_node,
            tested_variable,
        )
    {
        return Some("svf_single_source_c_allocator_return_v1");
    }
    None
}

/// Map a context-qualified execution MIR node to its canonical pre-expansion
/// MIR node for producer provenance checks.  The value is diagnostic/source
/// provenance, not a semantic identity or call-resolution key.
fn canonical_mir_node_id<'a>(icfg: &'a GlobalICFGOrdered, node_id: &'a str) -> Option<&'a str> {
    let dep = icfg.dependency_body_ingestion_v1.as_ref()?;
    let states = dep.context_execution_graph_v1.as_ref()?.get("states")?.as_array()?;
    states.iter().find_map(|state| {
        (state.get("execution_node")?.as_str()? == node_id)
            .then(|| state.get("canonical_node_id")?.as_str())
            .flatten()
    })
}

fn allocation_existence_guard_records(
    icfg: &GlobalICFGOrdered,
    identity_state: &AllocationIdentityState,
) -> Vec<AllocationExistenceGuardRecord> {
    let nodes: BTreeMap<&str, &GlobalICFGNode> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, node)| (id.as_str(), node))
        .collect();
    let canonical_edges: BTreeSet<(String, String)> = icfg
        .icfg_edges
        .iter()
        .map(|edge| (edge.source.clone(), edge.destination.clone()))
        .collect();
    let mut records = BTreeSet::new();

    for (call_node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(block) = node else { continue; };
        let Some(MirTerminator::Call {
            callee_def_path: Some(callee_def_path),
            arguments,
            return_place,
            return_target: Some(return_target),
            ..
        }) = block.terminator.as_ref() else { continue; };
        if !is_raw_pointer_is_null_def_path(callee_def_path) || arguments.len() != 1 {
            continue;
        }

        let Some(scope) = mir_function_scope_from_node_id(call_node_id) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);
        let Some(tested_variable) = ProgramVarId::rust(function, &arguments[0].arg) else { continue; };
        let Some(predicate_result_variable) = ProgramVarId::rust(function, return_place) else { continue; };

        let call_identity = identity_state
            .by_node
            .get(call_node_id)
            .cloned()
            .unwrap_or_default();
        let allocations = call_identity.points_to(&tested_variable);
        if allocations.len() != 1 {
            continue;
        }
        let allocation = allocations.iter().next().expect("singleton checked");
        let Some(producer_call_node) = rust_call_producing_variable(
            icfg,
            call_node_id,
            &tested_variable,
        ) else { continue; };
        let Some(allocation_return_basis) = c_malloc_origin_return_basis(
            icfg,
            allocation,
            producer_call_node,
            &tested_variable,
        ) else { continue; };

        let Some(switch_node_id) = canonical_rust_block_target(call_node_id, return_target) else { continue; };
        let Some(GlobalICFGNode::Mir(switch_block)) = nodes.get(switch_node_id.as_str()).copied() else { continue; };
        let Some(MirTerminator::SwitchInt { discr, targets, otherwise, .. }) = switch_block.terminator.as_ref() else { continue; };
        let Some(discr_variable) = ProgramVarId::rust(function, discr) else { continue; };
        if discr_variable != predicate_result_variable {
            continue;
        }

        let Some(otherwise) = otherwise.as_deref() else { continue; };
        let Some(otherwise_node) = canonical_rust_block_target(&switch_node_id, otherwise) else { continue; };
        let mut false_successor = None;
        let mut true_successor = None;
        for target in targets {
            let Some((value, block)) = parse_switch_target(target) else { continue; };
            let Some(target_node) = canonical_rust_block_target(&switch_node_id, block) else { continue; };
            match value {
                0 => false_successor = Some(target_node),
                1 => true_successor = Some(target_node),
                _ => {}
            }
        }

        // A boolean SwitchInt may encode one explicit value plus `otherwise`.
        // `is_null == true` is the null branch; false is the non-null branch.
        let (null_successor, non_null_successor) = match (true_successor, false_successor) {
            (Some(t), Some(f)) => (t, f),
            (Some(t), None) => (t, otherwise_node),
            (None, Some(f)) => (otherwise_node, f),
            (None, None) => continue,
        };
        if null_successor == non_null_successor {
            continue;
        }
        if !canonical_edges.contains(&(call_node_id.clone(), switch_node_id.clone()))
            || !canonical_edges.contains(&(switch_node_id.clone(), null_successor.clone()))
            || !canonical_edges.contains(&(switch_node_id.clone(), non_null_successor.clone()))
        {
            continue;
        }

        records.insert(AllocationExistenceGuardRecord {
            allocation: stable_allocation_id(allocation),
            producer_call_node: producer_call_node.to_string(),
            producer_call_canonical_node: canonical_mir_node_id(icfg, producer_call_node)
                .map(str::to_owned),
            predicate_call_node: call_node_id.clone(),
            switch_node: switch_node_id,
            tested_variable: tested_variable.canonical_string(),
            predicate_result_variable: predicate_result_variable.canonical_string(),
            null_successor,
            non_null_successor,
            callee_def_path: callee_def_path.clone(),
            allocation_return_basis,
            basis: "rust_raw_pointer_is_null_switch_v1",
        });
    }

    records.into_iter().collect()
}


fn has_represented_external_body_at(icfg: &GlobalICFGOrdered, node_id: &str) -> bool {
    icfg.ordered_nodes.iter().any(|(_, candidate)| {
        matches!(
            candidate,
            GlobalICFGNode::DummyCall(dummy)
                if !dummy.is_internal.unwrap_or(false)
                    && dummy.incoming_edge == node_id
                    && dummy.outgoing_edge.starts_with("llvm::")
        )
    })
}

fn is_bodyless_c_realloc_call(
    function_called: &str,
    call_text: &str,
    callee_def_path: Option<&str>,
    has_internal_rust_branch: bool,
    ffi_functions: &HashSet<String>,
) -> bool {
    // A Rust foreign item declared in `extern "C" { ... }` has a DefId local
    // to the selected crate.  `DefId::is_local()` therefore does *not* mean
    // that a local MIR body exists.  Phase B already uses the correct
    // proof boundary: exact foreign-declaration membership plus absence of an
    // internal Rust call branch.  Reuse that boundary here.
    if has_internal_rust_branch {
        return false;
    }

    let function_called = function_called.trim();
    let call_text = call_text.trim();
    let declared = ffi_functions.contains("realloc")
        && (function_called == "realloc"
            || call_text == "realloc"
            || call_text.starts_with("realloc(")
            || call_text.contains(" realloc(")
            || callee_def_path.is_some_and(|path| {
                path == "realloc" || path.ends_with("::realloc")
            }));
    let libc = (function_called.contains("libc::") && function_called.contains("::realloc"))
        || (call_text.contains("libc::") && call_text.contains("::realloc("))
        || callee_def_path.is_some_and(|path| {
            path.contains("libc::") && path.ends_with("::realloc")
        });
    declared || libc
}

fn reallocation_boundary_records(
    icfg: &GlobalICFGOrdered,
    identity_state: &AllocationIdentityState,
    ffi_functions: &HashSet<String>,
) -> Vec<ReallocationBoundaryRecord> {
    let mut records = BTreeSet::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(block) = node else { continue; };
        let Some(MirTerminator::Call {
            details,
            function_called,
            callee_def_path,
            arguments,
            return_place,
            ..
        }) = block.terminator.as_ref() else { continue; };
        let has_internal_rust_branch = icfg
            .rust_calls
            .iter()
            .any(|call| call.call_node == node_id.as_str());
        if arguments.is_empty()
            || !is_bodyless_c_realloc_call(
                function_called,
                details,
                callee_def_path.as_deref(),
                has_internal_rust_branch,
                ffi_functions,
            )
            || has_represented_external_body_at(icfg, node_id)
        {
            continue;
        }

        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);
        let Some(source_variable) = ProgramVarId::rust(function, &arguments[0].arg) else { continue; };
        let Some(result_variable) = ProgramVarId::rust(function, return_place) else { continue; };
        let memory = identity_state.by_node.get(node_id).cloned().unwrap_or_default();

        for allocation in memory.points_to(&source_variable) {
            let source_contract = allocation_contract(&allocation);
            let legacy_valid_source = source_contract.family == "c_malloc"
                && matches!(source_contract.operation, "malloc" | "calloc" | "realloc");
            records.insert(ReallocationBoundaryRecord {
                node: node_id.clone(),
                source_allocation: stable_allocation_id(&allocation),
                source_variable: source_variable.canonical_string(),
                result_variable: result_variable.canonical_string(),
                // This field is the family required by the realloc consumer,
                // not a reclassification of the source allocation.
                family: "c_malloc",
                operation: "realloc",
                certainty: "may_abstract",
                status: "conditional_unmodeled",
                basis: if legacy_valid_source {
                    "rust_foreign_decl_c_realloc_boundary_v1"
                } else {
                    "rust_foreign_decl_c_realloc_allocptr_family_v2"
                },
            });
        }
    }

    records.into_iter().collect()
}


fn positive_nonzero_usize_constant(argument: &str) -> bool {
    let t = argument.trim();
    let Some(rest) = t.strip_prefix("const ") else { return false; };
    let Some(digits) = rest.strip_suffix("_usize") else { return false; };
    !digits.is_empty()
        && digits.chars().all(|ch| ch.is_ascii_digit())
        && digits.parse::<u128>().is_ok_and(|value| value > 0)
}

fn definitely_zero_usize_constant(argument: &str) -> bool {
    let t = argument.trim();
    let Some(rest) = t.strip_prefix("const ") else { return false; };
    let Some(digits) = rest.strip_suffix("_usize") else { return false; };
    !digits.is_empty()
        && digits.chars().all(|ch| ch.is_ascii_digit())
        && digits.parse::<u128>().is_ok_and(|value| value == 0)
}

fn exact_rust_local(function: &str, raw: &str) -> Option<ProgramVarId> {
    let raw = raw.trim();
    let digits = if let Some(rest) = raw.strip_prefix('_') {
        rest
    } else if let Some(rest) = raw.strip_prefix("Local(_") {
        rest.strip_suffix(") [mutable]")
            .or_else(|| rest.strip_suffix(')'))?
    } else {
        return None;
    };
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let local = digits.parse::<u32>().ok()?;
    Some(ProgramVarId::Rust {
        function: function.to_string(),
        local,
    })
}

fn exact_mir_copy_source(function: &str, rvalue: &str) -> Option<ProgramVarId> {
    let source = rvalue.trim().strip_prefix("copy ")?;
    exact_rust_local(function, source)
}

/// Certify the MIR-local relation between a logical result local and the
/// actual operand consumed by a call terminator in the same basic block.
///
/// The accepted vocabulary is intentionally closed: either the call consumes
/// the result local directly, or its operand local has exactly one definition
/// in this block and that definition is `operand = copy result`.  No casts,
/// projections, dereferences, moves, transitive copies, or pretty-source
/// equivalence are accepted.
fn rust_call_operand_correlation_basis(
    block: &crate::structs::MirBasicBlock,
    function: &str,
    canonical_result: &str,
    raw_argument: &str,
) -> Option<(&'static str, String)> {
    let argument = exact_rust_local(function, raw_argument)?;
    let argument_canonical = argument.canonical_string();
    if argument_canonical == canonical_result {
        return Some(("rust_mir_direct_result_operand_v1", argument_canonical));
    }

    let mut defining_copy_count = 0usize;
    for statement in &block.statements {
        let Some(place) = statement
            .place
            .as_deref()
            .and_then(|raw| exact_rust_local(function, raw))
        else {
            continue;
        };
        if place != argument {
            continue;
        }
        let Some(source) = statement
            .rvalue
            .as_deref()
            .and_then(|raw| exact_mir_copy_source(function, raw))
        else {
            return None;
        };
        if source.canonical_string() != canonical_result {
            return None;
        }
        defining_copy_count += 1;
    }
    if defining_copy_count != 1 {
        return None;
    }

    Some((
        "rust_mir_single_local_copy_result_operand_v1",
        argument_canonical,
    ))
}

/// Outcome correlation is stricter than generic call-argument correlation:
/// the logical result local must still denote the value returned by realloc
/// throughout the predicate block.  This closes the same-basic-block gap that
/// a direct CFG-edge proof alone cannot exclude.
fn rust_realloc_outcome_operand_correlation_basis(
    block: &crate::structs::MirBasicBlock,
    function: &str,
    canonical_result: &str,
    raw_argument: &str,
) -> Option<(&'static str, String)> {
    if block.statements.iter().any(|statement| {
        statement
            .place
            .as_deref()
            .and_then(|raw| exact_rust_local(function, raw))
            .is_some_and(|variable| variable.canonical_string() == canonical_result)
    }) {
        return None;
    }
    rust_call_operand_correlation_basis(block, function, canonical_result, raw_argument)
}

fn rust_node_redefines_canonical_variable(
    node: &GlobalICFGNode,
    function: &str,
    canonical_variable: &str,
) -> bool {
    let GlobalICFGNode::Mir(block) = node else { return false; };
    if block.statements.iter().any(|statement| {
        statement.place.as_deref().and_then(|place| ProgramVarId::rust(function, place))
            .is_some_and(|variable| variable.canonical_string() == canonical_variable)
    }) {
        return true;
    }
    matches!(
        block.terminator.as_ref(),
        Some(MirTerminator::Call { return_place, .. })
            if ProgramVarId::rust(function, return_place)
                .is_some_and(|variable| variable.canonical_string() == canonical_variable)
    )
}

fn rust_node_consumes_result_obligation(
    icfg: &GlobalICFGOrdered,
    node_id: &str,
    node: &GlobalICFGNode,
    function: &str,
    canonical_variable: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    let GlobalICFGNode::Mir(block) = node else { return false; };
    let Some(MirTerminator::Call {
        details,
        function_called,
        callee_def_path,
        arguments,
        ..
    }) = block.terminator.as_ref() else { return false; };
    let Some(first) = arguments.first() else {
        return false;
    };
    if rust_call_operand_correlation_basis(
        block,
        function,
        canonical_variable,
        &first.arg,
    )
    .is_none()
    {
        return false;
    }
    let has_internal_rust_branch = icfg.rust_calls.iter().any(|call| call.call_node == node_id);
    let is_realloc = is_bodyless_c_realloc_call(
        function_called,
        details,
        callee_def_path.as_deref(),
        has_internal_rust_branch,
        ffi_functions,
    ) && !has_represented_external_body_at(icfg, node_id);
    is_realloc || is_deallocation_call(function_called, details, ffi_functions)
}

fn result_value_flow_is_stable_to_deallocation(
    icfg: &GlobalICFGOrdered,
    nodes: &BTreeMap<&str, &GlobalICFGNode>,
    successors: &BTreeMap<String, BTreeSet<String>>,
    predecessors: &BTreeMap<String, BTreeSet<String>>,
    start: &str,
    target: &str,
    function: &str,
    canonical_variable: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    let forward = reachable_from(start, successors);
    if !forward.contains(target) {
        return false;
    }
    let backward = reachable_from(target, predecessors);
    for node_id in forward.intersection(&backward) {
        let Some(node) = nodes.get(node_id.as_str()).copied() else { return false; };
        // Statements execute before the terminator, including in the target
        // block. Therefore a target-block assignment to the realloc-result
        // local invalidates the proof even though the target terminator itself
        // is the certified deallocation we are trying to justify.
        if rust_node_redefines_canonical_variable(node, function, canonical_variable) {
            return false;
        }
        // The target deallocation is the permitted final consumer. Any earlier
        // free/realloc of the same logical result local invalidates the proof.
        if node_id != target
            && rust_node_consumes_result_obligation(
                icfg,
                node_id,
                node,
                function,
                canonical_variable,
                ffi_functions,
            )
        {
            return false;
        }
    }
    true
}

fn conditional_reallocation_records(
    icfg: &GlobalICFGOrdered,
    ffi_functions: &HashSet<String>,
    boundaries: &[ReallocationBoundaryRecord],
    existence_guards: &[AllocationExistenceGuardRecord],
) -> Vec<ConditionalReallocationRecord> {
    let nodes: BTreeMap<&str, &GlobalICFGNode> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, node)| (id.as_str(), node))
        .collect();
    let canonical_edges: BTreeSet<(String, String)> = icfg
        .icfg_edges
        .iter()
        .map(|edge| (edge.source.clone(), edge.destination.clone()))
        .collect();
    let mut successors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut predecessors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (source, destination) in &canonical_edges {
        successors.entry(source.clone()).or_default().insert(destination.clone());
        predecessors.entry(destination.clone()).or_default().insert(source.clone());
    }
    let mut records = BTreeSet::new();

    for boundary in boundaries {
        // CR1 is a valid-realloc outcome protocol.  RBF2 family-consumer
        // records deliberately describe sources that are not certified as
        // valid c_malloc realloc origins, so they must never seed CR1.
        if boundary.basis != "rust_foreign_decl_c_realloc_boundary_v1" {
            continue;
        }
        let Some(source_guard) = existence_guards.iter().find(|guard| {
            guard.allocation == boundary.source_allocation
                && guard.tested_variable == boundary.source_variable
                && guard.non_null_successor == boundary.node
        }) else { continue; };

        let Some(GlobalICFGNode::Mir(realloc_block)) = nodes.get(boundary.node.as_str()).copied() else { continue; };
        let Some(MirTerminator::Call {
            callee_def_path: Some(reallocation_callee_def_path),
            arguments,
            ..
        }) = realloc_block.terminator.as_ref() else { continue; };
        if arguments.len() < 2 || !positive_nonzero_usize_constant(&arguments[1].arg) {
            continue;
        }
        let Some(scope) = mir_function_scope_from_node_id(&boundary.node) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);

        let mut outcome_candidates = Vec::new();
        for (predicate_node, candidate) in &icfg.ordered_nodes {
            if mir_function_scope_from_node_id(predicate_node).as_deref() != Some(scope.as_str())
                || !canonical_edges.contains(&(boundary.node.clone(), predicate_node.clone()))
            {
                continue;
            }
            let GlobalICFGNode::Mir(predicate_block) = candidate else { continue; };
            let Some(MirTerminator::Call {
                callee_def_path: Some(outcome_callee_def_path),
                arguments,
                return_place,
                return_target: Some(return_target),
                ..
            }) = predicate_block.terminator.as_ref() else { continue; };
            if !is_raw_pointer_is_null_def_path(outcome_callee_def_path) || arguments.len() != 1 {
                continue;
            }
            let Some((outcome_value_flow_basis, outcome_argument_variable)) =
                rust_realloc_outcome_operand_correlation_basis(
                    predicate_block,
                    function,
                    &boundary.result_variable,
                    &arguments[0].arg,
                )
            else {
                continue;
            };
            let Some(predicate_result) = ProgramVarId::rust(function, return_place) else { continue; };
            let Some(switch_node) = canonical_rust_block_target(predicate_node, return_target) else { continue; };
            let Some(GlobalICFGNode::Mir(switch_block)) = nodes.get(switch_node.as_str()).copied() else { continue; };
            let Some(MirTerminator::SwitchInt { discr, targets, otherwise, .. }) = switch_block.terminator.as_ref() else { continue; };
            let Some(discr_variable) = ProgramVarId::rust(function, discr) else { continue; };
            if discr_variable != predicate_result {
                continue;
            }
            let Some(otherwise) = otherwise.as_deref() else { continue; };
            let Some(otherwise_node) = canonical_rust_block_target(&switch_node, otherwise) else { continue; };
            let mut false_successor = None;
            let mut true_successor = None;
            for target in targets {
                let Some((value, block)) = parse_switch_target(target) else { continue; };
                let Some(target_node) = canonical_rust_block_target(&switch_node, block) else { continue; };
                match value {
                    0 => false_successor = Some(target_node),
                    1 => true_successor = Some(target_node),
                    _ => {}
                }
            }
            let (failure_successor, success_successor) = match (true_successor, false_successor) {
                (Some(t), Some(f)) => (t, f),
                (Some(t), None) => (t, otherwise_node),
                (None, Some(f)) => (otherwise_node, f),
                (None, None) => continue,
            };
            if failure_successor == success_successor
                || !canonical_edges.contains(&(predicate_node.clone(), switch_node.clone()))
                || !canonical_edges.contains(&(switch_node.clone(), failure_successor.clone()))
                || !canonical_edges.contains(&(switch_node.clone(), success_successor.clone()))
            {
                continue;
            }
            outcome_candidates.push((
                predicate_node.clone(),
                outcome_argument_variable,
                predicate_result.canonical_string(),
                switch_node,
                failure_successor,
                success_successor,
                outcome_callee_def_path.clone(),
                outcome_value_flow_basis,
            ));
        }
        if outcome_candidates.len() != 1 {
            continue;
        }
        let (
            outcome_predicate_call_node,
            outcome_argument_variable,
            outcome_predicate_result_variable,
            outcome_switch_node,
            failure_successor,
            success_successor,
            outcome_callee_def_path,
            outcome_value_flow_basis,
        ) = outcome_candidates.pop().expect("singleton checked");

        let reachable_success = reachable_from(&success_successor, &successors);
        let mut result_deallocations = BTreeSet::new();
        for (node_id, candidate) in &icfg.ordered_nodes {
            if !reachable_success.contains(node_id) {
                continue;
            }
            let GlobalICFGNode::Mir(block) = candidate else { continue; };
            let Some(MirTerminator::Call {
                details,
                function_called,
                callee_def_path,
                arguments,
                ..
            }) = block.terminator.as_ref() else { continue; };
            if arguments.len() != 1
                || !(is_c_free_function(function_called, ffi_functions)
                    || is_c_free_call_text(details, ffi_functions))
            {
                continue;
            }
            let has_internal_rust_branch = icfg
                .rust_calls
                .iter()
                .any(|call| call.call_node == node_id.as_str());
            if has_internal_rust_branch || has_represented_external_body_at(icfg, node_id) {
                continue;
            }
            let Some((argument_correlation_basis, argument_variable)) =
                rust_call_operand_correlation_basis(
                    block,
                    function,
                    &boundary.result_variable,
                    &arguments[0].arg,
                )
            else {
                continue;
            };
            let Some(callee_def_path) = callee_def_path.clone() else { continue; };
            if !result_value_flow_is_stable_to_deallocation(
                icfg,
                &nodes,
                &successors,
                &predecessors,
                &success_successor,
                node_id,
                function,
                &boundary.result_variable,
                ffi_functions,
            ) {
                continue;
            }
            result_deallocations.insert(ConditionalReallocationResultDeallocationRecord {
                node: node_id.clone(),
                variable: boundary.result_variable.clone(),
                argument_variable,
                callee_def_path,
                family: "c_malloc",
                operation: "free",
                basis: "rust_foreign_decl_c_free_result_v1",
                argument_correlation_basis,
                value_flow_basis: "rust_mir_result_no_redefinition_all_paths_v1",
            });
        }

        records.insert(ConditionalReallocationRecord {
            source_allocation: boundary.source_allocation.clone(),
            reallocation_node: boundary.node.clone(),
            source_variable: boundary.source_variable.clone(),
            result_variable: boundary.result_variable.clone(),
            source_existence_predicate_call_node: source_guard.predicate_call_node.clone(),
            outcome_predicate_call_node,
            outcome_argument_variable,
            outcome_predicate_result_variable,
            outcome_switch_node,
            failure_successor,
            success_successor,
            reallocation_callee_def_path: reallocation_callee_def_path.clone(),
            outcome_callee_def_path,
            family: "c_malloc",
            operation: "realloc",
            certainty: "may_abstract",
            size_semantics: "positive_nonzero_constant",
            status: "conditional_guarded",
            basis: "rust_foreign_decl_c_realloc_is_null_switch_v1",
            outcome_correlation_basis: "direct_cfg_edge_realloc_to_is_null_v1",
            outcome_value_flow_basis,
            result_deallocations: result_deallocations.into_iter().collect(),
        });
    }

    records.into_iter().collect()
}

fn typed_edge_records(icfg: &GlobalICFGOrdered) -> Vec<TypedEdgeRecord> {
    let mut records: Vec<_> = icfg
        .icfg_edges
        .iter()
        .map(|edge| TypedEdgeRecord {
            source: edge.source.clone(),
            destination: edge.destination.clone(),
            flow: match edge_flow_kind(edge) {
                EdgeFlowKind::Normal => "normal",
                EdgeFlowKind::Unwind => "unwind",
            },
            label: edge.label.clone(),
            source_label: edge.source_label.clone(),
            destination_label: edge.destination_label.clone(),
        })
        .collect();
    records.sort();
    records.dedup();
    records
}

fn append_opaque_summary_allocations(
    catalog: &mut Vec<AbstractAllocationRecord>,
    opaque: &[OpaqueMemoryEffectRecord],
) {
    for record in opaque {
        let label = match record.basis {
            "rust_dyn_trait_drop_vtable_summary_v1" => "opaque_dynamic_trait_drop_allocation_v1",
            "rust_higher_order_callback_opaque_summary_v1" => "opaque_higher_order_callback_allocation_v1",
            _ => continue,
        };
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic {
                scope: record.node.clone(),
                label: label.into(),
            },
            vec![],
        );
        let id = stable_allocation_id(&allocation);
        if catalog.iter().any(|existing| existing.id == id) { continue; }
        catalog.push(AbstractAllocationRecord {
            id,
            display: allocation.canonical_string(),
            site: allocation.site.clone(),
            context: allocation.context.clone(),
            allocator_contract: Some(allocation_contract(&allocation)),
        });
    }
    catalog.sort_by(|left, right| left.id.cmp(&right.id));
}

fn opaque_memory_effect_records(
    icfg: &GlobalICFGOrdered,
    reachable: &BTreeSet<String>,
) -> Vec<OpaqueMemoryEffectRecord> {
    let dep = icfg.dependency_body_ingestion_v1.as_ref();
    let has_certified_dynamic_drop = dep.is_some_and(|dep| {
        dep.exact_call_bindings.iter().any(|call| {
            call.get("body_status").and_then(serde_json::Value::as_str)
                == Some("dynamic_drop_summary")
                && call
                    .get("dynamic_drop_summary_v1")
                    .and_then(|value| value.get("semantics"))
                    .and_then(serde_json::Value::as_str)
                    == Some("opaque_dynamic_destructor_may_effect_v1")
        })
    });
    let certified_higher_order_nodes: BTreeSet<String> = dep
        .and_then(|dep| dep.context_execution_graph_v1.as_ref())
        .and_then(|ctx| ctx.get("opaque_higher_order_summaries"))
        .and_then(serde_json::Value::as_array)
        .into_iter().flatten()
        .filter(|record| {
            record.get("semantics").and_then(serde_json::Value::as_str)
                == Some("opaque_higher_order_callback_zero_or_more_v1")
                && record.get("provider_effect").and_then(serde_json::Value::as_str)
                    == Some("opaque_at_least_once_per_call")
                && record.get("callback_multiplicity").and_then(serde_json::Value::as_str)
                    == Some("zero_or_more")
                && record.get("memory_effect").and_then(serde_json::Value::as_str)
                    == Some("widen_all_tracked_to_top")
        })
        .filter_map(|record| record.get("execution_state").and_then(serde_json::Value::as_str).map(str::to_owned))
        .collect();

    let mut records = BTreeMap::<String, &'static str>::new();
    for edge in &icfg.icfg_edges {
        if !reachable.contains(&edge.source) { continue; }
        match edge.label.as_deref() {
            Some("DYNAMIC_TRAIT_DROP_SUMMARY_V1")
            | Some("DYNAMIC_TRAIT_DROP_SUMMARY_UNWIND_V1") if has_certified_dynamic_drop => {
                records.insert(edge.source.clone(), "rust_dyn_trait_drop_vtable_summary_v1");
            }
            Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1")
            | Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1")
                if certified_higher_order_nodes.contains(&edge.source) => {
                records.insert(edge.source.clone(), "rust_higher_order_callback_opaque_summary_v1");
            }
            _ => {}
        }
    }
    records.into_iter().map(|(node,basis)| OpaqueMemoryEffectRecord {
        node, effect:"may_read_write_allocate_deallocate", basis,
    }).collect()
}

fn validate_typed_edge_projection(
    typed_edges: &[TypedEdgeRecord],
    successors: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), Box<dyn Error>> {
    let mut projected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in typed_edges {
        projected
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.destination.clone());
    }
    let mut sources = BTreeSet::new();
    sources.extend(successors.keys().cloned());
    sources.extend(projected.keys().cloned());
    for source in sources {
        let legacy = successors.get(&source).cloned().unwrap_or_default();
        let typed = projected.get(&source).cloned().unwrap_or_default();
        if legacy != typed {
            return Err(format!(
                "typed_edge_flow_v1 projection mismatch at '{source}': successors={legacy:?} typed={typed:?}"
            )
            .into());
        }
    }
    Ok(())
}

fn panic_lifecycle_records(memory: &PanicLifecycleMemory) -> Vec<PanicLifecycleRecord> {
    memory
        .iter()
        .map(|(allocation, value)| PanicLifecycleRecord {
            allocation: allocation.to_string(),
            certainty: "may_abstract",
            may_own: value.may_own(),
            may_partial_drop: value.may_partial_drop(),
            may_stale_owner: value.may_stale_owner(),
            may_committed: value.may_committed(),
            may_complete: value.may_complete(),
        })
        .collect()
}

fn stable_allocation_id(allocation: &AbstractAllocId) -> String {
    // Struct/enum field order is fixed by serde derivation; JSON escaping keeps
    // arbitrary node/function text unambiguous.  CQPL treats this as an opaque ID.
    serde_json::to_string(allocation).expect("AbstractAllocId must serialize")
}

fn allocation_catalog(state: &AllocationIdentityState, schema_version: u32) -> Vec<AbstractAllocationRecord> {
    let mut ids: BTreeMap<String, AbstractAllocId> = BTreeMap::new();
    for mem in state.by_node.values().chain(state.event_by_node.values()) {
        for allocations in mem.points_to.values().chain(mem.access_bases.values()).chain(mem.place_points_to.values()) {
            for allocation in allocations {
                ids.entry(stable_allocation_id(allocation))
                    .or_insert_with(|| allocation.clone());
            }
        }

        // HT2 heap summaries are part of the authoritative MAY identity store.
        // Include both the addressed allocation and every canonical allocation
        // identity stored in its summarized value.
        for (base, value) in &mem.heap_values {
            ids.entry(stable_allocation_id(base))
                .or_insert_with(|| base.clone());
            for allocation in &value.root_access_bases {
                ids.entry(stable_allocation_id(allocation))
                    .or_insert_with(|| allocation.clone());
            }
            for allocations in value.points_to.values().chain(value.resources.values()) {
                for allocation in allocations {
                    ids.entry(stable_allocation_id(allocation))
                        .or_insert_with(|| allocation.clone());
                }
            }
        }
    }
    ids.into_iter()
        .map(|(id, allocation)| AbstractAllocationRecord {
            id,
            display: allocation.canonical_string(),
            allocator_contract: if schema_version == 2 { Some(allocation_contract(&allocation)) } else { None },
            site: allocation.site,
            context: allocation.context,
        })
        .collect()
}

fn collect_identity_program_variables(mem: &AllocationIdentityMemory) -> BTreeSet<Name> {
    fn collect_place(place: &PlaceId, out: &mut BTreeSet<Name>) {
        out.insert(place.base.canonical_string());
        for projection in &place.projection {
            if let PlaceProjection::Index { local } = projection {
                out.insert(local.canonical_string());
            }
        }
    }

    let mut out = BTreeSet::new();
    out.extend(mem.points_to.keys().map(ProgramVarId::canonical_string));
    out.extend(mem.access_bases.keys().map(ProgramVarId::canonical_string));
    out.extend(mem.stack_refs.keys().map(ProgramVarId::canonical_string));

    for place in mem.place_points_to.keys() {
        collect_place(place, &mut out);
    }
    for (place, targets) in &mem.place_stack_refs {
        collect_place(place, &mut out);
        for target in targets {
            collect_place(target, &mut out);
        }
    }
    for places in mem.stack_refs.values() {
        for place in places {
            collect_place(place, &mut out);
        }
    }
    for value in mem.heap_values.values() {
        for targets in value.stack_refs.values() {
            for place in targets {
                collect_place(place, &mut out);
            }
        }
    }

    out
}

fn identity_annotation(mem: &AllocationIdentityMemory) -> NodeIdentityAnnotation {
    NodeIdentityAnnotation {
        access_bases: mem.access_bases.iter().map(|(variable, allocations)| IdentityPointsToRecord {
            variable: variable.canonical_string(),
            allocations: allocations.iter().map(stable_allocation_id).collect(),
        }).collect(),
        points_to: mem
            .points_to
            .iter()
            .map(|(variable, allocations)| IdentityPointsToRecord {
                variable: variable.canonical_string(),
                allocations: allocations.iter().map(stable_allocation_id).collect(),
            })
            .collect(),
        stack_refs: mem
            .stack_refs
            .iter()
            .map(|(variable, places)| IdentityStackRefsRecord {
                variable: variable.canonical_string(),
                places: places.iter().cloned().collect(),
            })
            .collect(),
        place_points_to: mem
            .place_points_to
            .iter()
            .map(|(place, allocations)| IdentityPlacePointsToRecord {
                place: place.clone(),
                allocations: allocations.iter().map(stable_allocation_id).collect(),
            })
            .collect(),
        place_stack_refs: mem
            .place_stack_refs
            .iter()
            .map(|(place, targets)| IdentityPlaceStackRefsRecord {
                place: place.clone(),
                targets: targets.iter().cloned().collect(),
            })
            .collect(),
    }
}

fn identity_var_for_event(
    node_id: &str,
    node: &GlobalICFGNode,
    variable: &str,
) -> Option<ProgramVarId> {
    match node {
        GlobalICFGNode::Mir(_) => {
            let scope = mir_function_scope_from_node_id(node_id)?;
            let function = scope.strip_prefix("rust::").unwrap_or(&scope);
            ProgramVarId::rust(function.to_string(), variable)
        }
        GlobalICFGNode::Llvm(llvm) => {
            let function = llvm
                .function_name
                .clone()
                .or_else(|| {
                    node_id
                        .strip_prefix("llvm::")
                        .and_then(|rest| rest.split_once("::node"))
                        .map(|(f, _)| f.to_string())
                })?;
            let callsite = llvm_call_suffix_from_global_node_id(node_id)?.to_string();
            let id_text = variable
                .split_once('@')
                .map(|(id, _)| id)
                .unwrap_or(variable)
                .trim()
                .trim_start_matches('%');
            let var_id = id_text.parse::<usize>().ok()?;
            Some(ProgramVarId::c(function, var_id, Some(callsite)))
        }
        GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) | GlobalICFGNode::Terminal(_) => None,
    }
}

fn allocation_labels_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    labels: &[EventLabel],
    identity: &AllocationIdentityMemory,
    ffi_functions: &HashSet<String>,
    schema_version: u32,
) -> Vec<AllocationEventLabel> {
    let mut out = BTreeSet::new();
    for label in labels {
        // HT2-R2.4: allocation-level Drop is admitted only when rustc
        // producer evidence certifies an owning deallocation boundary.  String
        // is such a boundary for a String-origin allocation identity; its
        // nested Vec<u8> drop is an implementation detail and is coalesced per
        // allocation below rather than counted as a second free.
        if label.predicate == "drop" {
            if let GlobalICFGNode::Mir(bb) = node {
                if let Some(MirTerminator::Drop { deallocator_evidence, .. }) = &bb.terminator {
                    if deallocator_evidence.is_none() {
                        continue;
                    }
                }
            }
        }

        let Some(variable) = identity_var_for_event(node_id, node, &label.variable) else {
            continue;
        };
        let allocations = if label.predicate == "drop" {
            identity.deallocation_allocations(&variable)
        } else { identity.event_allocations(&variable) };
        // AllocationIdentityMemory is a MAY points-to/place-flow domain.
        // Even a singleton set means "the only represented MAY target", not
        // a MUST-target fact for every concrete state. Schema v2 therefore has
        // one allocation-event certainty only: may_abstract -> unk.
        let certainty = "may_abstract";
        for allocation in allocations {
            let deallocator_contract = if schema_version == 2 && label.predicate == "drop" {
                Some(deallocator_contract(node, ffi_functions))
            } else {
                None
            };

            // HT2-R2.4 destructor coalescing.  The AbstractAllocId created by
            // String::from(&str) denotes the String-owned buffer resource.  A
            // Drop(String) is therefore the semantic owner-boundary free.  The
            // subsequent Drop(Vec<u8, Global>) inside String drop glue releases
            // the same concrete storage and must not become a second abstract
            // drop event.  This rule is keyed by the allocator-origin contract,
            // not by rendered paths or source locations, so a genuine Vec
            // allocation keeps its ordinary rust_vec_global_drop semantics.
            if label.predicate == "drop"
                && deallocator_contract
                    .as_ref()
                    .and_then(|contract| contract.basis)
                    == Some("rust_vec_global_drop")
                && allocation_contract(&allocation).operation == "string_allocation"
            {
                continue;
            }

            out.insert(AllocationEventLabel {
                predicate: label.predicate,
                allocation: stable_allocation_id(&allocation),
                certainty,
                deallocator_contract,
            });
        }
    }
    out.into_iter().collect()
}

/// Preserve direct return evidence and add only known, return-rooted field
/// places. Resolution follows certified identity relations; it neither creates
/// aliases nor establishes ownership transfer or MUST escape.
fn return_escape_allocations(
    post_identity: &AllocationIdentityMemory,
    return_var: &ProgramVarId,
) -> BTreeSet<AbstractAllocId> {
    let mut allocations = post_identity.event_allocations(return_var);
    let places: BTreeSet<_> = post_identity
        .place_points_to
        .keys()
        .chain(post_identity.place_stack_refs.keys())
        .filter(|place| {
            place.base == *return_var
                && !place.projection.is_empty()
                && place
                    .projection
                    .iter()
                    .all(|projection| matches!(projection, PlaceProjection::Field { .. }))
        })
        .collect();
    for place in places {
        allocations.extend(post_identity.allocations_for_place(place.clone()));
    }
    allocations
}

fn allocation_disposition_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    event_identity: &AllocationIdentityMemory,
    post_identity: &AllocationIdentityMemory,
    allocation_labels: &[AllocationEventLabel],
) -> Vec<AllocationDispositionRecord> {
    let mut out = BTreeSet::new();

    // Existing allocation-centric drop events remain MAY.  v6S-r1 mirrors them
    // as obligation observations so disposition traces can be inspected without
    // changing the historical `drop_l` semantics.
    for label in allocation_labels.iter().filter(|l| l.predicate == "drop") {
        out.insert(AllocationDispositionRecord {
            allocation: label.allocation.clone(),
            kind: "may_deallocate",
            certainty: "may_abstract",
            obligation_effect: "may_discharge",
            basis: "allocation_drop_label_v1",
            source_variable: None,
            target_variable: None,
            callee_def_path: None,
        });
    }

    let GlobalICFGNode::Mir(bb) = node else {
        return out.into_iter().collect();
    };
    let Some(term) = &bb.terminator else {
        return out.into_iter().collect();
    };

    match term {
        MirTerminator::Call {
            arguments,
            return_place,
            allocation_disposition_evidence: Some(evidence),
            ..
        } => {
            let source_local = arguments.iter().find_map(|arg| canonical_mir_local(&arg.arg));
            let source_var = source_local
                .as_deref()
                .and_then(|local| identity_var_for_event(node_id, node, local));
            let allocations = source_var
                .as_ref()
                .map(|var| event_identity.event_allocations(var))
                .unwrap_or_default();
            let source_variable = source_var.as_ref().map(ProgramVarId::canonical_string);
            let return_variable = canonical_mir_local(return_place)
                .as_deref()
                .and_then(|local| identity_var_for_event(node_id, node, local))
                .map(|v| v.canonical_string());

            // v6U-A2 migration-only projection from producer-certified evidence.
            let projection =
                crate::library_effects_v1::allocation_disposition_projection_for_evidence(
                    &evidence.kind,
                );
            let target_variable = match projection.target_variable {
                crate::library_effects_v1::LegacyTargetVariable::Return => return_variable.clone(),
                crate::library_effects_v1::LegacyTargetVariable::None => None,
            };
            let kind = projection.kind;
            let obligation_effect = projection.obligation_effect;
            let basis = projection.basis;

            for allocation in allocations {
                out.insert(AllocationDispositionRecord {
                    allocation: stable_allocation_id(&allocation),
                    kind,
                    certainty: "may_abstract",
                    obligation_effect,
                    basis,
                    source_variable: source_variable.clone(),
                    target_variable: target_variable.clone(),
                    callee_def_path: Some(evidence.callee_def_path.clone()),
                });
            }
        }
        MirTerminator::Return { .. } => {
            // MIR local _0 is the return place.  If the post identity says it
            // may denote an AbstractAllocId, that allocation may escape to the
            // caller.  This is escape provenance, not proof that ownership was
            // safely discharged.
            if let Some(ret_var) = identity_var_for_event(node_id, node, "Local(_0)") {
                for allocation in return_escape_allocations(post_identity, &ret_var) {
                    out.insert(AllocationDispositionRecord {
                        allocation: stable_allocation_id(&allocation),
                        kind: "return_escape",
                        certainty: "may_abstract",
                        obligation_effect: "may_escape_to_caller",
                        basis: "rust_return_identity_v1",
                        source_variable: Some(ret_var.canonical_string()),
                        target_variable: None,
                        callee_def_path: None,
                    });
                }
            }
        }
        _ => {}
    }

    out.into_iter().collect()
}

fn allocation_contract(allocation: &AbstractAllocId) -> AllocationContract {
    // v6N-r1 deliberately preserves the v1 allocator-origin boundary.  The
    // new v2 capability strengthens deallocator evidence only; this avoids
    // silently re-certifying legacy origin summaries that were not designed as
    // producer-certified contracts.  Known C family naming uses `c_malloc` for
    // malloc/calloc and for POSIX allocation APIs explicitly specified as fresh
    // storage releasable with `free` (`strdup`, plus RN1 `realloc(NULL,n)`).  LLVM's
    // `"alloc-family"="malloc"` motivates the malloc/calloc/realloc/free family
    // relation, while the strdup extension is grounded separately in POSIX.
    match &allocation.site {
        AllocationSiteId::CCall { allocator, .. } if allocator == "malloc" =>
            AllocationContract::v1("c_malloc", "malloc", "c"),
        AllocationSiteId::CCall { allocator, .. } if allocator == "calloc" =>
            AllocationContract::v1("c_malloc", "calloc", "c"),
        AllocationSiteId::CCall { allocator, .. } if allocator == "strdup" =>
            AllocationContract::v1("c_malloc", "strdup", "c"),
        // RN1: this site is emitted only when the identity producer has a
        // MUST-null proof for realloc formal 0. POSIX/C semantics therefore
        // classify the returned non-null object exactly like malloc-family
        // storage, while the nullable result remains MAY in CQPL.
        AllocationSiteId::CCall { allocator, .. } if allocator == "realloc" =>
            AllocationContract::v1("c_malloc", "realloc", "c"),
        AllocationSiteId::RustCall { callee, .. } if memory_events::is_modeled_fresh_allocation(callee) => {
            let operation = if (callee.contains("std::boxed::Box::<") || callee.contains("alloc::boxed::Box::<"))
                && (callee.ends_with("::new") || callee.contains("::new::<"))
            {
                "box_allocation"
            } else if memory_events::is_exchange_malloc_call(callee) {
                "exchange_malloc"
            } else if callee.contains("alloc_zeroed") {
                "alloc_zeroed"
            } else if callee.contains("::alloc") {
                "alloc"
            } else if callee.contains("CString") {
                "cstring_allocation"
            } else if callee.contains("string::String") && callee.contains("From<&str>") {
                "string_allocation"
            } else {
                "rust_allocation"
            };
            AllocationContract::v1("rust_global", operation, "rust")
        }
        _ => AllocationContract::v1("unknown", "unknown", "unknown"),
    }
}

fn deallocator_contract(
    node: &GlobalICFGNode,
    ffi_functions: &HashSet<String>,
) -> AllocationContract {
    // allocation_contracts_v2 is a producer-certified *deallocator* refinement.
    // Official specification mapping:
    //
    // LLVM LangRef defines `"alloc-family"="malloc"` as the family shared by
    // malloc/calloc/realloc/free and `allockind("free")` as freeing `allocptr`:
    // https://llvm.org/docs/LangRef.html#alloc-family
    // https://llvm.org/docs/LangRef.html#allockind
    // CREMA serializes that LLVM family as `c_malloc` to avoid confusing the
    // abstract family name with one concrete allocation operation.  The pinned
    // LLVM IR used by FINAL112 does not necessarily carry those modern
    // attributes, so `structural_c_free_v1` means an exact direct `@free`
    // call observed in LLVM/SVF input, not a claim that allockind metadata was
    // present.  Attribute-certified libc summaries are a separate B1.1-r2 step.
    //
    // Rust Box/Vec `Global` facts are established upstream by rustc semantic
    // type identity (see `rust_drop_allocator_evidence` in icfg.rs) and the
    // official memory-layout contracts:
    // https://doc.rust-lang.org/std/boxed/index.html#memory-layout
    // https://doc.rust-lang.org/std/vec/index.html#memory-layout
    //
    // `std::alloc::dealloc` is documented to deallocate with the global allocator:
    // https://doc.rust-lang.org/std/alloc/fn.dealloc.html
    match node {
        GlobalICFGNode::Llvm(llvm)
            if llvm.node_kind_string == "FunCallBlock" && llvm.info.contains("@free(") =>
                AllocationContract::v2_deallocator(
                    "c_malloc", "free", "c", "structural_c_free_v1",
                ),
        GlobalICFGNode::Mir(bb) => match &bb.terminator {
            Some(MirTerminator::Drop { deallocator_evidence: Some(evidence), .. }) => {
                let basis = match evidence.kind {
                    RustDropAllocatorEvidenceKind::BoxGlobal => "rust_box_global_drop",
                    RustDropAllocatorEvidenceKind::VecGlobal => "rust_vec_global_drop",
                    RustDropAllocatorEvidenceKind::CStringGlobal => "rust_cstring_global_drop",
                    RustDropAllocatorEvidenceKind::StringGlobal => "rust_string_global_drop",
                };
                let mut contract = AllocationContract::v2_deallocator(
                    "rust_global", "drop", "rust", basis,
                );
                contract.owner_def_path = Some(evidence.owner_def_path.clone());
                contract.allocator_def_path = Some(evidence.allocator_def_path.clone());
                contract
            }
            Some(MirTerminator::Drop { deallocator_evidence: None, .. }) =>
                AllocationContract::v2_deallocator(
                    "unknown", "drop", "rust", "unresolved",
                ),
            Some(MirTerminator::Call { function_called, deallocator_evidence, details, .. }) => {
                let call_text = if details.is_empty() { function_called } else { details };
                if is_c_free_function(function_called, ffi_functions)
                    || is_c_free_call_text(call_text, ffi_functions)
                {
                    AllocationContract::v2_deallocator(
                        "c_malloc", "free", "c", "structural_c_free_v1",
                    )
                } else if let Some(evidence) = deallocator_evidence {
                    match evidence.kind {
                        RustCallDeallocatorEvidenceKind::GlobalDeallocApi => {
                            let mut contract = AllocationContract::v2_deallocator(
                                "rust_global", "dealloc", "rust", "rust_global_dealloc_api",
                            );
                            contract.callee_def_path = Some(evidence.callee_def_path.clone());
                            contract
                        }
                        RustCallDeallocatorEvidenceKind::MemDropOwnedBoxGlobal => {
                            let mut contract = AllocationContract::v2_deallocator(
                                "rust_global",
                                "drop",
                                "rust",
                                "rust_mem_drop_owned_box_global_v1",
                            );
                            contract.callee_def_path = Some(evidence.callee_def_path.clone());
                            contract.owner_def_path = evidence.owner_def_path.clone();
                            contract.allocator_def_path = evidence.allocator_def_path.clone();
                            contract
                        }
                    }
                } else if is_raw_dealloc_call(function_called) || is_raw_dealloc_call(call_text) {
                    // v1 recognized additional pretty-printed spellings. v2
                    // refuses to promote them without canonical API identity.
                    AllocationContract::v2_deallocator(
                        "unknown", "dealloc", "rust", "unresolved",
                    )
                } else if is_explicit_mem_drop(function_called) || is_explicit_mem_drop(call_text) {
                    AllocationContract::v2_deallocator(
                        "unknown", "drop", "rust", "unresolved",
                    )
                } else {
                    AllocationContract::v2_deallocator(
                        "unknown", "unknown", "unknown", "unresolved",
                    )
                }
            }
            _ => AllocationContract::v2_deallocator(
                "unknown", "unknown", "unknown", "unresolved",
            ),
        },
        _ => AllocationContract::v2_deallocator(
            "unknown", "unknown", "unknown", "unresolved",
        ),
    }
}

fn efx1_unique_function_effect<'a>(
    effects: &'a LlvmMemoryEffectsArtifactV1,
    name: &str,
) -> Option<&'a LlvmFunctionEffectsRecordV1> {
    let mut matches = effects
        .modules
        .iter()
        .flat_map(|module| module.functions.iter())
        .filter(|record| record.name == name);
    let first = matches.next()?;
    if matches.next().is_some() {
        // Multiple modules may legally contain same-named internal functions.
        // Without module identity at this boundary, do not guess.
        return None;
    }
    Some(first)
}

fn efx1_memory_is_nonmodifying(snapshot: &LlvmFunctionEffectsSnapshotV1) -> bool {
    if !snapshot.memory_explicit {
        return false;
    }
    [
        snapshot.memory.argmem.as_str(),
        snapshot.memory.inaccessiblemem.as_str(),
        snapshot.memory.other.as_str(),
    ]
    .into_iter()
    .all(|access| matches!(access, "none" | "read"))
}

fn efx1_snapshot_may_deallocate(snapshot: &LlvmFunctionEffectsSnapshotV1) -> bool {
    let dealloc_kind = snapshot
        .alloc_kind
        .iter()
        .any(|kind| matches!(kind.as_str(), "free" | "realloc"));
    dealloc_kind && snapshot.formals.iter().any(|formal| formal.allocptr)
}

fn efx1_snapshot_no_deallocation_basis(
    snapshot: &LlvmFunctionEffectsSnapshotV1,
    explicit: bool,
) -> Option<&'static str> {
    if snapshot.nofree {
        return Some(if explicit {
            "llvm16_explicit_nofree_v1"
        } else {
            "llvm16_tli_nofree_v1"
        });
    }
    // LLVM 16 Function::doesNotFreeMemory treats read-only/no-access memory
    // effects as sufficient to prove that the function does not free memory.
    // Preserve that as a distinct proof basis rather than mislabeling it nofree.
    if efx1_memory_is_nonmodifying(snapshot) {
        return Some(if explicit {
            "llvm16_explicit_nonmodifying_memory_v1"
        } else {
            "llvm16_tli_nonmodifying_memory_v1"
        });
    }
    None
}

/// Return a capability-gated LLVM16 effect classification for a named callee.
///
/// Explicit input-IR contracts take precedence.  TLI evidence is admitted only
/// when the cloned module actually changed a TLI-recognized declaration.  This
/// preserves the scientific distinction between source/frontend contracts and
/// LLVM name+prototype library inference.
fn efx1_callee_deallocation_contract(
    effects: &LlvmMemoryEffectsArtifactV1,
    callee: &str,
) -> Option<(&'static str, &'static str)> {
    if effects.schema != "llvm_memory_effects_v1"
        || effects.explicit_basis != "llvm16_explicit_input_ir_v1"
        || effects.tli_basis != "llvm16_tli_libfunc_attrs_v1"
    {
        return None;
    }
    let record = efx1_unique_function_effect(effects, callee)?;

    if efx1_snapshot_may_deallocate(&record.explicit) {
        return Some((
            "observed_may_deallocate",
            "llvm16_explicit_allockind_deallocation_v1",
        ));
    }
    if let Some(basis) = efx1_snapshot_no_deallocation_basis(&record.explicit, true) {
        return Some(("certified_absent", basis));
    }

    if record.tli_changed && record.tli_recognized {
        if efx1_snapshot_may_deallocate(&record.tli_inferred) {
            return Some((
                "observed_may_deallocate",
                "llvm16_tli_allockind_deallocation_v1",
            ));
        }
        if let Some(basis) = efx1_snapshot_no_deallocation_basis(&record.tli_inferred, false) {
            return Some(("certified_absent", basis));
        }
    }
    None
}

/// Structured positive closure for a body-backed C wrapper: if the LLVM input
/// contains a direct CallBase from `caller` to a callee whose explicit or
/// TLI-inferred contract is free/realloc + allocptr, the wrapper has a
/// conservative MAY-deallocation effect.  This is intentionally positive-only:
/// absence of such a call is never promoted to a negative certificate.
fn efx1_structured_direct_callee_deallocation_basis(
    effects: &LlvmMemoryEffectsArtifactV1,
    caller: &str,
) -> Option<&'static str> {
    let mut matching_modules = effects.modules.iter().filter(|module| {
        module.functions.iter().any(|record| record.name == caller)
    });
    let module = matching_modules.next()?;
    if matching_modules.next().is_some() {
        // Same-named wrapper in more than one module: module identity is not
        // available on the DummyCall boundary, so fail closed.
        return None;
    }

    for call in module.callsites_explicit.iter().filter(|call| call.caller == caller && call.direct) {
        let Some(callee) = call.callee.as_deref() else { continue; };
        let Some(record) = module.functions.iter().find(|record| record.name == callee) else {
            continue;
        };
        if efx1_snapshot_may_deallocate(&record.explicit) {
            return Some("llvm16_explicit_direct_callee_allockind_deallocation_v1");
        }
        if record.tli_changed && record.tli_recognized
            && efx1_snapshot_may_deallocate(&record.tli_inferred)
        {
            return Some("llvm16_tli_direct_callee_allockind_deallocation_v1");
        }
    }
    None
}

/// Independent LLVM16 evidence that corroborates an already-selected
/// historical external-effect basis.
///
/// `structural_c_free_v1` remains the primary provenance for historical
/// comparability. A matching structured LLVM CallBase + explicit/TLI
/// `allockind(free|realloc)+allocptr` contract is emitted only as additive
/// corroboration and never changes the MAY status or CQPL truth value.
fn external_deallocation_corroborating_bases(
    effects: Option<&LlvmMemoryEffectsArtifactV1>,
    outer_callee: &str,
    status: &str,
    basis: &str,
) -> Vec<&'static str> {
    if status != "observed_may_deallocate" || basis != "structural_c_free_v1" {
        return Vec::new();
    }
    let Some(effects) = effects else {
        return Vec::new();
    };
    efx1_structured_direct_callee_deallocation_basis(effects, outer_callee)
        .into_iter()
        .collect()
}

/// Bcontract-ND1 + EFX1 conservative producer classifier.
///
/// The classifier selects exactly one primary status/basis. Historical ND1
/// structural evidence is checked first so existing provenance remains stable.
/// EFX1 then closes bodyless frontiers and structured body-backed cases without
/// parsing SVF pretty strings. Independent LLVM evidence that agrees with an
/// already-selected historical basis is emitted separately by
/// `external_deallocation_corroborating_bases`.
fn classify_external_deallocation_effect(
    nodes: &[crate::structs::LlvmJsonNode],
    effects: Option<&LlvmMemoryEffectsArtifactV1>,
    outer_callee: &str,
) -> (&'static str, &'static str) {
    let has_body = nodes.iter().any(|node| {
        node.basic_block_name.is_some() || node.basic_block_info.is_some()
    });

    // Preserve the frozen positive structural oracle before introducing EFX1.
    let calls = nodes
        .iter()
        .filter(|node| node.node_kind_string == "FunCallBlock")
        .collect::<Vec<_>>();
    if calls.iter().any(|node| node.info.contains("@free(")) {
        return ("observed_may_deallocate", "structural_c_free_v1");
    }

    if let Some(effects) = effects {
        // Contracts on the outer function itself are valid for declarations or
        // definitions. TLI inference is admitted only when tli_changed is true,
        // which in EFX1 can occur only for declarations.
        if let Some(classification) = efx1_callee_deallocation_contract(effects, outer_callee) {
            return classification;
        }
        // For body-backed wrappers, structured CallBase identity supports a
        // positive MAY closure to free/realloc without parsing pretty strings.
        if has_body {
            if let Some(basis) = efx1_structured_direct_callee_deallocation_basis(effects, outer_callee) {
                return ("observed_may_deallocate", basis);
            }
        }
    }

    if !has_body {
        return ("unresolved", "svf_body_unavailable_v1");
    }

    if calls.is_empty() {
        return ("certified_absent", "svf_leaf_no_call_deallocation_v1");
    }

    ("unresolved", "svf_call_effect_unresolved_v1")
}

fn external_dummy_callee_callsite(dummy: &crate::structs::DummyNode) -> Option<(String, String)> {
    let rest = dummy.outgoing_edge.strip_prefix("llvm::")?;
    let (callee, tail) = rest.split_once("::node")?;
    let (_, suffix) = tail.split_once("::rust::")?;
    if callee.is_empty() || suffix.is_empty() {
        return None;
    }
    Some((callee.to_string(), format!("rust::{suffix}")))
}

/// Serialize the already-established Bmulti positional Rust -> C allocation
/// identity relation.  This is an observational certificate only: both sides
/// are read from the same post-DummyCall identity fixed point, and the producer
/// fails closed if their MAY allocation sets disagree.
fn ffi_argument_identity_records(
    icfg: &GlobalICFGOrdered,
    identity_state: &AllocationIdentityState,
) -> Result<Vec<FfiArgumentIdentityRecord>, Box<dyn Error>> {
    let mut out = BTreeSet::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::DummyCall(dummy) = node else { continue; };
        if dummy.is_internal != Some(false) || dummy.argument_bindings.is_empty() {
            continue;
        }

        let Some(caller_scope) = mir_function_scope_from_node_id(&dummy.incoming_edge) else {
            return Err(format!(
                "ffi_argument_identity_v1: cannot resolve Rust caller scope for '{node_id}'"
            ).into());
        };
        let caller_function = caller_scope.strip_prefix("rust::").unwrap_or(&caller_scope);
        let Some((callee, callsite)) = external_dummy_callee_callsite(dummy) else {
            return Err(format!(
                "ffi_argument_identity_v1: cannot resolve C callee/callsite for '{node_id}'"
            ).into());
        };
        let post = identity_state.by_node.get(node_id).cloned().unwrap_or_default();
        let mut seen_indices = BTreeSet::new();

        for binding in &dummy.argument_bindings {
            if !seen_indices.insert(binding.arg_index) {
                return Err(format!(
                    "ffi_argument_identity_v1: duplicate arg_index {} at '{node_id}'",
                    binding.arg_index
                ).into());
            }
            let Some(actual) = ProgramVarId::rust(caller_function.to_string(), &binding.mir_var) else {
                // Bmulti certifies source-language argument positions, not that every
                // MIR operand is a local place. Constants and other non-local operands
                // have no ProgramVarId in the allocation-identity domain and therefore
                // cannot carry an AbstractAllocId certificate here.
                //
                // This mirrors transfer_external_dummy_call_identity(), which skips
                // the same non-local bindings. Absence of a record is strictly
                // non-evidence; it must never be interpreted as proof that no
                // allocation flows through the argument.
                continue;
            };
            let raw_formal = binding
                .llvm_var
                .split_once('@')
                .map(|(id, _)| id)
                .unwrap_or(binding.llvm_var.as_str())
                .trim()
                .trim_start_matches('%');
            let formal_id = raw_formal.parse::<usize>().map_err(|_| {
                format!(
                    "ffi_argument_identity_v1: invalid SVF formal '{}' at '{node_id}'",
                    binding.llvm_var
                )
            })?;
            let formal = ProgramVarId::c(callee.clone(), formal_id, Some(callsite.clone()));

            let actual_allocs = post.event_allocations(&actual);
            let formal_allocs = post.event_allocations(&formal);
            if actual_allocs != formal_allocs {
                return Err(format!(
                    "ffi_argument_identity_v1: positional identity mismatch at '{node_id}' arg {}: actual={} formal={}",
                    binding.arg_index,
                    actual_allocs.len(),
                    formal_allocs.len(),
                ).into());
            }
            // An empty MAY identity set is not negative evidence.  Do not emit
            // a vacuous certificate that could be misread as proving absence
            // of allocation flow across the FFI boundary.
            if actual_allocs.is_empty() {
                continue;
            }

            let mut svf_pts = binding.svf_may_points_to.clone();
            svf_pts.sort_unstable();
            svf_pts.dedup();
            if !svf_pts.is_empty()
                && binding.svf_points_to_basis.as_deref() != Some("svf_andersen_wave_diff_may_v1")
            {
                return Err(format!(
                    "ffi_argument_identity_v1: nonempty SVF MAY set without Andersen basis at '{node_id}' arg {}",
                    binding.arg_index
                ).into());
            }

            out.insert(FfiArgumentIdentityRecord {
                node: node_id.clone(),
                callee: callee.clone(),
                callsite: callsite.clone(),
                arg_index: binding.arg_index,
                actual_variable: actual.canonical_string(),
                formal_variable: formal.canonical_string(),
                allocations: actual_allocs.iter().map(stable_allocation_id).collect(),
                certainty: "may_abstract",
                basis: "crema_bmulti_actual_formal_identity_v1",
                formal_mapping_basis: "svf_formal_arg_index_v1",
                svf_may_points_to: svf_pts,
                svf_points_to_basis: binding.svf_points_to_basis.clone(),
            });
        }
    }

    Ok(out.into_iter().collect())
}

fn external_deallocation_effect_records(
    icfg: &GlobalICFGOrdered,
) -> Vec<ExternalDeallocationEffectRecord> {
    let mut out = BTreeSet::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::DummyCall(dummy) = node else { continue; };
        if dummy.is_internal != Some(false) {
            continue;
        }
        let Some(rest) = dummy.outgoing_edge.strip_prefix("llvm::") else { continue; };
        let Some((callee, after_node)) = rest.split_once("::node") else { continue; };
        let Some((_, suffix)) = after_node.split_once("::rust::") else { continue; };
        let callsite = format!("rust::{suffix}");
        let prefix = format!("llvm::{callee}::node");
        let suffix = format!("::{callsite}");

        let body_nodes = icfg
            .ordered_nodes
            .iter()
            .filter_map(|(candidate_id, candidate)| {
                if !candidate_id.starts_with(&prefix) || !candidate_id.ends_with(&suffix) {
                    return None;
                }
                match candidate {
                    GlobalICFGNode::Llvm(llvm)
                        if llvm.function_name.as_deref() == Some(callee) => Some(llvm.clone()),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();

        let (status, basis) = classify_external_deallocation_effect(
            &body_nodes,
            icfg.llvm_memory_effects.as_ref(),
            callee,
        );
        let corroborating_bases = external_deallocation_corroborating_bases(
            icfg.llvm_memory_effects.as_ref(),
            callee,
            status,
            basis,
        );
        out.insert(ExternalDeallocationEffectRecord {
            node: node_id.clone(),
            callee: callee.to_string(),
            status,
            basis,
            corroborating_bases,
        });
    }

    out.into_iter().collect()
}


fn external_formal_memory_effect_records(
    icfg: &GlobalICFGOrdered,
    ffi_functions: &HashSet<String>,
    represented_c_functions: &HashSet<String>,
) -> Vec<ExternalFormalMemoryEffectRecord> {
    let mut out = BTreeSet::new();
    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(MirTerminator::Call {
            function_called,
            arguments,
            ..
        }) = bb.terminator.as_ref() else { continue; };
        let Some(contract) = external_function_memory_contract_v2(
            function_called,
            arguments.len(),
            ffi_functions,
            represented_c_functions,
        ) else { continue; };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);

        for rule in contract.rules {
            if !external_formal_memory_effect_rule_active_v2(&rule, arguments) { continue; }
            let Some(argument) = arguments.get(rule.formal_index) else { continue; };
            let Some(event_variable) = canonical_mir_local(&argument.arg) else { continue; };
            let Some(actual_variable) = ProgramVarId::rust(function, &argument.arg) else { continue; };
            out.insert(ExternalFormalMemoryEffectRecord {
                node: node_id.clone(),
                callee: contract.callee.to_string(),
                semantic_class: contract.semantic_class,
                formal_index: rule.formal_index,
                access: rule.access,
                event_variable,
                actual_variable: actual_variable.canonical_string(),
                extent_kind: rule.extent.kind(),
                extent_argument_index: rule.extent.argument_index(),
                basis: rule.basis,
                semantic_sources: rule.semantic_sources.to_vec(),
            });
        }
    }
    out.into_iter().collect()
}

#[derive(Debug, Clone, Serialize)]
struct ExternalReturnRelationRecord {
    node: String,
    callee: String,
    arity: usize,
    semantic_class: &'static str,
    relation_kind: &'static str,
    result_variable: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_formal_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_actual_variable: Option<String>,
    nullability: &'static str,
    ownership: &'static str,
    basis: &'static str,
    semantic_sources: Vec<&'static str>,
}

/// Independent call-binding certificate, extracted from MIR rather than from
/// the return-summary tuple. Consumers cross-check result and formal identity.
#[derive(Debug, Clone, Serialize)]
struct ExternalReturnCallBinding {
    node: String,
    callee: String,
    arguments: Vec<Option<String>>,
    result_variable: String,
    body_status: &'static str,
    basis: &'static str,
}

/// ENE1 is an evidence-only sidecar. No abstract-state or event APIs are called.
#[derive(Debug, Clone, Serialize)]
struct ExternalNegativeEvidenceRecord {
    node: String,
    callee: String,
    evidence_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    formal_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_variable: Option<String>,
    evidence_source: &'static str,
    basis: &'static str,
    evidence_module: usize,
    evidence_function: usize,
    attribute_origin: &'static str,
    // Exact positional MIR binding, independent of allocation identity.
    call_arguments: Vec<Option<String>>,
    body_status: &'static str,
    binding_basis: &'static str,
}

fn external_negative_evidence_records(
    icfg: &GlobalICFGOrdered, ffi: &HashSet<String>, represented: &HashSet<String>,
) -> Vec<ExternalNegativeEvidenceRecord> {
    let mut records = vec![];
    let Some(effects) = &icfg.llvm_memory_effects else { return records; };
    // The existing EFX1 loader verifies snapshots, TLI admissibility and clone
    // callsite cardinality/identity before constructing this ICFG.
    if effects.schema != "llvm_memory_effects_v1" || effects.llvm_version != "16.0.4"
        || effects.explicit_basis != "llvm16_explicit_input_ir_v1"
        || effects.tli_basis != "llvm16_tli_libfunc_attrs_v1" { return records; }
    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(MirTerminator::Call { function_called, arguments, .. }) = &bb.terminator else { continue; };
        let callee = function_called.trim();
        if !ffi.contains(callee) || represented.contains(callee) { continue; }
        let mut matches = effects.modules.iter().enumerate().flat_map(|(mi, m)|
            m.functions.iter().enumerate().filter(move |(_, f)| f.name == callee).map(move |(fi, f)| (mi, fi, f)));
        let Some((mi, fi, function)) = matches.next() else { continue; };
        if matches.next().is_some() || !function.is_declaration
            || !effects.modules[mi].input_ir_verified || !effects.modules[mi].tli_clone_verified
            || function.origin_explicit != "explicit_input_ir" || function.origin_inferred != "llvm_tli_inferred"
            || function.explicit.formals.len() != arguments.len() { continue; }
        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        let scope = scope.strip_prefix("rust::").unwrap_or(&scope);
        let actuals: Vec<_> = arguments.iter().map(|arg|
            if arg.arg.contains(" -> ") { None } else { ProgramVarId::rust(scope, &arg.arg).map(|var| var.canonical_string()) }).collect();
        let mut emit = |kind, index: Option<usize>, source, basis| {
            let actual = index.and_then(|i| actuals.get(i).cloned().flatten());
            if index.is_some() && actual.is_none() { return; }
            records.push(ExternalNegativeEvidenceRecord {
                node: node_id.clone(), callee: callee.into(), evidence_kind: kind,
                formal_index: index, actual_variable: actual, evidence_source: source, basis,
                evidence_module: mi, evidence_function: fi, attribute_origin: "callee_declaration",
                call_arguments: actuals.clone(), body_status: "bodyless", binding_basis: "rustc_mir_call_binding_v1",
            });
        };
        if function.explicit.nofree {
            emit("no_free_function", None, "llvm16_explicit_ir", "llvm16_explicit_function_nofree_v1");
        } else if function.tli_changed && function.tli_recognized && !function.explicit.nobuiltin
            && !function.explicit.optnone && function.tli_inferred.nofree {
            emit("no_free_function", None, "llvm16_verified_tli", "llvm16_tli_verified_function_nofree_v1");
        }
        for formal in &function.explicit.formals {
            if !formal.pointer_typed || formal.index >= actuals.len() { continue; }
            if formal.nofree { emit("no_free_formal", Some(formal.index), "llvm16_explicit_ir", "llvm16_explicit_formal_nofree_v1"); }
            if formal.nocapture { emit("no_capture_formal", Some(formal.index), "llvm16_explicit_ir", "llvm16_explicit_formal_nocapture_v1"); }
        }
    }
    records
}

fn external_return_relation_records(
    icfg: &GlobalICFGOrdered,
    ffi: &HashSet<String>,
    represented: &HashSet<String>,
) -> (Vec<ExternalReturnRelationRecord>, Vec<ExternalReturnCallBinding>) {
    let mut records = vec![];
    let mut bindings = vec![];
    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(MirTerminator::Call { function_called, arguments, return_place, .. }) = &bb.terminator else { continue; };
        let Some(contract) = crate::identity::external_return_contract(
            function_called, arguments.len(), represented.contains(function_called.trim()),
            arguments.get(2).is_some_and(|arg| arg.arg.trim() == "const 0_usize"), ffi,
        ) else { continue; };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);
        // Projected places are not the call's destination local. Never guess one.
        if return_place.contains(" -> ") { continue; }
        let Some(result) = ProgramVarId::rust(function, return_place) else { continue; };
        let source = if contract.relation_kind == "nullable_borrowed_external" { None }
            else { arguments.first().filter(|arg| !arg.arg.contains(" -> ")).and_then(|arg| ProgramVarId::rust(function, &arg.arg)) };
        if contract.relation_kind != "nullable_borrowed_external" && source.is_none() { continue; }
        let result_variable = result.canonical_string();
        records.push(ExternalReturnRelationRecord {
            node: node_id.clone(), callee: function_called.trim().to_string(), arity: arguments.len(),
            semantic_class: contract.semantic_class, relation_kind: contract.relation_kind,
            result_variable: result_variable.clone(), source_formal_index: source.as_ref().map(|_| 0),
            source_actual_variable: source.map(|source| source.canonical_string()),
            nullability: contract.nullability, ownership: contract.ownership,
            basis: "crema_err1_closed_contract_v1", semantic_sources: vec![contract.semantic_source],
        });
        bindings.push(ExternalReturnCallBinding {
            node: node_id.clone(), callee: function_called.trim().to_string(), result_variable,
            arguments: arguments.iter().map(|arg| ProgramVarId::rust(function, &arg.arg).map(|var| var.canonical_string())).collect(),
            body_status: "bodyless", basis: "rustc_mir_call_binding_v1",
        });
    }
    (records, bindings)
}

/// Compute the node set reachable from one explicit ICFG entry.
///
/// v6K intentionally uses the serialized canonical edge relation here.  No
/// exporter-only call/return edges are synthesized: if CREMA cannot represent
/// a transition in `icfg_edges`, CQPL must not silently gain it later.
fn reachable_from(
    entry: &str,
    successors: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let mut worklist = std::collections::VecDeque::new();
    reachable.insert(entry.to_string());
    worklist.push_back(entry.to_string());

    while let Some(node) = worklist.pop_front() {
        if let Some(succs) = successors.get(&node) {
            for succ in succs {
                if reachable.insert(succ.clone()) {
                    worklist.push_back(succ.clone());
                }
            }
        }
    }
    reachable
}

/// Certify the existing canonical topology, then serialize one record per call.
/// No node-id parsing or callgraph reconstruction participates in this boundary.
fn rust_call_return_relation_records(
    icfg: &GlobalICFGOrdered,
    successors: &BTreeMap<String, BTreeSet<String>>,
    node_ids: &BTreeSet<String>,
) -> Result<Vec<RustCallReturnRelationV1>, Box<dyn Error>> {
    validate_canonical_internal_rust_edges(icfg, successors)?;
    let mut dummy_calls = BTreeSet::new();
    let mut dummy_returns = BTreeSet::new();
    let mut records = Vec::with_capacity(icfg.rust_calls.len());
    for call in &icfg.rust_calls {
        // The canonical validator above certifies callee membership and edges.
        let function = &icfg.rust_functions[&call.callee_function];
        for node in [
            &call.call_node, &call.dummy_call_node, &function.entry_node,
            &call.dummy_ret_node, &call.return_node,
        ].into_iter().chain(function.return_nodes.iter()) {
            if !node_ids.contains(node) {
                return Err(format!("Rust call/return relation references missing canonical node '{node}'").into());
            }
        }
        if !dummy_calls.insert(&call.dummy_call_node) {
            return Err(format!("ambiguous Rust call/return relation: duplicate dummy call '{}'", call.dummy_call_node).into());
        }
        if !dummy_returns.insert(&call.dummy_ret_node) {
            return Err(format!("ambiguous Rust call/return relation: duplicate dummy return '{}'", call.dummy_ret_node).into());
        }
        let callee_return_nodes = function.return_nodes.iter().cloned()
            .collect::<BTreeSet<_>>().into_iter().collect();
        records.push(RustCallReturnRelationV1 {
            caller_function: call.caller_function.clone(),
            call_node: call.call_node.clone(),
            dummy_call_node: call.dummy_call_node.clone(),
            callee_function: call.callee_function.clone(),
            callee_entry_node: function.entry_node.clone(),
            callee_return_nodes,
            dummy_ret_node: call.dummy_ret_node.clone(),
            continuation_node: call.return_node.clone(),
            basis: "rust_call_metadata_v1",
        });
    }
    records.sort_by(|a, b| {
        (&a.caller_function, &a.call_node, &a.dummy_call_node, &a.callee_function,
         &a.dummy_ret_node, &a.continuation_node).cmp(
        &(&b.caller_function, &b.call_node, &b.dummy_call_node, &b.callee_function,
          &b.dummy_ret_node, &b.continuation_node))
    });
    Ok(records)
}

/// Check that Rust call metadata and the serialized ICFG relation describe the
/// *same* interprocedural topology.
///
/// Historically CREMA kept ordinary Rust call edges hidden in `rust_calls` and
/// `cqpl_export` reconstructed a different transition system.  That made the
/// abstract interpreter and model checker reason over distinct Kripke graphs.
/// v6K forbids this: metadata may carry bindings, but every control-flow
/// activation/return must already be an explicit edge in `icfg_edges`.
fn validate_canonical_internal_rust_edges(
    icfg: &GlobalICFGOrdered,
    successors: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), Box<dyn Error>> {
    let has_edge = |src: &str, dst: &str| {
        successors
            .get(src)
            .is_some_and(|succs| succs.contains(dst))
    };

    for call in &icfg.rust_calls {
        let Some(function) = icfg.rust_functions.get(&call.callee_function) else {
            return Err(format!(
                "canonical ICFG invariant violated: call '{}' names missing local callee '{}'",
                call.call_node, call.callee_function
            )
            .into());
        };

        if !has_edge(&call.call_node, &call.dummy_call_node) {
            return Err(format!(
                "canonical ICFG invariant violated: missing call edge '{}' -> '{}'",
                call.call_node, call.dummy_call_node
            )
            .into());
        }
        if !has_edge(&call.dummy_call_node, &function.entry_node) {
            return Err(format!(
                "canonical ICFG invariant violated: missing activation edge '{}' -> '{}' for callee '{}'",
                call.dummy_call_node, function.entry_node, call.callee_function
            )
            .into());
        }
        if !has_edge(&call.dummy_ret_node, &call.return_node) {
            return Err(format!(
                "canonical ICFG invariant violated: missing continuation edge '{}' -> '{}'",
                call.dummy_ret_node, call.return_node
            )
            .into());
        }

        for ret in &function.return_nodes {
            if !has_edge(ret, &call.dummy_ret_node) {
                return Err(format!(
                    "canonical ICFG invariant violated: missing return edge '{}' -> '{}' for callee '{}'",
                    ret, call.dummy_ret_node, call.callee_function
                )
                .into());
            }
        }
    }

    Ok(())
}

/// Return the MIR function/closure scope encoded in a GlobalICFG node id.
/// Example: `rust::main::{closure#0}::bb3` -> `rust::main::{closure#0}`.
fn mir_function_scope_from_node_id(node_id: &str) -> Option<String> {
    let (scope, bb) = node_id.rsplit_once("::bb")?;
    if !bb.is_empty() && bb.chars().all(|c| c.is_ascii_digit()) {
        Some(scope.to_string())
    } else {
        None
    }
}

/// Field index read by rustc CopyForDeref from a closure environment.
fn closure_field_copy_for_deref_index(rvalue: &str) -> Option<usize> {
    let caps = CLOSURE_FIELD_COPY_FOR_DEREF_RE.captures(rvalue.trim())?;
    caps.get(1)?.as_str().parse::<usize>().ok()
}

/// Build a closure-local MAY value-identity relation used only for syntactic
/// event labels.
///
/// rustc represents a captured raw pointer through two different kinds of
/// temporaries inside a closure.  For example, nightly-2024-11-21 emits:
///
///   _8 = deref_copy ((*_1).0: &*mut i32); // reference to capture field 0
///   _4 = copy (*_8);                       // raw pointer value in field 0
///   _3 = Box::from_raw(move _4);           // owner of that allocation
///
/// A later use of the same capture may use `_9 -> _7 -> _6` instead.  The
/// reference temporaries (`_8`, `_9`) are NOT the captured pointer value and
/// must not themselves be equated with the pointee.  We therefore propagate a
/// closure-field provenance through the direct dereference load and only then
/// close the resulting value locals with pointwise abstract aliases already
/// observed by CREMA (e.g. `_4 <-> _3` and `_7 <-> _6`).
///
/// Scientific boundary: this relation does not modify AbstractMemory and is
/// not used by semantic may predicates (`alloc/drop/own_forg`). It is a
/// conservative implementation-level augmentation for syntactic event labels
/// only.  It compensates for rustc temporary renaming of repeated reads of the
/// same captured value; it does not make different closure fields equivalent.
fn build_closure_event_aliases(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
) -> BTreeMap<(String, Name), BTreeSet<Name>> {
    // (closure scope, field index) -> reference temporaries produced by
    // CopyForDeref. These temporaries denote references to the capture slot,
    // not the captured pointer value itself.
    let mut field_refs: BTreeMap<(String, usize), BTreeSet<Name>> = BTreeMap::new();

    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        if !scope.contains("{closure#") {
            continue;
        }

        for stmt in &bb.statements {
            let (Some(place), Some(rvalue)) = (&stmt.place, &stmt.rvalue) else { continue; };
            let Some(field_idx) = closure_field_copy_for_deref_index(rvalue) else { continue; };
            let Some(dest_ref) = canonical_mir_local(place) else { continue; };
            field_refs
                .entry((scope.clone(), field_idx))
                .or_default()
                .insert(dest_ref);
        }
    }

    // Recover the actual captured values loaded through those reference
    // temporaries: `_4 = copy (*_8)` means `_4` carries the value of the
    // closure field whose reference was materialized in `_8`.
    let mut field_values: BTreeMap<(String, usize), BTreeSet<Name>> = BTreeMap::new();
    for (node_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(scope) = mir_function_scope_from_node_id(node_id) else { continue; };
        if !scope.contains("{closure#") {
            continue;
        }

        for stmt in &bb.statements {
            let (Some(place), Some(rvalue)) = (&stmt.place, &stmt.rvalue) else { continue; };
            let Some(caps) = DIRECT_DEREF_VALUE_RE.captures(rvalue.trim()) else { continue; };
            let Some(src_ref) = caps.get(1).and_then(|m| canonical_mir_local(m.as_str())) else {
                continue;
            };
            let Some(dest_value) = canonical_mir_local(place) else { continue; };

            for ((ref_scope, field_idx), refs) in &field_refs {
                if ref_scope == &scope && refs.contains(&src_ref) {
                    field_values
                        .entry((scope.clone(), *field_idx))
                        .or_default()
                        .insert(dest_value.clone());
                }
            }
        }
    }

    // Close each captured-value group with pointwise abstract aliases witnessed
    // in the same closure.  This lifts raw-pointer locals to the owning locals
    // created by from_raw without inventing a new abstract-memory relation.
    for ((scope, _field_idx), group) in field_values.iter_mut() {
        loop {
            let before = group.len();
            for (node_id, _node) in &icfg.ordered_nodes {
                if mir_function_scope_from_node_id(node_id).as_deref() != Some(scope.as_str()) {
                    continue;
                }
                let mem = abs_state.get(node_id).unwrap_or_default();
                for allocation in mem.state.keys() {
                    if allocation.set.iter().any(|v| group.contains(v)) {
                        group.extend(allocation.set.iter().cloned());
                    }
                }
            }
            if group.len() == before {
                break;
            }
        }
    }

    let mut lookup = BTreeMap::new();
    for ((scope, _field_idx), group) in field_values {
        if group.len() < 2 {
            continue;
        }
        for var in &group {
            lookup.insert((scope.clone(), var.clone()), group.clone());
        }
    }
    lookup
}

fn expand_closure_event_labels(
    node_id: &str,
    labels: &mut Vec<EventLabel>,
    aliases: &BTreeMap<(String, Name), BTreeSet<Name>>,
) {
    let Some(scope) = mir_function_scope_from_node_id(node_id) else { return; };
    if !scope.contains("{closure#") {
        return;
    }

    let original = labels.clone();
    let mut expanded: BTreeSet<EventLabel> = labels.iter().cloned().collect();
    for label in original {
        // Allocation labels denote a fresh syntactic allocation site and are not
        // alias-expanded. Drop/read/write are events on an existing value.
        if !matches!(label.predicate, "drop" | "read" | "write") {
            continue;
        }
        let key = (scope.clone(), label.variable.clone());
        if let Some(group) = aliases.get(&key) {
            for var in group {
                expanded.insert(EventLabel {
                    predicate: label.predicate,
                    variable: var.clone(),
                });
            }
        }
    }
    *labels = expanded.into_iter().collect();
}

fn memory_annotation(mem: &AbstractMemory) -> AbstractMemoryAnnotation {
    let cells = mem
        .state
        .iter()
        .map(|(allocation, value)| AbstractCell {
            aliases: allocation.set.iter().cloned().collect(),
            value: cell_value_name(*value),
        })
        .collect();
    AbstractMemoryAnnotation { cells }
}

fn allocation_memory_annotation(
    node_id: &str,
    node: &GlobalICFGNode,
    post: &AbstractMemory,
    identity: &AllocationIdentityMemory,
) -> AbstractAllocationMemoryAnnotation {
    let mut values: BTreeMap<String, CellValue> = BTreeMap::new();

    for (legacy_allocation, value) in &post.state {
        for alias in &legacy_allocation.set {
            let Some(var) = identity_var_for_event(node_id, node, alias) else {
                continue;
            };
            // allocation_post is a lifecycle-state projection, not event-subject
            // resolution. `event_allocations` deliberately follows stack_refs so
            // read/write/drop event subjects expressed through `&x` can still be
            // correlated with the allocation denoted by x. Applying that closure
            // here would instead join the CellValue of the reference temporary
            // itself into the pointee allocation (for example TOP for a `&Box<_>`
            // temporary), spuriously widening an otherwise ALLOC pointee to TOP.
            // Only direct heap points-to identities may contribute lifecycle state.
            for allocation in identity.points_to(&var) {
                let id = stable_allocation_id(&allocation);
                values
                    .entry(id)
                    .and_modify(|current| *current = current.join(*value))
                    .or_insert(*value);
            }
        }
    }

    // RN1: legacy AbstractMemory has no dedicated C realloc(NULL, n) transfer.
    // The identity producer nevertheless certifies a fresh nullable allocation
    // for that special case.  Preserve sound lifecycle information by giving
    // the fresh site TOP whenever no stronger legacy cell is available.
    for allocations in identity.points_to.values().chain(identity.place_points_to.values()) {
        for allocation in allocations {
            if matches!(
                &allocation.site,
                AllocationSiteId::CCall { allocator, .. } if allocator == "realloc"
            ) {
                values
                    .entry(stable_allocation_id(allocation))
                    .or_insert(CellValue::TOP);
            }
        }
    }

    AbstractAllocationMemoryAnnotation {
        cells: values
            .into_iter()
            .map(|(allocation, value)| AbstractAllocationCell {
                allocation,
                value: cell_value_name(value),
            })
            .collect(),
    }
}

fn cell_value_name(value: CellValue) -> &'static str {
    match value {
        CellValue::BOTTOM => "BOTTOM",
        CellValue::BOXTIMES => "BOXTIMES",
        CellValue::ALLOC => "ALLOC",
        CellValue::FREED => "FREED",
        CellValue::MB => "MB",
        CellValue::IMMB => "IMMB",
        CellValue::MV => "MV",
        CellValue::TOP => "TOP",
    }
}

fn language_of(id: &str) -> &'static str {
    if id.starts_with("rust::")
        || id.starts_with("Local(")
        || id.starts_with("Leak(Local(")
    {
        "rust"
    } else if id.starts_with("c::")
        || id.starts_with('%')
        || id.chars().next().is_some_and(|c| c.is_ascii_digit())
        || id.contains("@rust::")
    {
        "c"
    } else {
        "other"
    }
}

fn canonical_mir_local(raw: &str) -> Option<Name> {
    let m = MIR_LOCAL_RE.find(raw)?.as_str();
    if m.starts_with("Local(") {
        Some(m.to_string())
    } else {
        Some(format!("Local({m})"))
    }
}

fn mir_locals(raw: &str) -> BTreeSet<Name> {
    MIR_LOCAL_RE
        .find_iter(raw)
        .map(|m| {
            let s = m.as_str();
            if s.starts_with("Local(") {
                s.to_string()
            } else {
                format!("Local({s})")
            }
        })
        .collect()
}

fn mir_deref_locals(raw: &str) -> BTreeSet<Name> {
    MIR_DEREF_RE
        .captures_iter(raw)
        .filter_map(|caps| caps.get(1))
        .filter_map(|m| canonical_mir_local(m.as_str()))
        .collect()
}

fn collect_program_variables(node_id: &str, node: &GlobalICFGNode) -> BTreeSet<Name> {
    let mut vars = BTreeSet::new();
    match node {
        GlobalICFGNode::Mir(bb) => {
            for stmt in &bb.statements {
                vars.extend(mir_locals(&stmt.details));
                if let Some(place) = &stmt.place {
                    vars.extend(mir_locals(place));
                }
                if let Some(rvalue) = &stmt.rvalue {
                    vars.extend(mir_locals(rvalue));
                }
            }
            if let Some(term) = &bb.terminator {
                match term {
                    MirTerminator::Drop { dropped_value, details, .. } => {
                        vars.extend(mir_locals(dropped_value));
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::Call { arguments, return_place, details, .. } => {
                        for arg in arguments {
                            vars.extend(mir_locals(&arg.arg));
                        }
                        vars.extend(mir_locals(return_place));
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::SwitchInt { discr, details, .. } => {
                        vars.extend(mir_locals(discr));
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::Assert { cond, details, .. } => {
                        vars.extend(mir_locals(cond));
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::TailCall { arguments, details, .. } => {
                        for arg in arguments {
                            vars.extend(mir_locals(&arg.arg));
                        }
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::Yield { resume_arg, value, details, .. } => {
                        vars.extend(mir_locals(resume_arg));
                        vars.extend(mir_locals(value));
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::InlineAsm { operands, details, .. } => {
                        for operand in operands {
                            vars.extend(mir_locals(operand));
                        }
                        vars.extend(mir_locals(details));
                    }
                    MirTerminator::Goto { details, .. }
                    | MirTerminator::UnwindResume { details, .. }
                    | MirTerminator::UnwindTerminate { details, .. }
                    | MirTerminator::Return { details, .. }
                    | MirTerminator::Unreachable { details, .. }
                    | MirTerminator::CoroutineDrop { details, .. }
                    | MirTerminator::FalseEdge { details, .. }
                    | MirTerminator::FalseUnwind { details, .. }
                    | MirTerminator::Unhandled { details, .. } => {
                        vars.extend(mir_locals(details));
                    }
                }
            }
        }
        GlobalICFGNode::Llvm(llvm) => {
            for stmt in &llvm.svf_statements {
                if let Some(id) = stmt.result_var_id() {
                    vars.insert(scoped_svf_var(id, node_id));
                }
                if let Some(id) = stmt.rhs_var_id {
                    vars.insert(scoped_svf_var(id, node_id));
                }
                for id in stmt.normalized_operand_var_ids() {
                    vars.insert(scoped_svf_var(id, node_id));
                }
            }
            if let Some(ir) = llvm_free_ir_argument_id(&llvm.info) {
                vars.insert(scoped_ir_var(ir, node_id));
            }
        }
        GlobalICFGNode::DummyCall(d) | GlobalICFGNode::DummyRet(d) => {
            if let Some(v) = &d.mir_var {
                if let Some(v) = canonical_mir_local(v) {
                    vars.insert(v);
                }
            }
            if let Some(v) = &d.llvm_var {
                vars.insert(v.clone());
            }
        }
        GlobalICFGNode::Terminal(_) => {}
    }
    vars
}

fn parse_rust_source_span(raw: &str) -> Option<ParsedSourceSpan> {
    let caps = RUST_SOURCE_SPAN_RE.captures(raw.trim())?;
    Some(ParsedSourceSpan {
        file: caps.name("file")?.as_str().to_string(),
        start_line: caps.name("sl")?.as_str().parse().ok()?,
        start_column: caps.name("sc")?.as_str().parse().ok()?,
        end_line: caps.name("el")?.as_str().parse().ok()?,
        end_column: caps.name("ec")?.as_str().parse().ok()?,
    })
}

fn mir_statement_source_anchor(index: usize, stmt: &crate::structs::MirStatement) -> Option<SourceAnchor> {
    let raw_span = stmt.source_info.span.trim();
    if raw_span.is_empty() {
        return None;
    }
    Some(SourceAnchor {
        kind: "mir_statement",
        statement_index: Some(index),
        raw_span: raw_span.to_string(),
        parsed_span: parse_rust_source_span(raw_span),
        basis: "rustc_mir_source_info_v1",
    })
}

fn mir_terminator_source_info(term: &MirTerminator) -> &str {
    match term {
        MirTerminator::Goto { source_info, .. }
        | MirTerminator::SwitchInt { source_info, .. }
        | MirTerminator::UnwindResume { source_info, .. }
        | MirTerminator::UnwindTerminate { source_info, .. }
        | MirTerminator::Return { source_info, .. }
        | MirTerminator::Unreachable { source_info, .. }
        | MirTerminator::Drop { source_info, .. }
        | MirTerminator::Call { source_info, .. }
        | MirTerminator::TailCall { source_info, .. }
        | MirTerminator::Assert { source_info, .. }
        | MirTerminator::Yield { source_info, .. }
        | MirTerminator::CoroutineDrop { source_info, .. }
        | MirTerminator::FalseEdge { source_info, .. }
        | MirTerminator::FalseUnwind { source_info, .. }
        | MirTerminator::InlineAsm { source_info, .. }
        | MirTerminator::Unhandled { source_info, .. } => source_info,
    }
}

fn mir_terminator_source_anchor(term: &MirTerminator) -> Option<SourceAnchor> {
    let raw_span = mir_terminator_source_info(term).trim();
    if raw_span.is_empty() {
        return None;
    }
    Some(SourceAnchor {
        kind: "mir_terminator",
        statement_index: None,
        parsed_span: parse_rust_source_span(raw_span),
        raw_span: raw_span.to_string(),
        basis: "rustc_mir_source_info_v1",
    })
}

fn llvm_node_source_anchor(llvm: &crate::structs::LlvmJsonNode) -> Option<SourceAnchor> {
    let raw_span = llvm.node_source_loc.trim();
    if raw_span.is_empty() {
        return None;
    }
    Some(SourceAnchor {
        kind: "llvm_node",
        statement_index: None,
        raw_span: raw_span.to_string(),
        parsed_span: None,
        basis: "svf_llvm_node_source_loc_v1",
    })
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExternalFormalMemoryEffectRule {
    formal_index: usize,
    access: &'static str,
    extent: ExternalMemoryExtent,
    basis: &'static str,
    semantic_sources: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalMemoryExtent {
    BytesFromFormal(usize),
    CStringUntilNul,
}

impl ExternalMemoryExtent {
    fn kind(self) -> &'static str {
        match self {
            Self::BytesFromFormal(_) => "bytes_from_formal",
            Self::CStringUntilNul => "c_string_until_nul",
        }
    }

    fn argument_index(self) -> Option<usize> {
        match self {
            Self::BytesFromFormal(index) => Some(index),
            Self::CStringUntilNul => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExternalFunctionMemoryContract {
    callee: &'static str,
    semantic_class: &'static str,
    rules: Vec<ExternalFormalMemoryEffectRule>,
}

fn external_formal_memory_effect_rule_active_v2(
    rule: &ExternalFormalMemoryEffectRule,
    arguments: &[MirCallArgument],
) -> bool {
    let ExternalMemoryExtent::BytesFromFormal(size_index) = rule.extent else {
        return true;
    };
    let Some(size_argument) = arguments.get(size_index) else { return false; };
    // EFM2 is a MAY summary for unknown/positive extents, but a statically exact
    // zero byte-count cannot dereference the byte range for the modeled APIs.
    !definitely_zero_usize_constant(&size_argument.arg)
}

/// EFM2 closed semantic boundary for declaration-only external functions.
///
/// Scientific separation invariant:
/// - SVF/LLVM knowledge classifies the *external API formal role* only.
/// - Rust MIR/AllocationIdentityState supplies the actual operand and AbstractAllocId.
/// - no SVF variable/function object is equated with a Rust MIR variable/function object.
///
/// Official provider semantics from which this CREMA-owned versioned contract is derived:
/// - SVF ExtAPI exposes MEMCPY/MEMSET annotations and AbsExtAPI models strlen,
///   memcpy and memset: https://svf-tools.github.io/SVF-doxygen/html/classSVF_1_1ExtAPI.html
///   and https://svf-tools.github.io/SVF-doxygen/html/classSVF_1_1AbsExtAPI.html
/// - LLVM TargetLibraryInfo validates known library functions by name+prototype;
///   LLVM16 BuildLibCalls/MemoryLocation provide the read/write argument roles:
///   https://llvm.org/doxygen/classllvm_1_1TargetLibraryInfo.html
///   https://llvm.org/doxygen/BuildLibCalls_8h.html
///   https://llvm.org/doxygen/classllvm_1_1MemoryLocation.html
/// - POSIX write consumes bytes from buf: https://pubs.opengroup.org/onlinepubs/9690949599/functions/write.html
/// - POSIX memmove copies n bytes from source to destination and memchr searches
///   the first n bytes:
///   https://pubs.opengroup.org/onlinepubs/9799919799/functions/memmove.html
///   https://pubs.opengroup.org/onlinepubs/9799919799/functions/memchr.html
/// - POSIX/ISO C strchr searches a NUL-terminated string.
/// - LLVM 16 is the frozen corroborating library-semantics version:
///   https://releases.llvm.org/16.0.0/docs/LangRef.html
///
/// This function is deliberately closed and fail-closed at the current artifact boundary.
/// It does not claim that SVF/TLI ran on this bodyless call: their documented semantics
/// define the frozen CREMA contract. EFM2 admits only an exact selected-crate
/// foreign declaration name with exact arity; represented C bodies are excluded to avoid duplicating
/// observed LLVM/SVF events. Prototype-level TLI validation is not reconstructed here.
fn external_function_memory_contract_v2(
    function_called: &str,
    argument_count: usize,
    ffi_functions: &HashSet<String>,
    represented_c_functions: &HashSet<String>,
) -> Option<ExternalFunctionMemoryContract> {
    fn admitted(
        raw: &str,
        name: &'static str,
        ffi_functions: &HashSet<String>,
        represented_c_functions: &HashSet<String>,
    ) -> bool {
        if represented_c_functions.contains(name) {
            return false;
        }
        let t = raw.trim();
        t == name && ffi_functions.contains(name)
    }

    let mk = |callee, semantic_class, rules| ExternalFunctionMemoryContract {
        callee,
        semantic_class,
        rules,
    };

    if argument_count == 1 && admitted(function_called, "strlen", ffi_functions, represented_c_functions) {
        return Some(mk(
            "strlen",
            "strlen_read_c_string_v1",
            vec![ExternalFormalMemoryEffectRule {
                formal_index: 0,
                access: "read",
                extent: ExternalMemoryExtent::CStringUntilNul,
                basis: "crema_efm2_closed_contract_v1",
                semantic_sources: &["svf_absextapi_strlen_semantics_v1", "llvm16_tli_strlen_argmem_read_semantics_v1"],
            }],
        ));
    }
    if argument_count == 3 && admitted(function_called, "memset", ffi_functions, represented_c_functions) {
        return Some(mk(
            "memset",
            "memset_v1",
            vec![ExternalFormalMemoryEffectRule {
                formal_index: 0,
                access: "write",
                extent: ExternalMemoryExtent::BytesFromFormal(2),
                basis: "crema_efm2_closed_contract_v1",
                semantic_sources: &["svf_extapi_memset_semantics_v1", "llvm16_tli_memset_arg0_writeonly_semantics_v1"],
            }],
        ));
    }
    if argument_count == 3 && admitted(function_called, "memcpy", ffi_functions, represented_c_functions) {
        return Some(mk(
            "memcpy",
            "memcpy_v1",
            vec![
                ExternalFormalMemoryEffectRule {
                    formal_index: 0,
                    access: "write",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["svf_extapi_memcpy_semantics_v1", "llvm16_tli_memcpy_arg0_writeonly_semantics_v1"],
                },
                ExternalFormalMemoryEffectRule {
                    formal_index: 1,
                    access: "read",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["svf_extapi_memcpy_semantics_v1", "llvm16_tli_memcpy_arg1_readonly_semantics_v1"],
                },
            ],
        ));
    }
    if argument_count == 3 && admitted(function_called, "memcmp", ffi_functions, represented_c_functions) {
        return Some(mk(
            "memcmp",
            "memcmp_v1",
            vec![
                ExternalFormalMemoryEffectRule {
                    formal_index: 0,
                    access: "read",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["llvm16_tli_memcmp_argmem_read_semantics_v1", "llvm16_memorylocation_memcmp_formal_semantics_v1"],
                },
                ExternalFormalMemoryEffectRule {
                    formal_index: 1,
                    access: "read",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["llvm16_tli_memcmp_argmem_read_semantics_v1", "llvm16_memorylocation_memcmp_formal_semantics_v1"],
                },
            ],
        ));
    }
    if argument_count == 3 && admitted(function_called, "write", ffi_functions, represented_c_functions) {
        return Some(mk(
            "write",
            "posix_write_v1",
            vec![ExternalFormalMemoryEffectRule {
                formal_index: 1,
                access: "read",
                extent: ExternalMemoryExtent::BytesFromFormal(2),
                basis: "crema_efm2_closed_contract_v1",
                semantic_sources: &["posix_write_buffer_semantics_v1", "llvm16_tli_write_arg1_readonly_semantics_v1"],
            }],
        ));
    }
    if argument_count == 3 && admitted(function_called, "memmove", ffi_functions, represented_c_functions) {
        return Some(mk(
            "memmove",
            "memmove_v1",
            vec![
                ExternalFormalMemoryEffectRule {
                    formal_index: 0,
                    access: "write",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["posix_memmove_n_byte_copy_semantics_v1", "llvm16_memmove_formal_semantics_v1", "llvm16_tli_memmove_recognition_v1"],
                },
                ExternalFormalMemoryEffectRule {
                    formal_index: 1,
                    access: "read",
                    extent: ExternalMemoryExtent::BytesFromFormal(2),
                    basis: "crema_efm2_closed_contract_v1",
                    semantic_sources: &["posix_memmove_n_byte_copy_semantics_v1", "llvm16_memmove_formal_semantics_v1", "llvm16_tli_memmove_recognition_v1"],
                },
            ],
        ));
    }
    if argument_count == 3 && admitted(function_called, "memchr", ffi_functions, represented_c_functions) {
        return Some(mk(
            "memchr",
            "memchr_bounded_read_v1",
            vec![ExternalFormalMemoryEffectRule {
                formal_index: 0,
                access: "read",
                extent: ExternalMemoryExtent::BytesFromFormal(2),
                basis: "crema_efm2_closed_contract_v1",
                semantic_sources: &["posix_memchr_bounded_read_semantics_v1", "llvm16_tli_memchr_recognition_v1"],
            }],
        ));
    }
    if argument_count == 2 && admitted(function_called, "strchr", ffi_functions, represented_c_functions) {
        return Some(mk(
            "strchr",
            "strchr_read_c_string_v1",
            vec![ExternalFormalMemoryEffectRule {
                formal_index: 0,
                access: "read",
                extent: ExternalMemoryExtent::CStringUntilNul,
                basis: "crema_efm2_closed_contract_v1",
                semantic_sources: &["posix_strchr_c_string_read_semantics_v1"],
            }],
        ));
    }
    None
}

fn represented_c_function_names(icfg: &GlobalICFGOrdered) -> HashSet<String> {
    icfg.ordered_nodes
        .iter()
        .filter_map(|(_, node)| match node {
            GlobalICFGNode::Llvm(llvm) => llvm.function_name.clone(),
            _ => None,
        })
        .collect()
}

/// Exact structured admission for DCP1. This intentionally does not inspect
/// `details`, labels, allocation identity, or historical free-call text helpers.
fn bodyless_direct_c_free_call_contract<'a>(
    node: &'a GlobalICFGNode,
    ffi_functions: &HashSet<String>,
    represented_c_functions: &HashSet<String>,
) -> Option<&'a MirCallArgument> {
    let GlobalICFGNode::Mir(bb) = node else { return None; };
    let Some(MirTerminator::Call {
        function_called, callee_def_path, arguments, ..
    }) = &bb.terminator else { return None; };
    if function_called != "free" || callee_def_path.as_deref() != Some("free")
        || arguments.len() != 1 || !ffi_functions.contains("free")
        || represented_c_functions.contains("free") {
        return None;
    }
    arguments.first()
}

/// DCP1 proves only the exact call contract; this record has no event producer.
#[derive(Debug, Clone, Serialize)]
struct ExternalDeallocationCallProvenanceV1 {
    node: String,
    callee: &'static str,
    arity: usize,
    formal_index: usize,
    actual_variable: Option<String>,
    family: &'static str,
    operation: &'static str,
    language: &'static str,
    body_status: &'static str,
    certainty: &'static str,
    basis: &'static str,
}

fn dcp1_existing_canonical_actual(
    node_id: &str, argument: &MirCallArgument, catalog: &BTreeSet<Name>,
) -> Option<String> {
    let scope = mir_function_scope_from_node_id(node_id)?;
    let function = scope.strip_prefix("rust::")?;
    let canonical = ProgramVarId::rust(function, &argument.arg)?.canonical_string();
    catalog.contains(&canonical).then_some(canonical)
}

fn external_deallocation_call_provenance_records(
    icfg: &GlobalICFGOrdered,
    ffi_functions: &HashSet<String>,
    represented_c_functions: &HashSet<String>,
    catalog: &BTreeSet<Name>,
) -> Vec<ExternalDeallocationCallProvenanceV1> {
    let mut records = Vec::new();
    for (node_id, node) in &icfg.ordered_nodes {
        let Some(argument) = bodyless_direct_c_free_call_contract(
            node, ffi_functions, represented_c_functions,
        ) else { continue; };
        if !node_id.starts_with("rust::") || mir_function_scope_from_node_id(node_id).is_none() {
            continue;
        }
        records.push(ExternalDeallocationCallProvenanceV1 {
            node: node_id.clone(), callee: "free", arity: 1, formal_index: 0,
            actual_variable: dcp1_existing_canonical_actual(node_id, argument, catalog),
            family: "c_malloc", operation: "free", language: "c",
            body_status: "bodyless", certainty: "may_effect",
            basis: "rust_mir_exact_external_free_call_v1",
        });
    }
    records
}

const ELE1_FAMILIES: [&str; 6] = [
    "allocation_return", "reallocation", "deallocation", "formal_memory",
    "return_relation", "negative_evidence",
];

/// Count the accepted records and bind their call nodes directly to structured
/// MIR. In particular, neither a drop label nor a call's printed text can seed
/// the deallocation family.
fn external_library_effect_envelopes(
    icfg: &GlobalICFGOrdered,
    variables: &[ProgramVariable],
    allocations: &[AbstractAllocationRecord],
    boundaries: &[ReallocationBoundaryRecord],
    deallocations: &[ExternalDeallocationCallProvenanceV1],
    memory: &[ExternalFormalMemoryEffectRecord],
    relations: &[ExternalReturnRelationRecord],
    negative: &[ExternalNegativeEvidenceRecord],
) -> (Option<Vec<ExternalCallBindingV1>>, Option<Vec<ExternalLibraryEffectsV1>>) {
    let catalog: BTreeSet<_> = variables.iter().map(|v| v.id.as_str()).collect();
    let mut counts: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    let mut add = |node: &str, family: &'static str| {
        *counts.entry(node.to_owned()).or_default().entry(family).or_default() += 1;
    };
    for allocation in allocations {
        if let AllocationSiteId::CCall { node_id, .. } = &allocation.site {
            if node_id.starts_with("rust::")
                && allocation.allocator_contract.as_ref().is_some_and(|c|
                    c.family == "c_malloc"
                        && matches!(c.operation, "malloc" | "calloc" | "strdup" | "realloc"))
                && !has_represented_external_body_at(icfg, node_id)
            {
                add(node_id, "allocation_return");
            }
        }
    }
    for record in boundaries { add(&record.node, "reallocation"); }
    for record in deallocations { add(&record.node, "deallocation"); }
    for record in memory { add(&record.node, "formal_memory"); }
    for record in relations { add(&record.node, "return_relation"); }
    for record in negative { add(&record.node, "negative_evidence"); }

    let mut bindings = Vec::new();
    let mut envelopes = Vec::new();
    for (node_id, family_counts) in counts {
        let Some((_, GlobalICFGNode::Mir(block))) = icfg.ordered_nodes.iter()
            .find(|(id, _)| id == &node_id) else { continue; };
        let Some(MirTerminator::Call { function_called, arguments, return_place, .. }) =
            block.terminator.as_ref() else { continue; };
        if has_represented_external_body_at(icfg, &node_id) { continue; }
        let Some(scope) = mir_function_scope_from_node_id(&node_id) else { continue; };
        let function = scope.strip_prefix("rust::").unwrap_or(&scope);
        let canonical = |raw: &str| ProgramVarId::rust(function, raw)
            .map(|v| v.canonical_string())
            .filter(|id| catalog.contains(id.as_str()));
        let binding_id = format!("ele1:{node_id}");
        bindings.push(ExternalCallBindingV1 {
            binding_id: binding_id.clone(), node: node_id,
            rust_function_scope: scope.clone(),
            callee: function_called.trim().to_owned(),
            arity: arguments.len(),
            arguments: arguments.iter().map(|arg| canonical(&arg.arg)).collect(),
            result_variable: canonical(return_place),
            body_status: "bodyless", basis: "rustc_mir_external_call_binding_v1",
        });
        let effect_families = ELE1_FAMILIES.iter().copied()
            .filter(|family| family_counts.get(family).copied().unwrap_or_default() > 0)
            .collect();
        let effect_counts = ELE1_FAMILIES.iter().copied()
            .map(|family| (family, family_counts.get(family).copied().unwrap_or_default()))
            .collect();
        envelopes.push(ExternalLibraryEffectsV1 {
            binding_id, effect_families, effect_counts,
            basis: "crema_external_library_effects_v1",
        });
    }
    if bindings.is_empty() { (None, None) } else { (Some(bindings), Some(envelopes)) }
}

fn crema_ffi_body_coverage_reference(
    icfg: &GlobalICFGOrdered,
    node_id: &str,
) -> Option<String> {
    let (_, GlobalICFGNode::Mir(call_block)) = icfg.ordered_nodes.iter()
        .find(|(id, _)| id == node_id)? else { return None; };
    let Some(MirTerminator::Call { return_target, .. }) = call_block.terminator.as_ref() else { return None; };
    let call_edges: Vec<_> = icfg.icfg_edges.iter().filter(|edge|
        edge.source == node_id && edge.label.as_deref() == Some("FFI Call")
    ).collect();
    let [call_edge] = call_edges.as_slice() else { return None; };
    let dummy_call = &call_edge.destination;
    let entry_edges: Vec<_> = icfg.icfg_edges.iter().filter(|edge|
        edge.source == *dummy_call && edge.label.as_deref() == Some("dummyCall->LLVM Entry")
    ).collect();
    let [llvm_entry] = entry_edges.as_slice() else { return None; };
    let (_, GlobalICFGNode::Llvm(_)) = icfg.ordered_nodes.iter()
        .find(|(id, _)| id == &llvm_entry.destination)? else { return None; };
    let exits: Vec<_> = icfg.icfg_edges.iter().filter(|edge|
        edge.label.as_deref() == Some("LLVM Exit->dummyRet")
            && edge.destination.starts_with("dummyRet::")
            && edge.source.contains(node_id)
    ).collect();
    let [exit] = exits.as_slice() else { return None; };
    let (_, GlobalICFGNode::Llvm(_)) = icfg.ordered_nodes.iter()
        .find(|(id, _)| id == &exit.source)? else { return None; };
    let scope = node_id.rsplit_once("::bb")?.0;
    let continuation = return_target.as_deref()
        .map(|target| format!("{scope}::{target}"))
        .unwrap_or_else(|| format!("{scope}::end"));
    let returns: Vec<_> = icfg.icfg_edges.iter().filter(|edge|
        edge.source == exit.destination
            && edge.destination == continuation
            && edge.label.as_deref() == Some("dummyRet->MIR Return")
    ).collect();
    if returns.len() != 1 { return None; }
    Some(format!("icfg-ffi-body:{}->{};{}->{}", call_edge.source, llvm_entry.destination, exit.source, continuation))
}

fn dep1_call_semantic_coverage(
    icfg: &GlobalICFGOrdered,
    ele1_bindings: &[ExternalCallBindingV1],
    ele1_effects: &[ExternalLibraryEffectsV1],
    reachable_nodes: &BTreeSet<String>,
) -> Option<(Vec<serde_json::Value>, bool)> {
    let dep1 = icfg.dependency_body_ingestion_v1.as_ref()?;
    let mut links: BTreeMap<&str, Vec<&serde_json::Value>> = BTreeMap::new();
    for link in &dep1.icfg_callsite_links {
        if let Some(key) = link.get("call_key").and_then(serde_json::Value::as_str) {
            links.entry(key).or_default().push(link);
        }
    }
    let mut ledger = Vec::new();
    let mut complete = true;
    for call in &dep1.exact_call_bindings {
        let Some(call_key) = call.get("call_key").and_then(serde_json::Value::as_str) else {
            complete = false;
            ledger.push(serde_json::json!({
                "call_key":null,"semantic_coverage_path":"uncovered",
                "coverage_reference":[],"coverage_status":"missing_call_key"
            }));
            continue;
        };
        let caller_instance = call.get("caller_instance_id").and_then(serde_json::Value::as_str).unwrap_or("");
        let bb = call.get("basic_block").and_then(serde_json::Value::as_u64).unwrap_or(u64::MAX);
        let callee_identity = call.get("resolved_instance_id").and_then(serde_json::Value::as_str)
            .or_else(|| call.get("resolved_definition_id").and_then(serde_json::Value::as_str))
            .or_else(|| call.get("operand_definition_id").and_then(serde_json::Value::as_str))
            .unwrap_or("unresolved");
        let expected_key = match call.get("activation_kind").and_then(serde_json::Value::as_str) {
            Some("mir_drop") => {
                format!("dep1-drop-v1:{caller_instance}:bb{bb}:mir_drop:{callee_identity}")
            }
            Some("higher_order_callback") => {
                let ordinal = call.get("callback_ordinal").and_then(serde_json::Value::as_u64).unwrap_or(u64::MAX);
                if ordinal == u64::MAX
                    || call.get("callback_semantics").and_then(serde_json::Value::as_str) != Some("spawn_may_once")
                {
                    complete = false;
                    ledger.push(serde_json::json!({
                        "call_key":call_key,"semantic_coverage_path":"uncovered",
                        "coverage_reference":[],"coverage_status":"invalid_callback_certificate"
                    }));
                    continue;
                }
                format!("dep1-callback-v1:{caller_instance}:bb{bb}:callback{ordinal}:{callee_identity}")
            }
            _ => format!("dep1-call-v1:{caller_instance}:bb{bb}:{callee_identity}"),
        };
        if caller_instance.is_empty() || bb == u64::MAX || call_key != expected_key {
            complete = false;
            ledger.push(serde_json::json!({
                "call_key":call_key,"semantic_coverage_path":"uncovered",
                "coverage_reference":[],"coverage_status":"invalid_call_key"
            }));
            continue;
        }
        let status = call.get("body_status").and_then(serde_json::Value::as_str).unwrap_or("unknown");
        let scope_reason = call.get("scope_reason").and_then(serde_json::Value::as_str).unwrap_or("unknown");
        if status == "out_of_scope" {
            ledger.push(serde_json::json!({
                "call_key":call_key,"semantic_coverage_path":"uncovered",
                "coverage_reference":[],"coverage_status":"out_of_scope",
                "scope_reason":scope_reason
            }));
            continue;
        }
        let mut paths: Vec<(&str, String)> = Vec::new();
        let caller_is_local = dep1.body_statuses.iter().any(|body|
            body.get("instance_id").and_then(serde_json::Value::as_str) == Some(caller_instance)
                && body.get("local").and_then(serde_json::Value::as_bool) == Some(true)
        );
        let expected_callee = call.get("resolved_instance_id").and_then(serde_json::Value::as_str)
            .or_else(|| call.get("resolved_definition_id").and_then(serde_json::Value::as_str))
            .or_else(|| call.get("operand_definition_id").and_then(serde_json::Value::as_str))
            .unwrap_or("unresolved");
        let matching_links = links.get(call_key).cloned().unwrap_or_default();
        let links_are_exact = !matching_links.is_empty() && matching_links.iter().all(|link|
            link.get("caller_definition_id").and_then(serde_json::Value::as_str)
                == call.get("caller_definition_id").and_then(serde_json::Value::as_str)
            && link.get("caller_instance_id").and_then(serde_json::Value::as_str)
                == call.get("caller_instance_id").and_then(serde_json::Value::as_str)
            && link.get("basic_block").and_then(serde_json::Value::as_u64) == Some(bb)
            && link.get("callee_identity").and_then(serde_json::Value::as_str) == Some(expected_callee)
        );
        if (caller_is_local && matching_links.len() != 1)
            || (!matching_links.is_empty() && !links_are_exact)
        { complete = false; }
        if status == "represented_body" {
            let resolved = call.get("resolved_instance_id").and_then(serde_json::Value::as_str);
            let body_matches = resolved.map(|instance| dep1.body_statuses.iter().filter(|body|
                body.get("instance_id").and_then(serde_json::Value::as_str) == Some(instance)
                    && body.get("body_status").and_then(serde_json::Value::as_str) == Some("represented_body")
            ).count()).unwrap_or_default();
            let body_is_present = body_matches == 1;
            if body_is_present {
                paths.push(("dep1_rust_mir", format!("dep1-exact-call-binding:{call_key};dep1-body-instance:{}", resolved.unwrap())));
            }
        } else if status == "dynamic_drop_summary" {
            let summary=call.get("dynamic_drop_summary_v1");
            let certified=call.get("activation_kind").and_then(serde_json::Value::as_str)==Some("mir_drop")
                && call.get("scope_reason").and_then(serde_json::Value::as_str)==Some("dynamic_trait_drop_summary")
                && summary.and_then(|v|v.get("semantics")).and_then(serde_json::Value::as_str)
                    == Some("opaque_dynamic_destructor_may_effect_v1")
                && summary.and_then(|v|v.get("normal_memory_effect")).and_then(serde_json::Value::as_str)
                    == Some("widen_all_tracked_to_top")
                && summary.and_then(|v|v.get("unwind_memory_effect")).and_then(serde_json::Value::as_str)
                    == Some("widen_all_tracked_to_top");
            if certified {
                paths.push(("dep1_dynamic_trait_drop_summary",
                    format!("dep1-dynamic-drop-summary:{call_key}")));
            } else {
                complete=false;
            }
        }
        // Inspect existing CREMA/CQPL relations for every linked call so an
        // already represented DEP1 body cannot silently acquire a second,
        // incompatible coverage path.
        if links_are_exact {
            for link in matching_links {
                let Some(node) = link.get("icfg_node").and_then(serde_json::Value::as_str) else { continue; };
                if !reachable_nodes.contains(node) { continue; }
                if let Some(reference) = crema_ffi_body_coverage_reference(icfg, node) {
                    paths.push(("crema_ffi_body", reference));
                }
                for binding in ele1_bindings.iter().filter(|binding| binding.node == node) {
                    let accepted_envelope = ele1_effects.iter().filter(|effects|
                        effects.binding_id == binding.binding_id
                            && !effects.effect_families.is_empty()
                            && effects.basis == "crema_external_library_effects_v1"
                    ).count();
                    if accepted_envelope == 1 {
                        paths.push(("cqpl_external_library_model", binding.binding_id.clone()));
                    } else {
                        complete = false;
                    }
                }
            }
        }
        let forced_incomplete = matches!(status,
            "unresolved_scope_identity" | "unresolved_instance" | "virtual_or_dynamic_unresolved"
        ) || (status == "body_unavailable"
            && call.get("body_unavailable_reason").and_then(serde_json::Value::as_str)
                == Some("rust_mir_unavailable"));
        if forced_incomplete {
            // A different semantic layer cannot erase failed DEP1 identity or
            // an expected Rust MIR body that the selected Rust path requires.
            paths.clear();
            complete = false;
        }
        paths.sort();
        let (path, references, coverage_status) = match paths.as_slice() {
            [(path, reference)] => (*path, vec![reference.clone()], "covered"),
            [] => { complete = false; ("uncovered", Vec::new(), "uncovered") },
            many => {
                complete = false;
                ("uncovered", many.iter().map(|(_, reference)| reference.clone()).collect(), "conflicting_coverage")
            }
        };
        ledger.push(serde_json::json!({
            "call_key":call_key,
            "semantic_coverage_path":path,
            "coverage_reference":references,
            "coverage_status":coverage_status,
            "body_status":status,
            "scope_reason":scope_reason
        }));
    }
    ledger.sort_by(|a,b| a.get("call_key").and_then(serde_json::Value::as_str)
        .cmp(&b.get("call_key").and_then(serde_json::Value::as_str)));
    Some((ledger, complete))
}

fn insert_event_source(
    events: &mut EventSourceMap,
    label: EventLabel,
    anchor: Option<SourceAnchor>,
) {
    let anchors = events.entry(label).or_default();
    if let Some(anchor) = anchor {
        anchors.insert(anchor);
    }
}

/// RN1 allocation events are identity-backed rather than name-backed.  A CCall
/// site tagged `realloc` can only be created by the identity producer after a
/// MUST-null source proof, so projecting that fresh site back to the MIR return
/// local is the proof-carrying alloc event.  Ordinary realloc calls never create
/// such an identity and therefore cannot acquire an alloc event here.
fn add_realloc_null_allocation_event_source(
    node_id: &str,
    node: &GlobalICFGNode,
    event_identity: &AllocationIdentityMemory,
    events: &mut EventSourceMap,
) {
    let GlobalICFGNode::Mir(bb) = node else { return; };
    let Some(term) = bb.terminator.as_ref() else { return; };
    let anchor = mir_terminator_source_anchor(term);
    let Some(scope) = mir_function_scope_from_node_id(node_id) else { return; };
    let function = scope.strip_prefix("rust::").unwrap_or(&scope);

    for (variable, allocations) in &event_identity.points_to {
        let ProgramVarId::Rust { function: var_function, local } = variable else { continue; };
        if var_function != function {
            continue;
        }
        let is_fresh_realloc_null_site = allocations.iter().any(|allocation| {
            matches!(
                &allocation.site,
                AllocationSiteId::CCall { node_id: site_node, allocator }
                    if site_node == node_id && allocator == "realloc"
            )
        });
        if is_fresh_realloc_null_site {
            insert_event_source(
                events,
                EventLabel {
                    predicate: "alloc",
                    variable: format!("Local(_{local})"),
                },
                anchor.clone(),
            );
        }
    }
}

/// Exact source occurrences for the raw syntactic events used by the CQPL
/// exporter.  `labels_for_node` is intentionally a projection of this map, so
/// diagnostic provenance cannot drift from the event vocabulary that feeds
/// allocation_labels.
fn event_sources_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    llvm_names: &LlvmNameResolver,
    ffi_functions: &HashSet<String>,
    represented_c_functions: &HashSet<String>,
) -> EventSourceMap {
    let mut events = EventSourceMap::new();
    match node {
        GlobalICFGNode::Mir(bb) => {
            for (statement_index, stmt) in bb.statements.iter().enumerate() {
                let anchor = mir_statement_source_anchor(statement_index, stmt);
                // A dereference/projection read is a syntactic memory read.
                if let Some(rvalue) = &stmt.rvalue {
                    for v in mir_deref_locals(rvalue) {
                        insert_event_source(
                            &mut events,
                            EventLabel { predicate: "read", variable: v },
                            anchor.clone(),
                        );
                    }
                }
                // A write through a dereferenced place is a syntactic memory write.
                if let Some(place) = &stmt.place {
                    for v in mir_deref_locals(place) {
                        insert_event_source(
                            &mut events,
                            EventLabel { predicate: "write", variable: v },
                            anchor.clone(),
                        );
                    }
                }
                // Some rustc textual dumps expose the place only in `details`.
                if let Some((lhs, rhs)) = stmt.details.split_once('=') {
                    for v in mir_deref_locals(lhs) {
                        insert_event_source(
                            &mut events,
                            EventLabel { predicate: "write", variable: v },
                            anchor.clone(),
                        );
                    }
                    for v in mir_deref_locals(rhs) {
                        insert_event_source(
                            &mut events,
                            EventLabel { predicate: "read", variable: v },
                            anchor.clone(),
                        );
                    }
                }
            }

            if let Some(term) = &bb.terminator {
                let anchor = mir_terminator_source_anchor(term);
                match term {
                    MirTerminator::Drop { dropped_value, .. } => {
                        if let Some(v) = canonical_mir_local(dropped_value) {
                            insert_event_source(
                                &mut events,
                                EventLabel { predicate: "drop", variable: v },
                                anchor,
                            );
                        }
                    }
                    MirTerminator::Call {
                        function_called,
                        arguments,
                        return_place,
                        details,
                        allocation_disposition_evidence,
                        ..
                    } => {
                        let call_text = if details.is_empty() { function_called } else { details };
                        let first_arg = arguments
                            .iter()
                            .find_map(|a| canonical_mir_local(&a.arg))
                            .or_else(|| first_call_local_from_details(call_text));
                        let ret = canonical_mir_local(return_place);

                        if is_fresh_allocation_call(function_called, call_text, ffi_functions) {
                            if let Some(v) = ret {
                                insert_event_source(
                                    &mut events,
                                    EventLabel { predicate: "alloc", variable: v },
                                    anchor.clone(),
                                );
                            }
                        }

                        let certified_raw_pointer_drop = allocation_disposition_evidence
                            .as_ref()
                            .is_some_and(|e| matches!(
                                e.kind,
                                RustAllocationDispositionEvidenceKind::MemDropRawPointer
                            ));
                        if is_deallocation_call(function_called, call_text, ffi_functions)
                            && !certified_raw_pointer_drop
                        {
                            if let Some(v) = first_arg.clone() {
                                insert_event_source(
                                    &mut events,
                                    EventLabel { predicate: "drop", variable: v },
                                    anchor.clone(),
                                );
                            }
                        }

                        // EFM2: declaration-only external semantic memory effects.
                        // The external API contract classifies formal positions; the
                        // actual memory object remains the Rust MIR operand resolved by
                        // CREMA's existing allocation-identity domain.
                        if let Some(contract) = external_function_memory_contract_v2(
                            function_called,
                            arguments.len(),
                            ffi_functions,
                            represented_c_functions,
                        ) {
                            for rule in contract.rules {
                                if !external_formal_memory_effect_rule_active_v2(&rule, arguments) { continue; }
                                if let Some(argument) = arguments.get(rule.formal_index) {
                                    if let Some(v) = canonical_mir_local(&argument.arg) {
                                        insert_event_source(
                                            &mut events,
                                            EventLabel {
                                                predicate: rule.access,
                                                variable: v,
                                            },
                                            anchor.clone(),
                                        );
                                    }
                                }
                            }
                        }

                        if is_cstring_from_raw_read_summary(function_called)
                            || is_cstring_from_raw_read_summary(call_text)
                        {
                            if let Some(v) = first_arg.clone() {
                                insert_event_source(
                                    &mut events,
                                    EventLabel { predicate: "read", variable: v },
                                    anchor.clone(),
                                );
                            }
                        }

                        if is_ptr_read_call(function_called) || is_ptr_read_call(call_text) {
                            if let Some(v) = first_arg.clone() {
                                insert_event_source(
                                    &mut events,
                                    EventLabel { predicate: "read", variable: v },
                                    anchor.clone(),
                                );
                            }
                        }
                        if is_ptr_write_call(function_called)
                            || is_ptr_write_call(call_text)
                            || is_ptr_drop_in_place_call(function_called)
                            || is_ptr_drop_in_place_call(call_text)
                        {
                            if let Some(v) = first_arg {
                                insert_event_source(
                                    &mut events,
                                    EventLabel { predicate: "write", variable: v },
                                    anchor,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        GlobalICFGNode::Llvm(llvm) => {
            let anchor = llvm_node_source_anchor(llvm);
            if llvm.node_kind_string == "FunCallBlock" && is_c_malloc_family_alloc_call(&llvm.info) {
                for stmt in &llvm.svf_statements {
                    if stmt.stmt_type == "AddrStmt" {
                        if let Some(id) = stmt.lhs_var_id {
                            insert_event_source(
                                &mut events,
                                EventLabel {
                                    predicate: "alloc",
                                    variable: llvm_names.resolve_svf(&scoped_svf_var(id, node_id)),
                                },
                                anchor.clone(),
                            );
                        }
                    }
                }
            }

            if llvm.node_kind_string == "FunCallBlock" && llvm.info.contains("@free(") {
                if let Some(ir) = llvm_free_ir_argument_id(&llvm.info) {
                    let ir = scoped_ir_var(ir, node_id);
                    insert_event_source(
                        &mut events,
                        EventLabel {
                            predicate: "drop",
                            variable: llvm_names.resolve_ir(&ir).unwrap_or(ir),
                        },
                        anchor.clone(),
                    );
                }
            }

            for stmt in &llvm.svf_statements {
                match stmt.stmt_type.as_str() {
                    "LoadStmt" => {
                        if let Some(rhs) = stmt.rhs_var_id {
                            let address = scoped_svf_var(rhs, node_id);
                            // SVF models a source-level formal spill/reload as
                            // Load/Store over an LLVM `alloca` stack slot.  The
                            // identity analysis deliberately lets the pointed-to
                            // allocation value flow through that carrier, but
                            // the carrier access is not a read of the pointee.
                            // Suppress only addresses structurally certified as
                            // local stack slots; real pointee loads remain events.
                            if llvm_names.is_stack_slot_address(&address) {
                                continue;
                            }
                            insert_event_source(
                                &mut events,
                                EventLabel {
                                    predicate: "read",
                                    variable: llvm_names.resolve_svf(&address),
                                },
                                anchor.clone(),
                            );
                        }
                    }
                    "StoreStmt" => {
                        if let Some(lhs) = stmt.lhs_var_id {
                            let address = scoped_svf_var(lhs, node_id);
                            // As above, storing the pointer value into its local
                            // `alloca` carrier is not a write through that pointer.
                            if llvm_names.is_stack_slot_address(&address) {
                                continue;
                            }
                            insert_event_source(
                                &mut events,
                                EventLabel {
                                    predicate: "write",
                                    variable: llvm_names.resolve_svf(&address),
                                },
                                anchor.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) | GlobalICFGNode::Terminal(_) => {}
    }
    events
}

#[cfg(test)]
fn labels_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    llvm_names: &LlvmNameResolver,
    ffi_functions: &HashSet<String>,
) -> Vec<EventLabel> {
    event_sources_for_node(
        node_id,
        node,
        llvm_names,
        ffi_functions,
        &HashSet::new(),
    )
    .into_keys()
    .collect()
}

fn allocation_event_source_records_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    event_sources: &EventSourceMap,
    identity: &AllocationIdentityMemory,
    allocation_labels: &[AllocationEventLabel],
) -> Vec<AllocationEventSourceRecord> {
    // W1/HT2-R2.3: source provenance is an additive proof overlay for
    // allocation-level events that have already passed the semantic admission
    // gate.  It must never independently reinterpret a raw MIR event as an
    // allocation event.  In particular, a delegating/uncertified MIR `Drop`
    // remains a source anchor at node level but is not an allocation Drop(R).
    //
    // Keeping the admitted (predicate, allocation) set explicit makes the
    // producer satisfy the checker invariant by construction rather than by a
    // second, potentially divergent classification.
    let admitted: BTreeSet<(&'static str, String)> = allocation_labels
        .iter()
        .map(|label| (label.predicate, label.allocation.clone()))
        .collect();

    let mut grouped: BTreeMap<(&'static str, String), BTreeSet<SourceAnchor>> = BTreeMap::new();
    for (label, anchors) in event_sources {
        let Some(variable) = identity_var_for_event(node_id, node, &label.variable) else {
            continue;
        };
        // Resolve exactly the same abstract allocation relation used by
        // allocation_labels_for_node.  Drop uses the narrower deallocation
        // relation; read/write/use/alloc use ordinary event identity.
        let allocations = if label.predicate == "drop" {
            identity.deallocation_allocations(&variable)
        } else {
            identity.event_allocations(&variable)
        };
        for allocation in allocations {
            let stable = stable_allocation_id(&allocation);
            if !admitted.contains(&(label.predicate, stable.clone())) {
                continue;
            }
            grouped
                .entry((label.predicate, stable))
                .or_default()
                .extend(anchors.iter().cloned());
        }
    }
    grouped
        .into_iter()
        .map(|((predicate, allocation), anchors)| AllocationEventSourceRecord {
            predicate,
            allocation,
            certainty: "may_abstract",
            anchors: anchors.into_iter().collect(),
        })
        .collect()
}

fn node_source_provenance(
    node_id: &str,
    node: &GlobalICFGNode,
    event_sources: &EventSourceMap,
    identity: &AllocationIdentityMemory,
    allocation_labels: &[AllocationEventLabel],
) -> NodeSourceProvenance {
    let mut anchors = BTreeSet::new();
    let language = match node {
        GlobalICFGNode::Mir(bb) => {
            for (index, stmt) in bb.statements.iter().enumerate() {
                if let Some(anchor) = mir_statement_source_anchor(index, stmt) {
                    anchors.insert(anchor);
                }
            }
            if let Some(term) = &bb.terminator {
                if let Some(anchor) = mir_terminator_source_anchor(term) {
                    anchors.insert(anchor);
                }
            }
            "rust"
        }
        GlobalICFGNode::Llvm(llvm) => {
            if let Some(anchor) = llvm_node_source_anchor(llvm) {
                anchors.insert(anchor);
            }
            "c"
        }
        GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) | GlobalICFGNode::Terminal(_) => {
            "synthetic"
        }
    };

    NodeSourceProvenance {
        language,
        anchors: anchors.into_iter().collect(),
        allocation_events: allocation_event_source_records_for_node(
            node_id,
            node,
            event_sources,
            identity,
            allocation_labels,
        ),
    }
}

fn is_fresh_allocation_call(
    function_called: &str,
    call_text: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    memory_events::is_modeled_fresh_allocation(function_called)
        || memory_events::is_modeled_fresh_allocation(call_text)
        || is_c_malloc_call(function_called, ffi_functions)
        || is_c_malloc_call(call_text, ffi_functions)
}

fn is_deallocation_call(
    function_called: &str,
    call_text: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    is_raw_dealloc_call(function_called)
        || is_raw_dealloc_call(call_text)
        || is_c_free_function(function_called, ffi_functions)
        || is_c_free_call_text(call_text, ffi_functions)
        || (is_explicit_mem_drop(function_called) || is_explicit_mem_drop(call_text))
            && !is_raw_pointer_mem_drop(function_called)
            && !is_raw_pointer_mem_drop(call_text)
}

fn is_raw_dealloc_call(s: &str) -> bool {
    s.contains("std::alloc::dealloc") || s.contains("alloc::alloc::dealloc")
}

/// Pinned standard-library summary for nightly-2024-11-21.
///
/// `CString::from_raw(ptr)` executes `strlen(ptr)` before rebuilding the
/// CString allocation, therefore the call performs a concrete read through
/// `ptr`.  The exporter records only that read event here.  Ownership
/// restoration continues to be modeled by CREMA's abstract state and the
/// eventual MIR `Drop` remains the syntactic deallocation event.
fn is_cstring_from_raw_read_summary(s: &str) -> bool {
    s.contains("std::ffi::CString::from_raw")
        || s.contains("alloc::ffi::c_str::<impl std::ffi::CString>::from_raw")
}

fn is_ptr_read_call(s: &str) -> bool {
    s.contains("std::ptr::read::<")
        || s.contains("core::ptr::read::<")
        || s.contains("std::ptr::read_unaligned::<")
        || s.contains("core::ptr::read_unaligned::<")
        || s.contains("std::ptr::read_volatile::<")
        || s.contains("core::ptr::read_volatile::<")
}

fn is_ptr_write_call(s: &str) -> bool {
    s.contains("std::ptr::write::<")
        || s.contains("core::ptr::write::<")
        || s.contains("std::ptr::write_unaligned::<")
        || s.contains("core::ptr::write_unaligned::<")
        || s.contains("std::ptr::write_volatile::<")
        || s.contains("core::ptr::write_volatile::<")
}

fn is_ptr_drop_in_place_call(s: &str) -> bool {
    s.contains("std::ptr::drop_in_place::<") || s.contains("core::ptr::drop_in_place::<")
}

fn is_explicit_mem_drop(s: &str) -> bool {
    s.contains("std::mem::drop::<") || s.contains("core::mem::drop::<")
}

fn is_raw_pointer_mem_drop(s: &str) -> bool {
    is_explicit_mem_drop(s) && (s.contains("*mut ") || s.contains("*const "))
}

fn is_c_malloc_call(s: &str, ffi_functions: &HashSet<String>) -> bool {
    let t = s.trim();
    if matches!(t, "malloc" | "calloc" | "strdup") && ffi_functions.contains(t) {
        return true;
    }
    (t.contains("libc::")
        && (t.ends_with("::malloc") || t.ends_with("::calloc") || t.ends_with("::strdup")))
        || (ffi_functions.contains("malloc") && (t.starts_with("malloc(") || t.contains(" malloc(")))
        || (ffi_functions.contains("calloc") && (t.starts_with("calloc(") || t.contains(" calloc(")))
        || (ffi_functions.contains("strdup") && (t.starts_with("strdup(") || t.contains(" strdup(")))
}

fn is_c_free_function(s: &str, ffi_functions: &HashSet<String>) -> bool {
    let t = s.trim();
    (t == "free" && ffi_functions.contains("free"))
        || (t.contains("libc::") && t.ends_with("::free"))
}

fn is_c_free_call_text(s: &str, ffi_functions: &HashSet<String>) -> bool {
    (s.contains("libc::") && s.contains("::free("))
        || (ffi_functions.contains("free")
            && (s.trim_start().starts_with("free(") || s.contains(" free(")))
}

fn is_c_malloc_family_alloc_call(info: &str) -> bool {
    info.contains("@malloc(") || info.contains("@calloc(")
}

fn first_call_local_from_details(details: &str) -> Option<Name> {
    details
        .split("->")
        .next()
        .and_then(canonical_mir_local)
}

fn llvm_call_suffix_from_global_node_id(node_id: &str) -> Option<&str> {
    node_id.rfind("::rust::").map(|pos| &node_id[pos + 2..])
}

fn scoped_svf_var(var_id: usize, node_id: &str) -> Name {
    match llvm_call_suffix_from_global_node_id(node_id) {
        Some(suffix) => format!("{var_id}@{suffix}"),
        None => var_id.to_string(),
    }
}

fn scoped_ir_var(ir_id: usize, node_id: &str) -> Name {
    match llvm_call_suffix_from_global_node_id(node_id) {
        Some(suffix) => format!("%{ir_id}@{suffix}"),
        None => format!("%{ir_id}"),
    }
}

fn llvm_free_ir_argument_id(info: &str) -> Option<usize> {
    LLVM_FREE_ARG_RE
        .captures(info)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn llvm_flow_sources(stmt: &SvfStatement) -> Vec<usize> {
    match stmt.stmt_type.as_str() {
        "AssignStmt" | "CopyStmt" | "LoadStmt" | "StoreStmt" | "GepStmt" | "PhiStmt" | "SelectStmt" => {
            let mut out = Vec::new();
            if let Some(rhs) = stmt.rhs_var_id {
                out.push(rhs);
            }
            for operand in stmt.normalized_operand_var_ids() {
                if !out.contains(&operand) {
                    out.push(operand);
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

#[derive(Debug, Default)]
struct LlvmNameResolver {
    /// Directed provenance edge: lhs SVF id -> one source SVF id.
    svf_source: BTreeMap<Name, Name>,
    /// LLVM IR %N -> corresponding SVF result id.
    ir_to_svf: BTreeMap<Name, Name>,
    /// Callsite-scoped SVF addresses that are structurally backed by a local
    /// LLVM `alloca`.  These variables are pointer-value carrier cells, not
    /// heap pointees.  Keeping this classification separate from `svf_source`
    /// prevents spill/reload operations from becoming synthetic heap accesses.
    stack_slot_addresses: BTreeSet<Name>,
}

impl LlvmNameResolver {
    fn build(icfg: &GlobalICFGOrdered) -> Self {
        let mut out = Self::default();
        for (node_id, node) in &icfg.ordered_nodes {
            let GlobalICFGNode::Llvm(llvm) = node else { continue; };

            // The pinned SVF exporter represents a local LLVM stack slot with
            // an AddrStmt on the corresponding `alloca` instruction.  Record
            // that address before following any StoreStmt provenance edge: a
            // later store of a heap pointer into the slot must not reclassify
            // the slot address itself as the heap object being accessed.
            if llvm_info_is_stack_alloca(&llvm.info) {
                for stmt in &llvm.svf_statements {
                    if stmt.stmt_type == "AddrStmt" {
                        if let Some(lhs_id) = stmt.lhs_var_id {
                            out.stack_slot_addresses
                                .insert(scoped_svf_var(lhs_id, node_id));
                        }
                    }
                }
            }

            for stmt in &llvm.svf_statements {
                let Some(lhs_id) = stmt.result_var_id() else { continue; };
                let lhs = scoped_svf_var(lhs_id, node_id);
                if let Some(src_id) = llvm_flow_sources(stmt).into_iter().next() {
                    let src = scoped_svf_var(src_id, node_id);
                    if src != lhs {
                        out.svf_source.entry(lhs.clone()).or_insert(src);
                    }
                }

                if let Some(caps) = LLVM_IR_LHS_RE.captures(&stmt.stmt_info) {
                    if let Some(m) = caps.get(1) {
                        if let Ok(ir_id) = m.as_str().parse::<usize>() {
                            out.ir_to_svf
                                .insert(scoped_ir_var(ir_id, node_id), lhs);
                        }
                    }
                }
            }
        }
        out
    }

    fn resolve_svf(&self, name: &str) -> Name {
        let mut current = name.to_string();
        let mut seen = BTreeSet::new();
        while seen.insert(current.clone()) {
            let Some(next) = self.svf_source.get(&current) else { break; };
            current = next.clone();
        }
        current
    }

    fn resolve_ir(&self, ir: &str) -> Option<Name> {
        self.ir_to_svf.get(ir).map(|svf| self.resolve_svf(svf))
    }

    fn is_stack_slot_address(&self, name: &str) -> bool {
        self.stack_slot_addresses.contains(name)
    }
}

fn llvm_info_is_stack_alloca(info: &str) -> bool {
    let instruction = info
        .split_once('=')
        .map(|(_, rhs)| rhs.trim_start())
        .unwrap_or_else(|| info.trim_start());
    instruction == "alloca" || instruction.starts_with("alloca ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstract_domain::{Allocation, CellValue};
    use crate::structs::{
        DependencyBodyIngestionExport, DummyNode, IcfgEdge, MirBasicBlock, MirCallArgument,
        SourceInfoData, MirStatement,
        RustAllocationDispositionEvidence, RustAllocationDispositionEvidenceKind,
        RustCallDeallocatorEvidence, RustCallMetadata, RustDropAllocatorEvidence,
        RustFunctionMetadata, TerminalNode,
    };

    fn edge(a: &str, b: &str) -> IcfgEdge {
        IcfgEdge {
            source: a.into(),
            destination: b.into(),
            label: None,
            source_label: None,
            destination_label: None,
        }
    }

    fn return_projection_fixture() -> (ProgramVarId, AbstractAllocId, AllocationIdentityMemory) {
        (
            ProgramVarId::rust("producer", "Local(_0)").unwrap(),
            AbstractAllocId::new(
                AllocationSiteId::Synthetic {
                    scope: "return_projection_test".into(),
                    label: "A".into(),
                },
                vec![],
            ),
            AllocationIdentityMemory::default(),
        )
    }

    fn return_projection_place(base: &ProgramVarId, indices: &[u32]) -> PlaceId {
        PlaceId {
            base: base.clone(),
            projection: indices
                .iter()
                .map(|index| PlaceProjection::Field { index: *index })
                .collect(),
        }
    }

    fn return_projection_node() -> GlobalICFGNode {
        GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Return {
                details: "return".into(),
                source_info: "".into(),
            }),
        })
    }

    #[test]
    fn rust_unwind_edge_scope_native_context_ownership() {
        let (mut g,t,_)=rust_unwind_edge_scope_fixture();
        g.dependency_body_ingestion_v1=Some(serde_json::from_value(serde_json::json!({
            "capability":"dependency_body_ingestion_v1","target_triple":"","crate_correlations":[],
            "package_scope":[],"body_statuses":[],"exact_call_bindings":[],"icfg_callsite_links":[],
            "context_execution_graph_v1":{"p2_control_flow_complete":true,
                "states":[{"execution_node":"n0","concrete_instance_id":"instance-0","context":["context-0"]},
                          {"execution_node":"n1","concrete_instance_id":"instance-0","context":["context-0"]}],
                "represented_call_activations":[]}})).unwrap());
        let r=rust_unwind_edge_scope_records(&g,&t,&[],None).unwrap();
        assert_eq!(r[0].scope,RustUnwindEdgeScopeKindV1::IntraActivation);
        assert_eq!(r[0].basis,RustUnwindEdgeScopeBasisV1::ExecutionContextMembershipV1);
        g.dependency_body_ingestion_v1.as_mut().unwrap().context_execution_graph_v1.as_mut().unwrap()
            ["represented_call_activations"]=serde_json::json!([{"unwind_return_relations":[{
                "callee_unwind_state":"n0","caller_unwind_state":"n1"}]}]);
        assert_eq!(rust_unwind_edge_scope_records(&g,&t,&[],None).unwrap()[0].scope,RustUnwindEdgeScopeKindV1::Unresolved);
    }

    fn rust_unwind_edge_scope_fixture() -> (GlobalICFGOrdered,Vec<TypedEdgeRecord>,RustUnwindScopeEvidenceV1) {
        let g=GlobalICFGOrdered {ordered_nodes:vec![("n0".into(),return_projection_node()),("n1".into(),return_projection_node()),("n2".into(),return_projection_node())],
            icfg_edges:vec![],llvm_memory_effects:None,svf_solved_points_to:None,rust_functions:BTreeMap::new(),rust_calls:vec![],dependency_body_ingestion_v1:None};
        let typed=vec![TypedEdgeRecord {source:"n0".into(),destination:"n1".into(),flow:"unwind",label:None,source_label:None,destination_label:None}];
        let e=RustUnwindScopeEvidenceV1 {schema_version:1,node_owners:BTreeMap::from([("n0".into(),"owner-0".into()),("n1".into(),"owner-0".into()),("n2".into(),"owner-0".into())]),boundary_edges:vec![]};
        (g,typed,e)
    }
    fn rust_unwind_edge_scope_claims(r:&[RustCallUnwindRelationV1]) -> BTreeMap<(String,String),BTreeSet<RustActivationIdentity>> {
        let mut out:BTreeMap<_,BTreeSet<_>>=BTreeMap::new();
        for v in r {for e in &v.unwind_edges {out.entry((e.source_node.clone(),e.destination_node.clone())).or_default().insert(RustActivationIdentity {
            caller_function:v.caller_function.clone(),call_node:v.call_node.clone(),dummy_call_node:v.dummy_call_node.clone(),callee_function:v.callee_function.clone()});}}
        out
    }
    #[test]
    fn rust_unwind_edge_scope_intra_activation() {
        let (g,t,e)=rust_unwind_edge_scope_fixture();let r=rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap();
        assert_eq!(r[0].scope,RustUnwindEdgeScopeKindV1::IntraActivation);assert!(r[0].activation.is_none());
    }
    #[test]
    fn rust_unwind_edge_scope_rust_boundary() {
        let (g,m)=rust_call_unwind_relation_fixture(1);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();
        let r=rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();
        assert_eq!(r[0].scope,RustUnwindEdgeScopeKindV1::RustActivationBoundary);assert_eq!(r[0].activation.as_ref().unwrap()[0].call_node,"call0");
    }
    #[test]
    fn rust_unwind_edge_scope_shared_callee() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();
        let r=rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();
        assert_eq!(r.len(),2);for (i,r) in r.iter().enumerate(){assert_eq!(r.activation.as_ref().unwrap()[0].call_node,format!("call{i}"));}
    }
    #[test]
    fn rust_unwind_edge_scope_foreign_fanout_never_defaults_intra() {
        let (g,t,mut e)=rust_unwind_edge_scope_fixture();e.boundary_edges=vec![RustActivationUnwindEdge {source_node:"n0".into(),destination_node:"n1".into()}];
        let r=rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap();assert_eq!(r[0].scope,RustUnwindEdgeScopeKindV1::Unresolved);
    }
    #[test]
    fn rust_unwind_edge_scope_internal_cleanup_chain() {
        let (g,mut t,e)=rust_unwind_edge_scope_fixture();let mut second=t[0].clone();second.source="n1".into();second.destination="n2".into();t.push(second);
        assert!(rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap().iter().all(|r|r.scope==RustUnwindEdgeScopeKindV1::IntraActivation));
    }
    #[test]
    fn rust_unwind_edge_scope_terminal_no_invented_activation() {
        for action in ["terminate","unreachable"] {
            let (g,mut m)=rust_call_unwind_relation_fixture(1);m.activations[0]=crate::structs::RustActivationUnwindMetadata::direct(&g.rust_calls[0],action,false);
            let u=rust_call_unwind_relation_payload(&g,&m).unwrap();let r=rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();
            assert!(r.iter().all(|r|r.scope==RustUnwindEdgeScopeKindV1::Unresolved && r.activation.is_none()));
        }
    }
    #[test]
    fn rust_unwind_edge_scope_detached_no_provider_boundary() {
        let (g,m)=rust_call_unwind_relation_callback(false,true);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert!(rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap().iter().all(|r|r.scope==RustUnwindEdgeScopeKindV1::Unresolved));
    }
    #[test]
    fn rust_unwind_edge_scope_synchronous_unresolved() {
        let (g,m)=rust_call_unwind_relation_callback(false,false);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert!(rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap().iter().all(|r|r.scope==RustUnwindEdgeScopeKindV1::Unresolved));
    }
    #[test]
    fn rust_unwind_edge_scope_non_rust() {
        let (mut g,t,_)=rust_unwind_edge_scope_fixture();
        for (_,n) in &mut g.ordered_nodes { *n=serde_json::from_value(serde_json::json!({"node_type":"Llvm","node_data":{
            "node_id":0,"node_type":true,"info":"","node_kind_string":"","node_kind":0,"node_source_loc":"",
            "svf_statements":[],"incoming_edges":[],"outgoing_edges":[]}})).unwrap(); }
        assert_eq!(rust_unwind_edge_scope_records(&g,&t,&[],None).unwrap()[0].scope,RustUnwindEdgeScopeKindV1::NonRust);
    }
    #[test]
    fn rust_unwind_edge_scope_complete_unique_pairs() {
        let (g,mut t,e)=rust_unwind_edge_scope_fixture();t.push(t[0].clone());
        assert_eq!(rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap().len(),1);
    }
    #[test]
    fn rust_unwind_edge_scope_duplicate_rejected() {
        let (g,t,e)=rust_unwind_edge_scope_fixture();let mut r=rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap();r.push(r[0].clone());
        let pairs=BTreeSet::from([("n0".into(),"n1".into())]);assert!(validate_rust_unwind_edge_scopes(&r,&pairs,&BTreeMap::new()).is_err());
    }
    #[test]
    fn rust_unwind_edge_scope_f6b1_consistency() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();let t=typed_edge_records(&g);
        let r=rust_unwind_edge_scope_records(&g,&t,&u,None).unwrap();let pairs=t.iter().filter(|t|t.flow=="unwind").map(|t|(t.source.clone(),t.destination.clone())).collect();
        validate_rust_unwind_edge_scopes(&r,&pairs,&rust_unwind_edge_scope_claims(&u)).unwrap();
    }
    #[test]
    fn rust_unwind_edge_scope_wrong_activation_rejected() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();let t=typed_edge_records(&g);
        let mut r=rust_unwind_edge_scope_records(&g,&t,&u,None).unwrap();r[0].activation=r[1].activation.clone();
        let pairs=t.iter().filter(|t|t.flow=="unwind").map(|t|(t.source.clone(),t.destination.clone())).collect();
        assert!(validate_rust_unwind_edge_scopes(&r,&pairs,&rust_unwind_edge_scope_claims(&u)).is_err());
    }
    #[test]
    fn rust_unwind_edge_scope_deterministic_ordering() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let mut u=rust_call_unwind_relation_payload(&g,&m).unwrap();let mut t=typed_edge_records(&g);
        let a=rust_unwind_edge_scope_records(&g,&t,&u,None).unwrap();u.reverse();t.reverse();let b=rust_unwind_edge_scope_records(&g,&t,&u,None).unwrap();
        assert_eq!(serde_json::to_value(a).unwrap(),serde_json::to_value(b).unwrap());
    }
    #[test]
    fn rust_unwind_edge_scope_schema_v1() {
        let r=rust_call_return_relation_export(1,0);assert!(r.get("rust_unwind_edge_scopes").is_none());assert!(r.get("capabilities").is_none());
    }
    #[test]
    fn rust_unwind_edge_scope_schema_v2_empty() {
        let r=rust_call_return_relation_export(2,0);assert_eq!(r["rust_unwind_edge_scopes"],serde_json::json!([]));
        assert!(r["capabilities"].as_array().unwrap().iter().any(|c|c=="rust_unwind_edge_scope_v1"));
    }
    #[test]
    fn rust_unwind_edge_scope_f6b1_non_interference() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let u=rust_call_unwind_relation_payload(&g,&m).unwrap();let before=serde_json::to_value(&u).unwrap();
        rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();assert_eq!(before,serde_json::to_value(u).unwrap());
    }
    #[test]
    fn rust_unwind_edge_scope_f2_non_interference() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let before=rust_call_return_relation_payload(&g).unwrap();let u=rust_call_unwind_relation_payload(&g,&m).unwrap();
        rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();assert_eq!(before,rust_call_return_relation_payload(&g).unwrap());
    }
    #[test]
    fn rust_unwind_edge_scope_allocation_non_interference() {
        let (g,t,e)=rust_unwind_edge_scope_fixture();let before=serde_json::to_value(&g).unwrap();rust_unwind_edge_scope_records(&g,&t,&[],Some(&e)).unwrap();
        assert_eq!(before,serde_json::to_value(g).unwrap());return_projection_escape_artifact_disposition();
    }
    #[test]
    fn rust_unwind_edge_scope_shared_cleanup_activation_set() {
        let (mut g,mut m)=rust_call_unwind_relation_fixture(2);
        g.icfg_edges.last_mut().unwrap().destination="rust::caller::bb7".into();
        m.activations[1]=crate::structs::RustActivationUnwindMetadata::direct(&g.rust_calls[1],"cleanup(bb7)",false);
        let u=rust_call_unwind_relation_payload(&g,&m).unwrap();let r=rust_unwind_edge_scope_records(&g,&typed_edge_records(&g),&u,None).unwrap();
        assert_eq!(r.len(),1);assert_eq!(r[0].activation.as_ref().unwrap().len(),2);
    }

    fn rust_call_unwind_relation_fixture(count: usize) -> (GlobalICFGOrdered, RustActivationUnwindMetadataV1) {
        let mut graph = rust_call_return_relation_fixture(count, &["ret"]);
        graph.ordered_nodes.push(("rust::callee::bb2".into(), GlobalICFGNode::Mir(MirBasicBlock {
            block_id:2,statements:vec![],terminator:Some(MirTerminator::UnwindResume {details:String::new(),source_info:String::new()}) })));
        let mut records=Vec::new();
        for i in 0..count {
            let target=format!("bb{}",7+i);let destination=format!("rust::caller::{target}");
            graph.ordered_nodes.push((destination.clone(), GlobalICFGNode::Mir(MirBasicBlock {
                block_id:7+i,statements:vec![],terminator:None })));
            let mut unwind=edge("rust::callee::bb2",&destination);unwind.label=Some("Rust unwind propagate".into());graph.icfg_edges.push(unwind);
            let caller_node = graph.ordered_nodes.iter_mut().find(|(id,_)| id == &graph.rust_calls[i].call_node).unwrap();
            caller_node.1 = serde_json::from_value(serde_json::json!({"node_type":"Mir","node_data":{
                "block_id":i,"statements":[],"terminator":{"kind":"Call","details":"","source_info":"",
                "function_called":"opaque","arguments":[],"return_place":"_0","return_target":"bb1",
                "unwind_target":format!("cleanup({target})")}}})).unwrap();
            records.push(crate::structs::RustActivationUnwindMetadata::direct(&graph.rust_calls[i],&format!("cleanup({target})"),false));
        }
        (graph,RustActivationUnwindMetadataV1::new(records).unwrap())
    }
    fn rust_call_unwind_relation_payload(g:&GlobalICFGOrdered,m:&RustActivationUnwindMetadataV1) -> Result<Vec<RustCallUnwindRelationV1>,Box<dyn Error>> {
        let returns=rust_call_return_relation_payload(g)?;let ids=g.ordered_nodes.iter().map(|(id,_)|id.clone()).collect();
        rust_call_unwind_relation_records(g,m,&returns,&typed_edge_records(g),&ids)
    }
    fn rust_call_unwind_relation_semantics(action: &str) -> RustCallUnwindRelationV1 {
        let (g,mut m)=rust_call_unwind_relation_fixture(1);
        m.activations[0]=crate::structs::RustActivationUnwindMetadata::direct(&g.rust_calls[0],action,false);
        rust_call_unwind_relation_payload(&g,&m).unwrap().remove(0)
    }
    #[test]
    fn rust_call_unwind_relation_direct_cleanup() {
        let (g,m)=rust_call_unwind_relation_fixture(1);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert_eq!(r.len(),1);assert_eq!(r[0].unwind_semantics,RustCallUnwindSemanticsV1::Cleanup);
        assert_eq!(r[0].unwind_edges,vec![RustActivationUnwindEdge {source_node:"rust::callee::bb2".into(),destination_node:"rust::caller::bb7".into()}]);
    }
    #[test]
    fn rust_call_unwind_relation_shared_callee_cleanup_distinct() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert_eq!(r.len(),2);assert_eq!(r[0].unwind_edges[0].destination_node,"rust::caller::bb7");
        assert_eq!(r[1].unwind_edges[0].destination_node,"rust::caller::bb8");
    }
    #[test]
    fn rust_call_unwind_relation_direct_continue() {
        let r=rust_call_unwind_relation_semantics("continue");assert_eq!(r.unwind_semantics,RustCallUnwindSemanticsV1::Continue);assert!(r.unwind_edges.is_empty());
    }
    #[test]
    fn rust_call_unwind_relation_terminate() {
        let r=rust_call_unwind_relation_semantics("terminate");assert_eq!(r.unwind_semantics,RustCallUnwindSemanticsV1::Terminate);assert!(r.unwind_edges.is_empty());
    }
    #[test]
    fn rust_call_unwind_relation_unreachable() {
        let r=rust_call_unwind_relation_semantics("unreachable");assert_eq!(r.unwind_semantics,RustCallUnwindSemanticsV1::Unreachable);assert!(r.unwind_edges.is_empty());
    }
    fn rust_call_unwind_relation_callback(certified:bool,detached:bool) -> (GlobalICFGOrdered,RustActivationUnwindMetadataV1) {
        let (mut g,mut m)=rust_call_unwind_relation_fixture(1);
        if let GlobalICFGNode::Mir(block) = &mut g.ordered_nodes.iter_mut().find(|(id,_)| id == &g.rust_calls[0].call_node).unwrap().1 {
            if let Some(MirTerminator::Call { unwind_target, .. }) = &mut block.terminator { *unwind_target = "continue".into(); }
        }
        let policy=if detached {crate::structs::HigherOrderCallSemantics::SpawnMayOnce} else {crate::structs::HigherOrderCallSemantics::RepeatedCallbacks};
        m.activations[0]=crate::structs::RustActivationUnwindMetadata::callback(&g.rust_calls[0],Some(policy));
        if certified {
            g.dependency_body_ingestion_v1=Some(serde_json::from_value(serde_json::json!({
                "capability":"dependency_body_ingestion_v1","target_triple":"","crate_correlations":[],
                "package_scope":[],"body_statuses":[],"exact_call_bindings":[],"icfg_callsite_links":[],
                "context_execution_graph_v1":{"represented_call_activations":[{
                    "call_key":"certified-key","activation_kind":"higher_order_callback","callback_semantics":"repeated_callbacks",
                    "caller_call_state":"call0","dummy_call_state":"activation0","callee_entry_state":"entry",
                    "unwind_return_relations":[{"callee_unwind_state":"rust::callee::bb2", "caller_unwind_state":"rust::caller::bb7", "unwind_action":"cleanup(bb7)"}]
                }]}})).unwrap());
            m=crate::execution_graph::rust_activation_unwind_metadata_from_execution_graph(&g).unwrap().unwrap();
        }
        (g,m)
    }
    #[test]
    fn rust_call_unwind_relation_synchronous_callback_concrete() {
        let (g,m)=rust_call_unwind_relation_callback(true,false);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert_eq!(r[0].activation_kind,RustActivationKind::SynchronousCallback);
        assert_eq!(r[0].unwind_semantics,RustCallUnwindSemanticsV1::CertifiedCallback);assert_eq!(r[0].unwind_edges.len(),1);
    }
    #[test]
    fn rust_call_unwind_relation_synchronous_callback_unresolved() {
        let (g,m)=rust_call_unwind_relation_callback(false,false);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert_eq!(r[0].activation_kind,RustActivationKind::SynchronousCallback);assert_eq!(r[0].unwind_semantics,RustCallUnwindSemanticsV1::Unresolved);
        assert_eq!(r[0].unresolved_reason,Some(RustCallUnwindReasonV1::MissingCallbackUnwindEvidence));assert!(r[0].unwind_edges.is_empty());
    }
    #[test]
    fn rust_call_unwind_relation_detached_callback() {
        let (g,m)=rust_call_unwind_relation_callback(false,true);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert_eq!(r[0].activation_kind,RustActivationKind::DetachedCallback);assert_eq!(r[0].unwind_semantics,RustCallUnwindSemanticsV1::DetachedNoCallerUnwind);assert!(r[0].unwind_edges.is_empty());
    }
    #[test]
    fn rust_call_unwind_relation_provider_collision() {
        let (a,ma)=rust_call_unwind_relation_callback(false,false);let (b,mb)=rust_call_unwind_relation_callback(false,true);
        assert_eq!(serde_json::to_value(&a).unwrap(),serde_json::to_value(&b).unwrap());
        assert_ne!(serde_json::to_value(rust_call_unwind_relation_payload(&a,&ma).unwrap()).unwrap(),
            serde_json::to_value(rust_call_unwind_relation_payload(&b,&mb).unwrap()).unwrap());
    }
    #[test]
    fn rust_call_unwind_relation_shared_callback_separate() {
        let (g,_)=rust_call_unwind_relation_fixture(2);
        let m=RustActivationUnwindMetadataV1::new(g.rust_calls.iter().map(|c|
            crate::structs::RustActivationUnwindMetadata::callback(c,Some(crate::structs::HigherOrderCallSemantics::RepeatedCallbacks)))).unwrap();
        let r=rust_call_unwind_relation_payload(&g,&m).unwrap();assert_eq!(r.len(),2);assert_ne!(r[0].dummy_call_node,r[1].dummy_call_node);
    }
    #[test]
    fn rust_call_unwind_relation_missing_metadata() {
        let (g,mut m)=rust_call_unwind_relation_fixture(1);m.activations.clear();assert!(rust_call_unwind_relation_payload(&g,&m).is_err());
    }
    #[test]
    fn rust_call_unwind_relation_duplicate_metadata() {
        let (g,mut m)=rust_call_unwind_relation_fixture(1);m.activations.push(m.activations[0].clone());
        assert!(rust_call_unwind_relation_payload(&g,&m).unwrap_err().to_string().contains("duplicate"));
    }
    #[test]
    fn rust_call_unwind_relation_f2_mismatch() {
        let (g,m)=rust_call_unwind_relation_fixture(1);let mut r=rust_call_return_relation_payload(&g).unwrap();r[0].callee_entry_node="wrong".into();
        let ids=g.ordered_nodes.iter().map(|(id,_)|id.clone()).collect();
        assert!(rust_call_unwind_relation_records(&g,&m,&r,&typed_edge_records(&g),&ids).is_err());
    }
    #[test]
    fn rust_call_unwind_relation_wrong_flow() {
        let (mut g,m)=rust_call_unwind_relation_fixture(1);g.icfg_edges.last_mut().unwrap().label=Some("Normal".into());
        assert!(rust_call_unwind_relation_payload(&g,&m).unwrap_err().to_string().contains("Unwind flow"));
    }
    #[test]
    fn rust_call_unwind_relation_foreign_fanout_rejection() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let r=rust_call_unwind_relation_payload(&g,&m).unwrap();
        assert!(r[0].unwind_edges.iter().all(|e|e.destination_node=="rust::caller::bb7"));
        assert!(r[1].unwind_edges.iter().all(|e|e.destination_node=="rust::caller::bb8"));
    }
    #[test]
    fn rust_call_unwind_relation_deterministic_ordering() {
        let (a,ma)=rust_call_unwind_relation_fixture(2);let mut b=a.clone();let mut mb=ma.clone();
        b.rust_calls.reverse();b.ordered_nodes.reverse();b.icfg_edges.reverse();mb.activations.reverse();
        assert_eq!(serde_json::to_value(rust_call_unwind_relation_payload(&a,&ma).unwrap()).unwrap(),
            serde_json::to_value(rust_call_unwind_relation_payload(&b,&mb).unwrap()).unwrap());
    }
    #[test]
    fn rust_call_unwind_relation_schema_v1() {
        let r=rust_call_return_relation_export(1,0);assert!(r.get("rust_call_unwind_relations").is_none());
        assert!(r.get("capabilities").is_none());
    }
    #[test]
    fn rust_call_unwind_relation_schema_v2_empty() {
        let r=rust_call_return_relation_export(2,0);assert_eq!(r["rust_call_unwind_relations"],serde_json::json!([]));
        assert!(r["capabilities"].as_array().unwrap().iter().any(|c|c=="rust_call_unwind_relation_v1"));
    }
    #[test]
    fn rust_call_unwind_relation_f2_non_interference() {
        let (g,m)=rust_call_unwind_relation_fixture(2);let before=rust_call_return_relation_payload(&g).unwrap();
        rust_call_unwind_relation_payload(&g,&m).unwrap();assert_eq!(before,rust_call_return_relation_payload(&g).unwrap());
    }
    #[test]
    fn rust_call_unwind_relation_disposition_non_interference() {
        return_projection_escape_artifact_disposition();return_projection_escape_field_return();return_projection_escape_nested_field();
    }

    fn rust_call_return_relation_fixture(count: usize, returns: &[&str]) -> GlobalICFGOrdered {
        let mut graph = GlobalICFGOrdered {
            ordered_nodes: vec![], icfg_edges: vec![], llvm_memory_effects: None,
            svf_solved_points_to: None, rust_functions: BTreeMap::new(),
            rust_calls: vec![], dependency_body_ingestion_v1: None,
        };
        graph.rust_functions.insert("callee".into(), RustFunctionMetadata {
            name: "callee".into(), arg_count: 0, entry_node: "entry".into(),
            return_nodes: returns.iter().map(|s| s.to_string()).collect(),
        });
        let mut ids = BTreeSet::from(["entry".to_string()]);
        ids.extend(returns.iter().map(|s| s.to_string()));
        for i in 0..count {
            let call = RustCallMetadata {
                caller_function: "caller".into(), call_node: format!("call{i}"),
                dummy_call_node: format!("activation{i}"), callee_function: "callee".into(),
                dummy_ret_node: format!("matched{i}"), return_node: format!("continuation{i}"),
                arguments: vec![], return_place: "_0".into(), is_closure: false,
            };
            ids.extend([call.call_node.clone(), call.dummy_call_node.clone(),
                call.dummy_ret_node.clone(), call.return_node.clone()]);
            graph.icfg_edges.extend([
                edge(&call.call_node, &call.dummy_call_node),
                edge(&call.dummy_call_node, "entry"),
                edge(&call.dummy_ret_node, &call.return_node),
            ]);
            for ret in returns { graph.icfg_edges.push(edge(ret, &call.dummy_ret_node)); }
            graph.rust_calls.push(call);
        }
        graph.ordered_nodes = ids.into_iter().map(|id| (id, GlobalICFGNode::Mir(
            MirBasicBlock { block_id: 0, statements: vec![], terminator: None }
        ))).collect();
        graph
    }

    fn rust_call_return_relation_payload(graph: &GlobalICFGOrdered) -> Result<Vec<RustCallReturnRelationV1>, Box<dyn Error>> {
        let mut successors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for edge in &graph.icfg_edges {
            successors.entry(edge.source.clone()).or_default().insert(edge.destination.clone());
        }
        let ids = graph.ordered_nodes.iter().map(|(id, _)| id.clone()).collect();
        rust_call_return_relation_records(graph, &successors, &ids)
    }

    fn rust_call_return_relation_export(schema: u32, count: usize) -> serde_json::Value {
        let graph = rust_call_return_relation_fixture(count, &[]);
        let path = std::env::temp_dir().join(format!("crema-relation-{schema}-{}-{:?}.json",
            std::process::id(), std::thread::current().id()));
        let mut state = AbstractState::default();
        let mut memory = AbstractMemory::default();
        memory.set_cell_value(&"Local(_0)".to_string(), CellValue::TOP);
        state.insert("entry".into(), memory);
        export_cqpl_annotated_icfg_with_identity(&graph, &state,
            &AllocationIdentityState::default(), "entry", schema, &path).unwrap();
        let value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        value
    }

    #[test]
    fn rust_call_return_relation_single_call() {
        let records = rust_call_return_relation_payload(&rust_call_return_relation_fixture(1, &["ret"])).unwrap();
        assert_eq!(records, vec![RustCallReturnRelationV1 {
            caller_function: "caller".into(), call_node: "call0".into(),
            dummy_call_node: "activation0".into(), callee_function: "callee".into(),
            callee_entry_node: "entry".into(), callee_return_nodes: vec!["ret".into()],
            dummy_ret_node: "matched0".into(), continuation_node: "continuation0".into(),
            basis: "rust_call_metadata_v1",
        }]);
    }

    #[test]
    fn rust_call_return_relation_shared_callee_two_callsites() {
        let records = rust_call_return_relation_payload(&rust_call_return_relation_fixture(2, &["ret"])).unwrap();
        assert_eq!(records.len(), 2);
        let (a, b) = (&records[0], &records[1]);
        assert_eq!(a.callee_function, b.callee_function);
        assert_eq!(a.callee_entry_node, b.callee_entry_node);
        assert_eq!(a.callee_return_nodes, b.callee_return_nodes);
        assert_ne!(a.call_node, b.call_node);
        assert_ne!(a.dummy_call_node, b.dummy_call_node);
        assert_ne!(a.dummy_ret_node, b.dummy_ret_node);
        assert_ne!(a.continuation_node, b.continuation_node);
    }

    #[test]
    fn rust_call_return_relation_multiple_return_blocks() {
        let records = rust_call_return_relation_payload(&rust_call_return_relation_fixture(1, &["ret_b", "ret_a", "ret_b"])).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].callee_return_nodes, vec!["ret_a", "ret_b"]);
    }

    #[test]
    fn rust_call_return_relation_no_normal_return() {
        let graph = rust_call_return_relation_fixture(1, &[]);
        let before = graph.icfg_edges.clone();
        let records = rust_call_return_relation_payload(&graph).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].callee_return_nodes.is_empty());
        assert_eq!(serde_json::to_value(&graph.icfg_edges).unwrap(), serde_json::to_value(&before).unwrap());
    }

    fn rust_call_return_relation_missing_edge(source: &str, destination: &str, message: &str) {
        let mut graph = rust_call_return_relation_fixture(1, &["ret"]);
        graph.icfg_edges.retain(|e| e.source != source || e.destination != destination);
        let error = rust_call_return_relation_payload(&graph).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }

    #[test]
    fn rust_call_return_relation_missing_call_edge() {
        rust_call_return_relation_missing_edge("call0", "activation0", "missing call edge");
    }
    #[test]
    fn rust_call_return_relation_missing_activation_edge() {
        rust_call_return_relation_missing_edge("activation0", "entry", "missing activation edge");
    }
    #[test]
    fn rust_call_return_relation_missing_callee_return_edge() {
        rust_call_return_relation_missing_edge("ret", "matched0", "missing return edge");
    }
    #[test]
    fn rust_call_return_relation_missing_continuation_edge() {
        rust_call_return_relation_missing_edge("matched0", "continuation0", "missing continuation edge");
    }

    #[test]
    fn rust_call_return_relation_duplicate_dummy_call_ambiguity() {
        let mut graph = rust_call_return_relation_fixture(2, &["ret"]);
        graph.rust_calls[1].dummy_call_node = "activation0".into();
        graph.icfg_edges.push(edge("call1", "activation0"));
        assert!(rust_call_return_relation_payload(&graph).unwrap_err().to_string().contains("duplicate dummy call"));
    }

    #[test]
    fn rust_call_return_relation_duplicate_dummy_return_ambiguity() {
        let mut graph = rust_call_return_relation_fixture(2, &["ret"]);
        graph.rust_calls[1].dummy_ret_node = "matched0".into();
        graph.icfg_edges.push(edge("matched0", "continuation1"));
        assert!(rust_call_return_relation_payload(&graph).unwrap_err().to_string().contains("duplicate dummy return"));
    }

    #[test]
    fn rust_call_return_relation_deterministic_ordering() {
        let a = rust_call_return_relation_fixture(2, &["ret_b", "ret_a"]);
        let mut b = a.clone();
        b.rust_calls.reverse();
        b.rust_functions.get_mut("callee").unwrap().return_nodes.reverse();
        b.icfg_edges.reverse();
        assert_eq!(serde_json::to_string(&rust_call_return_relation_payload(&a).unwrap()).unwrap(),
            serde_json::to_string(&rust_call_return_relation_payload(&b).unwrap()).unwrap());
    }

    #[test]
    fn rust_call_return_relation_schema_v1() {
        let value = rust_call_return_relation_export(1, 0);
        assert!(value.get("rust_call_return_relations").is_none());
        assert!(value.get("capabilities").is_none());
    }

    #[test]
    fn rust_call_return_relation_schema_v2_empty() {
        let value = rust_call_return_relation_export(2, 0);
        assert_eq!(value["rust_call_return_relations"], serde_json::json!([]));
        assert!(value["capabilities"].as_array().unwrap().iter().any(|c| c == "rust_call_return_relation_v1"));
    }

    #[test]
    fn rust_call_return_relation_capability_coexistence() {
        let value = rust_call_return_relation_export(2, 0);
        let caps = value["capabilities"].as_array().unwrap();
        assert!(caps.iter().any(|c| c == "return_projection_escape_v1"));
        assert!(caps.iter().any(|c| c == "rust_call_return_relation_v1"));
    }

    #[test]
    fn rust_call_return_relation_schema_v2_nonempty() {
        let value = rust_call_return_relation_export(2, 1);
        let records = value["rust_call_return_relations"].as_array().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["call_node"], "call0");
        assert_eq!(records[0]["continuation_node"], "continuation0");
        assert_eq!(records[0]["basis"], "rust_call_metadata_v1");
        assert_eq!(records[0]["callee_return_nodes"], serde_json::json!([]));
        assert_eq!(records[0].as_object().unwrap().len(), 9);
    }

    #[test]
    fn rust_call_return_relation_missing_callee() {
        let mut graph = rust_call_return_relation_fixture(1, &["ret"]);
        graph.rust_functions.clear();
        assert!(rust_call_return_relation_payload(&graph).unwrap_err().to_string().contains("missing local callee"));
    }

    #[test]
    fn rust_call_return_relation_missing_canonical_node() {
        let mut graph = rust_call_return_relation_fixture(1, &["ret"]);
        graph.ordered_nodes.retain(|(id, _)| id != "ret");
        assert!(rust_call_return_relation_payload(&graph).unwrap_err().to_string().contains("missing canonical node"));
    }

    #[test]
    fn return_projection_escape_direct_return() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.assign_points_to(root.clone(), BTreeSet::from([allocation.clone()]));
        assert_eq!(return_escape_allocations(&memory, &root), BTreeSet::from([allocation]));
        assert_eq!(return_escape_allocations(&memory, &root), memory.event_allocations(&root));
    }

    #[test]
    fn return_projection_escape_field_return() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.assign_place_points_to(return_projection_place(&root, &[0]), BTreeSet::from([allocation.clone()]));
        assert_eq!(return_escape_allocations(&memory, &root), BTreeSet::from([allocation]));
    }

    #[test]
    fn return_projection_escape_nested_field() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.assign_place_points_to(return_projection_place(&root, &[0, 2]), BTreeSet::from([allocation.clone()]));
        assert_eq!(return_escape_allocations(&memory, &root), BTreeSet::from([allocation]));
    }

    #[test]
    fn return_projection_escape_non_return_root() {
        let (root, allocation, mut memory) = return_projection_fixture();
        let other = ProgramVarId::rust("producer", "Local(_1)").unwrap();
        memory.assign_place_points_to(return_projection_place(&other, &[0]), BTreeSet::from([allocation]));
        assert!(return_escape_allocations(&memory, &root).is_empty());
    }

    #[test]
    fn return_projection_escape_foreign_function() {
        let (root, allocation, mut memory) = return_projection_fixture();
        let other = ProgramVarId::rust("foreign", "Local(_0)").unwrap();
        memory.assign_place_points_to(return_projection_place(&other, &[0]), BTreeSet::from([allocation]));
        assert!(return_escape_allocations(&memory, &root).is_empty());
    }

    #[test]
    fn return_projection_escape_unsupported_projections() {
        let (root, allocation, _) = return_projection_fixture();
        for unsupported in [
            PlaceProjection::Deref,
            PlaceProjection::Index { local: root.clone() },
            PlaceProjection::Opaque { text: "unknown".into() },
        ] {
            for projection in [vec![unsupported.clone()], vec![PlaceProjection::Field { index: 0 }, unsupported]] {
                let mut memory = AllocationIdentityMemory::default();
                let place = PlaceId { base: root.clone(), projection };
                memory.assign_place_points_to(place.clone(), BTreeSet::from([allocation.clone()]));
                memory.assign_place_stack_refs(place, BTreeSet::from([return_projection_place(&root, &[])]));
                assert!(return_escape_allocations(&memory, &root).is_empty());
            }
        }
    }

    #[test]
    fn return_projection_escape_deduplication() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.assign_points_to(root.clone(), BTreeSet::from([allocation.clone()]));
        memory.assign_place_points_to(return_projection_place(&root, &[0]), BTreeSet::from([allocation.clone()]));
        assert_eq!(return_escape_allocations(&memory, &root), BTreeSet::from([allocation]));
        assert_eq!(allocation_disposition_for_node("rust::producer::bb0", &return_projection_node(), &memory, &memory, &[]).len(), 1);
    }

    #[test]
    fn return_projection_escape_artifact_disposition() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.assign_place_points_to(return_projection_place(&root, &[0]), BTreeSet::from([allocation.clone()]));
        let records = allocation_disposition_for_node("rust::producer::bb0", &return_projection_node(), &AllocationIdentityMemory::default(), &memory, &[]);
        assert_eq!(records.len(), 1);
        let value = serde_json::to_value(&records[0]).unwrap();
        assert_eq!(value, serde_json::json!({
            "allocation": stable_allocation_id(&allocation),
            "kind": "return_escape",
            "certainty": "may_abstract",
            "obligation_effect": "may_escape_to_caller",
            "basis": "rust_return_identity_v1",
            "source_variable": "rust::producer::Local(_0)"
        }));
        assert!(records[0].callee_def_path.is_none());
        assert!(records[0].target_variable.is_none());
    }

    #[test]
    fn return_projection_escape_stack_reference_evidence() {
        let (root, allocation, mut memory) = return_projection_fixture();
        let referent = ProgramVarId::rust("producer", "Local(_2)").unwrap();
        memory.assign_points_to(referent.clone(), BTreeSet::from([allocation.clone()]));
        memory.assign_place_stack_refs(return_projection_place(&root, &[0, 2]), BTreeSet::from([return_projection_place(&referent, &[])]));
        assert_eq!(return_escape_allocations(&memory, &root), BTreeSet::from([allocation]));
    }

    #[test]
    fn return_projection_escape_resource_only_is_not_evidence() {
        let (root, allocation, mut memory) = return_projection_fixture();
        memory.place_resources.insert(return_projection_place(&root, &[0]), BTreeSet::from([allocation]));
        assert!(return_escape_allocations(&memory, &root).is_empty());
    }

    #[test]
    fn return_projection_escape_schema_v2_capability() {
        let graph = GlobalICFGOrdered {
            ordered_nodes: vec![("rust::producer::bb0".into(), return_projection_node())],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: Default::default(),
            rust_calls: vec![],
            dependency_body_ingestion_v1: None,
        };
        let mut state = AbstractState::default();
        let mut memory = AbstractMemory::default();
        memory.set_cell_value(&"Local(_0)".to_string(), CellValue::TOP);
        state.insert("rust::producer::bb0".into(), memory);
        let path = std::env::temp_dir().join(format!("crema-return-projection-capability-{}.json", std::process::id()));
        export_cqpl_annotated_icfg_with_identity(&graph, &state, &AllocationIdentityState::default(), "rust::producer::bb0", 2, &path).unwrap();
        let value: serde_json::Value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert!(value["capabilities"].as_array().unwrap().iter().any(|cap| cap == "return_projection_escape_v1"));
    }

    #[test]
    fn canonical_mir_variable_parser_is_stable() {
        assert_eq!(canonical_mir_local("copy _12"), Some("Local(_12)".into()));
        assert_eq!(canonical_mir_local("Local(_7) [mutable]"), Some("Local(_7)".into()));
    }

    fn efm2_call(function_called: &str, args: &[&str]) -> GlobalICFGNode {
        GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: format!("call {function_called}"),
                source_info: "<efm2-test>".into(),
                function_called: function_called.into(),
                callee_def_path: Some(function_called.into()),
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: args
                    .iter()
                    .map(|arg| MirCallArgument {
                        arg: (*arg).into(),
                        is_mutable: Some(false),
                    })
                    .collect(),
                return_place: "_0".into(),
                return_target: Some("bb1".into()),
                unwind_target: "continue".into(),
            }),
        })
    }

    #[test]
    fn dcp1_exact_structured_free_admission() {
        let ffi = HashSet::from(["free".to_string()]);
        let empty = HashSet::new();
        let call = efm2_call("free", &["Local(_1)"]);
        assert!(bodyless_direct_c_free_call_contract(&call, &ffi, &empty).is_some());
        assert!(bodyless_direct_c_free_call_contract(&call, &empty, &empty).is_none());
        assert!(bodyless_direct_c_free_call_contract(&call, &ffi, &ffi).is_none());
        assert!(bodyless_direct_c_free_call_contract(&efm2_call("free", &[]), &ffi, &empty).is_none());
        assert!(bodyless_direct_c_free_call_contract(&efm2_call("free", &["_1", "_2"]), &ffi, &empty).is_none());
        for callee in ["my_free", "free_wrapper", "realloc", "std::alloc::dealloc", "mem::drop"] {
            assert!(bodyless_direct_c_free_call_contract(&efm2_call(callee, &["_1"]), &ffi, &empty).is_none());
        }
        let mut poisoned = efm2_call("my_free", &["_1"]);
        if let GlobalICFGNode::Mir(bb) = &mut poisoned {
            if let Some(MirTerminator::Call { details, .. }) = &mut bb.terminator {
                *details = "call free(copy _1)".into();
            }
        }
        assert!(bodyless_direct_c_free_call_contract(&poisoned, &ffi, &empty).is_none());
        let mut wrong_decl = call.clone();
        if let GlobalICFGNode::Mir(bb) = &mut wrong_decl {
            if let Some(MirTerminator::Call { callee_def_path, .. }) = &mut bb.terminator {
                *callee_def_path = Some("other::free".into());
            }
        }
        assert!(bodyless_direct_c_free_call_contract(&wrong_decl, &ffi, &empty).is_none());
        let argument = bodyless_direct_c_free_call_contract(&call, &ffi, &empty).unwrap();
        let catalog = BTreeSet::from(["rust::main::Local(_1)".to_string()]);
        assert_eq!(dcp1_existing_canonical_actual("rust::main::bb0", argument, &catalog),
                   Some("rust::main::Local(_1)".into()));
        assert_eq!(dcp1_existing_canonical_actual("rust::main::bb0", argument, &BTreeSet::new()), None);
        // An unscoped legacy local does not authorize a new scoped variable.
        assert_eq!(dcp1_existing_canonical_actual("rust::main::bb0", argument,
                   &BTreeSet::from(["Local(_1)".to_string()])), None);
    }

    #[test]
    fn efm2_classifier_is_exact_arity_closed_ffi_gated_and_body_aware() {
        let ffi = [
            "strlen", "memcmp", "memcpy", "memmove", "memset", "memchr", "strchr", "write",
        ]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let represented = HashSet::new();

        for (callee, arity, semantic_class, rules) in [
            ("strlen", 1, "strlen_read_c_string_v1", vec![(0, "read", ExternalMemoryExtent::CStringUntilNul)]),
            ("memcmp", 3, "memcmp_v1", vec![(0, "read", ExternalMemoryExtent::BytesFromFormal(2)), (1, "read", ExternalMemoryExtent::BytesFromFormal(2))]),
            ("memcpy", 3, "memcpy_v1", vec![(0, "write", ExternalMemoryExtent::BytesFromFormal(2)), (1, "read", ExternalMemoryExtent::BytesFromFormal(2))]),
            ("memmove", 3, "memmove_v1", vec![(0, "write", ExternalMemoryExtent::BytesFromFormal(2)), (1, "read", ExternalMemoryExtent::BytesFromFormal(2))]),
            ("memset", 3, "memset_v1", vec![(0, "write", ExternalMemoryExtent::BytesFromFormal(2))]),
            ("memchr", 3, "memchr_bounded_read_v1", vec![(0, "read", ExternalMemoryExtent::BytesFromFormal(2))]),
            ("strchr", 2, "strchr_read_c_string_v1", vec![(0, "read", ExternalMemoryExtent::CStringUntilNul)]),
            ("write", 3, "posix_write_v1", vec![(1, "read", ExternalMemoryExtent::BytesFromFormal(2))]),
        ] {
            let contract = external_function_memory_contract_v2(callee, arity, &ffi, &represented)
                .unwrap_or_else(|| panic!("missing EFM2 contract for {callee}/{arity}"));
            assert_eq!(contract.semantic_class, semantic_class);
            assert_eq!(
                contract.rules.iter().map(|rule| (rule.formal_index, rule.access, rule.extent)).collect::<Vec<_>>(),
                rules,
                "wrong formal roles for {callee}",
            );
            assert!(contract.rules.iter().all(|rule| rule.basis == "crema_efm2_closed_contract_v1"));
        }

        for (callee, wrong_arity) in [("memmove", 2), ("memchr", 2), ("strchr", 3)] {
            assert!(external_function_memory_contract_v2(callee, wrong_arity, &ffi, &represented).is_none());
        }
        assert!(external_function_memory_contract_v2("memmove", 3, &HashSet::new(), &represented).is_none());
        for (raw, arity) in [
            ("libc::memmove", 3),
            ("my_memmove", 3),
            ("memmove_wrapper", 3),
            ("foo_strchr", 2),
        ] {
            assert!(external_function_memory_contract_v2(raw, arity, &ffi, &represented).is_none());
        }
        assert!(external_function_memory_contract_v2("strcpy", 2, &ffi, &represented).is_none());

        for (callee, arity) in [
            ("strlen", 1), ("memcmp", 3), ("memcpy", 3), ("memmove", 3),
            ("memset", 3), ("memchr", 3), ("strchr", 2), ("write", 3),
        ] {
            let represented = [callee.to_string()].into_iter().collect::<HashSet<_>>();
            assert!(external_function_memory_contract_v2(callee, arity, &ffi, &represented).is_none());
        }
    }

    #[test]
    fn ene1_producer_evidence_identity_and_no_semantic_transfer() {
        let ffi = HashSet::from(["observe".to_string()]);
        let mut snapshot = LlvmFunctionEffectsSnapshotV1::default();
        snapshot.formals = vec![
            crate::structs::LlvmFormalEffectsSnapshotV1 { index: 0, pointer_typed: true, ..Default::default() },
            crate::structs::LlvmFormalEffectsSnapshotV1 { index: 1, pointer_typed: true, ..Default::default() },
        ];
        let mut icfg = GlobalICFGOrdered {
            ordered_nodes: vec![("rust::main::bb0".into(), efm2_call("observe", &["Local(_1)", "Local(_1)"]))],
            icfg_edges: vec![], llvm_memory_effects: None, svf_solved_points_to: None,
            rust_functions: Default::default(), rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        // U7/U9/U10: absence never creates either opposite behavior.
        icfg.llvm_memory_effects = Some(efx1_test_artifact("observe", snapshot.clone(), snapshot.clone(), false, false));
        assert!(external_negative_evidence_records(&icfg, &ffi, &HashSet::new()).is_empty());
        // U1/U3/U4/U8: independent copies, even when actuals alias.
        snapshot.nofree = true; snapshot.formals[0].nofree = true; snapshot.formals[0].nocapture = true;
        icfg.llvm_memory_effects = Some(efx1_test_artifact("observe", snapshot.clone(), snapshot.clone(), false, false));
        let before = serde_json::to_value(&icfg).unwrap();
        let records = external_negative_evidence_records(&icfg, &ffi, &HashSet::new());
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].evidence_kind, "no_free_function");
        assert_eq!(records[0].basis, "llvm16_explicit_function_nofree_v1");
        for record in &records[1..] {
            assert_eq!(record.formal_index, Some(0));
            assert_eq!(record.actual_variable.as_deref(), Some("rust::main::Local(_1)"));
        }
        // U11/U12: no mutation to ICFG, ordinary events or identity/state.
        assert_eq!(serde_json::to_value(&icfg).unwrap(), before);
        // U6: body present or declaration mismatch cannot be summarized.
        assert!(external_negative_evidence_records(&icfg, &ffi, &ffi).is_empty());
        icfg.llvm_memory_effects.as_mut().unwrap().modules[0].functions[0].is_declaration = false;
        assert!(external_negative_evidence_records(&icfg, &ffi, &HashSet::new()).is_empty());
        // U5: do not guess a projected actual.
        icfg.ordered_nodes[0].1 = efm2_call("observe", &["Local(_1) -> Deref", "Local(_1)"]);
        icfg.llvm_memory_effects = Some(efx1_test_artifact("observe", snapshot.clone(), snapshot.clone(), false, false));
        assert_eq!(external_negative_evidence_records(&icfg, &ffi, &HashSet::new()).len(), 1);
        // U2: verified TLI nofree is distinct; inferred formal attributes are inadmissible.
        let mut explicit = snapshot.clone(); explicit.nofree = false;
        explicit.formals[0].nofree = false; explicit.formals[0].nocapture = false;
        icfg.llvm_memory_effects = Some(efx1_test_artifact("observe", explicit, snapshot, true, true));
        let records = external_negative_evidence_records(&icfg, &ffi, &HashSet::new());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].basis, "llvm16_tli_verified_function_nofree_v1");
        icfg.llvm_memory_effects.as_mut().unwrap().modules[0].functions[0].tli_recognized = false;
        assert!(external_negative_evidence_records(&icfg, &ffi, &HashSet::new()).is_empty());
    }

    #[test]
    fn err1_records_bind_real_result_and_source_without_projected_guesses() {
        let ffi = ["memcpy", "memmove", "memset", "memchr", "strchr", "getenv"].into_iter().map(str::to_string).collect();
        for (callee, args) in [
            ("memcpy", vec!["Local(_1)", "Local(_2)", "const 0_usize"]),
            ("memmove", vec!["Local(_1)", "Local(_2)", "const 0_usize"]),
            ("memset", vec!["Local(_1)", "const 1_i32", "const 0_usize"]),
            ("memchr", vec!["Local(_1)", "const 1_i32", "Local(_2)"]),
            ("strchr", vec!["Local(_1)", "const 1_i32"]),
            ("getenv", vec!["Local(_1)"]),
        ] {
            let mut node = efm2_call(callee, &args);
            if let GlobalICFGNode::Mir(bb) = &mut node {
                if let Some(MirTerminator::Call { return_place, .. }) = &mut bb.terminator { *return_place = "Local(_9)".into(); }
            }
            let mut icfg = GlobalICFGOrdered { ordered_nodes:vec![("rust::main::bb0".into(), node)], icfg_edges:vec![], llvm_memory_effects:None, svf_solved_points_to:None, rust_functions:Default::default(), rust_calls:vec![],
            dependency_body_ingestion_v1: None,};
            let (records, bindings) = external_return_relation_records(&icfg, &ffi, &HashSet::new());
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].result_variable, "rust::main::Local(_9)");
            assert_eq!(bindings[0].result_variable, records[0].result_variable);
            assert_eq!(records[0].source_actual_variable.as_deref(), if callee == "getenv" {None} else {Some("rust::main::Local(_1)")});
            assert!(external_return_relation_records(&icfg, &ffi, &HashSet::from([callee.into()])).0.is_empty());
            if let GlobalICFGNode::Mir(bb) = &mut icfg.ordered_nodes[0].1 {
                if let Some(MirTerminator::Call { return_place, .. }) = &mut bb.terminator { *return_place = "Local(_9) -> Deref".into(); }
            }
            assert!(external_return_relation_records(&icfg, &ffi, &HashSet::new()).0.is_empty());
        }
    }

    #[test]
    fn ele1_binding_is_extracted_from_structured_mir_call() {
        let mut call = efm2_call("memcpy", &["Local(_1)", "Local(_2)", "const 0_usize"]);
        if let GlobalICFGNode::Mir(bb) = &mut call {
            if let Some(MirTerminator::Call { return_place, .. }) = &mut bb.terminator {
                *return_place = "Local(_9)".into();
            }
        }
        let mut icfg = GlobalICFGOrdered {
            ordered_nodes: vec![("rust::main::bb0".into(), call)], icfg_edges: vec![],
            llvm_memory_effects: None, svf_solved_points_to: None,
            rust_functions: Default::default(), rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let ffi = HashSet::from(["memcpy".into()]);
        let (relations, _) = external_return_relation_records(&icfg, &ffi, &HashSet::new());
        let vars = [1, 2, 9].map(|n| ProgramVariable {
            id: format!("rust::main::Local(_{n})"), language: "rust",
        });
        let build = |icfg: &GlobalICFGOrdered| external_library_effect_envelopes(
            icfg, &vars, &[], &[], &[], &[], &relations, &[],
        );
        let (bindings, envelopes) = build(&icfg);
        let bindings = bindings.unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].binding_id, "ele1:rust::main::bb0");
        assert_eq!(bindings[0].arity, 3);
        assert_eq!(bindings[0].arguments, vec![Some(vars[0].id.clone()), Some(vars[1].id.clone()), None]);
        assert_eq!(bindings[0].result_variable, Some(vars[2].id.clone()));
        assert_eq!(envelopes.unwrap()[0].effect_counts["return_relation"], 1);
        if let GlobalICFGNode::Mir(bb) = &mut icfg.ordered_nodes[0].1 {
            if let Some(MirTerminator::Call { return_place, .. }) = &mut bb.terminator {
                *return_place = "Local(_8)".into();
            }
        }
        assert_eq!(build(&icfg).0.unwrap()[0].result_variable, None);
    }

    #[test]
    fn efm2_bodyless_calls_emit_only_argument_specific_existing_use_labels() {
        let ffi = ["strlen", "memcmp", "memcpy", "memmove", "memset", "memchr", "strchr", "write"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let resolver = LlvmNameResolver::default();
        let represented = HashSet::new();

        let cases = [
            ("strlen", vec!["Local(_1)"], vec![("read", "Local(_1)")]),
            ("memset", vec!["Local(_1)", "const 0_i32", "const 8_usize"], vec![("write", "Local(_1)")]),
            ("memcpy", vec!["Local(_1)", "Local(_2)", "const 8_usize"], vec![("write", "Local(_1)"), ("read", "Local(_2)")]),
            ("memcmp", vec!["Local(_1)", "Local(_2)", "const 8_usize"], vec![("read", "Local(_1)"), ("read", "Local(_2)")]),
            ("memmove", vec!["Local(_1)", "Local(_2)", "const 8_usize"], vec![("write", "Local(_1)"), ("read", "Local(_2)")]),
            ("memchr", vec!["Local(_1)", "const 1_i32", "const 8_usize"], vec![("read", "Local(_1)")]),
            ("strchr", vec!["Local(_1)", "const 1_i32"], vec![("read", "Local(_1)")]),
            ("write", vec!["const 1_i32", "Local(_2)", "const 8_usize"], vec![("read", "Local(_2)")]),
        ];

        for (callee, args, expected) in cases {
            let call = efm2_call(callee, &args);
            let events = event_sources_for_node(
                "rust::main::bb0",
                &call,
                &resolver,
                &ffi,
                &represented,
            );
            let actual = events
                .keys()
                .filter(|e| matches!(e.predicate, "read" | "write"))
                .map(|e| (e.predicate, e.variable.as_str()))
                .collect::<BTreeSet<_>>();
            let expected = expected.into_iter().collect::<BTreeSet<_>>();
            assert_eq!(actual, expected, "wrong EFM2 labels for {callee}");
        }
    }

    #[test]
    fn efm2_definite_zero_extent_suppresses_memory_event_and_record() {
        let ffi = ["memcpy", "memcmp", "memmove", "memset", "memchr", "write"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let resolver = LlvmNameResolver::default();
        let represented = HashSet::new();
        let calls = [
            efm2_call("memcpy", &["Local(_1)", "Local(_2)", "const 0_usize"]),
            efm2_call("memcmp", &["Local(_1)", "Local(_2)", "const 0_usize"]),
            efm2_call("memmove", &["Local(_1)", "Local(_2)", "const 0_usize"]),
            efm2_call("memset", &["Local(_1)", "const 1_i32", "const 0_usize"]),
            efm2_call("memchr", &["Local(_1)", "const 1_i32", "const 0_usize"]),
            efm2_call("write", &["const 1_i32", "Local(_1)", "const 0_usize"]),
        ];
        for call in &calls {
            let events = event_sources_for_node(
                "rust::main::bb0", call, &resolver, &ffi, &represented,
            );
            assert!(events.keys().all(|e| !matches!(e.predicate, "read" | "write")));
        }

        for call in calls {
            let icfg = GlobalICFGOrdered {
                ordered_nodes: vec![("rust::main::bb0".into(), call)],
                icfg_edges: vec![], llvm_memory_effects: None, svf_solved_points_to: None,
                rust_functions: Default::default(), rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
            assert!(external_formal_memory_effect_records(&icfg, &ffi, &represented).is_empty());
        }
    }

    #[test]
    fn efm2_dynamic_extent_records_preserve_explicit_extent_and_formal_actual_separation() {
        let ffi = ["memmove".to_string()].into_iter().collect::<HashSet<_>>();
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(
                "rust::main::bb0".into(),
                efm2_call("memmove", &["Local(_1)", "Local(_2)", "Local(_3)"]),
            )],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let records = external_formal_memory_effect_records(&icfg, &ffi, &HashSet::new());
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|r| {
            r.formal_index == 0
                && r.access == "write"
                && r.event_variable == "Local(_1)"
                && r.actual_variable == "rust::main::Local(_1)"
                && r.extent_kind == "bytes_from_formal"
                && r.extent_argument_index == Some(2)
                && r.basis == "crema_efm2_closed_contract_v1"
                && r.semantic_sources == vec!["posix_memmove_n_byte_copy_semantics_v1", "llvm16_memmove_formal_semantics_v1", "llvm16_tli_memmove_recognition_v1"]
        }));
        assert!(records.iter().any(|r| {
            r.formal_index == 1
                && r.access == "read"
                && r.event_variable == "Local(_2)"
                && r.actual_variable == "rust::main::Local(_2)"
                && r.extent_kind == "bytes_from_formal"
                && r.extent_argument_index == Some(2)
                && r.basis == "crema_efm2_closed_contract_v1"
        }));

        let events = event_sources_for_node(
            "rust::main::bb0", &icfg.ordered_nodes[0].1, &LlvmNameResolver::default(),
            &ffi, &HashSet::new(),
        );
        assert_eq!(
            events.keys().filter(|event| matches!(event.predicate, "read" | "write")).count(),
            2,
            "dynamic extent must retain both MAY effects",
        );
    }

    #[test]
    fn represented_c_stack_slot_transport_is_not_exported_as_pointee_access() {
        use crate::structs::{LlvmJsonNode, SvfStatement};

        fn stmt(kind: &str, lhs: Option<usize>, rhs: Option<usize>, info: &str) -> SvfStatement {
            SvfStatement {
                stmt_id: 0,
                stmt_type: kind.into(),
                stmt_info: info.into(),
                edge_id: None,
                pta_edge: None,
                lhs_var_id: lhs,
                rhs_var_id: rhs,
                res_var_id: None,
                operand_var_ids: None,
                operand_vars: None,
                call_inst: None,
                is_conditional: None,
                condition_var_id: None,
                successors: None,
            }
        }

        fn llvm(info: &str, statements: Vec<SvfStatement>) -> GlobalICFGNode {
            GlobalICFGNode::Llvm(LlvmJsonNode {
                node_id: 0,
                node_type: false,
                info: info.into(),
                node_kind_string: "IntraBlock".into(),
                node_kind: 0,
                node_source_loc: String::new(),
                function_name: Some("body".into()),
                basic_block: Some(0),
                basic_block_name: None,
                basic_block_info: None,
                svf_statements: statements,
                incoming_edges: vec![],
                outgoing_edges: vec![],
            })
        }

        let alloca_id = "llvm::body::node3::rust::main::bb5";
        let spill_store_id = "llvm::body::node4::rust::main::bb5";
        let spill_load_id = "llvm::body::node5::rust::main::bb5";
        let gep_id = "llvm::body::node6::rust::main::bb5";
        let pointee_read_id = "llvm::body::node7::rust::main::bb5";
        let pointee_write_id = "llvm::body::node8::rust::main::bb5";

        let alloca = llvm(
            "%p.addr = alloca ptr, align 8",
            vec![stmt("AddrStmt", Some(8), Some(9), "AddrStmt: [Var8 <-- Var9]")],
        );
        let spill_store = llvm(
            "store ptr %p, ptr %p.addr, align 8",
            vec![stmt("StoreStmt", Some(8), Some(7), "StoreStmt: [Var8 <-- Var7]")],
        );
        let spill_load = llvm(
            "%0 = load ptr, ptr %p.addr, align 8",
            vec![stmt("LoadStmt", Some(12), Some(8), "LoadStmt: [Var12 <-- Var8]")],
        );
        let gep = llvm(
            "%arrayidx = getelementptr inbounds i8, ptr %0, i64 0",
            vec![stmt("GepStmt", Some(13), Some(12), "GepStmt: [Var13 <-- Var12]")],
        );
        let pointee_read = llvm(
            "%1 = load i8, ptr %arrayidx, align 1",
            vec![stmt("LoadStmt", Some(15), Some(13), "LoadStmt: [Var15 <-- Var13]")],
        );
        let pointee_write = llvm(
            "store i8 7, ptr %arrayidx, align 1",
            vec![stmt("StoreStmt", Some(13), Some(16), "StoreStmt: [Var13 <-- Var16]")],
        );

        // Build the resolver over the complete carrier chain.  This checks the
        // essential separation: value provenance must still flow
        //   Var7 -> memory[Var8] -> Var12 -> Var13
        // even though accesses *to Var8 itself* are suppressed as heap events.
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![
                (alloca_id.into(), alloca.clone()),
                (spill_store_id.into(), spill_store.clone()),
                (spill_load_id.into(), spill_load.clone()),
                (gep_id.into(), gep),
                (pointee_read_id.into(), pointee_read.clone()),
                (pointee_write_id.into(), pointee_write.clone()),
            ],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let resolver = LlvmNameResolver::build(&icfg);
        let formal = scoped_svf_var(7, spill_store_id);
        let slot = scoped_svf_var(8, alloca_id);
        let gep_address = scoped_svf_var(13, pointee_read_id);

        assert!(resolver.is_stack_slot_address(&slot));
        assert_eq!(resolver.resolve_svf(&gep_address), formal);

        let spill_store_events = event_sources_for_node(
            spill_store_id,
            &spill_store,
            &resolver,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(spill_store_events.keys().all(|event| event.predicate != "write"));

        let spill_load_events = event_sources_for_node(
            spill_load_id,
            &spill_load,
            &resolver,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(spill_load_events.keys().all(|event| event.predicate != "read"));

        let read_events = event_sources_for_node(
            pointee_read_id,
            &pointee_read,
            &resolver,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(read_events.keys().any(|event| {
            event.predicate == "read" && event.variable == formal
        }));

        let write_events = event_sources_for_node(
            pointee_write_id,
            &pointee_write,
            &resolver,
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(write_events.keys().any(|event| {
            event.predicate == "write" && event.variable == formal
        }));
    }

    #[test]
    fn memory_annotation_preserves_alias_component_and_cell_value() {
        let mut mem = AbstractMemory::default();
        let mut set = BTreeSet::new();
        set.insert("Local(_1)".to_string());
        set.insert("9@rust::main::bb0".to_string());
        mem.state.insert(Allocation { set }, CellValue::TOP);
        let ann = memory_annotation(&mem);
        assert_eq!(ann.cells.len(), 1);
        assert_eq!(ann.cells[0].value, "TOP");
        assert_eq!(ann.cells[0].aliases, vec!["9@rust::main::bb0", "Local(_1)"]);
    }

    #[test]
    fn r2_ffi_argument_identity_serializes_existing_bmulti_may_identity() {
        let node_id = "dummyCall::rust::main::bb3::rust::main::bb3";
        let dummy = GlobalICFGNode::DummyCall(DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: "rust::main::bb3".into(),
            outgoing_edge: "llvm::c_free_i32::node1::rust::main::bb3".into(),
            id: "dc".into(),
            mir_var: Some("Local(_1)".into()),
            llvm_var: Some("36@rust::main::bb3".into()),
            argument_bindings: vec![crate::structs::DummyArgumentBinding {
                arg_index: 0,
                mir_var: "Local(_1)".into(),
                llvm_var: "36@rust::main::bb3".into(),
                svf_may_points_to: vec![],
                svf_points_to_basis: Some("svf_andersen_wave_diff_may_v1".into()),
            }],
            is_internal: Some(false),
        });
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(node_id.into(), dummy)],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let actual = ProgramVarId::rust("main", "Local(_1)").unwrap();
        let formal = ProgramVarId::c("c_free_i32", 36, Some("rust::main::bb3".into()));
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            vec![],
        );
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(actual.clone(), BTreeSet::from([allocation.clone()]));
        memory.assign_points_to(formal.clone(), BTreeSet::from([allocation.clone()]));
        let mut state = AllocationIdentityState::default();
        state.by_node.insert(node_id.into(), memory);

        let records = ffi_argument_identity_records(&icfg, &state).unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.arg_index, 0);
        assert_eq!(record.actual_variable, actual.canonical_string());
        assert_eq!(record.formal_variable, formal.canonical_string());
        assert_eq!(record.allocations, vec![stable_allocation_id(&allocation)]);
        assert_eq!(record.certainty, "may_abstract");
        assert_eq!(record.basis, "crema_bmulti_actual_formal_identity_v1");
    }


    #[test]
    fn r2_ffi_argument_identity_skips_non_local_actual_but_keeps_later_local_binding() {
        let node_id = "dummyCall::rust::main::bb3::rust::main::bb3";
        let dummy = GlobalICFGNode::DummyCall(DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: "rust::main::bb3".into(),
            outgoing_edge: "llvm::c_mixed::node1::rust::main::bb3".into(),
            id: "dc".into(),
            mir_var: Some("const 7_i32".into()),
            llvm_var: Some("10@rust::main::bb3".into()),
            argument_bindings: vec![
                crate::structs::DummyArgumentBinding {
                    arg_index: 0,
                    mir_var: "const 7_i32".into(),
                    llvm_var: "10@rust::main::bb3".into(),
                    svf_may_points_to: vec![],
                    svf_points_to_basis: None,
                },
                crate::structs::DummyArgumentBinding {
                    arg_index: 1,
                    mir_var: "Local(_1)".into(),
                    llvm_var: "36@rust::main::bb3".into(),
                    svf_may_points_to: vec![],
                    svf_points_to_basis: Some("svf_andersen_wave_diff_may_v1".into()),
                },
            ],
            is_internal: Some(false),
        });
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(node_id.into(), dummy)],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};

        let actual = ProgramVarId::rust("main", "Local(_1)").unwrap();
        let formal = ProgramVarId::c("c_mixed", 36, Some("rust::main::bb3".into()));
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            vec![],
        );
        let mut memory = AllocationIdentityMemory::default();
        memory.assign_points_to(actual.clone(), BTreeSet::from([allocation.clone()]));
        memory.assign_points_to(formal.clone(), BTreeSet::from([allocation.clone()]));
        let mut state = AllocationIdentityState::default();
        state.by_node.insert(node_id.into(), memory);

        let records = ffi_argument_identity_records(&icfg, &state).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].arg_index, 1);
        assert_eq!(records[0].actual_variable, actual.canonical_string());
        assert_eq!(records[0].formal_variable, formal.canonical_string());
        assert_eq!(records[0].allocations, vec![stable_allocation_id(&allocation)]);
    }

    #[test]
    fn dereference_statement_produces_read_and_write_labels() {
        let bb = MirBasicBlock {
            block_id: 0,
            statements: vec![MirStatement {
                resource_place_relation_v1: None,
                source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
                kind: "Assign".into(),
                details: "(*_1) = copy (*_2)".into(),
                place: Some("(*_1)".into()),
                is_mutable: Some(true),
                rvalue: Some("copy (*_2)".into()),
            }],
            terminator: None,
        };
        let node = GlobalICFGNode::Mir(bb);
        let labels = labels_for_node("rust::main::bb0", &node, &LlvmNameResolver::default(), &HashSet::new());
        assert!(labels.iter().any(|l| l.predicate == "write" && l.variable == "Local(_1)"));
        assert!(labels.iter().any(|l| l.predicate == "read" && l.variable == "Local(_2)"));
    }

    #[test]
    fn w1_rust_source_span_is_structured_without_losing_raw_provenance() {
        let raw = "/tmp/project/src/main.rs:12:17: 12:29 (#7)";
        let span = parse_rust_source_span(raw).expect("structured rustc source span");
        assert_eq!(span.file, "/tmp/project/src/main.rs");
        assert_eq!(span.start_line, 12);
        assert_eq!(span.start_column, 17);
        assert_eq!(span.end_line, 12);
        assert_eq!(span.end_column, 29);
        assert!(parse_rust_source_span("synthetic span").is_none());
    }

    #[test]
    fn w1_event_source_map_preserves_statement_and_terminator_occurrences() {
        let statement_span = "/tmp/project/src/main.rs:12:5: 12:23 (#1)";
        let drop_span = "/tmp/project/src/main.rs:13:5: 13:14 (#1)";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![MirStatement {
                resource_place_relation_v1: None,
                source_info: SourceInfoData { span: statement_span.into(), scope: "0".into() },
                kind: "Assign".into(),
                details: "(*_1) = copy (*_2)".into(),
                place: Some("(*_1)".into()),
                is_mutable: Some(true),
                rvalue: Some("copy (*_2)".into()),
            }],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_3)".into(),
                source_info: drop_span.into(),
                return_target: "bb1".into(),
                unwind_target: "unreachable".into(),
                dropped_value: "_3".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });

        let sources = event_sources_for_node(
            "rust::main::bb0",
            &node,
            &LlvmNameResolver::default(),
            &HashSet::new(),
            &HashSet::new(),
        );
        let read = sources
            .get(&EventLabel { predicate: "read", variable: "Local(_2)".into() })
            .expect("read occurrence");
        assert!(read.iter().any(|anchor| {
            anchor.kind == "mir_statement"
                && anchor.statement_index == Some(0)
                && anchor.raw_span == statement_span
        }));
        let drop = sources
            .get(&EventLabel { predicate: "drop", variable: "Local(_3)".into() })
            .expect("drop occurrence");
        assert!(drop.iter().any(|anchor| {
            anchor.kind == "mir_terminator"
                && anchor.statement_index.is_none()
                && anchor.raw_span == drop_span
        }));
    }

    #[test]
    fn w1_allocation_event_source_is_joined_through_existing_event_identity_only() {
        let drop_span = "/tmp/project/src/main.rs:21:5: 21:14 (#2)";
        let node_id = "rust::main::bb0";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_3)".into(),
                source_info: drop_span.into(),
                return_target: "bb1".into(),
                unwind_target: "unreachable".into(),
                dropped_value: "_3".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(ProgramVarId::rust("main", "_3").unwrap(), allocation.clone());
        let event_sources = event_sources_for_node(
            node_id,
            &node,
            &LlvmNameResolver::default(),
            &HashSet::new(),
            &HashSet::new(),
        );
        let raw: Vec<EventLabel> = event_sources.keys().cloned().collect();
        let admitted = allocation_labels_for_node(
            node_id,
            &node,
            &raw,
            &identity,
            &HashSet::new(),
            2,
        );
        assert!(admitted.is_empty(), "uncertified MIR Drop is not an allocation-level deallocation");
        let records = allocation_event_source_records_for_node(
            node_id,
            &node,
            &event_sources,
            &identity,
            &admitted,
        );
        assert!(
            records.is_empty(),
            "source provenance must not resurrect an allocation event rejected by semantic admission",
        );
    }

    #[test]
    fn ht2_r2p3_source_provenance_is_exact_projection_of_admitted_vec_drop() {
        let drop_span = "/tmp/project/src/main.rs:31:5: 31:14 (#3)";
        let node_id = "rust::main::bb6";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 6,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_2)".into(),
                source_info: drop_span.into(),
                return_target: "bb7".into(),
                unwind_target: "continue".into(),
                dropped_value: "_2".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::VecGlobal,
                    owner_def_path: "alloc::vec::Vec".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "vec-buffer".into() },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust("main", "_2").expect("MIR local"),
            allocation.clone(),
        );
        let event_sources = event_sources_for_node(
            node_id,
            &node,
            &LlvmNameResolver::default(),
            &HashSet::new(),
            &HashSet::new(),
        );
        let raw: Vec<EventLabel> = event_sources.keys().cloned().collect();
        let admitted = allocation_labels_for_node(
            node_id,
            &node,
            &raw,
            &identity,
            &HashSet::new(),
            2,
        );
        assert_eq!(admitted.len(), 1);
        let records = allocation_event_source_records_for_node(
            node_id,
            &node,
            &event_sources,
            &identity,
            &admitted,
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].predicate, "drop");
        assert_eq!(records[0].allocation, stable_allocation_id(&allocation));
        assert_eq!(records[0].certainty, "may_abstract");
        assert_eq!(records[0].anchors.len(), 1);
        assert_eq!(records[0].anchors[0].raw_span, drop_span);
    }

    #[test]
    fn ht2_r2p3_source_provenance_never_exceeds_allocation_labels() {
        let node_id = "rust::main::bb0";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_3)".into(),
                source_info: "/tmp/project/src/main.rs:41:5: 41:14 (#4)".into(),
                return_target: "bb1".into(),
                unwind_target: "continue".into(),
                dropped_value: "_3".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "owned".into() },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust("main", "_3").expect("MIR local"),
            allocation,
        );
        let event_sources = event_sources_for_node(
            node_id,
            &node,
            &LlvmNameResolver::default(),
            &HashSet::new(),
            &HashSet::new(),
        );
        let raw: Vec<EventLabel> = event_sources.keys().cloned().collect();
        let admitted = allocation_labels_for_node(
            node_id,
            &node,
            &raw,
            &identity,
            &HashSet::new(),
            2,
        );
        let provenance = node_source_provenance(
            node_id,
            &node,
            &event_sources,
            &identity,
            &admitted,
        );
        let admitted_keys: BTreeSet<(&'static str, String)> = admitted
            .iter()
            .map(|label| (label.predicate, label.allocation.clone()))
            .collect();
        for record in provenance.allocation_events {
            assert!(
                admitted_keys.contains(&(record.predicate, record.allocation)),
                "source provenance must be a subset of semantically admitted allocation labels",
            );
        }
        assert!(admitted.is_empty());
    }

    #[test]
    fn exporter_keeps_graph_successors_and_post_state_without_fabricating_pre() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let n1 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: None });
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![("rust::main::bb0".into(), n0), ("rust::main::bb1".into(), n1)],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::bb1")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let mut s = AbstractState::default();
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&"Local(_1)".to_string(), CellValue::ALLOC);
        s.insert("rust::main::bb0".into(), mem);

        let path = std::env::temp_dir().join(format!("crema-cqpl-export-{}.json", std::process::id()));
        export_cqpl_annotated_icfg(&g, &s, "rust::main::bb0", &path).unwrap();
        let value: serde_json::Value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(path);

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["entry"], "rust::main::bb0");
        assert_eq!(value["nodes"][0]["successors"][0], "rust::main::bb1");
        assert_eq!(value["nodes"][0]["pre"]["cells"].as_array().unwrap().len(), 0);
        assert_eq!(value["nodes"][0]["post"]["cells"][0]["value"], "ALLOC");
    }

    #[test]
    fn schema_v2_preserves_typed_edge_flow_and_original_labels_additively() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let n1 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: None });
        let edge = IcfgEdge {
            source: "rust::main::bb0".into(),
            destination: "rust::main::bb1".into(),
            label: Some("Call unwind".into()),
            source_label: Some("call-site".into()),
            destination_label: Some("cleanup".into()),
        };
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![("rust::main::bb0".into(), n0), ("rust::main::bb1".into(), n1)],
            icfg_edges: vec![edge],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let mut state = AbstractState::default();
        let mut memory = AbstractMemory::default();
        memory.set_cell_value(&"Local(_1)".to_string(), CellValue::TOP);
        state.insert("rust::main::bb0".into(), memory);
        let identity = AllocationIdentityState::default();

        let path = std::env::temp_dir().join(format!("crema-cqpl-typed-edge-{}.json", std::process::id()));
        export_cqpl_annotated_icfg_with_identity(&g, &state, &identity, "rust::main::bb0", 2, &path).unwrap();
        let value: serde_json::Value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(path);

        assert!(value["capabilities"].as_array().unwrap().iter().any(|c| c.as_str() == Some("typed_edge_flow_v1")));
        assert!(value["capabilities"].as_array().unwrap().iter().any(|c| c.as_str() == Some("source_provenance_v1")));
        assert_eq!(value["nodes"][0]["source_provenance"]["language"], "rust");
        assert_eq!(value["nodes"][0]["source_provenance"]["anchors"], serde_json::json!([]));
        assert_eq!(value["nodes"][0]["successors"], serde_json::json!(["rust::main::bb1"]));
        assert_eq!(value["typed_edges"].as_array().unwrap().len(), 1);
        let typed = &value["typed_edges"][0];
        assert_eq!(typed["source"], "rust::main::bb0");
        assert_eq!(typed["destination"], "rust::main::bb1");
        assert_eq!(typed["flow"], "unwind");
        assert_eq!(typed["label"], "Call unwind");
        assert_eq!(typed["source_label"], "call-site");
        assert_eq!(typed["destination_label"], "cleanup");
    }

    #[test]
    fn schema_v2_exports_hybrid_dependency_context_capability_without_claiming_v3() {
        let n0=GlobalICFGNode::Mir(MirBasicBlock {block_id:0,statements:vec![],terminator:None});
        let g=GlobalICFGOrdered {
            llvm_memory_effects:None,svf_solved_points_to:None,
            ordered_nodes:vec![("rust::main::bb0".into(),n0)],icfg_edges:vec![],
            rust_functions:Default::default(),rust_calls:vec![],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),
                crate_correlations:vec![],package_scope:vec![],body_statuses:vec![],exact_call_bindings:vec![],
                icfg_callsite_links:vec![],icfg_stitching_v1:None,
                context_execution_graph_v1:Some(serde_json::json!({
                    "capability":"context_explicit_icfg_v4",
                    "p2_control_flow_complete":true,
                    "dependency_context_policy":"entry_exact_then_dependency_internal_call_string_k1",
                    "dependency_context_k":1,
                    "recursive_context_policy":"scc_local_call_string_k1",
                    "recursive_context_k":1,
                    "states":[],"represented_call_activations":[],"opaque_higher_order_summaries":[]
                })),
            }),
        };
        // The schema-v2 exporter deliberately rejects artifacts whose
        // quantifier/program-variable domain is empty.  This test exercises
        // the context capability projection, so construct the smallest valid
        // abstract state instead of bypassing that exporter invariant.
        let mut state=AbstractState::default();
        let mut memory=AbstractMemory::default();
        memory.set_cell_value(&"Local(_1)".to_string(), CellValue::TOP);
        state.insert("rust::main::bb0".into(), memory);
        let identity=AllocationIdentityState::default();
        let path=std::env::temp_dir().join(format!("crema-cqpl-context-v4-{}.json",std::process::id()));
        export_cqpl_annotated_icfg_with_identity(&g,&state,&identity,"rust::main::bb0",2,&path).unwrap();
        let value:serde_json::Value=serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        let _=std::fs::remove_file(path);
        let caps=value["capabilities"].as_array().unwrap();
        let has=|name:&str|caps.iter().any(|c|c.as_str()==Some(name));
        assert!(has("context_explicit_icfg_v1"));
        assert!(has("context_explicit_icfg_v4"));
        assert!(has("dependency_region_local_k1_v1"));
        assert!(has("recursive_scc_local_k1_v1"));
        assert!(!has("context_explicit_icfg_v3"),"v4 must not masquerade as exact-outer-context v3");
    }

    #[test]
    fn exporter_rejects_dangling_canonical_edge() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![("rust::main::bb0".into(), n0)],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::terminate")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let s = AbstractState::default();
        let path = std::env::temp_dir().join(format!("crema-cqpl-dangling-export-{}.json", std::process::id()));
        let err = export_cqpl_annotated_icfg(&g, &s, "rust::main::bb0", &path).unwrap_err();
        let _ = std::fs::remove_file(path);
        assert!(err.to_string().contains("not closed over the node domain"));
    }

    #[test]
    fn exporter_preserves_explicit_terminal_successor() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let terminal = GlobalICFGNode::Terminal(TerminalNode { reason: "unwind_terminate".into() });
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".into(), n0),
                ("rust::main::terminate".into(), terminal),
            ],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::terminate")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        // The exporter intentionally requires a non-empty program-variable
        // domain.  Seed one unrelated tracked variable so this regression test
        // exercises only the canonical terminal-edge property instead of
        // violating the exporter's quantifier-domain precondition.
        let mut s = AbstractState::default();
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&"Local(_1)".to_string(), CellValue::TOP);
        s.insert("rust::main::bb0".into(), mem);

        let path = std::env::temp_dir().join(format!("crema-cqpl-terminal-export-{}.json", std::process::id()));
        export_cqpl_annotated_icfg(&g, &s, "rust::main::bb0", &path).unwrap();
        let value: serde_json::Value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(path);
        let main = value["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "rust::main::bb0").unwrap();
        let terminal = value["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "rust::main::terminate").unwrap();
        assert_eq!(main["successors"][0], "rust::main::terminate");
        assert_eq!(terminal["successors"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn exporter_validates_and_preserves_canonical_internal_rust_edges() {
        let call_site = "rust::main::bb0";
        let dummy_call_id = "dummyCall::rust::main::bb0::rust::main::bb0_internal";
        let dummy_ret_id = "dummyRet::rust::main::0::rust::main::bb0_internal";
        let callee_entry = "rust::callee::bb0";
        let callee_return = "rust::callee::bb1";
        let caller_return = "rust::main::bb1";

        let main_call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: "call callee".into(),
                source_info: "x".into(),
                function_called: "callee".into(),
                callee_def_path: Some("callee".into()),
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: true,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: Vec::<MirCallArgument>::new(),
                return_place: "_0".into(),
                return_target: Some("bb1".into()),
                unwind_target: "unreachable".into(),
            }),
        });
        let main_ret = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: Some(MirTerminator::Return {
                details: "return".into(),
                source_info: "x".into(),
            }),
        });
        let dummy_call = GlobalICFGNode::DummyCall(DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: call_site.into(),
            outgoing_edge: callee_entry.into(),
            id: "dc".into(),
            mir_var: None,
            llvm_var: None,
            argument_bindings: Vec::new(),
            is_internal: Some(true),
        });
        let dummy_ret = GlobalICFGNode::DummyRet(DummyNode {
            dummy_node_name: "dummyRet".into(),
            incoming_edge: callee_return.into(),
            outgoing_edge: caller_return.into(),
            id: "dr".into(),
            mir_var: Some("_0".into()),
            llvm_var: Some("Local _0".into()),
            argument_bindings: Vec::new(),
            is_internal: Some(true),
        });
        let callee_0 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Goto {
                details: "goto".into(),
                source_info: "x".into(),
                target: "bb1".into(),
            }),
        });
        let callee_1 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: Some(MirTerminator::Return {
                details: "return".into(),
                source_info: "x".into(),
            }),
        });

        let mut rust_functions = BTreeMap::new();
        rust_functions.insert(
            "callee".into(),
            RustFunctionMetadata {
                name: "callee".into(),
                arg_count: 0,
                entry_node: callee_entry.into(),
                return_nodes: vec![callee_return.into()],
            },
        );
        let rust_calls = vec![RustCallMetadata {
            caller_function: "main".into(),
            call_node: call_site.into(),
            callee_function: "callee".into(),
            dummy_call_node: dummy_call_id.into(),
            dummy_ret_node: dummy_ret_id.into(),
            arguments: vec![],
            return_place: "_0".into(),
            return_node: caller_return.into(),
            is_closure: false,
        }];

        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                (call_site.into(), main_call),
                (dummy_call_id.into(), dummy_call),
                (dummy_ret_id.into(), dummy_ret),
                (caller_return.into(), main_ret),
                (callee_entry.into(), callee_0),
                (callee_return.into(), callee_1),
            ],
            icfg_edges: vec![
                edge(call_site, dummy_call_id),
                edge(dummy_call_id, callee_entry),
                edge(callee_entry, callee_return),
                edge(callee_return, dummy_ret_id),
                edge(dummy_ret_id, caller_return),
            ],
            rust_functions,
            rust_calls,

            dependency_body_ingestion_v1: None,};

        let state = AbstractState::default();
        let path = std::env::temp_dir().join(format!(
            "crema-cqpl-canonical-call-export-{}.json",
            std::process::id()
        ));
        export_cqpl_annotated_icfg(&g, &state, call_site, &path).unwrap();
        let value: serde_json::Value = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        let _ = std::fs::remove_file(path);

        let nodes = value["nodes"].as_array().unwrap();
        let successors_of = |id: &str| -> Vec<String> {
            nodes
                .iter()
                .find(|n| n["id"].as_str() == Some(id))
                .unwrap()["successors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(successors_of(call_site), vec![dummy_call_id.to_string()]);
        assert_eq!(successors_of(dummy_call_id), vec![callee_entry.to_string()]);
        assert_eq!(successors_of(callee_return), vec![dummy_ret_id.to_string()]);
        assert_eq!(successors_of(dummy_ret_id), vec![caller_return.to_string()]);
    }

    #[test]
    fn exporter_rejects_hidden_internal_call_relation() {
        let mut g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None })),
                ("dummyCall::x".into(), GlobalICFGNode::DummyCall(DummyNode {
                    dummy_node_name: "dummyCall".into(), incoming_edge: "rust::main::bb0".into(),
                    outgoing_edge: "rust::callee::bb0".into(), id: "dc".into(), mir_var: None, llvm_var: None,
                    argument_bindings: Vec::new(),
                    is_internal: Some(true),
                })),
                ("dummyRet::x".into(), GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name: "dummyRet".into(), incoming_edge: "rust::callee::bb1".into(),
                    outgoing_edge: "rust::main::bb1".into(), id: "dr".into(), mir_var: None, llvm_var: None,
                    argument_bindings: Vec::new(),
                    is_internal: Some(true),
                })),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: None })),
                ("rust::callee::bb0".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None })),
                ("rust::callee::bb1".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: Some(MirTerminator::Return { details: "return".into(), source_info: "x".into() }) })),
            ],
            icfg_edges: vec![edge("rust::main::bb0", "dummyCall::x")],
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        g.rust_functions.insert("callee".into(), RustFunctionMetadata {
            name: "callee".into(), arg_count: 0, entry_node: "rust::callee::bb0".into(),
            return_nodes: vec!["rust::callee::bb1".into()],
        });
        g.rust_calls.push(RustCallMetadata {
            caller_function: "main".into(), call_node: "rust::main::bb0".into(), callee_function: "callee".into(),
            dummy_call_node: "dummyCall::x".into(), dummy_ret_node: "dummyRet::x".into(), arguments: vec![],
            return_place: "_0".into(), return_node: "rust::main::bb1".into(), is_closure: false,
        });
        let path = std::env::temp_dir().join(format!("crema-cqpl-hidden-edge-{}.json", std::process::id()));
        let err = export_cqpl_annotated_icfg(&g, &AbstractState::default(), "rust::main::bb0", &path).unwrap_err();
        let _ = std::fs::remove_file(path);
        assert!(err.to_string().contains("missing activation edge"));
    }

    #[test]
    fn closure_event_aliases_connect_repeated_reads_of_the_same_capture_field() {
        let mk_stmt = |place: &str, rvalue: &str| MirStatement {
            resource_place_relation_v1: None,
            source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
            kind: "Assign".into(),
            details: format!("{place} = {rvalue}"),
            place: Some(place.into()),
            is_mutable: Some(false),
            rvalue: Some(rvalue.into()),
        };

        let c0 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![
                mk_stmt("_8", "deref_copy ((*_1).0: &*mut i32)"),
                mk_stmt("_4", "copy (*_8)"),
            ],
            terminator: None,
        });
        let c1 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_3)".into(),
                source_info: "x".into(),
                return_target: "bb2".into(),
                unwind_target: "unreachable".into(),
                dropped_value: "_3".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });
        let c2 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 2,
            statements: vec![
                mk_stmt("_9", "deref_copy ((*_1).0: &*mut i32)"),
                mk_stmt("_7", "copy (*_9)"),
            ],
            terminator: None,
        });
        let c3 = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 3,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_6)".into(),
                source_info: "x".into(),
                return_target: "bb4".into(),
                unwind_target: "unreachable".into(),
                dropped_value: "_6".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });

        let scope = "rust::main::{closure#0}";
        let n0 = format!("{scope}::bb0");
        let n1 = format!("{scope}::bb1");
        let n2 = format!("{scope}::bb2");
        let n3 = format!("{scope}::bb3");
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![
                (n0.clone(), c0),
                (n1.clone(), c1.clone()),
                (n2.clone(), c2),
                (n3.clone(), c3.clone()),
            ],
            icfg_edges: vec![edge(&n0, &n1), edge(&n1, &n2), edge(&n2, &n3)],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};

        let mut state = AbstractState::default();
        let mut first = AbstractMemory::default();
        first.state.insert(
            Allocation {
                set: ["Local(_3)".to_string(), "Local(_4)".to_string()]
                    .into_iter()
                    .collect(),
            },
            CellValue::FREED,
        );
        state.insert(n1.clone(), first);

        let mut second = AbstractMemory::default();
        second.state.insert(
            Allocation {
                set: ["Local(_6)".to_string(), "Local(_7)".to_string()]
                    .into_iter()
                    .collect(),
            },
            CellValue::FREED,
        );
        state.insert(n3.clone(), second);

        let aliases = build_closure_event_aliases(&g, &state);
        let group = aliases
            .get(&(scope.to_string(), "Local(_3)".to_string()))
            .expect("first owner must be closure-event equivalent");
        assert!(group.contains("Local(_4)"));
        assert!(group.contains("Local(_6)"));
        assert!(group.contains("Local(_7)"));
        assert!(!group.contains("Local(_8)"));
        assert!(!group.contains("Local(_9)"));

        let mut first_labels = labels_for_node(
            &n1,
            &c1,
            &LlvmNameResolver::default(),
            &HashSet::new(),
        );
        expand_closure_event_labels(&n1, &mut first_labels, &aliases);
        assert!(first_labels.iter().any(|l| {
            l.predicate == "drop" && l.variable == "Local(_6)"
        }));

        let mut second_labels = labels_for_node(
            &n3,
            &c3,
            &LlvmNameResolver::default(),
            &HashSet::new(),
        );
        expand_closure_event_labels(&n3, &mut second_labels, &aliases);
        assert!(second_labels.iter().any(|l| {
            l.predicate == "drop" && l.variable == "Local(_3)"
        }));
    }


    #[test]
    fn v6s_raw_pointer_mem_drop_is_not_exported_as_deallocation_label() {
        let call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 42,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: "_0 = core::mem::drop::<opaque>(copy _1)".into(),
                source_info: "<v6s-raw-drop-test>".into(),
                function_called: "core::mem::drop::<opaque>".into(),
                callee_def_path: Some("core::mem::drop".into()),
                deallocator_evidence: None,
                allocation_disposition_evidence: Some(RustAllocationDispositionEvidence {
                    kind: RustAllocationDispositionEvidenceKind::MemDropRawPointer,
                    callee_def_path: "core::mem::drop".into(),
                    owner_def_path: None,
                }),
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![MirCallArgument {
                    arg: "Local(_1)".into(),
                    is_mutable: Some(false),
                }],
                return_place: "_0".into(),
                return_target: Some("bb43".into()),
                unwind_target: "continue".into(),
            }),
        });

        let labels = labels_for_node(
            "rust::main::bb42",
            &call,
            &LlvmNameResolver::default(),
            &HashSet::new(),
        );
        assert!(!labels.iter().any(|l| l.predicate == "drop"));
    }

    #[test]
    fn cstring_from_raw_call_gets_read_summary_but_not_drop_at_call_site() {
        let call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 10,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: "_16 = std::ffi::CString::from_raw(copy _4)".into(),
                source_info: "<cqpl-test>".into(),
                function_called: "std::ffi::CString::from_raw".into(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![MirCallArgument {
                    arg: "Local(_4)".into(),
                    is_mutable: Some(false),
                }],
                return_place: "_16".into(),
                return_target: Some("bb11".into()),
                unwind_target: "continue".into(),
            }),
        });

        let labels = labels_for_node(
            "rust::main::bb10",
            &call,
            &LlvmNameResolver::default(),
            &HashSet::new(),
        );

        assert!(labels.iter().any(|l| {
            l.predicate == "read" && l.variable == "Local(_4)"
        }));
        assert!(!labels.iter().any(|l| l.predicate == "drop"));
    }

    #[test]
    fn ownership_reconstruction_is_not_generically_misclassified_as_read() {
        let call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: "_2 = std::boxed::Box::<i32>::from_raw(copy _1)".into(),
                source_info: "<cqpl-test>".into(),
                function_called: "std::boxed::Box::<i32>::from_raw".into(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: Vec::new(),
                resolved_instance_callees: Vec::new(),
                instance_dispatch_observed: false,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![MirCallArgument {
                    arg: "Local(_1)".into(),
                    is_mutable: Some(false),
                }],
                return_place: "_2".into(),
                return_target: Some("bb2".into()),
                unwind_target: "continue".into(),
            }),
        });

        let labels = labels_for_node(
            "rust::main::bb1",
            &call,
            &LlvmNameResolver::default(),
            &HashSet::new(),
        );

        assert!(!labels.iter().any(|l| l.predicate == "read"));
        assert!(!labels.iter().any(|l| l.predicate == "drop"));
    }

    #[test]
    fn closure_event_aliases_do_not_merge_distinct_capture_fields() {
        let mk_stmt = |place: &str, rvalue: &str| MirStatement {
            resource_place_relation_v1: None,
            source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
            kind: "Assign".into(),
            details: format!("{place} = {rvalue}"),
            place: Some(place.into()),
            is_mutable: Some(false),
            rvalue: Some(rvalue.into()),
        };
        let scope = "rust::main::{closure#0}";
        let n0 = format!("{scope}::bb0");
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![
                mk_stmt("_8", "deref_copy ((*_1).0: &*mut i32)"),
                mk_stmt("_4", "copy (*_8)"),
                mk_stmt("_9", "deref_copy ((*_1).1: &*mut i32)"),
                mk_stmt("_7", "copy (*_9)"),
            ],
            terminator: None,
        });
        let g = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![(n0, node)],
            icfg_edges: vec![],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let state = AbstractState::default();
        let aliases = build_closure_event_aliases(&g, &state);
        assert!(aliases
            .get(&(scope.to_string(), "Local(_4)".to_string()))
            .is_none());
        assert!(aliases
            .get(&(scope.to_string(), "Local(_7)".to_string()))
            .is_none());
    }

    #[test]
    fn age1_variable_catalog_is_closed_over_existence_guard_variables() {
        let mut variable_ids = BTreeSet::from(["rust::main::Local(_1)".to_string()]);
        let guards = vec![AllocationExistenceGuardRecord {
            allocation: "a#test".into(),
            producer_call_node: "rust::main::bb0".into(),
            producer_call_canonical_node: None,
            predicate_call_node: "rust::main::bb1".into(),
            switch_node: "rust::main::bb2".into(),
            tested_variable: "rust::main::Local(_1)".into(),
            predicate_result_variable: "rust::main::Local(_2)".into(),
            null_successor: "rust::main::bb4".into(),
            non_null_successor: "rust::main::bb3".into(),
            callee_def_path: "std::ptr::mut_ptr::<impl *mut T>::is_null".into(),
            allocation_return_basis: "svf_single_source_c_allocator_return_v1",
            basis: "rust_raw_pointer_is_null_switch_v1",
        }];

        close_variable_catalog_over_allocation_existence_guards(
            &mut variable_ids,
            Some(&guards),
        );

        assert!(variable_ids.contains("rust::main::Local(_1)"));
        assert!(variable_ids.contains("rust::main::Local(_2)"));
    }

    #[test]
    fn schema_v2_variable_catalog_is_closed_over_identity_program_vars() {
        let node_id = "rust::main::bb0".to_string();
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![(
                node_id.clone(),
                GlobalICFGNode::Mir(MirBasicBlock {
                    block_id: 0,
                    statements: vec![],
                    terminator: None,
                }),
            )],
            icfg_edges: vec![],
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};

        let alloc = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let rust_p = ProgramVarId::rust("main", "_1").unwrap();
        let rust_ref = ProgramVarId::rust("main", "_5").unwrap();
        let rust_place_base = ProgramVarId::rust("main", "_6").unwrap();
        let c_p = ProgramVarId::c("ffi_alloc", 7, Some("rust::main::bb0".into()));

        let mut mem = AllocationIdentityMemory::default();
        mem.points_to
            .insert(rust_p.clone(), BTreeSet::from([alloc.clone()]));
        mem.points_to
            .insert(c_p.clone(), BTreeSet::from([alloc.clone()]));
        mem.stack_refs.insert(
            rust_ref.clone(),
            BTreeSet::from([PlaceId {
                base: rust_place_base.clone(),
                projection: vec![],
            }]),
        );

        let mut identity_state = AllocationIdentityState::default();
        identity_state.by_node.insert(node_id.clone(), mem.clone());
        identity_state.event_by_node.insert(node_id.clone(), mem);

        let path = std::env::temp_dir().join(format!(
            "crema-v6k5-variable-domain-{}.json",
            std::process::id()
        ));
        export_cqpl_annotated_icfg_with_identity(
            &icfg,
            &AbstractState::default(),
            &identity_state,
            &node_id,
            2,
            &path,
        )
        .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let vars = json["variables"].as_array().unwrap();
        let catalog: BTreeMap<String, String> = vars
            .iter()
            .map(|v| {
                (
                    v["id"].as_str().unwrap().to_string(),
                    v["language"].as_str().unwrap().to_string(),
                )
            })
            .collect();

        assert_eq!(catalog.get(&rust_p.canonical_string()).map(String::as_str), Some("rust"));
        assert_eq!(catalog.get(&rust_ref.canonical_string()).map(String::as_str), Some("rust"));
        assert_eq!(
            catalog.get(&rust_place_base.canonical_string()).map(String::as_str),
            Some("rust")
        );
        assert_eq!(catalog.get(&c_p.canonical_string()).map(String::as_str), Some("c"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn schema_v2_export_resolves_events_from_intra_node_summary_not_only_post_state() {
        let node_id = "rust::main::bb0".to_string();
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![
                MirStatement {
                    resource_place_relation_v1: None,
                    source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
                    kind: "Assign".into(),
                    details: "Assign((_2, copy (*_1)))".into(),
                    place: Some("Local(_2) [mutable]".into()),
                    is_mutable: Some(true),
                    rvalue: Some("copy (*_1)".into()),
                },
                MirStatement {
                    resource_place_relation_v1: None,
                    source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
                    kind: "Assign".into(),
                    details: "Assign((_1, copy _4))".into(),
                    place: Some("Local(_1) [mutable]".into()),
                    is_mutable: Some(true),
                    rvalue: Some("copy _4".into()),
                },
            ],
            terminator: None,
        });
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: vec![(node_id.clone(), node)],
            icfg_edges: vec![],
            rust_functions: Default::default(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};

        let a = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let b = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "B".into() },
            Vec::new(),
        );
        let p = ProgramVarId::rust("main", "_1").unwrap();

        let mut post = AllocationIdentityMemory::default();
        post.assign_fresh(p.clone(), b.clone());
        let mut event = AllocationIdentityMemory::default();
        event.assign_points_to(p, BTreeSet::from([a.clone(), b.clone()]));

        let mut identity_state = AllocationIdentityState::default();
        identity_state.by_node.insert(node_id.clone(), post);
        identity_state.event_by_node.insert(node_id.clone(), event);

        let path = std::env::temp_dir().join(format!(
            "crema-v6g-event-summary-{}.json",
            std::process::id()
        ));
        export_cqpl_annotated_icfg_with_identity(
            &icfg,
            &AbstractState::default(),
            &identity_state,
            &node_id,
            2,
            &path,
        )
        .unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let labels = json["nodes"][0]["allocation_labels"].as_array().unwrap();
        let read_allocs: BTreeSet<String> = labels
            .iter()
            .filter(|label| label["predicate"].as_str() == Some("read"))
            .map(|label| label["allocation"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            read_allocs,
            BTreeSet::from([stable_allocation_id(&a), stable_allocation_id(&b)])
        );
        assert!(json["nodes"][0]["event_identity"].is_object());

        let vars = json["variables"].as_array().unwrap();
        assert!(vars.iter().any(|v| {
            v["id"].as_str() == Some("rust::main::Local(_1)")
                && v["language"].as_str() == Some("rust")
        }));
        assert_eq!(language_of("c::malloc::svf(7)@rust::main::bb0"), "c");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ht2_r2p2_uncertified_mir_drop_is_not_an_allocation_deallocation_event() {
        let scope = "main::{closure#0}";
        let node_id = format!("rust::{scope}::bb3");
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 3,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_6)".into(),
                source_info: "x".into(),
                return_target: "bb4".into(),
                unwind_target: "unreachable".into(),
                dropped_value: "_6".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });

        let a = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust(scope, "_6").expect("scoped MIR local"),
            a.clone(),
        );

        let raw = vec![EventLabel { predicate: "drop", variable: "Local(_6)".into() }];
        let labels = allocation_labels_for_node(&node_id, &node, &raw, &identity, &HashSet::new(), 2);
        assert!(
            labels.is_empty(),
            "language-level wrapper Drop without producer-certified deallocation must not fabricate drop_l(A)"
        );
    }


    #[test]
    fn c_malloc_allocation_contract_is_structural_c_malloc_family() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "llvm::alloc::node1::rust::main::bb0".into(),
                allocator: "malloc".into(),
            },
            Vec::new(),
        );
        let contract = allocation_contract(&allocation);
        assert_eq!(contract.family, "c_malloc");
        assert_eq!(contract.operation, "malloc");
        assert_eq!(contract.language, "c");
    }

    #[test]
    fn strdup_allocation_contract_is_structural_c_malloc_family() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb0".into(),
                allocator: "strdup".into(),
            },
            Vec::new(),
        );
        let contract = allocation_contract(&allocation);
        assert_eq!(contract.family, "c_malloc");
        assert_eq!(contract.operation, "strdup");
        assert_eq!(contract.language, "c");
    }

    #[test]
    fn rn1_realloc_null_allocation_contract_is_c_malloc_family() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb1".into(),
                allocator: "realloc".into(),
            },
            Vec::new(),
        );
        let contract = allocation_contract(&allocation);
        assert_eq!(contract.family, "c_malloc");
        assert_eq!(contract.operation, "realloc");
        assert_eq!(contract.language, "c");
    }

    #[test]
    fn rn1_realloc_null_age1_return_basis_reuses_bodyless_malloc_contract() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::CCall {
                node_id: "rust::main::bb1".into(),
                allocator: "realloc".into(),
            },
            Vec::new(),
        );
        let icfg = GlobalICFGOrdered {
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            ordered_nodes: Vec::new(),
            icfg_edges: Vec::new(),
            rust_functions: BTreeMap::new(),
            rust_calls: Vec::new(),

            dependency_body_ingestion_v1: None,};
        let tested = ProgramVarId::Rust { function: "main".into(), local: 1 };
        assert_eq!(
            c_malloc_origin_return_basis(&icfg, &allocation, "rust::main::bb1", &tested),
            Some("rust_foreign_decl_c_malloc_contract_v1")
        );
        assert_eq!(
            c_malloc_origin_return_basis(&icfg, &allocation, "rust::main::bb9", &tested),
            None,
            "represented/non-matching realloc sites must not inherit RN1 AGE1 proof"
        );
    }

    #[test]
    fn generic_mir_drop_contract_is_unknown_not_rust_global() {
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_6)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb5".into(),
                unwind_target: "continue".into(),
                dropped_value: "_6".into(),
                is_mutable: false,
                deallocator_evidence: None,
            }),
        });
        let contract = deallocator_contract(&node, &HashSet::new());
        assert_eq!(contract.family, "unknown");
        assert_eq!(contract.operation, "drop");
        assert_eq!(contract.language, "rust");
        assert_eq!(contract.basis, Some("unresolved"));
        assert!(contract.owner_def_path.is_none());
        assert!(contract.allocator_def_path.is_none());
    }

    #[test]
    fn v6n_typed_box_global_drop_contract_is_proof_carrying() {
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_1)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb5".into(),
                unwind_target: "continue".into(),
                dropped_value: "_1".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::BoxGlobal,
                    owner_def_path: "alloc::boxed::Box".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let contract = deallocator_contract(&node, &HashSet::new());
        assert_eq!(contract.family, "rust_global");
        assert_eq!(contract.operation, "drop");
        assert_eq!(contract.language, "rust");
        assert_eq!(contract.basis, Some("rust_box_global_drop"));
        assert_eq!(contract.owner_def_path.as_deref(), Some("alloc::boxed::Box"));
        assert_eq!(contract.allocator_def_path.as_deref(), Some("alloc::alloc::Global"));
    }

    #[test]
    fn b1_1_r1_typed_cstring_global_drop_contract_is_proof_carrying() {
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_1)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb5".into(),
                unwind_target: "continue".into(),
                dropped_value: "_1".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::CStringGlobal,
                    owner_def_path: "alloc::ffi::c_str::CString".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let contract = deallocator_contract(&node, &HashSet::new());
        assert_eq!(contract.family, "rust_global");
        assert_eq!(contract.operation, "drop");
        assert_eq!(contract.language, "rust");
        assert_eq!(contract.basis, Some("rust_cstring_global_drop"));
        assert_eq!(contract.owner_def_path.as_deref(), Some("alloc::ffi::c_str::CString"));
        assert_eq!(contract.allocator_def_path.as_deref(), Some("alloc::alloc::Global"));
    }

    #[test]
    fn ht2_r2_string_from_str_allocation_contract_is_rust_global_may_site() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::RustCall {
                node_id: "rust::main::bb1".into(),
                callee: "<std::string::String as std::convert::From<&str>>::from".into(),
            },
            vec![],
        );
        let contract = allocation_contract(&allocation);
        assert_eq!(contract.family, "rust_global");
        assert_eq!(contract.operation, "string_allocation");
        assert_eq!(contract.language, "rust");
    }

    #[test]
    fn ht2_r2p4_string_owner_drop_is_semantic_deallocation_boundary() {
        let scope = "main";
        let node_id = "rust::main::bb4";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_1)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb5".into(),
                unwind_target: "continue".into(),
                dropped_value: "_1".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::StringGlobal,
                    owner_def_path: "alloc::string::String".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let a = AbstractAllocId::new(
            AllocationSiteId::RustCall {
                node_id: "rust::main::bb1".into(),
                callee: "<std::string::String as std::convert::From<&str>>::from".into(),
            },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust(scope, "_1").expect("MIR local"),
            a.clone(),
        );
        let raw = vec![EventLabel { predicate: "drop", variable: "Local(_1)".into() }];
        let labels = allocation_labels_for_node(
            node_id, &node, &raw, &identity, &HashSet::new(), 2,
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].allocation, stable_allocation_id(&a));
        assert_eq!(
            labels[0].deallocator_contract.as_ref().and_then(|c| c.basis),
            Some("rust_string_global_drop"),
        );
    }

    #[test]
    fn ht2_r2p4_nested_vec_drop_is_coalesced_for_string_origin_resource() {
        let scope = "main";
        let node_id = "rust::main::bb6";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 6,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_2)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb7".into(),
                unwind_target: "continue".into(),
                dropped_value: "_2".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::VecGlobal,
                    owner_def_path: "alloc::vec::Vec".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let a = AbstractAllocId::new(
            AllocationSiteId::RustCall {
                node_id: "rust::main::bb1".into(),
                callee: "<std::string::String as std::convert::From<&str>>::from".into(),
            },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust(scope, "_2").expect("MIR local"),
            a,
        );
        let raw = vec![EventLabel { predicate: "drop", variable: "Local(_2)".into() }];
        let labels = allocation_labels_for_node(
            node_id, &node, &raw, &identity, &HashSet::new(), 2,
        );
        assert!(labels.is_empty(), "Vec implementation drop must not double-count a String owner-boundary free");
    }

    #[test]
    fn ht2_r2p2_certified_vec_drop_remains_allocation_deallocation_event() {
        let scope = "main";
        let node_id = "rust::main::bb6";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 6,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_2)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb7".into(),
                unwind_target: "continue".into(),
                dropped_value: "_2".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::VecGlobal,
                    owner_def_path: "alloc::vec::Vec".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let a = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "vec-buffer".into() },
            Vec::new(),
        );
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(
            ProgramVarId::rust(scope, "_2").expect("MIR local"),
            a.clone(),
        );
        let raw = vec![EventLabel { predicate: "drop", variable: "Local(_2)".into() }];
        let labels = allocation_labels_for_node(
            node_id, &node, &raw, &identity, &HashSet::new(), 2,
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].predicate, "drop");
        assert_eq!(labels[0].allocation, stable_allocation_id(&a));
        assert_eq!(
            labels[0].deallocator_contract.as_ref().and_then(|c| c.basis.as_deref()),
            Some("rust_vec_global_drop")
        );
    }

    #[test]
    fn v6n_typed_vec_global_drop_contract_is_proof_carrying() {
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::Drop {
                details: "drop(_1)".into(),
                source_info: "<cqpl-test>".into(),
                return_target: "bb5".into(),
                unwind_target: "continue".into(),
                dropped_value: "_1".into(),
                is_mutable: false,
                deallocator_evidence: Some(RustDropAllocatorEvidence {
                    kind: RustDropAllocatorEvidenceKind::VecGlobal,
                    owner_def_path: "alloc::vec::Vec".into(),
                    allocator_def_path: "alloc::alloc::Global".into(),
                }),
            }),
        });
        let contract = deallocator_contract(&node, &HashSet::new());
        assert_eq!(contract.family, "rust_global");
        assert_eq!(contract.operation, "drop");
        assert_eq!(contract.language, "rust");
        assert_eq!(contract.basis, Some("rust_vec_global_drop"));
    }

    #[test]
    fn v6n_r1a_global_dealloc_requires_producer_evidence_not_pretty_or_canonical_text() {
        fn call(with_evidence: bool) -> GlobalICFGNode {
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 5,
                statements: vec![],
                terminator: Some(MirTerminator::Call {
                    details: "std::alloc::dealloc(copy _1, copy _2)".into(),
                    source_info: "<cqpl-test>".into(),
                    function_called: "std::alloc::dealloc".into(),
                    // A canonical-looking path alone is diagnostic and must not
                    // authorize a family refinement in the exporter.
                    callee_def_path: Some("alloc::alloc::dealloc".into()),
                    deallocator_evidence: with_evidence.then(|| RustCallDeallocatorEvidence {
                        kind: RustCallDeallocatorEvidenceKind::GlobalDeallocApi,
                        callee_def_path: "alloc::alloc::dealloc".into(),
                        owner_def_path: None,
                        allocator_def_path: None,
                    }),
                    allocation_disposition_evidence: None,
                    atomic_ptr_evidence: None,
                    higher_order_evidence: None,
                    callee_is_local: false,
                    callback_def_paths: Vec::new(),
                    resolved_instance_callees: Vec::new(),
                    instance_dispatch_observed: false,
                    instance_dispatch_external: false,
                    instance_dispatch_unresolved: false,
                    arguments: Vec::new(),
                    return_place: "_0".into(),
                    return_target: Some("bb6".into()),
                    unwind_target: "continue".into(),
                }),
            })
        }
        let proven = deallocator_contract(&call(true), &HashSet::new());
        assert_eq!(proven.family, "rust_global");
        assert_eq!(proven.basis, Some("rust_global_dealloc_api"));
        assert_eq!(proven.callee_def_path.as_deref(), Some("alloc::alloc::dealloc"));

        let text_only = deallocator_contract(&call(false), &HashSet::new());
        assert_eq!(text_only.family, "unknown");
        assert_eq!(text_only.basis, Some("unresolved"));
        assert!(text_only.callee_def_path.is_none());
    }

    #[test]
    fn bcontract_drop1_mem_drop_owned_box_requires_producer_evidence() {
        fn call(with_evidence: bool) -> GlobalICFGNode {
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 17,
                statements: vec![],
                terminator: Some(MirTerminator::Call {
                    details: "core::mem::drop::<std::boxed::Box<i32>>(move _1)".into(),
                    source_info: "<drop1-test>".into(),
                    function_called: "core::mem::drop::<std::boxed::Box<i32>>".into(),
                    callee_def_path: Some("core::mem::drop".into()),
                    deallocator_evidence: with_evidence.then(|| RustCallDeallocatorEvidence {
                        kind: RustCallDeallocatorEvidenceKind::MemDropOwnedBoxGlobal,
                        callee_def_path: "core::mem::drop".into(),
                        owner_def_path: Some("alloc::boxed::Box".into()),
                        allocator_def_path: Some("alloc::alloc::Global".into()),
                    }),
                    allocation_disposition_evidence: None,
                    atomic_ptr_evidence: None,
                    higher_order_evidence: None,
                    callee_is_local: false,
                    callback_def_paths: Vec::new(),
                    resolved_instance_callees: Vec::new(),
                    instance_dispatch_observed: false,
                    instance_dispatch_external: false,
                    instance_dispatch_unresolved: false,
                    arguments: vec![MirCallArgument {
                        arg: "Local(_1)".into(),
                        is_mutable: Some(false),
                    }],
                    return_place: "_0".into(),
                    return_target: Some("bb18".into()),
                    unwind_target: "continue".into(),
                }),
            })
        }

        let proven = deallocator_contract(&call(true), &HashSet::new());
        assert_eq!(proven.family, "rust_global");
        assert_eq!(proven.operation, "drop");
        assert_eq!(proven.language, "rust");
        assert_eq!(proven.basis, Some("rust_mem_drop_owned_box_global_v1"));
        assert_eq!(proven.callee_def_path.as_deref(), Some("core::mem::drop"));
        assert_eq!(proven.owner_def_path.as_deref(), Some("alloc::boxed::Box"));
        assert_eq!(proven.allocator_def_path.as_deref(), Some("alloc::alloc::Global"));

        // Pretty/canonical text alone is not proof.
        let text_only = deallocator_contract(&call(false), &HashSet::new());
        assert_eq!(text_only.family, "unknown");
        assert_eq!(text_only.basis, Some("unresolved"));
        assert!(text_only.callee_def_path.is_none());
    }

    #[test]
    fn schema_v2_attaches_c_free_contract_to_rust_allocation_drop() {
        let allocation = AbstractAllocId::new(
            AllocationSiteId::RustCall {
                node_id: "rust::main::bb0".into(),
                callee: "std::boxed::Box::<i32>::new".into(),
            },
            Vec::new(),
        );
        let var = ProgramVarId::c("free_wrapper", 7, Some("rust::main::bb2".into()));
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_points_to(var, BTreeSet::from([allocation.clone()]));
        let node_id = "llvm::free_wrapper::node4::rust::main::bb2";
        let node = GlobalICFGNode::Llvm(crate::structs::LlvmJsonNode {
            node_id: 4, node_type: false, info: "call void @free(ptr %7)".into(),
            node_kind_string: "FunCallBlock".into(), node_kind: 0, node_source_loc: String::new(),
            function_name: Some("free_wrapper".into()), basic_block: None, basic_block_name: None,
            basic_block_info: None, svf_statements: vec![], incoming_edges: vec![], outgoing_edges: vec![],
        });
        let labels = vec![EventLabel { predicate: "drop", variable: "7@rust::main::bb2".into() }];
        let out = allocation_labels_for_node(node_id, &node, &labels, &identity, &HashSet::new(), 2);
        assert!(out.iter().any(|l|
            l.predicate == "drop"
                && l.allocation == stable_allocation_id(&allocation)
                && l.deallocator_contract.as_ref().is_some_and(|c| c.family == "c_malloc" && c.operation == "free" && c.language == "c" && c.basis == Some("structural_c_free_v1"))
        ));
        assert_eq!(allocation_contract(&allocation).family, "rust_global");
    }

    #[test]
    fn v6m_allocation_post_lifts_program_state_through_post_identity() {
        let node_id = "rust::main::bb0";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: None,
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let var = ProgramVarId::rust("main", "_1").unwrap();
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(var, allocation.clone());

        let mut post = AbstractMemory::default();
        post.state.insert(
            Allocation { set: BTreeSet::from(["Local(_1)".to_string()]) },
            CellValue::ALLOC,
        );
        post.state.insert(
            Allocation { set: BTreeSet::from(["Leak(Local(_1))".to_string()]) },
            CellValue::MV,
        );

        let lifted = allocation_memory_annotation(node_id, &node, &post, &identity);
        assert_eq!(lifted.cells.len(), 1);
        assert_eq!(lifted.cells[0].allocation, stable_allocation_id(&allocation));
        // Existing lattice law: ALLOC <= MV, therefore their join is MV.
        assert_eq!(lifted.cells[0].value, "MV");
    }

    #[test]
    fn v6m_allocation_post_does_not_lift_stack_reference_top_into_pointee_state() {
        let node_id = "rust::main::bb1";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: None,
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let owner = ProgramVarId::rust("main", "_1").unwrap();
        let reference = ProgramVarId::rust("main", "_8").unwrap();
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_fresh(owner.clone(), allocation.clone());
        identity.assign_stack_refs(
            reference,
            BTreeSet::from([PlaceId {
                base: owner,
                projection: Vec::new(),
            }]),
        );

        let mut post = AbstractMemory::default();
        post.state.insert(
            Allocation { set: BTreeSet::from(["Local(_1)".to_string()]) },
            CellValue::ALLOC,
        );
        post.state.insert(
            Allocation { set: BTreeSet::from(["Local(_8)".to_string()]) },
            CellValue::TOP,
        );

        let lifted = allocation_memory_annotation(node_id, &node, &post, &identity);
        assert_eq!(lifted.cells.len(), 1);
        assert_eq!(lifted.cells[0].allocation, stable_allocation_id(&allocation));
        assert_eq!(lifted.cells[0].value, "ALLOC");
    }

    #[test]
    fn v6m_allocation_post_joins_incompatible_alias_states_to_top() {
        let node_id = "rust::main::bb0";
        let node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: None,
        });
        let allocation = AbstractAllocId::new(
            AllocationSiteId::Synthetic { scope: "test".into(), label: "A".into() },
            Vec::new(),
        );
        let p = ProgramVarId::rust("main", "_1").unwrap();
        let q = ProgramVarId::rust("main", "_2").unwrap();
        let mut identity = AllocationIdentityMemory::default();
        identity.assign_points_to(p, BTreeSet::from([allocation.clone()]));
        identity.assign_points_to(q, BTreeSet::from([allocation.clone()]));

        let mut post = AbstractMemory::default();
        post.state.insert(
            Allocation { set: BTreeSet::from(["Local(_1)".to_string()]) },
            CellValue::ALLOC,
        );
        post.state.insert(
            Allocation { set: BTreeSet::from(["Local(_2)".to_string()]) },
            CellValue::FREED,
        );

        let lifted = allocation_memory_annotation(node_id, &node, &post, &identity);
        assert_eq!(lifted.cells.len(), 1);
        assert_eq!(lifted.cells[0].value, "TOP");
    }

    #[test]
    fn bcontract_nd1_leaf_body_certifies_no_deallocation() {
        let node = crate::structs::LlvmJsonNode {
            node_id: 1,
            node_type: false,
            info: "IntraBlock".into(),
            node_kind_string: "IntraBlock".into(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: Some("touch_second".into()),
            basic_block: None,
            basic_block_name: Some("entry".into()),
            basic_block_info: Some("entry".into()),
            svf_statements: vec![],
            incoming_edges: vec![],
            outgoing_edges: vec![],
        };
        assert_eq!(
            classify_external_deallocation_effect(&[node], None, ""),
            ("certified_absent", "svf_leaf_no_call_deallocation_v1")
        );
    }

    #[test]
    fn bcontract_nd1_direct_free_is_positive_may_deallocation() {
        let node = crate::structs::LlvmJsonNode {
            node_id: 2,
            node_type: false,
            info: "call void @free(ptr %p)".into(),
            node_kind_string: "FunCallBlock".into(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: Some("free_second".into()),
            basic_block: None,
            basic_block_name: Some("entry".into()),
            basic_block_info: Some("entry".into()),
            svf_statements: vec![],
            incoming_edges: vec![],
            outgoing_edges: vec![],
        };
        assert_eq!(
            classify_external_deallocation_effect(&[node], None, ""),
            ("observed_may_deallocate", "structural_c_free_v1")
        );
    }

    #[test]
    fn bcontract_nd1_other_call_remains_unresolved() {
        let node = crate::structs::LlvmJsonNode {
            node_id: 3,
            node_type: false,
            info: "call void @helper(ptr %p)".into(),
            node_kind_string: "FunCallBlock".into(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: Some("wrapper".into()),
            basic_block: None,
            basic_block_name: Some("entry".into()),
            basic_block_info: Some("entry".into()),
            svf_statements: vec![],
            incoming_edges: vec![],
            outgoing_edges: vec![],
        };
        assert_eq!(
            classify_external_deallocation_effect(&[node], None, ""),
            ("unresolved", "svf_call_effect_unresolved_v1")
        );
    }

    fn efx1_test_artifact(
        name: &str,
        explicit: LlvmFunctionEffectsSnapshotV1,
        inferred: LlvmFunctionEffectsSnapshotV1,
        tli_recognized: bool,
        tli_changed: bool,
    ) -> LlvmMemoryEffectsArtifactV1 {
        LlvmMemoryEffectsArtifactV1 {
            schema: "llvm_memory_effects_v1".into(),
            llvm_version: "16.0.4".into(),
            explicit_basis: "llvm16_explicit_input_ir_v1".into(),
            tli_basis: "llvm16_tli_libfunc_attrs_v1".into(),
            modules: vec![crate::structs::LlvmEffectsModuleV1 {
                input: "fixture.ll".into(),
                target_triple: "x86_64-pc-linux-gnu".into(),
                input_ir_verified: true,
                tli_clone_verified: true,
                functions: vec![LlvmFunctionEffectsRecordV1 {
                    name: name.into(),
                    is_declaration: true,
                    origin_explicit: "explicit_input_ir".into(),
                    explicit,
                    tli_recognized,
                    tli_libfunc: tli_recognized.then(|| name.into()),
                    origin_inferred: "llvm_tli_inferred".into(),
                    tli_inferred: inferred,
                    tli_changed,
                }],
                callsites_explicit: vec![],
                callsites_tli_inferred: vec![],
            }],
        }
    }

    #[test]
    fn efx1_tli_free_declaration_is_positive_may_deallocation() {
        let explicit = LlvmFunctionEffectsSnapshotV1::default();
        let mut inferred = LlvmFunctionEffectsSnapshotV1::default();
        inferred.alloc_kind = vec!["free".into()];
        inferred.formals = vec![crate::structs::LlvmFormalEffectsSnapshotV1 {
            index: 0,
            pointer_typed: true,
            allocptr: true,
            nocapture: true,
            ..Default::default()
        }];
        let artifact = efx1_test_artifact("free", explicit, inferred, true, true);
        assert_eq!(
            classify_external_deallocation_effect(&[], Some(&artifact), "free"),
            ("observed_may_deallocate", "llvm16_tli_allockind_deallocation_v1")
        );
    }


    #[test]
    fn efx1_structural_free_keeps_legacy_basis_and_adds_tli_corroboration() {
        let explicit = LlvmFunctionEffectsSnapshotV1::default();
        let mut inferred = LlvmFunctionEffectsSnapshotV1::default();
        inferred.alloc_kind = vec!["free".into()];
        inferred.formals = vec![crate::structs::LlvmFormalEffectsSnapshotV1 {
            index: 0,
            pointer_typed: true,
            allocptr: true,
            nocapture: true,
            ..Default::default()
        }];
        let mut artifact = efx1_test_artifact("free", explicit, inferred, true, true);
        artifact.modules[0].functions.push(LlvmFunctionEffectsRecordV1 {
            name: "wrapper".into(),
            is_declaration: false,
            origin_explicit: "explicit_input_ir".into(),
            explicit: LlvmFunctionEffectsSnapshotV1::default(),
            tli_recognized: false,
            tli_libfunc: None,
            origin_inferred: "llvm_tli_inferred".into(),
            tli_inferred: LlvmFunctionEffectsSnapshotV1::default(),
            tli_changed: false,
        });
        artifact.modules[0].callsites_explicit.push(
            crate::structs::LlvmCallsiteEffectsRecordV1 {
                caller: "wrapper".into(),
                ordinal: 0,
                direct: true,
                callee: Some("free".into()),
                callsite_memory_explicit: false,
                effective_memory: crate::structs::LlvmMemoryAccessV1::default(),
            },
        );

        assert_eq!(
            external_deallocation_corroborating_bases(
                Some(&artifact),
                "wrapper",
                "observed_may_deallocate",
                "structural_c_free_v1",
            ),
            vec!["llvm16_tli_direct_callee_allockind_deallocation_v1"]
        );
    }


    #[test]
    fn age1_raw_pointer_is_null_classifier_is_closed() {
        assert!(is_raw_pointer_is_null_def_path(
            "core::ptr::mut_ptr::<impl *mut u8>::is_null"
        ));
        assert!(is_raw_pointer_is_null_def_path(
            "std::ptr::const_ptr::<impl *const i32>::is_null"
        ));
        assert!(!is_raw_pointer_is_null_def_path(
            "core::option::Option::<*mut u8>::is_none"
        ));
    }

    #[test]
    fn age1_exact_single_source_svf_return_chain_is_fail_closed() {
        let exact = BTreeMap::from([
            (6usize, vec![("PhiStmt".to_string(), vec![13usize])]),
        ]);
        assert!(exact_single_source_svf_flow(
            &exact, 6, 13, &mut BTreeSet::new()
        ));

        let joined = BTreeMap::from([
            (6usize, vec![("PhiStmt".to_string(), vec![13usize, 14usize])]),
        ]);
        assert!(!exact_single_source_svf_flow(
            &joined, 6, 13, &mut BTreeSet::new()
        ));

        let selected = BTreeMap::from([
            (6usize, vec![("SelectStmt".to_string(), vec![13usize])]),
        ]);
        assert!(!exact_single_source_svf_flow(
            &selected, 6, 13, &mut BTreeSet::new()
        ));
    }


    #[test]
    fn rbf1_bodyless_realloc_classifier_is_foreign_decl_gated_and_exact() {
        let ffi = HashSet::from(["realloc".to_string()]);

        // Real rustc shape for an `extern "C"` declaration: the DefPath is
        // local to the crate even though no local MIR body exists.  Exact FFI
        // declaration membership + no internal branch is the proof boundary.
        assert!(is_bodyless_c_realloc_call(
            "b10a_realloc_branch_clean::realloc",
            "Call(_3 = realloc(copy _1, const 64_usize))",
            Some("b10a_realloc_branch_clean::realloc"),
            false,
            &ffi,
        ));

        // A real local Rust function with the same terminal name is not an
        // external bodyless realloc once the canonical ICFG has a Rust branch.
        assert!(!is_bodyless_c_realloc_call(
            "crate::realloc",
            "Call(_3 = realloc(copy _1, const 64_usize))",
            Some("crate::realloc"),
            true,
            &ffi,
        ));

        // Bare/locally-qualified names are never trusted without the extracted
        // foreign-declaration certificate.
        assert!(!is_bodyless_c_realloc_call(
            "crate::realloc",
            "Call(_3 = realloc(copy _1, const 64_usize))",
            Some("crate::realloc"),
            false,
            &HashSet::new(),
        ));

        // Explicit libc paths remain a separately recognized external family.
        assert!(is_bodyless_c_realloc_call(
            "libc::realloc",
            "Call(_3 = libc::realloc(copy _1, const 64_usize))",
            Some("libc::realloc"),
            false,
            &HashSet::new(),
        ));

        assert!(!is_bodyless_c_realloc_call(
            "reallocate",
            "Call(_3 = reallocate(copy _1, const 64_usize))",
            Some("reallocate"),
            false,
            &ffi,
        ));
    }

    #[test]
    fn rbf1_variable_catalog_is_closed_over_boundary_variables() {
        let mut variable_ids = BTreeSet::from(["rust::main::Local(_1)".to_string()]);
        let boundaries = vec![ReallocationBoundaryRecord {
            node: "rust::main::bb3".into(),
            source_allocation: "a#test".into(),
            source_variable: "rust::main::Local(_1)".into(),
            result_variable: "rust::main::Local(_3)".into(),
            family: "c_malloc",
            operation: "realloc",
            certainty: "may_abstract",
            status: "conditional_unmodeled",
            basis: "rust_foreign_decl_c_realloc_boundary_v1",
        }];

        close_variable_catalog_over_reallocation_boundaries(
            &mut variable_ids,
            Some(&boundaries),
        );

        assert!(variable_ids.contains("rust::main::Local(_1)"));
        assert!(variable_ids.contains("rust::main::Local(_3)"));
    }


    #[test]
    fn cr1_positive_nonzero_usize_classifier_is_closed() {
        assert!(positive_nonzero_usize_constant("const 64_usize"));
        assert!(positive_nonzero_usize_constant("const 1_usize"));
        assert!(!positive_nonzero_usize_constant("const 0_usize"));
        assert!(!positive_nonzero_usize_constant("const 64_u64"));
        assert!(!positive_nonzero_usize_constant("Local(_2)"));
        assert!(!positive_nonzero_usize_constant("const -1_isize"));
    }

    #[test]
    fn efm2_zero_usize_classifier_is_exact_and_closed() {
        assert!(definitely_zero_usize_constant("const 0_usize"));
        assert!(!definitely_zero_usize_constant("const 1_usize"));
        assert!(!definitely_zero_usize_constant("const 0_u64"));
        assert!(!definitely_zero_usize_constant("Local(_2)"));
    }


    #[test]
    fn cr1_v2_path_slice_is_forward_reverse_intersection() {
        let successors = BTreeMap::from([
            ("s".to_string(), BTreeSet::from(["m".to_string()])),
            ("m".to_string(), BTreeSet::from(["f".to_string()])),
        ]);
        let predecessors = BTreeMap::from([
            ("m".to_string(), BTreeSet::from(["s".to_string()])),
            ("f".to_string(), BTreeSet::from(["m".to_string()])),
        ]);
        let forward = reachable_from("s", &successors);
        let backward = reachable_from("f", &predecessors);
        assert_eq!(
            forward.intersection(&backward).cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["s".to_string(), "m".to_string(), "f".to_string()]),
        );
    }

    fn cr1_stmt(place: &str, rvalue: &str) -> MirStatement {
        MirStatement {
            resource_place_relation_v1: None,
            source_info: SourceInfoData {
                span: "test.rs:1:1: 1:1 (#0)".into(),
                scope: "scope[0]".into(),
            },
            kind: "Assign".into(),
            details: format!("Assign(({place}, {rvalue}))"),
            place: Some(place.into()),
            is_mutable: Some(true),
            rvalue: Some(rvalue.into()),
        }
    }

    fn cr1_block(statements: Vec<MirStatement>) -> MirBasicBlock {
        MirBasicBlock {
            block_id: 0,
            statements,
            terminator: None,
        }
    }

    #[test]
    fn cr1_v2_outcome_operand_accepts_direct_or_one_exact_copy_only() {
        let direct = cr1_block(vec![]);
        assert_eq!(
            rust_realloc_outcome_operand_correlation_basis(
                &direct,
                "main",
                "rust::main::Local(_3)",
                "Local(_3) [mutable]",
            ),
            Some((
                "rust_mir_direct_result_operand_v1",
                "rust::main::Local(_3)".into(),
            ))
        );

        let copied = cr1_block(vec![cr1_stmt("Local(_5) [mutable]", "copy _3")]);
        assert_eq!(
            rust_realloc_outcome_operand_correlation_basis(
                &copied,
                "main",
                "rust::main::Local(_3)",
                "Local(_5) [mutable]",
            ),
            Some((
                "rust_mir_single_local_copy_result_operand_v1",
                "rust::main::Local(_5)".into(),
            ))
        );

        let projected = cr1_block(vec![cr1_stmt("Local(_5) [mutable]", "copy (*_3)")]);
        assert!(rust_realloc_outcome_operand_correlation_basis(
            &projected,
            "main",
            "rust::main::Local(_3)",
            "Local(_5) [mutable]",
        )
        .is_none());

        let transitive = cr1_block(vec![
            cr1_stmt("Local(_6) [mutable]", "copy _3"),
            cr1_stmt("Local(_5) [mutable]", "copy _6"),
        ]);
        assert!(rust_realloc_outcome_operand_correlation_basis(
            &transitive,
            "main",
            "rust::main::Local(_3)",
            "Local(_5) [mutable]",
        )
        .is_none());
    }

    #[test]
    fn cr1_v2_outcome_operand_rejects_same_block_result_redefinition() {
        let block = cr1_block(vec![
            cr1_stmt("Local(_3) [mutable]", "copy _1"),
            cr1_stmt("Local(_5) [mutable]", "copy _3"),
        ]);
        assert!(rust_realloc_outcome_operand_correlation_basis(
            &block,
            "main",
            "rust::main::Local(_3)",
            "Local(_5) [mutable]",
        )
        .is_none());
    }

    #[test]
    fn cr1_v2_free_argument_correlation_is_separate_from_result_stability() {
        let block = cr1_block(vec![
            cr1_stmt("Local(_3) [mutable]", "copy _1"),
            cr1_stmt("Local(_7) [mutable]", "copy _3"),
        ]);
        assert_eq!(
            rust_call_operand_correlation_basis(
                &block,
                "main",
                "rust::main::Local(_3)",
                "Local(_7) [mutable]",
            ),
            Some((
                "rust_mir_single_local_copy_result_operand_v1",
                "rust::main::Local(_7)".into(),
            ))
        );
    }

    #[test]
    fn cr1_v2_consumption_recognizes_only_exact_single_copy_argument() {
        fn free_node(statements: Vec<MirStatement>) -> GlobalICFGNode {
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 7,
                statements,
                terminator: Some(MirTerminator::Call {
                    details: "_6 = free(move _7)".into(),
                    source_info: "<cr1-v2-test>".into(),
                    function_called: "free".into(),
                    callee_def_path: Some("free".into()),
                    deallocator_evidence: None,
                    allocation_disposition_evidence: None,
                    atomic_ptr_evidence: None,
                    higher_order_evidence: None,
                    callee_is_local: false,
                    callback_def_paths: Vec::new(),
                    resolved_instance_callees: Vec::new(),
                    instance_dispatch_observed: false,
                    instance_dispatch_external: true,
                    instance_dispatch_unresolved: false,
                    arguments: vec![MirCallArgument {
                        arg: "Local(_7) [mutable]".into(),
                        is_mutable: Some(true),
                    }],
                    return_place: "_6".into(),
                    return_target: Some("bb8".into()),
                    unwind_target: "unreachable".into(),
                }),
            })
        }

        let node_id = "rust::main::bb7".to_string();
        let exact = free_node(vec![cr1_stmt("Local(_7) [mutable]", "copy _3")]);
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(node_id.clone(), exact.clone())],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let ffi = HashSet::from(["free".to_string()]);
        assert!(rust_node_consumes_result_obligation(
            &icfg,
            &node_id,
            &exact,
            "main",
            "rust::main::Local(_3)",
            &ffi,
        ));

        let transitive = free_node(vec![
            cr1_stmt("Local(_8) [mutable]", "copy _3"),
            cr1_stmt("Local(_7) [mutable]", "copy _8"),
        ]);
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(node_id.clone(), transitive.clone())],
            icfg_edges: vec![],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        assert!(!rust_node_consumes_result_obligation(
            &icfg,
            &node_id,
            &transitive,
            "main",
            "rust::main::Local(_3)",
            &ffi,
        ));
    }

    #[test]
    fn cr1_v2_no_redefinition_checks_target_block_statements() {
        let start_id = "rust::main::bb6".to_string();
        let target_id = "rust::main::bb7".to_string();
        let start = GlobalICFGNode::Mir(cr1_block(vec![]));
        let target = GlobalICFGNode::Mir(cr1_block(vec![cr1_stmt(
            "Local(_3) [mutable]",
            "copy _1",
        )]));
        let icfg = GlobalICFGOrdered {
            ordered_nodes: vec![(start_id.clone(), start), (target_id.clone(), target)],
            icfg_edges: vec![edge(&start_id, &target_id)],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],

            dependency_body_ingestion_v1: None,};
        let nodes: BTreeMap<&str, &GlobalICFGNode> = icfg
            .ordered_nodes
            .iter()
            .map(|(id, node)| (id.as_str(), node))
            .collect();
        let successors = BTreeMap::from([(
            start_id.clone(),
            BTreeSet::from([target_id.clone()]),
        )]);
        let predecessors = BTreeMap::from([(
            target_id.clone(),
            BTreeSet::from([start_id.clone()]),
        )]);

        assert!(!result_value_flow_is_stable_to_deallocation(
            &icfg,
            &nodes,
            &successors,
            &predecessors,
            &start_id,
            &target_id,
            "main",
            "rust::main::Local(_3)",
            &HashSet::new(),
        ));
    }

    #[test]
    fn cr1_variable_catalog_is_closed_over_predicate_and_result_variables() {
        let mut variables = BTreeSet::new();
        let records = vec![ConditionalReallocationRecord {
            source_allocation: "A".into(),
            reallocation_node: "rust::main::bb3".into(),
            source_variable: "rust::main::Local(_1)".into(),
            result_variable: "rust::main::Local(_3)".into(),
            source_existence_predicate_call_node: "rust::main::bb1".into(),
            outcome_predicate_call_node: "rust::main::bb4".into(),
            outcome_argument_variable: "rust::main::Local(_5)".into(),
            outcome_predicate_result_variable: "rust::main::Local(_4)".into(),
            outcome_switch_node: "rust::main::bb5".into(),
            failure_successor: "rust::main::bb6".into(),
            success_successor: "rust::main::bb7".into(),
            reallocation_callee_def_path: "crate::realloc".into(),
            outcome_callee_def_path: "std::ptr::mut_ptr::<impl *mut T>::is_null".into(),
            family: "c_malloc",
            operation: "realloc",
            certainty: "may_abstract",
            size_semantics: "positive_nonzero_constant",
            status: "conditional_guarded",
            basis: "rust_foreign_decl_c_realloc_is_null_switch_v1",
            outcome_correlation_basis: "direct_cfg_edge_realloc_to_is_null_v1",
            outcome_value_flow_basis: "rust_mir_single_local_copy_result_operand_v1",
            result_deallocations: vec![ConditionalReallocationResultDeallocationRecord {
                node: "rust::main::bb7".into(),
                variable: "rust::main::Local(_3)".into(),
                argument_variable: "rust::main::Local(_7)".into(),
                callee_def_path: "crate::free".into(),
                family: "c_malloc",
                operation: "free",
                basis: "rust_foreign_decl_c_free_result_v1",
                argument_correlation_basis: "rust_mir_single_local_copy_result_operand_v1",
                value_flow_basis: "rust_mir_result_no_redefinition_all_paths_v1",
            }],
        }];
        close_variable_catalog_over_conditional_reallocations(&mut variables, Some(&records));
        assert!(variables.contains("rust::main::Local(_1)"));
        assert!(variables.contains("rust::main::Local(_3)"));
        assert!(variables.contains("rust::main::Local(_4)"));
        assert!(variables.contains("rust::main::Local(_5)"));
        assert!(variables.contains("rust::main::Local(_7)"));
    }

    fn cov1_call(status: &str, reason: Option<&str>) -> serde_json::Value {
        let mut call = serde_json::json!({
            "caller_instance_id":"instance-hash-v1:c","caller_definition_id":"defpathhash-v1:c",
            "basic_block":0,"resolved_definition_id":"defpathhash-v1:d",
            "resolved_instance_id":"instance-hash-v1:d","body_status":status,
            "runtime_eligible":true,"scope_reason":"runtime_dependency_eligible"
        });
        if let Some(reason) = reason { call["body_unavailable_reason"] = serde_json::json!(reason); }
        call["call_key"] = serde_json::json!("dep1-call-v1:instance-hash-v1:c:bb0:instance-hash-v1:d");
        call
    }

    fn cov1_icfg(call: serde_json::Value, links: Vec<serde_json::Value>, ffi_body: bool) -> GlobalICFGOrdered {
        let mut nodes = vec![("rust::main::bb0".into(), efm2_call("c_fn", &[]))];
        let mut edges = vec![];
        if ffi_body {
            nodes.push(("llvm::c_fn::entry".into(), GlobalICFGNode::Llvm(crate::structs::LlvmJsonNode {
                node_id:0,node_type:true,info:"entry".into(),node_kind_string:"FunEntryBlock".into(),
                node_kind:0,node_source_loc:String::new(),function_name:Some("c_fn".into()),
                basic_block:None,basic_block_name:None,basic_block_info:None,svf_statements:vec![],
                incoming_edges:vec![],outgoing_edges:vec![],
            })));
            nodes.push(("llvm::c_fn::exit::rust::main::bb0".into(), GlobalICFGNode::Llvm(crate::structs::LlvmJsonNode {
                node_id:1,node_type:true,info:"exit".into(),node_kind_string:"FunExitBlock".into(),
                node_kind:1,node_source_loc:String::new(),function_name:Some("c_fn".into()),
                basic_block:None,basic_block_name:None,basic_block_info:None,svf_statements:vec![],
                incoming_edges:vec![],outgoing_edges:vec![],
            })));
            edges.push(crate::structs::IcfgEdge { source:"rust::main::bb0".into(),destination:"dummy-call".into(),label:Some("FFI Call".into()),source_label:None,destination_label:None });
            edges.push(crate::structs::IcfgEdge { source:"dummy-call".into(),destination:"llvm::c_fn::entry".into(),label:Some("dummyCall->LLVM Entry".into()),source_label:None,destination_label:None });
            edges.push(crate::structs::IcfgEdge { source:"llvm::c_fn::exit::rust::main::bb0".into(),destination:"dummyRet::rust::main::bb1::rust::main::bb0".into(),label:Some("LLVM Exit->dummyRet".into()),source_label:None,destination_label:None });
            edges.push(crate::structs::IcfgEdge { source:"dummyRet::rust::main::bb1::rust::main::bb0".into(),destination:"rust::main::bb1".into(),label:Some("dummyRet->MIR Return".into()),source_label:None,destination_label:None });
        }
        let mut body_statuses = vec![serde_json::json!({
            "instance_id":"instance-hash-v1:c","body_status":"represented_body","local":true
        })];
        if call["body_status"] == "represented_body" {
            body_statuses.push(serde_json::json!({"instance_id":"instance-hash-v1:d","body_status":"represented_body"}));
        }
        GlobalICFGOrdered {
            ordered_nodes:nodes,icfg_edges:edges,llvm_memory_effects:None,svf_solved_points_to:None,
            rust_functions:Default::default(),rust_calls:vec![],
            dependency_body_ingestion_v1:Some(crate::structs::DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test-target".into(),
                crate_correlations:vec![],package_scope:vec![],body_statuses,
                exact_call_bindings:vec![call],icfg_callsite_links:links,
                icfg_stitching_v1:None,
                context_execution_graph_v1:None,
            }),
        }
    }

    fn cov1_link() -> serde_json::Value {
        serde_json::json!({
            "call_key":"dep1-call-v1:instance-hash-v1:c:bb0:instance-hash-v1:d",
            "icfg_node":"rust::main::bb0","caller_definition_id":"defpathhash-v1:c",
            "caller_instance_id":"instance-hash-v1:c","basic_block":0,"callee_identity":"instance-hash-v1:d"
        })
    }

    fn cov1_effect(binding_id: &str) -> ExternalLibraryEffectsV1 {
        ExternalLibraryEffectsV1 {
            binding_id:binding_id.into(),effect_families:vec!["return_relation".into()],
            effect_counts:BTreeMap::new(),basis:"crema_external_library_effects_v1",
        }
    }

    #[test]
    fn cov1_ledger_requires_one_exact_semantic_path() {
        let key = "dep1-call-v1:instance-hash-v1:c:bb0:instance-hash-v1:d";
        let reach = BTreeSet::from(["rust::main::bb0".to_string()]);
        let rust = cov1_icfg(cov1_call("represented_body", None), vec![cov1_link()], false);
        let (ledger, complete) = dep1_call_semantic_coverage(&rust, &[], &[], &reach).unwrap();
        assert!(complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "dep1_rust_mir");
        assert_eq!(ledger[0]["call_key"], key);

        let rust_with_ffi = cov1_icfg(cov1_call("represented_body", None), vec![cov1_link()], true);
        let (ledger, complete) = dep1_call_semantic_coverage(&rust_with_ffi, &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["coverage_status"], "conflicting_coverage");

        let rust_with_ele1 = cov1_icfg(cov1_call("represented_body", None), vec![cov1_link()], false);
        let rust_ele1_binding = ExternalCallBindingV1 {
            binding_id:"ele1:rust::main::bb0".into(),node:"rust::main::bb0".into(),
            rust_function_scope:"rust::main".into(),callee:"external".into(),arity:0,
            arguments:vec![],result_variable:None,body_status:"bodyless",
            basis:"rustc_mir_external_call_binding_v1",
        };
        let (ledger, complete) = dep1_call_semantic_coverage(
            &rust_with_ele1, &[rust_ele1_binding], &[cov1_effect("ele1:rust::main::bb0")], &reach,
        ).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["coverage_status"], "conflicting_coverage");

        let unresolved_scope = cov1_icfg(cov1_call("unresolved_scope_identity", None), vec![cov1_link()], false);
        let (ledger, complete) = dep1_call_semantic_coverage(&unresolved_scope, &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "uncovered");

        let ffi = cov1_icfg(cov1_call("body_unavailable", Some("intentionally_bodyless_foreign")), vec![cov1_link()], true);
        assert!(crema_ffi_body_coverage_reference(&ffi, "rust::main::bb0").is_some());
        let (ledger, complete) = dep1_call_semantic_coverage(&ffi, &[], &[], &reach).unwrap();
        assert!(complete, "{ledger:?}");
        assert_eq!(ledger[0]["semantic_coverage_path"], "crema_ffi_body");

        let bodyless = cov1_icfg(cov1_call("body_unavailable", Some("intentionally_bodyless_foreign")), vec![cov1_link()], false);
        let binding = ExternalCallBindingV1 { binding_id:"ele1:rust::main::bb0".into(),node:"rust::main::bb0".into(),rust_function_scope:"rust::main".into(),callee:"free".into(),arity:1,arguments:vec![],result_variable:None,body_status:"bodyless",basis:"rustc_mir_external_call_binding_v1" };
        let effects = vec![cov1_effect("ele1:rust::main::bb0")];
        let (ledger, complete) = dep1_call_semantic_coverage(&bodyless, &[binding.clone()], &effects, &reach).unwrap();
        assert!(complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "cqpl_external_library_model");

        let uncovered = cov1_icfg(cov1_call("body_unavailable", Some("intentionally_bodyless_foreign")), vec![], false);
        let (ledger, complete) = dep1_call_semantic_coverage(&uncovered, &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "uncovered");

        let conflict = cov1_icfg(cov1_call("body_unavailable", Some("intentionally_bodyless_foreign")), vec![cov1_link()], true);
        let (ledger, complete) = dep1_call_semantic_coverage(&conflict, &[binding], &effects, &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["coverage_status"], "conflicting_coverage");
    }

    #[test]
    fn cov1_ledger_accepts_certified_dynamic_trait_drop_summary() {
        let reach=BTreeSet::from(["rust::main::bb0".to_string()]);
        let key="dep1-drop-v1:instance-hash-v1:c:bb0:mir_drop:instance-hash-v1:d";
        let mut call=cov1_call("dynamic_drop_summary",None);
        call["activation_kind"]=serde_json::json!("mir_drop");
        call["scope_reason"]=serde_json::json!("dynamic_trait_drop_summary");
        call["call_key"]=serde_json::json!(key);
        call["dynamic_drop_summary_v1"]=serde_json::json!({
            "semantics":"opaque_dynamic_destructor_may_effect_v1",
            "normal_memory_effect":"widen_all_tracked_to_top",
            "unwind_memory_effect":"widen_all_tracked_to_top"
        });
        let mut link=cov1_link();
        link["call_key"]=serde_json::json!(key);
        let (ledger,complete)=dep1_call_semantic_coverage(
            &cov1_icfg(call,vec![link],false),&[],&[],&reach
        ).unwrap();
        assert!(complete,"{ledger:?}");
        assert_eq!(ledger[0]["semantic_coverage_path"],"dep1_dynamic_trait_drop_summary");
        assert_eq!(ledger[0]["coverage_status"],"covered");
    }

    #[test]
    fn dynamic_trait_drop_summary_exports_opaque_memory_effect_at_reachable_event_node() {
        let mut call = cov1_call("dynamic_drop_summary", None);
        call["activation_kind"] = serde_json::json!("mir_drop");
        call["scope_reason"] = serde_json::json!("dynamic_trait_drop_summary");
        call["dynamic_drop_summary_v1"] = serde_json::json!({
            "semantics":"opaque_dynamic_destructor_may_effect_v1",
            "normal_memory_effect":"widen_all_tracked_to_top",
            "unwind_memory_effect":"widen_all_tracked_to_top"
        });
        let mut icfg = cov1_icfg(call, vec![], false);
        icfg.icfg_edges.push(crate::structs::IcfgEdge {
            source:"rust::main::bb0".into(),
            destination:"rust::main::bb1".into(),
            label:Some("DYNAMIC_TRAIT_DROP_SUMMARY_V1".into()),
            source_label:None,
            destination_label:None,
        });
        let reach = BTreeSet::from(["rust::main::bb0".to_string()]);
        let records = opaque_memory_effect_records(&icfg, &reach);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].node, "rust::main::bb0");
        assert_eq!(records[0].effect, "may_read_write_allocate_deallocate");
        assert_eq!(records[0].basis, "rust_dyn_trait_drop_vtable_summary_v1");
    }

    #[test]
    fn opaque_dynamic_drop_adds_finite_synthetic_allocation_site() {
        let mut catalog = Vec::new();
        let opaque = vec![OpaqueMemoryEffectRecord {
            node: "rust::exec::bb7".into(),
            effect: "may_read_write_allocate_deallocate",
            basis: "rust_dyn_trait_drop_vtable_summary_v1",
        }];
        append_opaque_summary_allocations(&mut catalog, &opaque);
        assert_eq!(catalog.len(), 1);
        assert!(matches!(
            &catalog[0].site,
            AllocationSiteId::Synthetic { scope, label }
                if scope == "rust::exec::bb7"
                    && label == "opaque_dynamic_trait_drop_allocation_v1"
        ));
        assert_eq!(
            catalog[0].allocator_contract.as_ref().map(|contract| contract.family),
            Some("unknown")
        );
        append_opaque_summary_allocations(&mut catalog, &opaque);
        assert_eq!(catalog.len(), 1, "summary allocation identity must be idempotent");
    }

    #[test]
    fn opaque_higher_order_summary_exports_effect_and_synthetic_allocation() {
        let mut icfg=cov1_icfg(cov1_call("out_of_scope",None),vec![],false);
        let node="rust::main::bb0".to_string();
        icfg.icfg_edges.push(IcfgEdge {source:node.clone(),destination:node.clone(),
            label:Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1".into()),source_label:None,destination_label:None});
        icfg.icfg_edges.push(IcfgEdge {source:node.clone(),destination:node.clone(),
            label:Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1".into()),source_label:None,destination_label:None});
        icfg.dependency_body_ingestion_v1=Some(DependencyBodyIngestionExport {
            capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),
            crate_correlations:vec![],package_scope:vec![],body_statuses:vec![],exact_call_bindings:vec![],
            icfg_callsite_links:vec![],icfg_stitching_v1:None,
            context_execution_graph_v1:Some(serde_json::json!({
                "opaque_higher_order_summaries":[{
                    "semantics":"opaque_higher_order_callback_zero_or_more_v1",
                    "provider_effect":"opaque_at_least_once_per_call",
                    "callback_multiplicity":"zero_or_more",
                    "memory_effect":"widen_all_tracked_to_top",
                    "execution_state":node
                }]
            })),
        });
        let reach=BTreeSet::from([node.clone()]);
        let opaque=opaque_memory_effect_records(&icfg,&reach);
        assert_eq!(opaque.len(),1);
        assert_eq!(opaque[0].node,node);
        assert_eq!(opaque[0].basis,"rust_higher_order_callback_opaque_summary_v1");
        let mut catalog=vec![];
        append_opaque_summary_allocations(&mut catalog,&opaque);
        assert_eq!(catalog.len(),1);
        assert!(catalog[0].display.contains("opaque_higher_order_callback_allocation_v1"));
    }

    #[test]
    fn cov1_ledger_accepts_only_structurally_certified_callback_keys() {
        let reach=BTreeSet::from(["rust::main::bb0".to_string()]);
        let key="dep1-callback-v1:instance-hash-v1:c:bb0:callback0:instance-hash-v1:d";
        let mut call=cov1_call("represented_body",None);
        call["activation_kind"]=serde_json::json!("higher_order_callback");
        call["callback_semantics"]=serde_json::json!("spawn_may_once");
        call["callback_ordinal"]=serde_json::json!(0);
        call["call_key"]=serde_json::json!(key);
        let mut link=cov1_link();
        link["call_key"]=serde_json::json!(key);
        let (ledger,complete)=dep1_call_semantic_coverage(
            &cov1_icfg(call.clone(),vec![link],false),&[],&[],&reach
        ).unwrap();
        assert!(complete,"{ledger:?}");
        assert_eq!(ledger[0]["semantic_coverage_path"],"dep1_rust_mir");
        assert_eq!(ledger[0]["call_key"],key);

        call["callback_semantics"]=serde_json::json!("unknown");
        let (ledger,complete)=dep1_call_semantic_coverage(
            &cov1_icfg(call,vec![],false),&[],&[],&reach
        ).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["coverage_status"],"invalid_callback_certificate");
    }

    #[test]
    fn cov1_ledger_rejects_wrong_keys_and_transitive_link_mutations() {
        let reach = BTreeSet::from(["rust::main::bb0".to_string()]);
        let mut wrong_key = cov1_call("represented_body", None);
        wrong_key["call_key"] = serde_json::json!("mutated");
        let (ledger, complete) = dep1_call_semantic_coverage(&cov1_icfg(wrong_key, vec![], false), &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["coverage_status"], "invalid_call_key");

        let mut wrong_link = cov1_link();
        wrong_link["caller_definition_id"] = serde_json::json!("defpathhash-v1:other");
        let call = cov1_call("body_unavailable", Some("intentionally_bodyless_foreign"));
        let (ledger, complete) = dep1_call_semantic_coverage(&cov1_icfg(call.clone(), vec![wrong_link], false), &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "uncovered");

        let (ledger, complete) = dep1_call_semantic_coverage(&cov1_icfg(call, vec![cov1_link(), cov1_link()], false), &[], &[], &reach).unwrap();
        assert!(!complete);
        assert_eq!(ledger[0]["semantic_coverage_path"], "uncovered");
    }

}
