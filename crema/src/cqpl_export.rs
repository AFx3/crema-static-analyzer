use crate::abstract_domain::{AbstractMemory, AbstractState, CellValue, Name};
use crate::identity::{AllocationIdentityMemory, AllocationIdentityState};
use crate::mir_semantics::{mir_semantics_v2_enabled, semantic_labels_for_block};
use crate::panic_unwind::panic_unwind_lifecycle_v1_enabled;
use crate::panic_lifecycle_domain::{fixed_point_real_panic_lifecycle, PanicLifecycleMemory};
use crate::memory_events;
use crate::structs::{
    AbstractAllocId, AllocationSiteId, GlobalICFGNode, GlobalICFGOrdered, MirTerminator,
    PlaceId, PlaceProjection, ProgramVarId, RustDropAllocatorEvidence, RustDropAllocatorEvidenceKind,
    RustCallDeallocatorEvidence, RustCallDeallocatorEvidenceKind, RustAllocationDispositionEvidence, RustAllocationDispositionEvidenceKind, SvfStatement,
};
use crate::utils::load_ffi_functions;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use once_cell::sync::Lazy;

/// Versioned, read-only boundary from CREMA to the standalone CQPL checker.
///
/// Scientific invariant: this module never mutates the ICFG, abstract state,
/// taint state, or legacy memory-error detector state. It only serializes
/// information already produced by CREMA plus syntactic labels obtained by
/// inspecting the existing ICFG nodes.
pub const CQPL_ANNOTATED_ICFG_SCHEMA_VERSION: u32 = 1;
pub const CQPL_ANNOTATED_ICFG_IDENTITY_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize)]
struct AnnotatedIcfg {
    schema_version: u32,
    entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<&'static str>>,
    variables: Vec<ProgramVariable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allocations: Option<Vec<AbstractAllocationRecord>>,
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

pub fn export_cqpl_annotated_icfg(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    entry: &str,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    export_cqpl_annotated_icfg_versioned(icfg, abs_state, None, None, entry, 1, output_path)
}

pub fn export_cqpl_annotated_icfg_with_identity(
    icfg: &GlobalICFGOrdered,
    abs_state: &AbstractState,
    identity_state: &AllocationIdentityState,
    entry: &str,
    schema_version: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    export_cqpl_annotated_icfg_versioned(
        icfg,
        abs_state,
        Some(identity_state),
        Some(identity_state),
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
    entry: &str,
    schema_version: u32,
    output_path: &Path,
) -> Result<(), Box<dyn Error>> {
    export_cqpl_annotated_icfg_versioned(
        icfg,
        abs_state,
        Some(identity_state),
        Some(disposition_identity_state),
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
    validate_canonical_internal_rust_edges(icfg, &successors)?;

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
        // and therefore do not depend on that workaround.
        let raw_labels = labels_for_node(node_id, node, &llvm_names, &ffi_functions);
        let allocation_labels = if schema_version == 2 {
            let event_mem = identity_state
                .and_then(|state| state.event_by_node.get(node_id))
                .cloned()
                .unwrap_or_default();
            Some(allocation_labels_for_node(node_id, node, &raw_labels, &event_mem, &ffi_functions, schema_version))
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

    if variable_ids.is_empty() {
        return Err("CQPL annotated ICFG contains no program variables".into());
    }

    let variables = variable_ids
        .into_iter()
        .map(|id| ProgramVariable {
            language: language_of(&id),
            id,
        })
        .collect();

    let allocations = if schema_version == 2 {
        Some(allocation_catalog(identity_state.expect("checked schema-v2 identity state"), schema_version))
    } else {
        None
    };

    let output = AnnotatedIcfg {
        schema_version,
        entry: entry.to_string(),
        capabilities: if schema_version == 2 {
            let mut caps = vec![
                "allocation_contracts_v1",
                "allocation_contracts_v2",
                "allocation_state_v1",
                "allocation_disposition_v1",
                // B1.1: CString ownership handoff/reclaim extends the frozen
                // v6S-r1 disposition vocabulary.  Keep v1 declared for
                // backward-compatible base semantics and advertise the
                // additive closed refinement explicitly as v2.
                "allocation_disposition_v2",
            ];
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
        variables,
        allocations,
        nodes,
    };

    let file = File::create(output_path)?;
    serde_json::to_writer_pretty(file, &output)?;
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
        for allocations in mem.points_to.values().chain(mem.place_points_to.values()) {
            for allocation in allocations {
                ids.entry(stable_allocation_id(allocation))
                    .or_insert_with(|| allocation.clone());
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

    out
}

fn identity_annotation(mem: &AllocationIdentityMemory) -> NodeIdentityAnnotation {
    NodeIdentityAnnotation {
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
        let Some(variable) = identity_var_for_event(node_id, node, &label.variable) else {
            continue;
        };
        let allocations = identity.event_allocations(&variable);
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
                for allocation in post_identity.event_allocations(&ret_var) {
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
    // producer-certified contracts.  Known C family naming follows LLVM LangRef's
    // `"alloc-family"="malloc"` family for malloc/calloc/realloc/free:
    // https://llvm.org/docs/LangRef.html#alloc-family
    match &allocation.site {
        AllocationSiteId::CCall { allocator, .. } if allocator == "malloc" =>
            AllocationContract::v1("c_malloc", "malloc", "c"),
        AllocationSiteId::CCall { allocator, .. } if allocator == "calloc" =>
            AllocationContract::v1("c_malloc", "calloc", "c"),
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

fn labels_for_node(
    node_id: &str,
    node: &GlobalICFGNode,
    llvm_names: &LlvmNameResolver,
    ffi_functions: &HashSet<String>,
) -> Vec<EventLabel> {
    let mut labels = BTreeSet::new();
    match node {
        GlobalICFGNode::Mir(bb) => {
            for stmt in &bb.statements {
                // A dereference/projection read is a syntactic memory read.
                if let Some(rvalue) = &stmt.rvalue {
                    for v in mir_deref_locals(rvalue) {
                        labels.insert(EventLabel { predicate: "read", variable: v });
                    }
                }
                // A write through a dereferenced place is a syntactic memory write.
                if let Some(place) = &stmt.place {
                    for v in mir_deref_locals(place) {
                        labels.insert(EventLabel { predicate: "write", variable: v });
                    }
                }
                // Some rustc textual dumps expose the place only in `details`.
                if let Some((lhs, rhs)) = stmt.details.split_once('=') {
                    for v in mir_deref_locals(lhs) {
                        labels.insert(EventLabel { predicate: "write", variable: v });
                    }
                    for v in mir_deref_locals(rhs) {
                        labels.insert(EventLabel { predicate: "read", variable: v });
                    }
                }
            }

            if let Some(term) = &bb.terminator {
                match term {
                    MirTerminator::Drop { dropped_value, .. } => {
                        if let Some(v) = canonical_mir_local(dropped_value) {
                            labels.insert(EventLabel { predicate: "drop", variable: v });
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
                                labels.insert(EventLabel { predicate: "alloc", variable: v });
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
                                labels.insert(EventLabel { predicate: "drop", variable: v });
                            }
                        }

                        // Some standard-library calls are deliberately not inlined into
                        // CREMA's MIR ICFG, but their pinned implementation has a concrete
                        // memory-access effect.  CString::from_raw is one such call: in
                        // nightly-2024-11-21 it executes strlen(ptr) before reconstructing
                        // ownership.  Export that omitted read as a summary label; the
                        // later MIR Drop of the reconstructed CString remains the drop
                        // event, so this does NOT turn from_raw itself into a deallocation.
                        if is_cstring_from_raw_read_summary(function_called)
                            || is_cstring_from_raw_read_summary(call_text)
                        {
                            if let Some(v) = first_arg.clone() {
                                labels.insert(EventLabel { predicate: "read", variable: v });
                            }
                        }

                        if is_ptr_read_call(function_called) || is_ptr_read_call(call_text) {
                            if let Some(v) = first_arg.clone() {
                                labels.insert(EventLabel { predicate: "read", variable: v });
                            }
                        }
                        if is_ptr_write_call(function_called)
                            || is_ptr_write_call(call_text)
                            || is_ptr_drop_in_place_call(function_called)
                            || is_ptr_drop_in_place_call(call_text)
                        {
                            if let Some(v) = first_arg {
                                labels.insert(EventLabel { predicate: "write", variable: v });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        GlobalICFGNode::Llvm(llvm) => {
            if llvm.node_kind_string == "FunCallBlock" && is_c_malloc_family_alloc_call(&llvm.info) {
                for stmt in &llvm.svf_statements {
                    if stmt.stmt_type == "AddrStmt" {
                        if let Some(id) = stmt.lhs_var_id {
                            labels.insert(EventLabel {
                                predicate: "alloc",
                                variable: llvm_names.resolve_svf(&scoped_svf_var(id, node_id)),
                            });
                        }
                    }
                }
            }

            if llvm.node_kind_string == "FunCallBlock" && llvm.info.contains("@free(") {
                if let Some(ir) = llvm_free_ir_argument_id(&llvm.info) {
                    let ir = scoped_ir_var(ir, node_id);
                    labels.insert(EventLabel {
                        predicate: "drop",
                        variable: llvm_names.resolve_ir(&ir).unwrap_or(ir),
                    });
                }
            }

            for stmt in &llvm.svf_statements {
                match stmt.stmt_type.as_str() {
                    // Load reads through the rhs pointer/location.
                    "LoadStmt" => {
                        if let Some(rhs) = stmt.rhs_var_id {
                            labels.insert(EventLabel {
                                predicate: "read",
                                variable: llvm_names.resolve_svf(&scoped_svf_var(rhs, node_id)),
                            });
                        }
                    }
                    // Store writes through the lhs pointer/location. The rhs is
                    // a value flow, not necessarily a pointee memory access.
                    "StoreStmt" => {
                        if let Some(lhs) = stmt.lhs_var_id {
                            labels.insert(EventLabel {
                                predicate: "write",
                                variable: llvm_names.resolve_svf(&scoped_svf_var(lhs, node_id)),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) | GlobalICFGNode::Terminal(_) => {}
    }
    labels.into_iter().collect()
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

fn is_box_new_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && s.contains(">::new")
}

fn is_raw_alloc_zeroed_call(s: &str) -> bool {
    s.contains("std::alloc::alloc_zeroed") || s.contains("alloc::alloc::alloc_zeroed")
}

fn is_raw_alloc_call(s: &str) -> bool {
    (s.contains("std::alloc::alloc") || s.contains("alloc::alloc::alloc"))
        && !is_raw_alloc_zeroed_call(s)
        && !s.contains("handle_alloc_error")
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
    if (t == "malloc" || t == "calloc") && ffi_functions.contains(t) {
        return true;
    }
    (t.contains("libc::") && (t.ends_with("::malloc") || t.ends_with("::calloc")))
        || (ffi_functions.contains("malloc") && (t.starts_with("malloc(") || t.contains(" malloc(")))
        || (ffi_functions.contains("calloc") && (t.starts_with("calloc(") || t.contains(" calloc(")))
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
}

impl LlvmNameResolver {
    fn build(icfg: &GlobalICFGOrdered) -> Self {
        let mut out = Self::default();
        for (node_id, node) in &icfg.ordered_nodes {
            let GlobalICFGNode::Llvm(llvm) = node else { continue; };
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstract_domain::{Allocation, CellValue};
    use crate::structs::{DummyNode, IcfgEdge, MirBasicBlock, MirCallArgument, SourceInfoData, MirStatement, RustCallMetadata, RustFunctionMetadata, TerminalNode, RustAllocationDispositionEvidence, RustAllocationDispositionEvidenceKind};

    fn edge(a: &str, b: &str) -> IcfgEdge {
        IcfgEdge {
            source: a.into(),
            destination: b.into(),
            label: None,
            source_label: None,
            destination_label: None,
        }
    }

    #[test]
    fn canonical_mir_variable_parser_is_stable() {
        assert_eq!(canonical_mir_local("copy _12"), Some("Local(_12)".into()));
        assert_eq!(canonical_mir_local("Local(_7) [mutable]"), Some("Local(_7)".into()));
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
    fn dereference_statement_produces_read_and_write_labels() {
        let bb = MirBasicBlock {
            block_id: 0,
            statements: vec![MirStatement {
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
    fn exporter_keeps_graph_successors_and_post_state_without_fabricating_pre() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let n1 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: None });
        let g = GlobalICFGOrdered {
            ordered_nodes: vec![("rust::main::bb0".into(), n0), ("rust::main::bb1".into(), n1)],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::bb1")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
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
    fn exporter_rejects_dangling_canonical_edge() {
        let n0 = GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None });
        let g = GlobalICFGOrdered {
            ordered_nodes: vec![("rust::main::bb0".into(), n0)],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::terminate")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
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
            ordered_nodes: vec![
                ("rust::main::bb0".into(), n0),
                ("rust::main::terminate".into(), terminal),
            ],
            icfg_edges: vec![edge("rust::main::bb0", "rust::main::terminate")],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
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
            is_internal: Some(true),
        });
        let dummy_ret = GlobalICFGNode::DummyRet(DummyNode {
            dummy_node_name: "dummyRet".into(),
            incoming_edge: callee_return.into(),
            outgoing_edge: caller_return.into(),
            id: "dr".into(),
            mir_var: Some("_0".into()),
            llvm_var: Some("Local _0".into()),
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
        };

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
            ordered_nodes: vec![
                ("rust::main::bb0".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None })),
                ("dummyCall::x".into(), GlobalICFGNode::DummyCall(DummyNode {
                    dummy_node_name: "dummyCall".into(), incoming_edge: "rust::main::bb0".into(),
                    outgoing_edge: "rust::callee::bb0".into(), id: "dc".into(), mir_var: None, llvm_var: None,
                    is_internal: Some(true),
                })),
                ("dummyRet::x".into(), GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name: "dummyRet".into(), incoming_edge: "rust::callee::bb1".into(),
                    outgoing_edge: "rust::main::bb1".into(), id: "dr".into(), mir_var: None, llvm_var: None,
                    is_internal: Some(true),
                })),
                ("rust::main::bb1".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: None })),
                ("rust::callee::bb0".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 0, statements: vec![], terminator: None })),
                ("rust::callee::bb1".into(), GlobalICFGNode::Mir(MirBasicBlock { block_id: 1, statements: vec![], terminator: Some(MirTerminator::Return { details: "return".into(), source_info: "x".into() }) })),
            ],
            icfg_edges: vec![edge("rust::main::bb0", "dummyCall::x")],
            rust_functions: BTreeMap::new(),
            rust_calls: vec![],
        };
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
            ordered_nodes: vec![
                (n0.clone(), c0),
                (n1.clone(), c1.clone()),
                (n2.clone(), c2),
                (n3.clone(), c3.clone()),
            ],
            icfg_edges: vec![edge(&n0, &n1), edge(&n1, &n2), edge(&n2, &n3)],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };

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
            ordered_nodes: vec![(n0, node)],
            icfg_edges: vec![],
            rust_functions: Default::default(),
            rust_calls: Vec::new(),
        };
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
    fn schema_v2_variable_catalog_is_closed_over_identity_program_vars() {
        let node_id = "rust::main::bb0".to_string();
        let icfg = GlobalICFGOrdered {
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
        };

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
                    source_info: SourceInfoData { span: "x".into(), scope: "0".into() },
                    kind: "Assign".into(),
                    details: "Assign((_2, copy (*_1)))".into(),
                    place: Some("Local(_2) [mutable]".into()),
                    is_mutable: Some(true),
                    rvalue: Some("copy (*_1)".into()),
                },
                MirStatement {
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
            ordered_nodes: vec![(node_id.clone(), node)],
            icfg_edges: vec![],
            rust_functions: Default::default(),
            rust_calls: vec![],
        };

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
    fn schema_v2_allocation_labels_preserve_may_certainty_without_closure_label_expansion() {
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
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].predicate, "drop");
        assert_eq!(labels[0].allocation, stable_allocation_id(&a));
        assert_eq!(labels[0].certainty, "may_abstract");
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
                    }),
                    allocation_disposition_evidence: None,
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

}
