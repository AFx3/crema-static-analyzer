//! DEP1-P2 context-explicit Rust execution graph construction.
//!
//! The builder consumes the compiler-certified body/call relation and the
//! canonical MIR CFG.  It does not copy the old shared Return -> dummyRet
//! fan-out: represented calls and their return pops are reconstructed from
//! exact call bindings and MIR terminators.

use crate::structs::{
    execution_state_node_id, CanonicalCodeNodeIdentity, DependencyBodyIngestionExport,
    DummyNode, ExecutionStateIdentity, GlobalICFGNode, GlobalICFGOrdered, IcfgEdge,
    MirBasicBlock, MirTerminator, RustCallMetadata, RustFunctionMetadata,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Read-only typed view of the existing DEP1 activation ledger. No graph,
/// JSON provenance or CQPL payload is changed by this adapter.
pub fn rust_activation_unwind_metadata_from_execution_graph(
    icfg: &GlobalICFGOrdered,
) -> Result<Option<crate::structs::RustActivationUnwindMetadataV1>, String> {
    use crate::structs::{HigherOrderCallSemantics, RustActivationIdentity, RustActivationMetadataSource,
        RustActivationUnwindAction, RustActivationUnwindEdge, RustActivationUnwindMetadata,
        RustActivationUnwindMetadataV1, RustActivationUnwindSemantics};
    let Some(graph) = icfg.dependency_body_ingestion_v1.as_ref()
        .and_then(|dep| dep.context_execution_graph_v1.as_ref()) else { return Ok(None); };
    let records = graph.get("represented_call_activations").and_then(Value::as_array)
        .ok_or("execution graph lacks structured activation ledger")?;
    let nodes: BTreeMap<_, _> = icfg.ordered_nodes.iter().map(|(id, node)| (id.as_str(), node)).collect();
    let mut used = BTreeSet::new();
    let mut result = Vec::new();
    for call in &icfg.rust_calls {
        let entry = &icfg.rust_functions.get(&call.callee_function)
            .ok_or("missing callee function metadata")?.entry_node;
        let candidates: Vec<_> = records.iter().enumerate().filter(|(_, r)| {
            r.get("caller_call_state").and_then(Value::as_str) == Some(call.call_node.as_str())
                && r.get("dummy_call_state").and_then(Value::as_str) == Some(call.dummy_call_node.as_str())
                && r.get("callee_entry_state").and_then(Value::as_str) == Some(entry.as_str())
        }).collect();
        let [(index, activation)] = candidates.as_slice() else {
            return Err("missing or ambiguous execution activation identity".into());
        };
        if !used.insert(*index) { return Err("execution activation reused by distinct call metadata".into()); }
        let call_key = activation.get("call_key").and_then(Value::as_str)
            .filter(|key| !key.is_empty()).ok_or("missing producer call key")?;
        let kind = activation.get("activation_kind").and_then(Value::as_str);
        let mut metadata = if kind == Some("higher_order_callback") {
            let policy: Option<HigherOrderCallSemantics> = activation.get("callback_semantics")
                .map(|value| serde_json::from_value(value.clone())).transpose()
                .map_err(|_| "unsupported producer callback policy")?;
            let mut metadata = RustActivationUnwindMetadata::callback(call, policy);
            if metadata.activation_kind == crate::structs::RustActivationKind::SynchronousCallback {
                // Only the callback's explicit unwind relations certify its
                // unwind. Never substitute the provider's unwind_action here.
                if let Some(relations) = activation.get("unwind_return_relations").and_then(Value::as_array)
                    .filter(|relations| !relations.is_empty()) {
                    let mut actions = Vec::new();
                    let mut edges = BTreeSet::new();
                    for relation in relations {
                        let source = relation.get("callee_unwind_state").and_then(Value::as_str)
                            .ok_or("missing callback unwind source")?;
                        let destination = relation.get("caller_unwind_state").and_then(Value::as_str)
                            .ok_or("missing callback unwind destination")?;
                        if !nodes.contains_key(source) || !nodes.contains_key(destination)
                            || !icfg.icfg_edges.iter().any(|edge| edge.source == source && edge.destination == destination
                                && crate::panic_unwind::edge_flow_kind(edge) == crate::panic_unwind::EdgeFlowKind::Unwind) {
                            return Err("callback unwind relation lacks canonical Unwind edge".into());
                        }
                        actions.push(RustActivationUnwindAction::from_mir_action(relation.get("unwind_action")
                            .and_then(Value::as_str).ok_or("missing callback relation action")?)?);
                        edges.insert(RustActivationUnwindEdge { source_node: source.into(), destination_node: destination.into() });
                    }
                    if actions.iter().any(|action| action != &actions[0]) {
                        return Err("ambiguous callback activation unwind actions".into());
                    }
                    metadata.unwind = RustActivationUnwindSemantics::CertifiedCallback {
                        action: actions[0].clone(), unwind_edges: edges.into_iter().collect() };
                }
            } else if metadata.activation_kind == crate::structs::RustActivationKind::DetachedCallback {
                if activation.get("unwind_return_relations").and_then(Value::as_array)
                    .is_some_and(|relations| !relations.is_empty()) {
                    return Err("detached callback acquired a caller unwind relation".into());
                }
            }
            metadata
        } else {
            let node = nodes.get(call.call_node.as_str()).ok_or("missing direct activation MIR")?;
            let (action, is_drop) = match node {
                GlobalICFGNode::Mir(MirBasicBlock { terminator: Some(MirTerminator::Call { unwind_target, .. }), .. })
                    if kind == Some("mir_call") => (unwind_target.as_str(), false),
                GlobalICFGNode::Mir(MirBasicBlock { terminator: Some(MirTerminator::Drop { unwind_target, .. }), .. })
                    if kind == Some("mir_drop") => (unwind_target.as_str(), true),
                _ => return Err("execution activation kind conflicts with direct MIR".into()),
            };
            RustActivationUnwindMetadata::direct(call, action, is_drop)
        };
        metadata.activation = RustActivationIdentity::from(call);
        metadata.source = RustActivationMetadataSource::ExecutionActivation { call_key: call_key.into() };
        result.push(metadata);
    }
    Ok(Some(RustActivationUnwindMetadataV1::new(result)?))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Activation {
    instance: String,
    context: Vec<String>,
}

fn scope_of_node(id: &str) -> Option<&str> {
    let (scope, block) = id.rsplit_once("::bb")?;
    (!block.is_empty() && block.bytes().all(|b| b.is_ascii_digit()))
        .then(|| scope.strip_prefix("rust::"))
        .flatten()
}

fn encoded_proxy_id(kind: &str, call_key: &str, context: &[String]) -> String {
    let mut bytes = b"crema-execution-proxy-v1".to_vec();
    for value in std::iter::once(kind).chain(std::iter::once(call_key)).chain(context.iter().map(String::as_str)) {
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes { use std::fmt::Write; let _ = write!(&mut hex, "{byte:02x}"); }
    format!("{kind}::exec-v1::{hex}")
}

fn represented_statuses(
    dep: &DependencyBodyIngestionExport,
) -> Result<BTreeMap<String, &Value>, String> {
    let mut represented = BTreeMap::new();
    for body in &dep.body_statuses {
        if body.get("body_status").and_then(Value::as_str) != Some("represented_body") {
            continue;
        }
        let Some(instance) = body.get("instance_id").and_then(Value::as_str) else {
            return Err("represented body record is missing its compiler Instance identity".into());
        };
        if let Some(previous) = represented.insert(instance.to_owned(), body) {
            if previous != body {
                return Err(format!("duplicate conflicting MIR body records for concrete Instance {instance}"));
            }
        }
    }
    Ok(represented)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecursiveSccFrame {
    scc_id: String,
    /// Exact context at the first activation inside this SCC.  Internal
    /// recursive calls preserve this prefix and replace only the local SCC
    /// suffix with the newest static call_key (k=1 within the SCC).
    entry_context: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyRegionFrame {
    /// Context immediately after the exact application -> dependency entry.
    /// Calls that remain inside the dependency region preserve this prefix and
    /// replace only the dependency-local suffix with the newest static
    /// call_key (k=1 within the region).
    entry_context: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct CallGraphSccs {
    instance_to_scc: BTreeMap<String, String>,
    recursive_sccs: BTreeSet<String>,
    members: BTreeMap<String, Vec<String>>,
}

impl CallGraphSccs {
    fn scc_of(&self, instance: &str) -> Option<&str> {
        self.instance_to_scc.get(instance).map(String::as_str)
    }

    fn recursive_scc_of(&self, instance: &str) -> Option<&str> {
        let id = self.scc_of(instance)?;
        self.recursive_sccs.contains(id).then_some(id)
    }

    fn same_recursive_scc(&self, a: &str, b: &str) -> Option<&str> {
        let sa = self.recursive_scc_of(a)?;
        (self.recursive_scc_of(b) == Some(sa)).then_some(sa)
    }
}

/// Deterministic Tarjan decomposition restricted to the represented call graph
/// reachable from the selected root.  The condensation graph is acyclic; only
/// SCCs with more than one member or a self-edge require bounded contexts.
fn call_graph_sccs(
    graph: &BTreeMap<String, BTreeSet<String>>,
    root: &str,
) -> CallGraphSccs {
    fn reachable(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
    ) {
        if !seen.insert(node.to_owned()) { return; }
        for next in graph.get(node).into_iter().flatten() {
            reachable(next, graph, seen);
        }
    }

    fn strongconnect(
        node: String,
        graph: &BTreeMap<String, BTreeSet<String>>,
        reachable: &BTreeSet<String>,
        next_index: &mut usize,
        indices: &mut BTreeMap<String, usize>,
        lowlink: &mut BTreeMap<String, usize>,
        stack: &mut Vec<String>,
        on_stack: &mut BTreeSet<String>,
        components: &mut Vec<Vec<String>>,
    ) {
        let index = *next_index;
        *next_index += 1;
        indices.insert(node.clone(), index);
        lowlink.insert(node.clone(), index);
        stack.push(node.clone());
        on_stack.insert(node.clone());

        for next in graph.get(&node).into_iter().flatten() {
            if !reachable.contains(next) { continue; }
            if !indices.contains_key(next) {
                strongconnect(
                    next.clone(), graph, reachable, next_index, indices, lowlink,
                    stack, on_stack, components,
                );
                let next_low = lowlink[next];
                let current = lowlink[&node];
                lowlink.insert(node.clone(), current.min(next_low));
            } else if on_stack.contains(next) {
                let next_index = indices[next];
                let current = lowlink[&node];
                lowlink.insert(node.clone(), current.min(next_index));
            }
        }

        if lowlink[&node] == indices[&node] {
            let mut component = Vec::new();
            loop {
                let member = stack.pop().expect("Tarjan stack contains SCC root");
                on_stack.remove(&member);
                component.push(member.clone());
                if member == node { break; }
            }
            component.sort();
            components.push(component);
        }
    }

    let mut reachable_nodes = BTreeSet::new();
    reachable(root, graph, &mut reachable_nodes);
    let mut next_index = 0usize;
    let mut indices = BTreeMap::new();
    let mut lowlink = BTreeMap::new();
    let mut stack = Vec::new();
    let mut on_stack = BTreeSet::new();
    let mut components = Vec::new();
    for node in reachable_nodes.iter().cloned() {
        if !indices.contains_key(&node) {
            strongconnect(
                node, graph, &reachable_nodes, &mut next_index, &mut indices,
                &mut lowlink, &mut stack, &mut on_stack, &mut components,
            );
        }
    }
    components.sort_by(|a,b| a.first().cmp(&b.first()));

    let mut out = CallGraphSccs::default();
    for members in components {
        let first = members.first().cloned().unwrap_or_else(|| "empty".into());
        let scc_id = format!("dep1-scc-v1:{first}");
        let recursive = members.len() > 1
            || members.first().is_some_and(|member|
                graph.get(member).is_some_and(|nexts| nexts.contains(member)));
        if recursive { out.recursive_sccs.insert(scc_id.clone()); }
        for member in &members {
            out.instance_to_scc.insert(member.clone(), scc_id.clone());
        }
        out.members.insert(scc_id, members);
    }
    out
}

#[derive(Debug, Clone)]
struct ContextTransition {
    callee_context: Vec<String>,
    callee_recursive_frame: Option<RecursiveSccFrame>,
    callee_dependency_frame: Option<DependencyRegionFrame>,
    kind: &'static str,
    recursive_scc_id: Option<String>,
    recursive_entry_context: Option<Vec<String>>,
    dependency_entry_context: Option<Vec<String>>,
}

/// Hybrid finite context policy.
///
/// * Calls that stay in application/local code retain the exact call string.
/// * The first application -> dependency call is an exact push and establishes
///   a dependency-region entry frame.
/// * Dependency -> dependency calls retain that exact entry frame and keep only
///   the newest dependency-local call_key (k=1).
/// * Recursive SCCs remain SCC-local k=1.  A recursive frame may live inside a
///   dependency region; when the SCC is exited, the dependency frame restores
///   the dependency-region quotient.
///
/// This is a semantic partition by producer-certified `body_status.local`, not
/// a graph-size threshold or package-name heuristic.
fn next_context_transition(
    caller: &Activation,
    caller_recursive_frame: Option<&RecursiveSccFrame>,
    caller_dependency_frame: Option<&DependencyRegionFrame>,
    callee_instance: &str,
    call_key: &str,
    sccs: &CallGraphSccs,
    instance_local: &BTreeMap<String, bool>,
) -> Result<ContextTransition, String> {
    let caller_local = *instance_local.get(&caller.instance).ok_or_else(||
        format!("missing local/dependency classification for caller {}", caller.instance))?;
    let callee_local = *instance_local.get(callee_instance).ok_or_else(||
        format!("missing local/dependency classification for callee {callee_instance}"))?;

    // Recursion has the strongest quotient: if caller and callee are in the
    // same recursive SCC, preserve that SCC's exact entry and replace only its
    // local recursive suffix.  The independent dependency frame is propagated
    // so that an eventual SCC exit back into dependency code can return to the
    // coarser dependency-region quotient.
    if let Some(scc_id) = sccs.same_recursive_scc(&caller.instance, callee_instance) {
        let frame = caller_recursive_frame.ok_or_else(|| format!(
            "recursive SCC activation {} lacks its entry frame for {scc_id}", caller.instance
        ))?;
        if frame.scc_id != scc_id {
            return Err(format!(
                "recursive SCC frame mismatch for {}: frame={}, graph={scc_id}",
                caller.instance, frame.scc_id
            ));
        }
        let mut context = frame.entry_context.clone();
        context.push(call_key.to_owned());
        let dep_frame = match (caller_dependency_frame, callee_local) {
            // A dependency frame remains latent while a mixed recursive SCC
            // temporarily executes local callback/application code.  The SCC
            // quotient is already the active precision boundary; retaining the
            // frame is required so a later same-SCC edge back into dependency
            // code resumes the original dependency-region entry rather than
            // fabricating a new one.
            (Some(frame), _) => Some(frame.clone()),
            // A local recursive SCC may enter dependency code for the first
            // time.  Establish that dependency entry at the already-bounded
            // SCC callee context; recursion has already conservatively merged
            // older recursive history.
            (None, false) => Some(DependencyRegionFrame { entry_context: context.clone() }),
            (None, true) => None,
        };
        return Ok(ContextTransition {
            callee_context: context,
            callee_recursive_frame: Some(frame.clone()),
            callee_dependency_frame: dep_frame.clone(),
            kind: "recursive_scc_k1_replace",
            recursive_scc_id: Some(scc_id.to_owned()),
            recursive_entry_context: Some(frame.entry_context.clone()),
            dependency_entry_context: dep_frame.map(|f| f.entry_context),
        });
    }

    // First compute the non-recursive context transition.  Internal dependency
    // calls quotient the prior dependency history to entry + newest callsite;
    // all other boundaries are exact pushes.
    let (context, dep_frame, mut kind) = if !caller_local && !callee_local {
        let frame = caller_dependency_frame.ok_or_else(|| format!(
            "dependency activation {} lacks its entry frame", caller.instance
        ))?;
        let mut context = frame.entry_context.clone();
        context.push(call_key.to_owned());
        (context, Some(frame.clone()), "dependency_region_k1_replace")
    } else {
        let mut context = caller.context.clone();
        context.push(call_key.to_owned());
        if !callee_local {
            let frame = DependencyRegionFrame { entry_context: context.clone() };
            (context, Some(frame), "dependency_region_entry_exact_push")
        } else {
            (context, None, "exact_push")
        }
    };

    // Entering a recursive SCC records the already-computed finite context as
    // the SCC entry.  For exact boundaries preserve the historical R4 name;
    // dependency k=1 entry remains labelled by its actual quotient operation.
    let (recursive_frame, recursive_scc_id, recursive_entry_context) =
        if let Some(scc_id) = sccs.recursive_scc_of(callee_instance) {
            let frame = RecursiveSccFrame {
                scc_id: scc_id.to_owned(),
                entry_context: context.clone(),
            };
            if kind == "exact_push" { kind = "recursive_scc_entry_exact_push"; }
            (Some(frame.clone()), Some(scc_id.to_owned()), Some(frame.entry_context))
        } else {
            (None, None, None)
        };

    Ok(ContextTransition {
        callee_context: context,
        callee_recursive_frame: recursive_frame,
        callee_dependency_frame: dep_frame.clone(),
        kind,
        recursive_scc_id,
        recursive_entry_context,
        dependency_entry_context: dep_frame.map(|f| f.entry_context),
    })
}

fn execution_scope(instance: &str, context: &[String]) -> String {
    let id = execution_state_node_id(&ExecutionStateIdentity {
        code: CanonicalCodeNodeIdentity { concrete_instance_id: instance.to_owned(), basic_block: 0 },
        context: context.to_vec(),
    });
    id.rsplit_once("::bb").expect("execution node id has a block suffix").0.to_owned()
}

fn context_edge(source: String, destination: String, label: Option<String>) -> IcfgEdge {
    IcfgEdge { source, destination, label, source_label: None, destination_label: None }
}

/// Return the canonical pre-context dummyCall proxy owned by one exact represented
/// activation. Normal calls use the compiler-correlated RustCallMetadata. A MIR
/// call with no normal return has no RustCallMetadata return relation, so certify
/// its proxy structurally from the unique `Rust Call -> dummyCall` edge whose
/// DummyCall points to the exact represented callee entry. This proxy is legacy
/// plumbing: P2 rebuilds a context-qualified proxy for every finite activation
/// and must retire the canonical copy even when the call diverges.
fn represented_call_canonical_proxy(
    call_node: &str,
    callee_entry: &str,
    relation: Option<&RustCallMetadata>,
    edge_map: &BTreeMap<String, Vec<IcfgEdge>>,
    node_map: &BTreeMap<String, GlobalICFGNode>,
) -> Result<String, String> {
    if let Some(relation) = relation {
        return Ok(relation.dummy_call_node.clone());
    }
    let candidates: BTreeSet<String> = edge_map
        .get(call_node)
        .into_iter()
        .flatten()
        .filter(|edge| edge.label.as_deref() == Some("Rust Call -> dummyCall"))
        .filter_map(|edge| match node_map.get(&edge.destination) {
            Some(GlobalICFGNode::DummyCall(dummy))
                if dummy.incoming_edge == call_node && dummy.outgoing_edge == callee_entry =>
            {
                Some(edge.destination.clone())
            }
            _ => None,
        })
        .collect();
    if candidates.len() != 1 {
        return Err(format!(
            "diverging represented call {call_node} has {} canonical dummyCall proxies to exact callee entry {callee_entry}",
            candidates.len()
        ));
    }
    Ok(candidates.iter().next().expect("one certified diverging proxy").clone())
}

/// Canonical interprocedural provenance, reconstructed per exact activation.
/// IcfgEdge currently serializes labels rather than a semantic edge-kind enum.
fn is_dep1_matched_unwind_template(edge: &IcfgEdge) -> bool {
    edge.label.as_deref() == Some("DEP1 unwind exit -> matched caller cleanup")
}

/// Canonical A3 edges that encode an interprocedural unwind before P2 context
/// expansion.  These labels are not sufficient evidence by themselves: a
/// candidate edge is elided only after `certify_legacy_cross_body_unwind_template`
/// proves that it is the cleanup relation of the exact activation represented by
/// the current context.
fn is_legacy_cross_body_unwind_template(edge: &IcfgEdge) -> bool {
    matches!(
        edge.label.as_deref(),
        Some("Rust unwind propagate") | Some("Rust drop unwind propagate")
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExactActivationParent {
    call_key: String,
    caller_instance: String,
    callee_instance: String,
    caller_call_node: String,
    activation_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertifiedLegacyUnwindTemplate {
    call_key: String,
    caller_instance: String,
    callee_instance: String,
    canonical_source: String,
    canonical_destination: String,
    source_execution_state: String,
    exact_unwind_action: String,
    /// Exact parent context for an unquotiented activation.  Recursive SCC k=1
    /// and dependency-region k=1 states may merge several parents, so there is
    /// no unique parent context at the callee state itself in those cases.
    parent_context: Option<Vec<String>>,
    callee_context: Vec<String>,
}

/// Prove that a canonical A3 cross-body unwind template is being observed
/// inside a P2 activation with an exact parent.  The static template may fan
/// out to cleanup nodes of *other* callsites of the same canonical body, so its
/// destination is audit provenance only.  The execution successor is rebuilt
/// from COV1 call_key -> exact DEP1 binding -> concrete caller/callee Instance
/// and the exact caller MIR unwind action.  Any identity mismatch remains
/// fail-closed.
fn certify_legacy_cross_body_unwind_template(
    edge: &IcfgEdge,
    source_node: &GlobalICFGNode,
    source_execution_state: &str,
    activation: &Activation,
    recursive_frame: Option<&RecursiveSccFrame>,
    dependency_frame: Option<&DependencyRegionFrame>,
    root_instance: &str,
    parent_by_call_key: &BTreeMap<String, ExactActivationParent>,
    sccs: &CallGraphSccs,
    instance_local: &BTreeMap<String, bool>,
    instance_function: &BTreeMap<String, String>,
    node_map: &BTreeMap<String, GlobalICFGNode>,
) -> Result<Option<CertifiedLegacyUnwindTemplate>, String> {
    if !is_legacy_cross_body_unwind_template(edge) {
        return Ok(None);
    }
    if !is_unwind_exit(source_node) {
        return Err(format!(
            "legacy cross-body unwind template does not originate at a MIR unwind exit: {} -> {}",
            edge.source, edge.destination
        ));
    }
    let Some(call_key) = activation.context.last() else {
        return Err(format!(
            "legacy cross-body unwind template escapes the root activation: {} -> {}",
            edge.source, edge.destination
        ));
    };
    let parent = parent_by_call_key.get(call_key).ok_or_else(|| {
        format!(
            "legacy cross-body unwind template has no exact parent activation for call_key {call_key}"
        )
    })?;
    if parent.callee_instance != activation.instance {
        return Err(format!(
            "legacy cross-body unwind template activation mismatch for {call_key}: expected callee {}, current {}",
            parent.callee_instance, activation.instance
        ));
    }

    // Exact call strings prove the immediate parent directly.  Bounded states
    // (recursive SCC-local k=1 or dependency-region k=1) intentionally merge
    // several histories, so the static call_key proves caller/callee identity
    // while exact parent contexts are recovered from activation records.
    let bounded_recursive_state = recursive_frame
        .is_some_and(|frame| activation.context != frame.entry_context);
    let bounded_dependency_state = dependency_frame
        .is_some_and(|frame| activation.context != frame.entry_context);
    if bounded_recursive_state {
        let frame = recursive_frame.expect("bounded recursive state has frame");
        if sccs.same_recursive_scc(&parent.caller_instance, &parent.callee_instance)
            != Some(frame.scc_id.as_str())
        {
            return Err(format!(
                "legacy cross-body unwind template recursive parent is outside SCC {} for {call_key}",
                frame.scc_id
            ));
        }
    } else if bounded_dependency_state {
        if instance_local.get(&parent.caller_instance).copied() != Some(false)
            || instance_local.get(&parent.callee_instance).copied() != Some(false)
        {
            return Err(format!(
                "legacy cross-body unwind template dependency-k1 parent is not dependency->dependency for {call_key}"
            ));
        }
    } else if activation.context.len() == 1 {
        if parent.caller_instance != root_instance {
            return Err(format!(
                "legacy cross-body unwind template has a non-root parent for first activation {call_key}: {}",
                parent.caller_instance
            ));
        }
    } else if !activation.context.is_empty() {
        let previous_key = &activation.context[activation.context.len() - 2];
        let previous = parent_by_call_key.get(previous_key).ok_or_else(|| {
            format!(
                "legacy cross-body unwind template context has no exact binding for ancestor call_key {previous_key}"
            )
        })?;
        if previous.callee_instance != parent.caller_instance {
            return Err(format!(
                "legacy cross-body unwind template context chain mismatch: {previous_key} enters {}, but {call_key} claims caller {}",
                previous.callee_instance, parent.caller_instance
            ));
        }
    }

    let _caller_function = instance_function.get(&parent.caller_instance).ok_or_else(|| {
        format!(
            "legacy cross-body unwind template caller Instance has no function identity: {}",
            parent.caller_instance
        )
    })?;
    let call_node = node_map.get(&parent.caller_call_node).ok_or_else(|| {
        format!(
            "legacy cross-body unwind template exact caller node is absent: {}",
            parent.caller_call_node
        )
    })?;
    let unwind_target = match (parent.activation_kind.as_str(), call_node) {
        (
            "mir_call",
            GlobalICFGNode::Mir(MirBasicBlock {
                terminator: Some(MirTerminator::Call { unwind_target, .. }),
                ..
            }),
        ) => unwind_target,
        (
            "mir_drop",
            GlobalICFGNode::Mir(MirBasicBlock {
                terminator: Some(MirTerminator::Drop { unwind_target, .. }),
                ..
            }),
        ) => unwind_target,
        _ => {
            return Err(format!(
                "legacy cross-body unwind template exact activation {call_key} has mismatched MIR kind {}",
                parent.activation_kind
            ));
        }
    };
    let parent_context = if bounded_recursive_state || bounded_dependency_state {
        None
    } else {
        Some(activation.context[..activation.context.len() - 1].to_vec())
    };
    Ok(Some(CertifiedLegacyUnwindTemplate {
        call_key: parent.call_key.clone(),
        caller_instance: parent.caller_instance.clone(),
        callee_instance: parent.callee_instance.clone(),
        canonical_source: edge.source.clone(),
        canonical_destination: edge.destination.clone(),
        source_execution_state: source_execution_state.to_owned(),
        exact_unwind_action: unwind_target.clone(),
        parent_context,
        callee_context: activation.context.clone(),
    }))

}

/// Pinned MIR's cleanup target is serialized as `cleanup(bbN)`.  `continue`
/// means that unwinding escapes the current body; it is not a normal return.
fn cleanup_block(target: &str) -> Option<&str> {
    target.strip_prefix("cleanup(")?.strip_suffix(')')
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertifiedOpaqueHigherOrderSummary {
    canonical_call_node: String,
    canonical_callback_proxies: Vec<String>,
    canonical_callback_entries: Vec<String>,
    canonical_callback_return_proxies: Vec<String>,
    canonical_continuation: String,
}

/// Recognize only producer-authored higher-order callback boundaries.  This
/// fallback never treats the canonical Rust callback body as an exact DEP1
/// activation: when no concrete callback Instance certificate exists, the
/// body is intentionally replaced by a conservative opaque effect.
fn certify_opaque_higher_order_summary(
    static_id: &str,
    node: &GlobalICFGNode,
    certified_callback_proxies: &BTreeSet<&str>,
    edge_map: &BTreeMap<String, Vec<IcfgEdge>>,
    node_map: &BTreeMap<String, GlobalICFGNode>,
    rust_calls: &[RustCallMetadata],
) -> Result<Option<CertifiedOpaqueHigherOrderSummary>, String> {
    let GlobalICFGNode::Mir(MirBasicBlock {
        terminator: Some(MirTerminator::Call {
            callback_def_paths, return_target: Some(return_target), ..
        }), ..
    }) = node else { return Ok(None); };
    if callback_def_paths.is_empty() { return Ok(None); }

    let all_callback_edges = edge_map.get(static_id).into_iter().flatten()
        .filter(|edge| edge.label.as_deref() == Some("Higher-order callback -> dummyCall"))
        .collect::<Vec<_>>();
    let callback_edges = all_callback_edges.iter().copied()
        .filter(|edge| !certified_callback_proxies.contains(edge.destination.as_str()))
        .collect::<Vec<_>>();
    if callback_edges.is_empty() { return Ok(None); }
    if callback_edges.len() != all_callback_edges.len() {
        return Err(format!(
            "higher-order callback boundary {static_id} mixes exact and uncertified callback proxies; refusing mixed precise/opaque semantics"
        ));
    }

    let Some(scope) = scope_of_node(static_id) else {
        return Err(format!("higher-order summary caller is not a MIR block: {static_id}"));
    };
    let continuation = format!("rust::{scope}::{return_target}");
    let outgoing = edge_map.get(static_id).into_iter().flatten().collect::<Vec<_>>();
    let direct_continuations = outgoing.iter()
        .filter(|edge| edge.destination == continuation)
        .count();
    if direct_continuations != 1 {
        return Err(format!(
            "higher-order callback boundary {static_id} has {direct_continuations} producer-authored normal continuations to {continuation}"
        ));
    }
    // Once callback proxies are removed, a MIR Call boundary may leave only
    // through its exact normal continuation or its canonical Call-unwind edge.
    // Refuse any more exotic producer shape rather than accidentally leaving a
    // path that bypasses the opaque effect.
    let callback_destinations = all_callback_edges.iter()
        .map(|edge| edge.destination.as_str()).collect::<BTreeSet<_>>();
    let unexpected = outgoing.iter().filter(|edge| {
        !callback_destinations.contains(edge.destination.as_str())
            && edge.destination != continuation
            && edge.label.as_deref() != Some("Call unwind")
    }).map(|edge| format!("{}:{:?}", edge.destination, edge.label)).collect::<Vec<_>>();
    if !unexpected.is_empty() {
        return Err(format!(
            "higher-order callback boundary {static_id} has unsupported non-callback exits: {}",
            unexpected.join(", ")
        ));
    }

    let mut proxies = Vec::new();
    let mut entries = Vec::new();
    let mut return_proxies = Vec::new();
    for edge in callback_edges {
        let Some(GlobalICFGNode::DummyCall(proxy)) = node_map.get(&edge.destination) else {
            return Err(format!("higher-order callback edge at {static_id} does not target a dummyCall"));
        };
        if proxy.incoming_edge != static_id {
            return Err(format!(
                "higher-order callback proxy {} has mismatched incoming provenance {} (expected {static_id})",
                edge.destination, proxy.incoming_edge
            ));
        }
        if !matches!(node_map.get(&proxy.outgoing_edge), Some(GlobalICFGNode::Mir(_))) {
            return Err(format!("higher-order callback proxy {} does not target an existing Rust MIR body", edge.destination));
        }
        let exact_entry_edges = edge_map.get(&edge.destination).into_iter().flatten()
            .filter(|next| next.destination == proxy.outgoing_edge
                && next.label.as_deref() == Some("dummyCall -> Rust Entry"))
            .count();
        if exact_entry_edges != 1 {
            return Err(format!(
                "higher-order callback proxy {} has {exact_entry_edges} canonical Rust-entry edges", edge.destination
            ));
        }
        // RustCallMetadata is the producer-authored pairing between the
        // callback call proxy and its return proxy.  An opaque summary must
        // retire both sides of that legacy plumbing; otherwise the non-MIR
        // preservation pass could reintroduce dummyCall -> Rust Entry or
        // dummyRet -> provider paths that bypass the opaque effect.
        let relations = rust_calls.iter().filter(|call|
            call.call_node == static_id
                && call.is_closure
                && call.dummy_call_node == edge.destination
        ).collect::<Vec<_>>();
        let [relation] = relations.as_slice() else {
            return Err(format!(
                "higher-order callback proxy {} at {static_id} has {} producer RustCallMetadata relations",
                edge.destination, relations.len()
            ));
        };
        let callback_scope = scope_of_node(&proxy.outgoing_edge)
            .ok_or_else(|| format!("higher-order callback proxy {} targets a non-MIR callback entry", edge.destination))?;
        if relation.callee_function.as_str() != callback_scope {
            return Err(format!(
                "higher-order callback proxy {} callee scope mismatch: metadata={} entry={callback_scope}",
                edge.destination, relation.callee_function
            ));
        }
        if !relation.dummy_ret_node.is_empty() {
            let Some(GlobalICFGNode::DummyRet(ret)) = node_map.get(&relation.dummy_ret_node) else {
                return Err(format!(
                    "higher-order callback proxy {} has missing/non-dummyRet return proxy {}",
                    edge.destination, relation.dummy_ret_node
                ));
            };
            let incoming_matches = ret.incoming_edge == relation.callee_function
                || scope_of_node(&ret.incoming_edge).is_some_and(|scope| scope == relation.callee_function.as_str());
            if !incoming_matches {
                return Err(format!(
                    "higher-order callback return proxy {} has mismatched callback provenance {} (expected {})",
                    relation.dummy_ret_node, ret.incoming_edge, relation.callee_function
                ));
            }
            let returns_to_recorded_target = ret.outgoing_edge == relation.return_node
                || edge_map.get(&relation.dummy_ret_node).into_iter().flatten()
                    .any(|next| next.destination == relation.return_node);
            if !returns_to_recorded_target {
                return Err(format!(
                    "higher-order callback return proxy {} does not return to producer-recorded target {}",
                    relation.dummy_ret_node, relation.return_node
                ));
            }
            return_proxies.push(relation.dummy_ret_node.clone());
        }
        proxies.push(edge.destination.clone());
        entries.push(proxy.outgoing_edge.clone());
    }
    proxies.sort();
    proxies.dedup();
    entries.sort();
    entries.dedup();
    return_proxies.sort();
    return_proxies.dedup();
    Ok(Some(CertifiedOpaqueHigherOrderSummary {
        canonical_call_node: static_id.to_owned(),
        canonical_callback_proxies: proxies,
        canonical_callback_entries: entries,
        canonical_callback_return_proxies: return_proxies,
        canonical_continuation: continuation,
    }))
}

fn matched_unwind_relation_label(
    source: &str,
    opaque_dynamic_drop_unwind_states: &BTreeSet<String>,
    opaque_higher_order_unwind_states: &BTreeSet<String>,
) -> &'static str {
    if opaque_dynamic_drop_unwind_states.contains(source) {
        "DYNAMIC_TRAIT_DROP_SUMMARY_UNWIND_V1"
    } else if opaque_higher_order_unwind_states.contains(source) {
        "OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1"
    } else {
        "DEP1 unwind exit -> matched caller cleanup"
    }
}

fn is_unwind_exit(node: &GlobalICFGNode) -> bool {
    let GlobalICFGNode::Mir(block) = node else { return false; };
    match block.terminator.as_ref() {
        Some(MirTerminator::UnwindResume { .. }) => true,
        Some(MirTerminator::Call { unwind_target, .. })
        | Some(MirTerminator::Drop { unwind_target, .. })
        | Some(MirTerminator::Assert { unwind_target, .. }) => {
            unwind_target == "continue"
        }
        Some(MirTerminator::InlineAsm { unwind_target: Some(unwind_target), .. }) => {
            unwind_target == "continue"
        }
        _ => false,
    }
}

/// Build a finite execution graph from the selected application Instance.
/// Returns the graph and its root execution-node identity.  Application/local
/// activations retain exact call strings. Dependency-internal calls use a
/// dependency-region k=1 quotient, recursive SCCs use SCC-local k=1, and the
/// downstream monotone fixed points consume the resulting finite graph. Missing
/// exact body/call correlations remain fail-closed.
pub fn context_expand(icfg: &mut GlobalICFGOrdered, selected_entry: &str) -> Result<String, String> {
    let Some(dep) = icfg.dependency_body_ingestion_v1.clone() else {
        return Ok(selected_entry.to_owned());
    };
    if dep.context_execution_graph_v1.is_none() && dep.exact_call_bindings.iter().any(|b| b["activation_kind"] == "mir_drop") {
        // Optional evidence snapshot of the real producer graph before mutation.
        // It never changes graph construction or downstream program semantics.
        if let Ok(path) = std::env::var("CREMA_INTERNAL_DG2_CANONICAL_GRAPH") {
            let bytes = serde_json::to_vec_pretty(icfg).map_err(|e| e.to_string())?;
            std::fs::write(path,bytes).map_err(|e| format!("DG2 canonical evidence write failed: {e}"))?;
        }
    }
    let represented = represented_statuses(&dep)?;
    let mut instance_local = BTreeMap::<String, bool>::new();
    for (instance, body) in &represented {
        let local = body.get("local").and_then(Value::as_bool).ok_or_else(||
            format!("represented Instance {instance} lacks producer-certified local/dependency classification"))?;
        instance_local.insert(instance.clone(), local);
    }
    let generated_drop_control = dep.exact_call_bindings.iter().any(|b|
        b["activation_kind"] == "mir_drop" && b["body_status"] == "represented_body"
    );
    for binding in &dep.exact_call_bindings {
        if binding["required_program_body"] != true { continue; }
        let target = binding["resolved_instance_id"].as_str()
            .ok_or("required_program_body_unrepresented: unresolved Instance")?;
        if binding["body_status"] != "represented_body" || !represented.contains_key(target) {
            return Err(format!("required_program_body_unrepresented: {target}"));
        }
    }
    let mut instance_function = BTreeMap::<String, String>::new();
    let mut callsite_node_by_key = BTreeMap::<String, String>::new();
    for link in &dep.icfg_callsite_links {
        if let (Some(key), Some(node)) = (link.get("call_key").and_then(Value::as_str), link.get("icfg_node").and_then(Value::as_str)) {
            if callsite_node_by_key.insert(key.to_owned(), node.to_owned()).is_some_and(|old| old != node) {
                return Err(format!("ambiguous ICFG callsite link for {key}"));
            }
        }
    }

    for (instance, body) in &represented {
        if body.get("local").and_then(Value::as_bool) != Some(true) {
            instance_function.insert(instance.clone(), format!("dep1-instance::{instance}"));
        }
    }

    // Exact caller-node relations bind local instances to their producer MIR
    // function scope.  Callee bindings are joined to RustCallMetadata only at
    // the already compiler-correlated call node.
    for binding in &dep.exact_call_bindings {
        let node = binding.get("icfg_node").and_then(Value::as_str)
            .or_else(|| binding.get("call_key").and_then(Value::as_str).and_then(|key| callsite_node_by_key.get(key).map(String::as_str)));
        let Some(node) = node else { continue; };
        if let Some(instance) = binding.get("caller_instance_id").and_then(Value::as_str) {
            if represented.contains_key(instance) {
                if let Some(scope) = scope_of_node(node) {
                    if represented[instance].get("local").and_then(Value::as_bool) == Some(true) {
                        if let Some(previous) = instance_function.insert(instance.to_owned(), scope.to_owned()) {
                            if previous != scope { return Err(format!("ambiguous local body scope for {instance}")); }
                        }
                    }
                }
            }
        }
        let Some(callee) = binding.get("resolved_instance_id").and_then(Value::as_str) else { continue; };
        if !represented.contains_key(callee) || represented[callee].get("local").and_then(Value::as_bool) != Some(true) { continue; }
        let matches: Vec<_> = icfg.rust_calls.iter().filter(|call| call.call_node == node).collect();
        if matches.len() == 1 {
            let function = matches[0].callee_function.clone();
            if let Some(previous) = instance_function.insert(callee.to_owned(), function.clone()) {
                if previous != function { return Err(format!("ambiguous local callee scope for {callee}")); }
            }
        }
    }

    let selected_scope = scope_of_node(selected_entry)
        .ok_or_else(|| format!("selected entry is not a Rust MIR block: {selected_entry}"))?;
    let roots: Vec<_> = represented.iter().filter_map(|(instance, body)| {
        (body.get("local").and_then(Value::as_bool) == Some(true)
            && (instance_function.get(instance).is_some_and(|scope| scope == selected_scope)
                || instance_function.get(instance).is_none()))
            .then_some(instance.clone())
    }).collect();
    let [root_instance] = roots.as_slice() else {
        return Err(format!("expected one compiler-certified root Instance for {selected_entry}, found {}", roots.len()));
    };
    instance_function.insert(root_instance.clone(), selected_scope.to_owned());
    for (instance, function) in &instance_function {
        if !icfg.rust_functions.contains_key(function) {
            return Err(format!("represented Instance {instance} has no Rust function metadata for {function}"));
        }
    }

    let mut binding_by_node = BTreeMap::new();
    let mut callback_bindings_by_node = BTreeMap::<(String,String),Vec<(String,&Value)>>::new();
    for binding in &dep.exact_call_bindings {
        let (Some(key), Some(caller)) = (
            binding.get("call_key").and_then(Value::as_str),
            binding.get("caller_instance_id").and_then(Value::as_str),
        ) else { continue; };
        let Some(node) = binding.get("icfg_node").and_then(Value::as_str)
            .or_else(|| callsite_node_by_key.get(key).map(String::as_str)) else { continue; };
        let slot = (caller.to_owned(), node.to_owned());
        if binding.get("activation_kind").and_then(Value::as_str) == Some("higher_order_callback") {
            callback_bindings_by_node.entry(slot).or_default().push((key.to_owned(),binding));
            continue;
        }
        if let Some((prior_key, prior_binding)) = binding_by_node.insert(slot.clone(), (key.to_owned(), binding)) {
            if prior_key != key || prior_binding != binding {
                return Err(format!("multiple compiler call bindings claim the same caller/call MIR node: {} {}", slot.0, slot.1));
            }
        }
    }
    for callbacks in callback_bindings_by_node.values_mut() {
        callbacks.sort_by(|a,b| a.0.cmp(&b.0));
    }

    let mut call_graph = BTreeMap::<String, BTreeSet<String>>::new();
    let mut represented_calls = BTreeMap::<(String, String), (String, String, &Value, Option<&RustCallMetadata>)>::new();
    for ((caller, node), (key, binding)) in &binding_by_node {
        if binding.get("body_status").and_then(Value::as_str) != Some("represented_body") { continue; }
        let Some(callee) = binding.get("resolved_instance_id").and_then(Value::as_str) else { continue; };
        if !represented.contains_key(callee) || !instance_function.contains_key(callee) { continue; }
        if !instance_function.contains_key(caller) { continue; }
        let matches: Vec<_> = icfg.rust_calls.iter().filter(|call| call.call_node == *node
            && call.callee_function == instance_function[callee]).collect();
        let node_record = icfg.ordered_nodes.iter().find(|(id, _)| id == node)
            .ok_or_else(|| format!("exact call node is absent: {node}"))?;
        let diverges = matches!(&node_record.1, GlobalICFGNode::Mir(MirBasicBlock { terminator: Some(MirTerminator::Call { return_target: None, .. }), .. }));
        let relation = match matches.as_slice() {
            [relation] => Some(*relation),
            [] if diverges => None,
            _ => return Err(format!("call {key} has {} exact RustCallMetadata links", matches.len())),
        };
        represented_calls.insert((caller.clone(), node.clone()),
            (key.clone(), callee.to_owned(), *binding, relation));
        call_graph.entry(caller.clone()).or_default().insert(callee.to_owned());
    }

    // Higher-order callbacks are independent activations rooted at the same MIR
    // Call node as their (possibly out-of-scope) provider.  They are never
    // collapsed into `represented_calls`, because SpawnMayOnce must preserve the
    // direct caller continuation in addition to the callback branch.
    let mut represented_callbacks = BTreeMap::<
        (String,String),
        Vec<(String,String,&Value,&RustCallMetadata)>
    >::new();
    for ((caller,node), callbacks) in &callback_bindings_by_node {
        if !instance_function.contains_key(caller) { continue; }
        for (key,binding) in callbacks {
            if binding.get("body_status").and_then(Value::as_str) != Some("represented_body") { continue; }
            if binding.get("callback_semantics").and_then(Value::as_str) != Some("spawn_may_once") {
                return Err(format!("unsupported exact higher-order callback semantics for {key}"));
            }
            let Some(callee) = binding.get("resolved_instance_id").and_then(Value::as_str) else {
                return Err(format!("higher-order callback {key} lacks a concrete Instance"));
            };
            if !represented.contains_key(callee) || !instance_function.contains_key(callee) {
                return Err(format!("higher-order callback {key} has no represented concrete body"));
            }
            let matches: Vec<_> = icfg.rust_calls.iter().filter(|call|
                call.call_node == *node && call.is_closure
                    && call.callee_function == instance_function[callee]
            ).collect();
            let [relation] = matches.as_slice() else {
                return Err(format!("higher-order callback {key} has {} exact RustCallMetadata links", matches.len()));
            };
            represented_callbacks.entry((caller.clone(),node.clone())).or_default().push((
                key.clone(), callee.to_owned(), *binding, *relation
            ));
            call_graph.entry(caller.clone()).or_default().insert(callee.to_owned());
        }
    }
    for callbacks in represented_callbacks.values_mut() {
        callbacks.sort_by(|a,b| a.0.cmp(&b.0));
    }
    call_graph.entry(root_instance.clone()).or_default();
    let call_sccs = call_graph_sccs(&call_graph, root_instance);

    // Index the exact activation relation by its COV1 call_key.  This is used
    // only to certify canonical A3 unwind templates before they are replaced
    // by the activation-specific P2 relation.  Conflicting keys are fatal.
    let mut parent_by_call_key = BTreeMap::<String, ExactActivationParent>::new();
    for ((caller, call_node), (call_key, callee, binding, _)) in &represented_calls {
        let activation_kind = binding
            .get("activation_kind")
            .and_then(Value::as_str)
            .unwrap_or("mir_call")
            .to_owned();
        let parent = ExactActivationParent {
            call_key: call_key.clone(),
            caller_instance: caller.clone(),
            callee_instance: callee.clone(),
            caller_call_node: call_node.clone(),
            activation_kind,
        };
        if let Some(previous) = parent_by_call_key.insert(call_key.clone(), parent.clone()) {
            if previous != parent {
                return Err(format!(
                    "conflicting exact represented activation records for call_key {call_key}"
                ));
            }
        }
    }

    let node_map: BTreeMap<String, GlobalICFGNode> = icfg.ordered_nodes.iter().cloned().collect();
    if node_map.len() != icfg.ordered_nodes.len() {
        return Err("source ICFG has duplicate node identities before context expansion".into());
    }
    let mut nodes_by_scope = BTreeMap::<String, Vec<(String, GlobalICFGNode)>>::new();
    for (id, node) in &node_map {
        if let Some(scope) = scope_of_node(id) { nodes_by_scope.entry(scope.to_owned()).or_default().push((id.clone(), node.clone())); }
    }
    let mut edge_map = BTreeMap::<String, Vec<IcfgEdge>>::new();
    for edge in &icfg.icfg_edges { edge_map.entry(edge.source.clone()).or_default().push(edge.clone()); }

    // Canonical call proxies are pre-context producer plumbing.  Resolve them
    // once from the exact represented activation relation and retire them after
    // rebuilding the context-qualified proxies.  Diverging MIR calls have no
    // RustCallMetadata return relation, so their dummyCall is certified
    // structurally by `represented_call_canonical_proxy`.
    let mut represented_call_proxy_by_site = BTreeMap::<(String, String), String>::new();
    for ((caller, call_node), (_, callee, _, relation)) in &represented_calls {
        let callee_function = instance_function.get(callee.as_str())
            .ok_or_else(|| format!("missing represented callee function for {callee}"))?;
        let callee_entry = icfg.rust_functions.get(callee_function)
            .ok_or_else(|| format!("missing Rust function metadata for {callee_function}"))?
            .entry_node.as_str();
        let proxy = represented_call_canonical_proxy(
            call_node, callee_entry, *relation, &edge_map, &node_map,
        )?;
        represented_call_proxy_by_site.insert((caller.clone(), call_node.clone()), proxy);
    }

    let root_activation = Activation { instance: root_instance.clone(), context: vec![] };
    let mut queue = VecDeque::from([root_activation.clone()]);
    let mut recursive_frames = BTreeMap::<Activation, RecursiveSccFrame>::new();
    let mut dependency_frames = BTreeMap::<Activation, DependencyRegionFrame>::new();
    if let Some(scc_id) = call_sccs.recursive_scc_of(root_instance) {
        recursive_frames.insert(root_activation.clone(), RecursiveSccFrame {
            scc_id: scc_id.to_owned(),
            entry_context: vec![],
        });
    }
    if instance_local.get(root_instance).copied() == Some(false) {
        dependency_frames.insert(root_activation, DependencyRegionFrame { entry_context: vec![] });
    }
    let mut activations = BTreeSet::new();
    let mut generated_nodes = BTreeMap::<String, GlobalICFGNode>::new();
    let mut generated_edges = Vec::<IcfgEdge>::new();
    let mut generated_functions = BTreeMap::<String, RustFunctionMetadata>::new();
    let mut generated_calls = Vec::<RustCallMetadata>::new();
    let mut state_records = Vec::<Value>::new();
    let mut represented_call_records = Vec::<Value>::new();
    let mut certified_legacy_unwind_templates = Vec::<CertifiedLegacyUnwindTemplate>::new();
    let mut opaque_higher_order_summary_records = Vec::<Value>::new();
    let mut opaque_higher_order_unwind_states = BTreeSet::<String>::new();
    let mut opaque_higher_order_retired_proxies = BTreeSet::<String>::new();
    // Execution states whose MIR Drop is a certified opaque dynamic-trait
    // destructor with `unwind continue`.  When such a state is used as the
    // first source of a cross-activation unwind relation, preserve the opaque
    // summary label so the edge-sensitive abstract transfer widens *all*
    // tracked memory to TOP before ordinary unwind propagation continues.
    let mut opaque_dynamic_drop_unwind_states = BTreeSet::<String>::new();
    let mut static_exec_ids = BTreeMap::<String, BTreeSet<String>>::new();
    let mut proxy_remaps = BTreeMap::<String, BTreeSet<String>>::new();
    let mut entry_by_activation = BTreeMap::<Activation, String>::new();

    while let Some(activation) = queue.pop_front() {
        if !activations.insert(activation.clone()) { continue; }
        let recursive_frame = recursive_frames.get(&activation).cloned();
        let dependency_frame = dependency_frames.get(&activation).cloned();
        let function = instance_function.get(&activation.instance)
            .ok_or_else(|| format!("missing function identity for Instance {}", activation.instance))?;
        let function_meta = &icfg.rust_functions[function];
        let body_nodes = nodes_by_scope.get(function).cloned().unwrap_or_default();
        if body_nodes.is_empty() { return Err(format!("represented body has no MIR nodes: {function}")); }
        let mut state_ids = BTreeMap::<String, String>::new();
        let mut execution_scope_nodes = Vec::<(String, String, GlobalICFGNode)>::new();
        for (static_id, node) in &body_nodes {
            let GlobalICFGNode::Mir(block) = node else { continue; };
            let exec_id = execution_state_node_id(&ExecutionStateIdentity {
                code: CanonicalCodeNodeIdentity { concrete_instance_id: activation.instance.clone(), basic_block: block.block_id as u32 },
                context: activation.context.clone(),
            });
            state_ids.insert(static_id.clone(), exec_id.clone());
            static_exec_ids.entry(static_id.clone()).or_default().insert(exec_id.clone());
            if generated_nodes.insert(exec_id.clone(), node.clone()).is_some() {
                return Err(format!("duplicate canonical execution-state identity at {exec_id}"));
            }
            execution_scope_nodes.push((static_id.clone(), exec_id.clone(), node.clone()));
            state_records.push(json!({
                "execution_node":exec_id,
                "canonical_code_node":{"concrete_instance_id":activation.instance,"basic_block":block.block_id},
                "canonical_node_id":static_id,
                "concrete_instance_id":activation.instance,
                "basic_block":block.block_id,
                "context":activation.context,
                "recursive_scc_id":recursive_frame.as_ref().map(|frame| frame.scc_id.clone()),
                "recursive_entry_context":recursive_frame.as_ref().map(|frame| frame.entry_context.clone()),
                "recursive_context_k":recursive_frame.as_ref().map(|_| 1u64),
                "dependency_entry_context":dependency_frame.as_ref().map(|frame| frame.entry_context.clone()),
                "dependency_context_k":dependency_frame.as_ref().map(|_| 1u64),
            }));
        }
        let entry_bb = function_meta.entry_node.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok())
            .ok_or_else(|| format!("invalid body entry node {}", function_meta.entry_node))?;
        let entry_exec = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: activation.instance.clone(), basic_block: entry_bb }, context: activation.context.clone() });
        if !generated_nodes.contains_key(&entry_exec) { return Err(format!("execution entry missing for {function}")); }
        entry_by_activation.insert(activation.clone(), entry_exec.clone());
        let exec_scope = execution_scope(&activation.instance, &activation.context);
        let return_nodes = function_meta.return_nodes.iter().filter_map(|id| state_ids.get(id).cloned()).collect::<Vec<_>>();
        generated_functions.insert(exec_scope.trim_start_matches("rust::").to_owned(), RustFunctionMetadata { name: exec_scope.trim_start_matches("rust::").to_owned(), arg_count: function_meta.arg_count, entry_node: entry_exec, return_nodes });

        let local_call_nodes: Vec<_> = represented_calls.iter().filter(|((caller, _), _)| caller == &activation.instance).collect();
        let local_by_static_node: BTreeMap<_, _> = local_call_nodes.iter().map(|((_, node), info)| (node.as_str(), *info)).collect();
        let local_callback_nodes: Vec<_> = represented_callbacks.iter().filter(|((caller,_),_)| caller == &activation.instance).collect();
        let local_callbacks_by_static_node: BTreeMap<_,_> = local_callback_nodes.iter()
            .map(|((_,node),callbacks)| (node.as_str(), *callbacks)).collect();
        let local_dynamic_drop_summary_nodes: BTreeSet<&str> = binding_by_node.iter()
            .filter(|((caller,_),(_,binding))| {
                caller == &activation.instance
                    && binding.get("activation_kind").and_then(Value::as_str) == Some("mir_drop")
                    && binding.get("body_status").and_then(Value::as_str) == Some("dynamic_drop_summary")
                    && binding.get("dynamic_drop_summary_v1")
                        .and_then(|summary| summary.get("semantics"))
                        .and_then(Value::as_str) == Some("opaque_dynamic_destructor_may_effect_v1")
            })
            .map(|((_,node),_)| node.as_str())
            .collect();
        let mut local_opaque_higher_order_summaries = BTreeMap::<String, CertifiedOpaqueHigherOrderSummary>::new();
        for (static_id, _, node) in &execution_scope_nodes {
            // If the provider itself has an exact represented-body binding, its
            // MIR body owns the semantics. Never compose that precise activation
            // implicitly with an opaque callback/provider summary.
            if local_by_static_node.contains_key(static_id.as_str()) { continue; }
            let certified_callback_proxies: BTreeSet<&str> = local_callbacks_by_static_node
                .get(static_id.as_str()).into_iter().flat_map(|callbacks| callbacks.iter())
                .map(|(_,_,_,relation)| relation.dummy_call_node.as_str()).collect();
            if let Some(summary) = certify_opaque_higher_order_summary(
                static_id, node, &certified_callback_proxies, &edge_map, &node_map, &icfg.rust_calls,
            )? {
                opaque_higher_order_retired_proxies.extend(summary.canonical_callback_proxies.iter().cloned());
                opaque_higher_order_retired_proxies.extend(summary.canonical_callback_return_proxies.iter().cloned());
                local_opaque_higher_order_summaries.insert(static_id.clone(), summary);
            }
        }
        for (static_id, exec_id, node) in &execution_scope_nodes {
            if local_dynamic_drop_summary_nodes.contains(static_id.as_str()) {
                let GlobalICFGNode::Mir(MirBasicBlock {
                    terminator: Some(MirTerminator::Drop { unwind_target, .. }), ..
                }) = node else {
                    return Err(format!("dynamic Drop summary is attached to a non-Drop MIR node: {static_id}"));
                };
                if unwind_target == "continue" {
                    opaque_dynamic_drop_unwind_states.insert(exec_id.clone());
                }
            }
            if local_opaque_higher_order_summaries.contains_key(static_id) {
                let GlobalICFGNode::Mir(MirBasicBlock {
                    terminator: Some(MirTerminator::Call { unwind_target, .. }), ..
                }) = node else {
                    return Err(format!("opaque higher-order summary is attached to a non-Call MIR node: {static_id}"));
                };
                if unwind_target == "continue" {
                    opaque_higher_order_unwind_states.insert(exec_id.clone());
                }
            }
        }

        // Preserve existing non-DEP1 semantic paths (notably the C/LLVM
        // producer graph) through context-qualified Rust boundaries.  The
        // represented C/LLVM body itself remains unchanged.  If one static
        // foreign boundary is reached under multiple Rust contexts, its shared
        // LLVM graph cannot encode the Rust context pop; fail closed instead
        // of introducing cross-return fanout.
        for (static_id, exec_id, node) in &execution_scope_nodes {
            if local_by_static_node.contains_key(static_id.as_str()) { continue; }
            let Some(MirTerminator::Call { return_target, .. }) = (match node {
                GlobalICFGNode::Mir(block) => block.terminator.as_ref(), _ => None,
            }) else { continue; };
            let certified_callback_proxies: BTreeSet<&str> = local_callbacks_by_static_node
                .get(static_id.as_str()).into_iter().flat_map(|callbacks| callbacks.iter())
                .map(|(_,_,_,relation)| relation.dummy_call_node.as_str()).collect();
            let opaque_callback_proxies: BTreeSet<&str> = local_opaque_higher_order_summaries
                .get(static_id).into_iter().flat_map(|summary| summary.canonical_callback_proxies.iter())
                .map(String::as_str).collect();
            let Some(old_call_proxy) = edge_map.get(static_id).into_iter().flatten()
                .find(|edge| {
                    node_map.get(&edge.destination).is_some_and(|n| matches!(n, GlobalICFGNode::DummyCall(_)))
                        && !certified_callback_proxies.contains(edge.destination.as_str())
                        && !opaque_callback_proxies.contains(edge.destination.as_str())
                })
                .map(|edge| edge.destination.clone()) else { continue; };
            let proxy = encoded_proxy_id("dummyCall", static_id, &activation.context);
            if proxy_remaps.entry(old_call_proxy.clone()).or_default().insert(proxy.clone()) && proxy_remaps[&old_call_proxy].len() > 1 {
                return Err(format!("non-DEP1 semantic boundary {static_id} is shared by multiple execution contexts; exact foreign return context is not representable without cloning C/LLVM"));
            }
            if let Some(GlobalICFGNode::DummyCall(old)) = node_map.get(&old_call_proxy) {
                if scope_of_node(&old.outgoing_edge).is_some() {
                    return Err(format!("nonrepresented call {static_id} still points directly to a Rust body; refusing to fabricate a context without an exact represented binding"));
                }
                let mut cloned = old.clone(); cloned.id = proxy.clone(); cloned.incoming_edge = exec_id.clone();
                generated_nodes.insert(proxy, GlobalICFGNode::DummyCall(cloned));
            }
            let Some(target) = return_target else { continue; };
            let Some(scope) = scope_of_node(static_id) else { continue; };
            let continuation = format!("rust::{scope}::{target}");
            for (old_ret, old_node) in &node_map {
                let GlobalICFGNode::DummyRet(old) = old_node else { continue; };
                let returns_here = old.outgoing_edge == continuation || edge_map.get(old_ret).into_iter().flatten().any(|edge| edge.destination == continuation);
                if !returns_here { continue; }
                let ret_proxy = encoded_proxy_id("dummyRet", static_id, &activation.context);
                let mappings = proxy_remaps.entry(old_ret.clone()).or_default();
                mappings.insert(ret_proxy.clone());
                if mappings.len() > 1 { return Err(format!("non-DEP1 return boundary {old_ret} is shared by multiple execution contexts")); }
                let mut cloned = old.clone(); cloned.id = ret_proxy.clone();
                cloned.outgoing_edge = state_ids.get(&continuation).cloned().ok_or_else(|| format!("foreign return continuation absent: {continuation}"))?;
                generated_nodes.insert(ret_proxy, GlobalICFGNode::DummyRet(cloned));
            }
        }

        // Clone ordinary body control and all nonrepresented semantic paths.
        // Represented call and Return edges are deliberately omitted here.
        for (static_id, exec_id, node) in &execution_scope_nodes {
            let represented_call = local_by_static_node.get(static_id.as_str()).copied();
            let represented_callback_set = local_callbacks_by_static_node.get(static_id.as_str()).copied();
            let is_return = matches!(node, GlobalICFGNode::Mir(MirBasicBlock { terminator: Some(MirTerminator::Return { .. }), .. }));
            for edge in edge_map.get(static_id).into_iter().flatten() {
                // A certified callback proxy is rebuilt below with the concrete
                // callback Instance/context; never clone the shared A3 proxy.
                if represented_callback_set.is_some_and(|callbacks| callbacks.iter().any(|(_,_,_,relation)| relation.dummy_call_node == edge.destination)) {
                    continue;
                }
                if local_opaque_higher_order_summaries.get(static_id).is_some_and(|summary|
                    summary.canonical_callback_proxies.iter().any(|proxy| proxy == &edge.destination))
                {
                    continue;
                }
                // Never clone a shared callee's canonical cleanup fanout.
                // The activation-specific reconstruction below owns this pop.
                if is_dep1_matched_unwind_template(edge) { continue; }
                if let Some(certified) = certify_legacy_cross_body_unwind_template(
                    edge,
                    node,
                    exec_id,
                    &activation,
                    recursive_frame.as_ref(),
                    dependency_frame.as_ref(),
                    root_instance,
                    &parent_by_call_key,
                    &call_sccs,
                    &instance_local,
                    &instance_function,
                    &node_map,
                )? {
                    certified_legacy_unwind_templates.push(certified);
                    continue;
                }
                if represented_call.is_some() { continue; }
                if is_return && edge.label.as_deref().is_some_and(|l| l.contains("Return -> dummyRet")) { continue; }
                let destination = if let Some(mapped) = state_ids.get(&edge.destination) {
                    mapped.clone()
                } else if let Some(mapped_set) = proxy_remaps.get(&edge.destination) {
                    if mapped_set.len() != 1 { return Err(format!("ambiguous semantic proxy mapping for {}", edge.destination)); }
                    mapped_set.iter().next().expect("one proxy mapping").clone()
                } else {
                    edge.destination.clone()
                };
                if edge.label.as_deref().is_some_and(|l| l.contains("Return -> dummyRet")) { continue; }
                if scope_of_node(&destination).is_some() && !state_ids.values().any(|mapped| mapped == &destination) {
                    return Err(format!("unrepresented cross-body Rust edge {} -> {} would bypass an exact body binding", exec_id, destination));
                }
                let cloned_label = if let Some(summary) = local_opaque_higher_order_summaries.get(static_id) {
                    // Every path through an uncertified higher-order provider
                    // must cross the opaque effect at least once.  The provider
                    // itself may read/write/allocate/deallocate even when it
                    // invokes its callback zero times.  Preserve unwind typing
                    // separately so the edge-sensitive transfer also clears the
                    // unassigned return place on exceptional exit.
                    if edge.destination == summary.canonical_continuation {
                        Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1".into())
                    } else if edge.label.as_deref() == Some("Call unwind") {
                        Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1".into())
                    } else {
                        edge.label.clone()
                    }
                } else {
                    edge.label.clone()
                };
                generated_edges.push(context_edge(exec_id.clone(), destination, cloned_label));
            }

            if let Some(summary) = local_opaque_higher_order_summaries.get(static_id) {
                // The normal/unwind exits above execute the opaque provider
                // effect once even when the callback executes zero times.  This
                // self-loop represents any additional hidden callback/provider
                // iterations. Reusing the same execution node makes the
                // abstraction finite and the downstream worklist joins TOP to a
                // fixed point.
                generated_edges.push(context_edge(
                    exec_id.clone(), exec_id.clone(),
                    Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1".into()),
                ));
                eprintln!("DEP1_OPAQUE_HIGHER_ORDER_SUMMARY_V1: {}", json!({
                    "canonical_call_node":summary.canonical_call_node.clone(),
                    "execution_state":exec_id,
                    "callback_count":summary.canonical_callback_proxies.len(),
                    "semantics":"opaque_higher_order_callback_zero_or_more_v1",
                    "provider_effect":"opaque_at_least_once_per_call",
                    "callback_multiplicity":"zero_or_more"
                }));
                opaque_higher_order_summary_records.push(json!({
                    "semantics":"opaque_higher_order_callback_zero_or_more_v1",
                    "provider_effect":"opaque_at_least_once_per_call",
                    "callback_multiplicity":"zero_or_more",
                    "memory_effect":"widen_all_tracked_to_top",
                    "canonical_call_node":summary.canonical_call_node.clone(),
                    "canonical_callback_proxies":summary.canonical_callback_proxies.clone(),
                    "canonical_callback_entries":summary.canonical_callback_entries.clone(),
                    "canonical_callback_return_proxies":summary.canonical_callback_return_proxies.clone(),
                    "canonical_continuation":summary.canonical_continuation.clone(),
                    "caller_instance_id":activation.instance,
                    "context":activation.context,
                    "execution_state":exec_id
                }));
            }

            // Rebuild each certified higher-order callback as its own concrete
            // activation.  SpawnMayOnce deliberately retains the direct caller
            // continuation that was cloned above; the callback branch is an
            // additional MAY execution, not a replacement normal call.
            if let Some(callbacks) = represented_callback_set {
                let GlobalICFGNode::Mir(MirBasicBlock {
                    terminator:Some(MirTerminator::Call {return_target:Some(return_target),..}),..
                }) = node else {
                    return Err(format!("higher-order callback certificate is attached to a non-returning MIR Call: {static_id}"));
                };
                let caller_scope = scope_of_node(static_id).ok_or("invalid callback caller MIR scope")?;
                let continuation_static = format!("rust::{caller_scope}::{return_target}");
                let continuation = state_ids.get(&continuation_static).cloned()
                    .ok_or_else(|| format!("higher-order callback continuation absent: {continuation_static}"))?;
                for (call_key, callee_id, binding, relation) in callbacks {
                    if binding.get("callback_semantics").and_then(Value::as_str) != Some("spawn_may_once") {
                        return Err(format!("unsupported higher-order callback semantics for {call_key}"));
                    }
                    let transition = next_context_transition(
                        &activation,
                        recursive_frame.as_ref(),
                        dependency_frame.as_ref(),
                        callee_id,
                        call_key,
                        &call_sccs,
                        &instance_local,
                    )?;
                    let callee_context = transition.callee_context.clone();
                    let callee_activation = Activation { instance: callee_id.clone(), context: callee_context.clone() };
                    if let Some(frame) = transition.callee_recursive_frame.clone() {
                        if let Some(previous) = recursive_frames.insert(callee_activation.clone(), frame.clone()) {
                            if previous != frame {
                                return Err(format!(
                                    "conflicting recursive SCC frames for callback activation {} {:?}",
                                    callee_activation.instance, callee_activation.context
                                ));
                            }
                        }
                    }
                    if let Some(frame) = transition.callee_dependency_frame.clone() {
                        if let Some(previous) = dependency_frames.insert(callee_activation.clone(), frame.clone()) {
                            if previous != frame {
                                return Err(format!(
                                    "conflicting dependency-region frames for callback activation {} {:?}",
                                    callee_activation.instance, callee_activation.context
                                ));
                            }
                        }
                    }
                    let callee_function = instance_function.get(callee_id.as_str())
                        .ok_or_else(|| format!("missing callback function for {callee_id}"))?;
                    let callee_entry_bb = icfg.rust_functions[callee_function].entry_node
                        .rsplit_once("::bb").and_then(|(_,bb)|bb.parse::<u32>().ok())
                        .ok_or("invalid callback entry")?;
                    let callee_entry = execution_state_node_id(&ExecutionStateIdentity {
                        code:CanonicalCodeNodeIdentity {concrete_instance_id:callee_id.clone(),basic_block:callee_entry_bb},
                        context:callee_context.clone(),
                    });
                    let call_proxy = encoded_proxy_id("dummyCall",call_key,&activation.context);
                    let mut call_dummy = node_map.get(relation.dummy_call_node.as_str())
                        .and_then(|n| match n { GlobalICFGNode::DummyCall(d)=>Some(d.clone()), _=>None })
                        .ok_or_else(|| format!("higher-order callback {call_key} lacks canonical dummyCall"))?;
                    call_dummy.id=call_proxy.clone();
                    call_dummy.incoming_edge=exec_id.clone();
                    call_dummy.outgoing_edge=callee_entry.clone();
                    generated_nodes.insert(call_proxy.clone(),GlobalICFGNode::DummyCall(call_dummy));
                    // If this provider also contains any uncertified callback
                    // component, its hidden provider/callback behavior may
                    // affect memory before dispatching an otherwise exact
                    // callback.  Force that exact branch through the same
                    // opaque effect rather than permitting a precision-only
                    // branch that bypasses the conservative summary.  Fully
                    // certified providers (e.g. spawn_may_once) retain the
                    // exact R3A label and transfer.
                    let callback_branch_label = if local_opaque_higher_order_summaries.contains_key(static_id) {
                        "OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1"
                    } else {
                        "Higher-order callback -> dummyCall"
                    };
                    generated_edges.push(context_edge(
                        exec_id.clone(), call_proxy.clone(), Some(callback_branch_label.into())
                    ));
                    generated_edges.push(context_edge(call_proxy.clone(),callee_entry.clone(),Some("dummyCall -> Rust Entry".into())));
                    queue.push_back(callee_activation);

                    let ret_id = encoded_proxy_id("dummyRet",call_key,&activation.context);
                    let mut return_records=Vec::new();
                    let return_function=&icfg.rust_functions[callee_function];
                    for return_static in &return_function.return_nodes {
                        let return_bb=return_static.rsplit_once("::bb").and_then(|(_,bb)|bb.parse::<u32>().ok())
                            .ok_or("invalid callback Return block")?;
                        let return_exec=execution_state_node_id(&ExecutionStateIdentity {
                            code:CanonicalCodeNodeIdentity {concrete_instance_id:callee_id.clone(),basic_block:return_bb},
                            context:callee_context.clone(),
                        });
                        let mut ret_node=node_map.get(relation.dummy_ret_node.as_str())
                            .and_then(|n|match n { GlobalICFGNode::DummyRet(d)=>Some(d.clone()), _=>None })
                            .unwrap_or(DummyNode {
                                dummy_node_name:"dummyRet".into(),incoming_edge:String::new(),outgoing_edge:continuation.clone(),
                                id:ret_id.clone(),mir_var:None,llvm_var:None,argument_bindings:vec![],is_internal:Some(true)
                            });
                        ret_node.id=ret_id.clone();
                        ret_node.incoming_edge=return_exec.clone();
                        ret_node.outgoing_edge=continuation.clone();
                        generated_nodes.insert(ret_id.clone(),GlobalICFGNode::DummyRet(ret_node));
                        generated_edges.push(context_edge(return_exec.clone(),ret_id.clone(),Some("Rust Return -> dummyRet".into())));
                        generated_edges.push(context_edge(ret_id.clone(),continuation.clone(),Some("dummyRet -> Rust Continuation".into())));
                        return_records.push(json!({"callee_return_state":return_exec,"dummy_ret_state":ret_id,"continuation_state":continuation}));
                    }
                    let callback_exec_scope=execution_scope(callee_id,&callee_context);
                    // `callbacks` is borrowed from the per-node callback index,
                    // so `relation` is a reference to the stored reference.
                    // Clone the metadata value, not the reference, before
                    // context-qualifying its node identities.
                    let mut call_meta: RustCallMetadata=(**relation).clone();
                    call_meta.caller_function=exec_scope.trim_start_matches("rust::").to_owned();
                    call_meta.call_node=exec_id.clone();
                    call_meta.dummy_call_node=call_proxy.clone();
                    call_meta.callee_function=callback_exec_scope.trim_start_matches("rust::").to_owned();
                    call_meta.dummy_ret_node=ret_id.clone();
                    call_meta.return_node=continuation.clone();
                    generated_calls.push(call_meta);
                    represented_call_records.push(json!({
                        "call_key":call_key,
                        "activation_kind":"higher_order_callback",
                        "callback_semantics":"spawn_may_once",
                        "caller_instance_id":activation.instance,
                        "caller_call_state":exec_id,
                        "callee_instance_id":callee_id,
                        "callee_entry_state":callee_entry,
                        "parent_context":activation.context,
                        "callee_context":callee_context,
                        "context_transition":transition.kind,
                        "recursive_scc_id":transition.recursive_scc_id,
                        "recursive_entry_context":transition.recursive_entry_context,
                        "recursive_context_k":transition.recursive_scc_id.as_ref().map(|_|1u64),
                        "dependency_entry_context":transition.dependency_entry_context,
                        "dependency_context_k":transition.callee_dependency_frame.as_ref().map(|_|1u64),
                        "return_matching":match transition.kind {
                            "recursive_scc_k1_replace" => "scc_k1_may_parent",
                            "dependency_region_k1_replace" => "dependency_k1_may_parent",
                            _ => "exact_call_key_pop",
                        },
                        "dummy_call_state":call_proxy,
                        "dummy_ret_state":ret_id,
                        "return_target":return_target,
                        "continuation_state":continuation,
                        "normal_return_relations":return_records,
                        "unwind_action":"detached_callback_no_caller_unwind",
                        "callee_body_status":"represented_body",
                        "direct_continuation_required":true
                    }));
                }
            }

            let Some((call_key, callee_id, binding, relation)) = represented_call else { continue; };
            let drop_return;
            let (return_target,unwind_target) = match node {
                GlobalICFGNode::Mir(MirBasicBlock {terminator:Some(MirTerminator::Call {return_target,unwind_target,..}),..})
                    if binding["activation_kind"] != "mir_drop" => (return_target,unwind_target),
                GlobalICFGNode::Mir(MirBasicBlock {terminator:Some(MirTerminator::Drop {return_target,unwind_target,..}),..})
                    if binding["activation_kind"] == "mir_drop" => {
                        drop_return=Some(return_target.clone()); (&drop_return,unwind_target)
                    },
                _ => return Err(format!("exact represented activation {call_key} has mismatched MIR kind")),
            };
            let old_call_proxy = represented_call_proxy_by_site
                .get(&(activation.instance.clone(), static_id.clone()))
                .ok_or_else(|| format!("represented call {call_key} lacks a certified canonical dummyCall proxy"))?;
            let call_proxy = encoded_proxy_id("dummyCall", call_key, &activation.context);
            let mut call_dummy = node_map.get(old_call_proxy).and_then(|n| match n { GlobalICFGNode::DummyCall(d) => Some(d.clone()), _ => None })
                .unwrap_or(DummyNode { dummy_node_name:"dummyCall".into(), incoming_edge:static_id.clone(), outgoing_edge:String::new(), id:call_proxy.clone(), mir_var:None, llvm_var:None, argument_bindings:vec![], is_internal:Some(true) });
            let transition = next_context_transition(
                &activation,
                recursive_frame.as_ref(),
                dependency_frame.as_ref(),
                callee_id,
                call_key,
                &call_sccs,
                &instance_local,
            )?;
            let callee_context = transition.callee_context.clone();
            let callee_activation = Activation { instance: callee_id.clone(), context: callee_context.clone() };
            if let Some(frame) = transition.callee_recursive_frame.clone() {
                match recursive_frames.insert(callee_activation.clone(), frame.clone()) {
                    Some(previous) if previous != frame => return Err(format!(
                        "recursive SCC activation acquired conflicting entry frames: {:?}", callee_activation
                    )),
                    _ => {}
                }
            }
            if let Some(frame) = transition.callee_dependency_frame.clone() {
                match dependency_frames.insert(callee_activation.clone(), frame.clone()) {
                    Some(previous) if previous != frame => return Err(format!(
                        "dependency-region activation acquired conflicting entry frames: {:?}", callee_activation
                    )),
                    _ => {}
                }
            }
            let callee_function = instance_function.get(callee_id.as_str())
                .ok_or_else(|| format!("missing callee function for {callee_id}"))?;
            let callee_entry_bb = icfg.rust_functions[callee_function].entry_node.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid callee entry")?;
            let callee_entry = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: callee_id.clone(), basic_block: callee_entry_bb }, context: callee_context.clone() });
            call_dummy.id = call_proxy.clone(); call_dummy.incoming_edge = exec_id.clone(); call_dummy.outgoing_edge = callee_entry.clone();
            generated_nodes.insert(call_proxy.clone(), GlobalICFGNode::DummyCall(call_dummy));
            generated_edges.push(context_edge(exec_id.clone(), call_proxy.clone(), Some("Rust Call -> dummyCall".into())));
            generated_edges.push(context_edge(call_proxy.clone(), callee_entry.clone(), Some("dummyCall -> Rust Entry".into())));
            queue.push_back(callee_activation);

            let mut call_meta = relation.map(|r| r.clone());
            if let Some(call_meta) = call_meta.as_mut() {
                call_meta.caller_function = exec_scope.trim_start_matches("rust::").to_owned();
                call_meta.call_node = exec_id.clone();
                call_meta.dummy_call_node = call_proxy.clone();
                call_meta.callee_function = execution_scope(&callee_id, &callee_context).trim_start_matches("rust::").to_owned();
                call_meta.dummy_ret_node = if return_target.is_some() { encoded_proxy_id("dummyRet", call_key, &activation.context) } else { String::new() };
            }
            let mut continuation_record = Value::Null;
            let mut return_records = Vec::<Value>::new();
            if let Some(target) = return_target {
                let call_meta = call_meta.as_mut().ok_or_else(|| format!("normal represented call has no RustCallMetadata: {call_key}"))?;
                let caller_scope = scope_of_node(static_id).ok_or("invalid caller MIR scope")?;
                let continuation_static = format!("rust::{caller_scope}::{target}");
                let continuation = state_ids.get(&continuation_static).cloned().ok_or_else(|| format!("MIR return_target {continuation_static} not present in caller activation"))?;
                continuation_record = json!(continuation);
                let return_function = &icfg.rust_functions[callee_function];
                for return_static in &return_function.return_nodes {
                    let return_bb = return_static.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid callee Return block")?;
                    let return_exec = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: callee_id.clone(), basic_block: return_bb }, context: callee_context.clone() });
                    let ret_id = call_meta.dummy_ret_node.clone();
                    let ret_node = node_map.get(relation.expect("normal call relation").dummy_ret_node.as_str()).and_then(|n| match n { GlobalICFGNode::DummyRet(d) => Some(d.clone()), _ => None })
                        .unwrap_or(DummyNode { dummy_node_name:"dummyRet".into(), incoming_edge:String::new(), outgoing_edge:continuation.clone(), id:ret_id.clone(), mir_var:None, llvm_var:None, argument_bindings:vec![], is_internal:Some(true) });
                    let mut ret_node = ret_node; ret_node.id = ret_id.clone(); ret_node.incoming_edge = return_exec.clone(); ret_node.outgoing_edge = continuation.clone();
                    generated_nodes.insert(ret_id.clone(), GlobalICFGNode::DummyRet(ret_node));
                    generated_edges.push(context_edge(return_exec.clone(), ret_id.clone(), Some("Rust Return -> dummyRet".into())));
                    generated_edges.push(context_edge(ret_id.clone(), continuation.clone(), Some("dummyRet -> Rust Continuation".into())));
                    return_records.push(json!({"callee_return_state":return_exec,"dummy_ret_state":ret_id,"continuation_state":continuation}));
                }
                call_meta.return_node = continuation;
            } else {
                if let Some(call_meta) = call_meta.as_mut() { call_meta.dummy_ret_node.clear(); }
            }
            // Preserve MIR unwind action as provenance only.  Activation-specific
            // cleanup/continue/terminate relations are reconstructed after the
            // finite SCC-local context graph is closed, by a monotone fixed point.
            represented_call_records.push(json!({
                "call_key":call_key,
                "caller_instance_id":activation.instance,
                "caller_call_state":exec_id,
                "callee_instance_id":callee_id,
                "callee_entry_state":callee_entry,
                "parent_context":activation.context,
                "callee_context":callee_context,
                "context_transition":transition.kind,
                "recursive_scc_id":transition.recursive_scc_id,
                "recursive_entry_context":transition.recursive_entry_context,
                "recursive_context_k":transition.recursive_scc_id.as_ref().map(|_|1u64),
                "dependency_entry_context":transition.dependency_entry_context,
                "dependency_context_k":transition.callee_dependency_frame.as_ref().map(|_|1u64),
                "return_matching":match transition.kind {
                    "recursive_scc_k1_replace" => "scc_k1_may_parent",
                    "dependency_region_k1_replace" => "dependency_k1_may_parent",
                    _ => "exact_call_key_pop",
                },
                "dummy_call_state":call_proxy,
                "dummy_ret_state":if return_target.is_some() {call_meta.as_ref().map(|m|m.dummy_ret_node.clone()).unwrap_or_default()} else {String::new()},
                "return_target":return_target,
                "continuation_state":continuation_record,
                "normal_return_relations":return_records,
                "unwind_action":unwind_target,
                "callee_body_status":"represented_body"
            }));
            {
                let record = represented_call_records.last_mut().expect("activation record");
                record["activation_kind"] = json!(if binding["activation_kind"] == "mir_drop" { "mir_drop" } else { "mir_call" });
            }
            if let Some(call_meta) = call_meta { generated_calls.push(call_meta); }
            let _ = binding;
        }
    }

    if !represented_call_records.is_empty() || generated_drop_control || !certified_legacy_unwind_templates.is_empty() {
        // Recursive and dependency quotients invalidate the old activation-tree
        // assumption. Compute unwind-exit summaries as a monotone least fixed
        // point over the finite hybrid-context activation graph. Sets only grow and
        // the node domain is finite, so this terminates without recursion-depth
        // truncation or selection of one history over another.
        let mut exits = BTreeMap::<Activation,BTreeSet<String>>::new();
        for owner in activations.iter().cloned() {
            let function = &instance_function[&owner.instance];
            let mut owner_exits = BTreeSet::new();
            for (static_id,node) in &nodes_by_scope[function] {
                // A represented Call/Drop with Continue is an activation, not
                // an ordinary unwind-exit bypass at its caller callsite.
                if represented_calls.contains_key(&(owner.instance.clone(),static_id.clone())) { continue; }
                if !is_unwind_exit(node) { continue; }
                let GlobalICFGNode::Mir(block)=node else { continue; };
                owner_exits.insert(execution_state_node_id(&ExecutionStateIdentity {
                    code:CanonicalCodeNodeIdentity {concrete_instance_id:owner.instance.clone(),basic_block:block.block_id as u32},
                    context:owner.context.clone()}));
            }
            exits.insert(owner,owner_exits);
        }

        // `continue` propagates the child's unwind summary to an owner-local
        // proxy. Repeat until no owner acquires a new exit. Detached spawned
        // callbacks deliberately do not propagate panic into the spawning
        // thread and therefore contribute no caller unwind exit.
        loop {
            let mut changed=false;
            for record in &represented_call_records {
                if record.get("activation_kind").and_then(Value::as_str)==Some("higher_order_callback") { continue; }
                if record.get("unwind_action").and_then(Value::as_str)!=Some("continue") { continue; }
                let owner=Activation {
                    instance:record["caller_instance_id"].as_str().ok_or("activation caller Instance")?.to_owned(),
                    context:record["parent_context"].as_array().ok_or("activation parent context")?.iter()
                        .map(|s|s.as_str().map(str::to_owned).ok_or("invalid parent context")).collect::<Result<_,_>>()?,
                };
                let child=Activation {
                    instance:record["callee_instance_id"].as_str().ok_or("callee Instance")?.to_owned(),
                    context:record["callee_context"].as_array().ok_or("callee context")?.iter()
                        .map(|s|s.as_str().map(str::to_owned).ok_or("invalid callee context")).collect::<Result<_,_>>()?,
                };
                let child_has_exit=exits.get(&child).is_some_and(|set|!set.is_empty());
                if !child_has_exit { continue; }
                let key=record["call_key"].as_str().ok_or("activation without key")?;
                let proxy=encoded_proxy_id("unwindExit",key,&owner.context);
                generated_nodes.entry(proxy.clone()).or_insert_with(|| GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name:"unwindExit".into(),incoming_edge:String::new(),outgoing_edge:String::new(),
                    id:proxy.clone(),mir_var:None,llvm_var:None,argument_bindings:vec![],is_internal:Some(true)}));
                if exits.entry(owner).or_default().insert(proxy) { changed=true; }
            }
            if !changed { break; }
        }

        // Materialize each exact/quotiented activation's unwind relation using
        // the stable exit summaries. Multiple recursive or dependency histories
        // that map to one bounded state contribute to the same MAY successors.
        for record in &mut represented_call_records {
            if record.get("activation_kind").and_then(Value::as_str)==Some("higher_order_callback") {
                record["unwind_return_relations"]=json!([]);
                continue;
            }
            let key=record["call_key"].as_str().ok_or("activation without key")?.to_owned();
            let owner=Activation {
                instance:record["caller_instance_id"].as_str().ok_or("activation caller Instance")?.to_owned(),
                context:record["parent_context"].as_array().ok_or("activation parent context")?.iter()
                    .map(|s|s.as_str().map(str::to_owned).ok_or("invalid parent context")).collect::<Result<_,_>>()?,
            };
            let child=Activation {
                instance:record["callee_instance_id"].as_str().ok_or("callee Instance")?.to_owned(),
                context:record["callee_context"].as_array().ok_or("callee context")?.iter()
                    .map(|s|s.as_str().map(str::to_owned).ok_or("invalid callee context")).collect::<Result<_,_>>()?,
            };
            let child_exits=exits.get(&child).cloned().unwrap_or_default();
            let action=record["unwind_action"].as_str().ok_or("unwind action")?.to_owned();
            let destination=if let Some(bb)=cleanup_block(&action) {
                let index=bb.strip_prefix("bb").and_then(|s|s.parse::<u32>().ok()).ok_or("invalid cleanup BB")?;
                Some(execution_state_node_id(&ExecutionStateIdentity {
                    code:CanonicalCodeNodeIdentity {concrete_instance_id:owner.instance.clone(),basic_block:index},
                    context:owner.context.clone()}))
            } else if action=="continue" && !child_exits.is_empty() {
                Some(encoded_proxy_id("unwindExit",&key,&owner.context))
            } else if action=="terminate" && !child_exits.is_empty() {
                let terminal=encoded_proxy_id("terminal",&key,&owner.context);
                generated_nodes.entry(terminal.clone()).or_insert_with(|| GlobalICFGNode::Terminal(
                    crate::structs::TerminalNode {reason:"unwind_terminate".into()}));
                Some(terminal)
            } else { None };
            let mut relations=Vec::new();
            if let Some(to)=destination {
                if !generated_nodes.contains_key(&to) {return Err(format!("matched unwind destination missing for {key}"));}
                for from in child_exits {
                    // The first propagation edge leaving an opaque dynamic
                    // destructor must retain the opaque label. Otherwise an
                    // `unwind continue` path would be transferred by the generic
                    // Drop-unwind rule, which only widens the dropped local and
                    // would under-approximate hidden destructor side effects.
                    let label = matched_unwind_relation_label(
                        &from,
                        &opaque_dynamic_drop_unwind_states,
                        &opaque_higher_order_unwind_states,
                    );
                    generated_edges.push(context_edge(from.clone(),to.clone(),Some(label.into())));
                    relations.push(json!({"callee_unwind_state":from,"caller_unwind_state":to,"unwind_action":action,
                        "caller_context":owner.context,"callee_context":child.context,
                        "edge_semantics":label}));
                }
            }
            record["unwind_return_relations"]=json!(relations);
        }
    }

    let root_function = instance_function[root_instance].clone();
    let root_meta = &icfg.rust_functions[&root_function];
    let root_bb = root_meta.entry_node.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid root entry block")?;
    let root_exec = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: root_instance.clone(), basic_block: root_bb }, context:vec![] });
    let mut replaced_proxy_ids = BTreeSet::<String>::new();
    // Every represented activation rebuilds its dummyCall in the finite context
    // graph. Retire the canonical proxy for both returning and diverging calls;
    // otherwise a legacy dummyCall -> Rust Entry edge can re-enter a represented
    // dependency Instance without an activation certificate and becomes
    // ambiguous as soon as that Instance has more than one bounded context.
    replaced_proxy_ids.extend(represented_call_proxy_by_site.values().cloned());
    for (_, _, _, relation) in represented_calls.values() {
        if let Some(relation) = relation {
            if !relation.dummy_ret_node.is_empty() { replaced_proxy_ids.insert(relation.dummy_ret_node.clone()); }
        }
    }
    for callbacks in represented_callbacks.values() {
        for (_,_,_,relation) in callbacks {
            replaced_proxy_ids.insert(relation.dummy_call_node.clone());
            if !relation.dummy_ret_node.is_empty() { replaced_proxy_ids.insert(relation.dummy_ret_node.clone()); }
        }
    }
    // Opaque HOC summaries replace the complete producer-authored callback
    // proxy plumbing.  Retire both dummyCall and dummyRet proxies so the
    // generic non-MIR preservation pass cannot resurrect an uncertified Rust
    // callback body or a legacy callback-return loop.
    replaced_proxy_ids.extend(opaque_higher_order_retired_proxies);
    replaced_proxy_ids.extend(proxy_remaps.keys().cloned());
    for (id, node) in &node_map {
        if scope_of_node(id).is_some() || replaced_proxy_ids.contains(id) { continue; }
        generated_nodes.entry(id.clone()).or_insert_with(|| node.clone());
    }
    // Non-MIR producer edges (including the existing paired C/LLVM path) are
    // retained once.  Only their Rust boundary proxies/continuations are
    // rewritten.  A shared proxy that would need two context destinations was
    // rejected above rather than flattened.
    for edge in &icfg.icfg_edges {
        if scope_of_node(&edge.source).is_some() { continue; }
        if (replaced_proxy_ids.contains(&edge.source) && !proxy_remaps.contains_key(&edge.source))
            || (replaced_proxy_ids.contains(&edge.destination) && !proxy_remaps.contains_key(&edge.destination)) { continue; }
        let remap_endpoint = |endpoint: &str| -> Result<String, String> {
            if let Some(set) = proxy_remaps.get(endpoint) {
                if set.len() != 1 { return Err(format!("non-MIR edge endpoint has ambiguous context mapping: {endpoint}")); }
                return Ok(set.iter().next().expect("one mapping").clone());
            }
            if let Some(set) = static_exec_ids.get(endpoint) {
                if set.len() != 1 {
                    return Err(format!("non-MIR edge endpoint {endpoint} is reached under multiple Rust contexts"));
                }
                return Ok(set.iter().next().expect("one mapping").clone());
            }
            if scope_of_node(endpoint).is_some() {
                return Err(format!("non-MIR producer edge targets an unrepresented Rust body node: {endpoint}"));
            }
            Ok(endpoint.to_owned())
        };
        let source = remap_endpoint(&edge.source)?;
        let destination = remap_endpoint(&edge.destination)?;
        generated_edges.push(context_edge(source, destination, edge.label.clone()));
    }
    generated_edges.sort_by(|a,b| (&a.source,&a.destination,&a.label).cmp(&(&b.source,&b.destination,&b.label)));
    generated_edges.dedup_by(|a,b| a.source==b.source && a.destination==b.destination && a.label==b.label);
    let generated_ids: BTreeSet<_> = generated_nodes.keys().cloned().collect();
    for edge in &generated_edges {
        if !generated_ids.contains(&edge.source) || !generated_ids.contains(&edge.destination) {
            return Err(format!("context graph edge is not closed over generated nodes: {} -> {}", edge.source, edge.destination));
        }
    }
    // Every canonical A3 cross-body unwind template that was elided above is
    // justified by an exact activation.  When that activation can unwind, the
    // bottom-up reconstruction must contain the exact source state once.  For
    // `unreachable`, no replacement edge is correct by construction.
    for certified in &certified_legacy_unwind_templates {
        let matching_records: Vec<_> = represented_call_records.iter().filter(|record| {
            record["call_key"].as_str()==Some(certified.call_key.as_str())
                && record["callee_context"]==json!(certified.callee_context)
        }).collect();
        if matching_records.is_empty() {
            return Err(format!(
                "certified legacy unwind template has no represented activation record for {}",
                certified.call_key
            ));
        }
        let should_have_successor=certified.exact_unwind_action!="unreachable";
        let mut any_relation=false;
        for record in matching_records {
            let relations=record["unwind_return_relations"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            if !relations.is_empty() { any_relation=true; }
            if !should_have_successor && !relations.is_empty() {
                return Err(format!(
                    "unreachable exact unwind action unexpectedly acquired a successor for {}",
                    certified.call_key
                ));
            }
            for relation in relations {
                if relation["callee_context"]!=json!(certified.callee_context) {
                    return Err(format!(
                        "certified legacy unwind template replacement changed callee context for {}",
                        certified.call_key
                    ));
                }
                if let Some(parent)=certified.parent_context.as_ref() {
                    if relation["caller_context"]!=json!(parent) {
                        return Err(format!(
                            "certified legacy unwind template replacement changed exact caller context for {}",
                            certified.call_key
                        ));
                    }
                }
            }
        }
        if should_have_successor && !any_relation {
            return Err(format!(
                "certified legacy unwind template lost its activation-specific unwind relation for {}: action={}",
                certified.call_key,certified.exact_unwind_action
            ));
        }
    }



    for call in &represented_call_records {
        let call_key = call["call_key"].as_str().unwrap_or("<missing-call-key>");
        let call_state = call["caller_call_state"].as_str().unwrap_or("");
        let call_proxy = call["dummy_call_state"].as_str().unwrap_or("");
        let callee_entry = call["callee_entry_state"].as_str().unwrap_or("");
        if generated_edges.iter().filter(|e| e.source == call_state && e.destination == call_proxy).count() != 1
            || generated_edges.iter().filter(|e| e.source == call_proxy && e.destination == callee_entry).count() != 1 {
            return Err(format!("represented call {call_key} does not have exactly one call->entry relation"));
        }
        if let Some(continuation) = call["continuation_state"].as_str() {
            let direct_count=generated_edges.iter().filter(|e| e.source == call_state && e.destination == continuation).count();
            if call.get("activation_kind").and_then(Value::as_str) == Some("higher_order_callback") {
                if call.get("callback_semantics").and_then(Value::as_str) != Some("spawn_may_once") || direct_count != 1 {
                    return Err(format!("represented callback {call_key} does not preserve exactly one SpawnMayOnce direct continuation"));
                }
            } else if direct_count != 0 {
                return Err(format!("represented call {call_key} retains a direct continuation bypass"));
            }
        }
        for relation in call["normal_return_relations"].as_array().into_iter().flatten() {
            let from = relation["callee_return_state"].as_str().unwrap_or("");
            let proxy = relation["dummy_ret_state"].as_str().unwrap_or("");
            let to = relation["continuation_state"].as_str().unwrap_or("");
            if generated_edges.iter().filter(|e| e.source == from && e.destination == proxy).count() != 1
                || generated_edges.iter().filter(|e| e.source == proxy && e.destination == to).count() != 1 {
                return Err(format!("represented call {call_key} has an unpaired normal return"));
            }
        }
    }
    let certified_legacy_unwind_records = certified_legacy_unwind_templates
        .iter()
        .map(|record| {
            json!({
                "call_key":record.call_key,
                "caller_instance_id":record.caller_instance,
                "callee_instance_id":record.callee_instance,
                "canonical_source":record.canonical_source,
                "canonical_destination":record.canonical_destination,
                "source_execution_state":record.source_execution_state,
                "exact_unwind_action":record.exact_unwind_action,
                "parent_context":record.parent_context,
                "callee_context":record.callee_context,
                "replacement_relation":"DEP1 unwind exit -> matched caller cleanup"
            })
        })
        .collect::<Vec<_>>();
    let execution_state_count = state_records.len();
    let activation_count = activations.len();
    let max_execution_context_depth = activations.iter().map(|a| a.context.len()).max().unwrap_or(0);
    let count = generated_nodes.len();
    let canonical_count = state_records.iter().map(|r| r["canonical_code_node"].to_string()).collect::<BTreeSet<_>>().len();
    icfg.ordered_nodes = generated_nodes.into_iter().collect();
    icfg.icfg_edges = generated_edges;
    icfg.rust_functions = generated_functions;
    icfg.rust_calls = generated_calls;
    if let Some(dep) = icfg.dependency_body_ingestion_v1.as_mut() {
        let recursive_scc_records=call_sccs.members.iter()
            .filter(|(id,_)|call_sccs.recursive_sccs.contains(*id))
            .map(|(id,members)|json!({"scc_id":id,"members":members,"context_policy":"local_call_string_k1"}))
            .collect::<Vec<_>>();
        let has_recursive_scc = !call_sccs.recursive_sccs.is_empty();
        let local_instance_count = instance_local.values().filter(|local| **local).count();
        let dependency_instance_count = instance_local.len().saturating_sub(local_instance_count);
        let has_dependency_region = dependency_instance_count > 0;
        let capability = if has_dependency_region {
            "context_explicit_icfg_v4"
        } else if has_recursive_scc {
            "context_explicit_icfg_v3"
        } else {
            "context_explicit_icfg_v1"
        };
        let status = if has_dependency_region {
            "complete_with_exact_local_context_dependency_region_k1_and_scc_local_k1"
        } else if has_recursive_scc {
            "complete_with_exact_outer_context_and_scc_local_k1"
        } else {
            "complete_for_acyclic_represented_rust_calls_and_certified_callbacks"
        };
        dep.context_execution_graph_v1 = Some(json!({
            "capability":capability,
            "status":status,
            "p2_control_flow_complete":true,
            "local_context_policy":"exact_call_string",
            "dependency_context_policy":if has_dependency_region {Some("entry_exact_then_dependency_internal_call_string_k1")} else {None::<&str>},
            "dependency_context_k":if has_dependency_region {Some(1u64)} else {None::<u64>},
            "recursive_context_policy":if has_recursive_scc {Some("scc_local_call_string_k1")} else {None::<&str>},
            "recursive_context_k":if has_recursive_scc {Some(1u64)} else {None::<u64>},
            "local_instance_count":local_instance_count,
            "dependency_instance_count":dependency_instance_count,
            "activation_count":activation_count,
            "max_execution_context_depth":max_execution_context_depth,
            "acquired_bodies":represented.len(),"canonical_code_nodes":canonical_count,"execution_states":execution_state_count,
            "proxy_and_non_mir_nodes":count.saturating_sub(execution_state_count),
            "states":state_records,"represented_call_activations":represented_call_records,
            "certified_legacy_cross_body_unwind_templates":certified_legacy_unwind_records,
            "opaque_higher_order_summaries":opaque_higher_order_summary_records,
            "call_graph_scc":if call_sccs.recursive_sccs.is_empty(){"acyclic"}else{"recursive_scc_local_k1"},
            "recursive_sccs":recursive_scc_records
        }));
    }
    // Certify that the preserved ledger is deterministically addressable by
    // the finalized activation identities. This read-only view is not exported.
    let _ = rust_activation_unwind_metadata_from_execution_graph(icfg)?;
    Ok(root_exec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matched_unwind_template_filter_is_exact() {
        for label in ["Call unwind", "Drop unwind", "Assert unwind", "InlineAsm unwind",
            "Rust unwind propagate", "Rust drop unwind propagate", "Goto", "not-unwind-diagnostic"] {
            assert!(!is_dep1_matched_unwind_template(&context_edge("u".into(), "v".into(), Some(label.into()))));
        }
        assert!(is_dep1_matched_unwind_template(&context_edge("u".into(), "v".into(),
            Some("DEP1 unwind exit -> matched caller cleanup".into()))));
        assert!(!is_dep1_matched_unwind_template(&context_edge("u".into(), "v".into(), None)));
    }

    #[test]
    fn cleanup_target_is_structural_and_exact() {
        assert_eq!(cleanup_block("cleanup(bb12)"), Some("bb12"));
        assert_eq!(cleanup_block("continue"), None);
        assert_eq!(cleanup_block("unreachable"), None);
        assert_eq!(cleanup_block("cleanup(bb1)junk"), None);
    }

    #[test]
    fn unwind_exit_is_not_a_normal_return() {
        let unwind = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 4,
            statements: vec![],
            terminator: Some(MirTerminator::UnwindResume { details: "resume".into(), source_info: "".into() }),
        });
        let normal = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 5,
            statements: vec![],
            terminator: Some(MirTerminator::Return { details: "return".into(), source_info: "".into() }),
        });
        assert!(is_unwind_exit(&unwind));
        assert!(!is_unwind_exit(&normal));
    }

    #[test]
    fn foreign_proxy_identity_is_call_and_context_qualified() {
        assert_ne!(
            encoded_proxy_id("dummyRet", "site-A", &["outer-A".into()]),
            encoded_proxy_id("dummyRet", "site-B", &["outer-B".into()]),
        );
    }


    #[test]
    fn recursive_scc_detection_is_reachable_and_exact() {
        let graph=BTreeMap::from([
            ("root".into(),BTreeSet::from(["a".into()])),
            ("a".into(),BTreeSet::from(["b".into()])),
            ("b".into(),BTreeSet::from(["a".into()])),
            ("dead".into(),BTreeSet::from(["dead".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"root");
        let ab=sccs.same_recursive_scc("a","b").expect("a/b mutual recursion").to_owned();
        assert!(sccs.recursive_sccs.contains(&ab));
        assert!(sccs.scc_of("root").is_some());
        assert!(sccs.scc_of("dead").is_none(),"unreachable SCC must not affect selected root");
    }

    #[test]
    fn scc_local_k1_preserves_exact_entry_and_replaces_only_recursive_suffix() {
        let graph=BTreeMap::from([
            ("outer".into(),BTreeSet::from(["a".into()])),
            ("a".into(),BTreeSet::from(["b".into()])),
            ("b".into(),BTreeSet::from(["a".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"outer");
        let local=BTreeMap::from([("outer".into(),true),("a".into(),true),("b".into(),true)]);
        let outer=Activation {instance:"outer".into(),context:vec!["OUTER".into()]};
        let entry=next_context_transition(&outer,None,None,"a","ENTER",&sccs,&local).unwrap();
        assert_eq!(entry.kind,"recursive_scc_entry_exact_push");
        assert_eq!(entry.callee_context,vec!["OUTER","ENTER"]);
        let a=Activation {instance:"a".into(),context:entry.callee_context.clone()};
        let ab=next_context_transition(&a,entry.callee_recursive_frame.as_ref(),None,"b","AB",&sccs,&local).unwrap();
        assert_eq!(ab.callee_context,vec!["OUTER","ENTER","AB"]);
        let b=Activation {instance:"b".into(),context:ab.callee_context.clone()};
        let ba=next_context_transition(&b,ab.callee_recursive_frame.as_ref(),None,"a","BA",&sccs,&local).unwrap();
        assert_eq!(ba.callee_context,vec!["OUTER","ENTER","BA"]);
        let a_again=Activation {instance:"a".into(),context:ba.callee_context.clone()};
        let again=next_context_transition(&a_again,ba.callee_recursive_frame.as_ref(),None,"b","AB",&sccs,&local).unwrap();
        assert_eq!(again.callee_context,ab.callee_context,"recursive history must converge at k=1");
    }

    #[test]
    fn dependency_region_k1_preserves_exact_application_entry_and_replaces_internal_suffix() {
        let graph=BTreeMap::from([
            ("app".into(),BTreeSet::from(["dep-a".into()])),
            ("dep-a".into(),BTreeSet::from(["dep-b".into()])),
            ("dep-b".into(),BTreeSet::from(["dep-c".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"app");
        let local=BTreeMap::from([
            ("app".into(),true),("dep-a".into(),false),("dep-b".into(),false),("dep-c".into(),false),
        ]);
        let app=Activation {instance:"app".into(),context:vec!["APP-OUTER".into()]};
        let enter=next_context_transition(&app,None,None,"dep-a","ENTER",&sccs,&local).unwrap();
        assert_eq!(enter.kind,"dependency_region_entry_exact_push");
        assert_eq!(enter.callee_context,vec!["APP-OUTER","ENTER"]);
        let dep_entry=enter.callee_dependency_frame.as_ref().expect("dependency entry frame");
        assert_eq!(dep_entry.entry_context,vec!["APP-OUTER","ENTER"]);

        let a=Activation {instance:"dep-a".into(),context:enter.callee_context.clone()};
        let ab=next_context_transition(&a,None,Some(dep_entry),"dep-b","AB",&sccs,&local).unwrap();
        assert_eq!(ab.kind,"dependency_region_k1_replace");
        assert_eq!(ab.callee_context,vec!["APP-OUTER","ENTER","AB"]);

        let b=Activation {instance:"dep-b".into(),context:ab.callee_context.clone()};
        let bc=next_context_transition(&b,None,ab.callee_dependency_frame.as_ref(),"dep-c","BC",&sccs,&local).unwrap();
        assert_eq!(bc.callee_context,vec!["APP-OUTER","ENTER","BC"],
            "dependency history before the newest internal callsite must be quotiented");
    }

    #[test]
    fn dependency_region_k1_merges_acyclic_history_at_shared_static_edge() {
        let graph=BTreeMap::from([
            ("app".into(),BTreeSet::from(["a".into()])),
            ("a".into(),BTreeSet::from(["b".into(),"c".into()])),
            ("b".into(),BTreeSet::from(["d".into()])),
            ("c".into(),BTreeSet::from(["d".into()])),
            ("d".into(),BTreeSet::from(["e".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"app");
        let local=BTreeMap::from([
            ("app".into(),true),("a".into(),false),("b".into(),false),
            ("c".into(),false),("d".into(),false),("e".into(),false),
        ]);
        let app=Activation {instance:"app".into(),context:vec![]};
        let enter=next_context_transition(&app,None,None,"a","ENTER",&sccs,&local).unwrap();
        let a=Activation {instance:"a".into(),context:enter.callee_context.clone()};
        let b=next_context_transition(&a,None,enter.callee_dependency_frame.as_ref(),"b","AB",&sccs,&local).unwrap();
        let c=next_context_transition(&a,None,enter.callee_dependency_frame.as_ref(),"c","AC",&sccs,&local).unwrap();
        let b_state=Activation {instance:"b".into(),context:b.callee_context.clone()};
        let c_state=Activation {instance:"c".into(),context:c.callee_context.clone()};
        let d1=next_context_transition(&b_state,None,b.callee_dependency_frame.as_ref(),"d","BD",&sccs,&local).unwrap();
        let d2=next_context_transition(&c_state,None,c.callee_dependency_frame.as_ref(),"d","CD",&sccs,&local).unwrap();
        assert_ne!(d1.callee_context,d2.callee_context,"different immediate callsites stay distinguished");
        let d1_state=Activation {instance:"d".into(),context:d1.callee_context.clone()};
        let d2_state=Activation {instance:"d".into(),context:d2.callee_context.clone()};
        let e1=next_context_transition(&d1_state,None,d1.callee_dependency_frame.as_ref(),"e","DE",&sccs,&local).unwrap();
        let e2=next_context_transition(&d2_state,None,d2.callee_dependency_frame.as_ref(),"e","DE",&sccs,&local).unwrap();
        assert_eq!(e1.callee_context,e2.callee_context,
            "histories that reach the same dependency callsite must converge at k=1");
        assert_eq!(e1.callee_context,vec!["ENTER","DE"]);
    }

    #[test]
    fn local_calls_remain_exact_even_when_reentered_from_bounded_dependency_context() {
        let graph=BTreeMap::from([
            ("app".into(),BTreeSet::from(["dep".into()])),
            ("dep".into(),BTreeSet::from(["callback".into()])),
            ("callback".into(),BTreeSet::from(["helper".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"app");
        let local=BTreeMap::from([
            ("app".into(),true),("dep".into(),false),("callback".into(),true),("helper".into(),true),
        ]);
        let app=Activation {instance:"app".into(),context:vec![]};
        let enter=next_context_transition(&app,None,None,"dep","ENTER",&sccs,&local).unwrap();
        let dep=Activation {instance:"dep".into(),context:enter.callee_context.clone()};
        let cb=next_context_transition(&dep,None,enter.callee_dependency_frame.as_ref(),"callback","CALLBACK",&sccs,&local).unwrap();
        assert_eq!(cb.kind,"exact_push");
        assert!(cb.callee_dependency_frame.is_none());
        assert_eq!(cb.callee_context,vec!["ENTER","CALLBACK"]);
        let callback=Activation {instance:"callback".into(),context:cb.callee_context.clone()};
        let helper=next_context_transition(&callback,None,None,"helper","HELPER",&sccs,&local).unwrap();
        assert_eq!(helper.callee_context,vec!["ENTER","CALLBACK","HELPER"]);
    }

    #[test]
    fn recursive_scc_inside_dependency_region_is_finite_and_returns_to_dependency_quotient() {
        let graph=BTreeMap::from([
            ("app".into(),BTreeSet::from(["dep-a".into()])),
            ("dep-a".into(),BTreeSet::from(["rec".into()])),
            ("rec".into(),BTreeSet::from(["rec".into(),"dep-z".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"app");
        let local=BTreeMap::from([
            ("app".into(),true),("dep-a".into(),false),("rec".into(),false),("dep-z".into(),false),
        ]);
        let app=Activation {instance:"app".into(),context:vec![]};
        let enter=next_context_transition(&app,None,None,"dep-a","ENTER",&sccs,&local).unwrap();
        let dep=Activation {instance:"dep-a".into(),context:enter.callee_context.clone()};
        let rec_entry=next_context_transition(&dep,None,enter.callee_dependency_frame.as_ref(),"rec","TO-REC",&sccs,&local).unwrap();
        assert_eq!(rec_entry.kind,"dependency_region_k1_replace");
        let rec=Activation {instance:"rec".into(),context:rec_entry.callee_context.clone()};
        let recur=next_context_transition(&rec,rec_entry.callee_recursive_frame.as_ref(),rec_entry.callee_dependency_frame.as_ref(),"rec","RECUR",&sccs,&local).unwrap();
        assert_eq!(recur.kind,"recursive_scc_k1_replace");
        let rec_again=Activation {instance:"rec".into(),context:recur.callee_context.clone()};
        let exit=next_context_transition(&rec_again,recur.callee_recursive_frame.as_ref(),recur.callee_dependency_frame.as_ref(),"dep-z","EXIT",&sccs,&local).unwrap();
        assert_eq!(exit.kind,"dependency_region_k1_replace");
        assert_eq!(exit.callee_context,vec!["ENTER","EXIT"],
            "exiting the recursive SCC must restore the dependency entry quotient");
    }

    #[test]
    fn mixed_recursive_scc_retains_dependency_frame_across_local_callback() {
        let graph=BTreeMap::from([
            ("app".into(),BTreeSet::from(["dep".into()])),
            ("dep".into(),BTreeSet::from(["callback".into()])),
            ("callback".into(),BTreeSet::from(["dep".into()])),
        ]);
        let sccs=call_graph_sccs(&graph,"app");
        let local=BTreeMap::from([
            ("app".into(),true),("dep".into(),false),("callback".into(),true),
        ]);
        let app=Activation {instance:"app".into(),context:vec![]};
        let enter=next_context_transition(&app,None,None,"dep","ENTER",&sccs,&local).unwrap();
        let dep=Activation {instance:"dep".into(),context:enter.callee_context.clone()};
        let cb=next_context_transition(
            &dep,
            enter.callee_recursive_frame.as_ref(),
            enter.callee_dependency_frame.as_ref(),
            "callback","TO-CB",&sccs,&local,
        ).unwrap();
        assert_eq!(cb.kind,"recursive_scc_k1_replace");
        assert!(cb.callee_dependency_frame.is_some(),
            "dependency frame must stay latent while the mixed SCC executes local code");
        let callback=Activation {instance:"callback".into(),context:cb.callee_context.clone()};
        let back=next_context_transition(
            &callback,
            cb.callee_recursive_frame.as_ref(),
            cb.callee_dependency_frame.as_ref(),
            "dep","BACK",&sccs,&local,
        ).unwrap();
        assert_eq!(back.kind,"recursive_scc_k1_replace");
        assert_eq!(back.dependency_entry_context,enter.dependency_entry_context);
        assert!(back.callee_dependency_frame.is_some());
    }

    fn call_block(unwind_target: &str) -> GlobalICFGNode {
        GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: "child".into(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: true,
                callback_def_paths: vec![],
                resolved_instance_callees: vec![],
                instance_dispatch_observed: true,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![],
                return_place: "_0".into(),
                return_target: Some("bb1".into()),
                unwind_target: unwind_target.into(),
            }),
        })
    }

    #[test]
    fn legacy_cross_body_unwind_requires_exact_activation_and_cleanup() {
        let activation = Activation {
            instance: "child-instance".into(),
            context: vec!["K".into()],
        };
        let mut parents = BTreeMap::new();
        parents.insert(
            "K".into(),
            ExactActivationParent {
                call_key: "K".into(),
                caller_instance: "root-instance".into(),
                callee_instance: "child-instance".into(),
                caller_call_node: "rust::root::bb0".into(),
                activation_kind: "mir_call".into(),
            },
        );
        let functions = BTreeMap::from([
            ("root-instance".into(), "root".into()),
            ("child-instance".into(), "child".into()),
        ]);
        let local = BTreeMap::from([
            ("root-instance".into(), true),
            ("child-instance".into(), true),
        ]);
        let source = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 9,
            statements: vec![],
            terminator: Some(MirTerminator::UnwindResume {
                details: String::new(),
                source_info: String::new(),
            }),
        });
        let nodes = BTreeMap::from([("rust::root::bb0".into(), call_block("cleanup(bb2)"))]);
        let edge = context_edge(
            "rust::child::bb9".into(),
            "rust::root::bb2".into(),
            Some("Rust unwind propagate".into()),
        );
        let source_exec = execution_state_node_id(&ExecutionStateIdentity {
            code: CanonicalCodeNodeIdentity {
                concrete_instance_id: "child-instance".into(),
                basic_block: 9,
            },
            context: vec!["K".into()],
        });
        let certified = certify_legacy_cross_body_unwind_template(
            &edge,
            &source,
            &source_exec,
            &activation,
            None,
            None,
            "root-instance",
            &parents,
            &CallGraphSccs::default(),
            &local,
            &functions,
            &nodes,
        )
        .unwrap()
        .expect("exact legacy unwind must be certified");
        assert_eq!(certified.call_key, "K");
        assert_eq!(certified.canonical_destination, "rust::root::bb2");
        assert_eq!(certified.exact_unwind_action, "cleanup(bb2)");
        assert_eq!(certified.parent_context, Some(Vec::<String>::new()));
    }

    #[test]
    fn legacy_cross_body_unwind_other_static_fanout_is_elided_under_exact_context() {
        let activation = Activation {
            instance: "child-instance".into(),
            context: vec!["K".into()],
        };
        let parents = BTreeMap::from([
            (
                "K".into(),
                ExactActivationParent {
                    call_key: "K".into(),
                    caller_instance: "root-instance".into(),
                    callee_instance: "child-instance".into(),
                    caller_call_node: "rust::root::bb0".into(),
                    activation_kind: "mir_call".into(),
                },
            ),
        ]);
        let functions = BTreeMap::from([
            ("root-instance".into(), "root".into()),
            ("child-instance".into(), "child".into()),
        ]);
        let local = BTreeMap::from([
            ("root-instance".into(), true),
            ("child-instance".into(), true),
        ]);
        let source = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 9,
            statements: vec![],
            terminator: Some(MirTerminator::UnwindResume {
                details: String::new(),
                source_info: String::new(),
            }),
        });
        let nodes = BTreeMap::from([("rust::root::bb0".into(), call_block("cleanup(bb2)"))]);
        let edge = context_edge(
            "rust::child::bb9".into(),
            "rust::root::bb3".into(),
            Some("Rust unwind propagate".into()),
        );
        let certified = certify_legacy_cross_body_unwind_template(
            &edge,
            &source,
            "exec-child-bb9",
            &activation,
            None,
            None,
            "root-instance",
            &parents,
            &CallGraphSccs::default(),
            &local,
            &functions,
            &nodes,
        )
        .unwrap()
        .expect("static fanout is a template, not the execution successor");
        assert_eq!(certified.canonical_destination, "rust::root::bb3");
        assert_eq!(certified.exact_unwind_action, "cleanup(bb2)");
    }

    #[test]
    fn dependency_k1_legacy_unwind_uses_activation_records_not_fabricated_exact_parent() {
        let activation=Activation {instance:"child-dep".into(),context:vec!["ENTER".into(),"K".into()]};
        let dep_frame=DependencyRegionFrame {entry_context:vec!["ENTER".into()]};
        let parents=BTreeMap::from([(
            "K".into(),ExactActivationParent {call_key:"K".into(),caller_instance:"parent-dep".into(),
                callee_instance:"child-dep".into(),caller_call_node:"rust::parent::bb0".into(),activation_kind:"mir_call".into()}
        )]);
        let functions=BTreeMap::from([("parent-dep".into(),"parent".into()),("child-dep".into(),"child".into())]);
        let local=BTreeMap::from([("parent-dep".into(),false),("child-dep".into(),false)]);
        let source=GlobalICFGNode::Mir(MirBasicBlock {block_id:9,statements:vec![],
            terminator:Some(MirTerminator::UnwindResume {details:String::new(),source_info:String::new()})});
        let nodes=BTreeMap::from([("rust::parent::bb0".into(),call_block("cleanup(bb2)"))]);
        let edge=context_edge("rust::child::bb9".into(),"rust::parent::bb2".into(),Some("Rust unwind propagate".into()));
        let certified=certify_legacy_cross_body_unwind_template(
            &edge,&source,"exec-child",&activation,None,Some(&dep_frame),"root-app",&parents,
            &CallGraphSccs::default(),&local,&functions,&nodes).unwrap().expect("dependency k1 unwind must certify");
        assert_eq!(certified.parent_context,None,
            "bounded dependency state has multiple possible exact parents; do not fabricate one");
        assert_eq!(certified.callee_context,vec!["ENTER","K"]);
    }

    #[test]
    fn context_expand_replaces_legacy_unwind_fanout_with_exact_context_pop() {
        let root_call = call_block("cleanup(bb2)");
        let root_ret = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 1,
            statements: vec![],
            terminator: Some(MirTerminator::Return {
                details: String::new(),
                source_info: String::new(),
            }),
        });
        let root_cleanup = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 2,
            statements: vec![],
            terminator: Some(MirTerminator::Return {
                details: String::new(),
                source_info: String::new(),
            }),
        });
        let child_unwind = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 0,
            statements: vec![],
            terminator: Some(MirTerminator::UnwindResume {
                details: String::new(),
                source_info: String::new(),
            }),
        });
        let mut graph = GlobalICFGOrdered {
            ordered_nodes: vec![
                ("rust::root::bb0".into(), root_call),
                ("rust::root::bb1".into(), root_ret),
                ("rust::root::bb2".into(), root_cleanup),
                ("rust::child::bb0".into(), child_unwind),
            ],
            icfg_edges: vec![
                context_edge(
                    "rust::child::bb0".into(),
                    "rust::root::bb2".into(),
                    Some("Rust unwind propagate".into()),
                ),
                // Same canonical body, another static caller cleanup.  This
                // fanout must not survive in the K execution context.
                context_edge(
                    "rust::child::bb0".into(),
                    "rust::other_caller::bb7".into(),
                    Some("Rust unwind propagate".into()),
                ),
            ],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: BTreeMap::from([
                (
                    "root".into(),
                    RustFunctionMetadata {
                        name: "root".into(),
                        arg_count: 0,
                        entry_node: "rust::root::bb0".into(),
                        return_nodes: vec!["rust::root::bb1".into(), "rust::root::bb2".into()],
                    },
                ),
                (
                    "child".into(),
                    RustFunctionMetadata {
                        name: "child".into(),
                        arg_count: 0,
                        entry_node: "rust::child::bb0".into(),
                        return_nodes: vec![],
                    },
                ),
            ]),
            rust_calls: vec![RustCallMetadata {
                caller_function: "root".into(),
                call_node: "rust::root::bb0".into(),
                callee_function: "child".into(),
                dummy_call_node: "dummyCall::old".into(),
                dummy_ret_node: "dummyRet::old".into(),
                arguments: vec![],
                return_place: "_0".into(),
                return_node: "rust::root::bb1".into(),
                is_closure: false,
            }],
            dependency_body_ingestion_v1: Some(DependencyBodyIngestionExport {
                capability: "dependency_body_ingestion_v1".into(),
                target_triple: "test".into(),
                crate_correlations: vec![],
                package_scope: vec![],
                body_statuses: vec![
                    json!({"instance_id":"root-instance","body_status":"represented_body","local":true}),
                    json!({"instance_id":"child-instance","body_status":"represented_body","local":true}),
                ],
                exact_call_bindings: vec![json!({
                    "call_key":"K",
                    "caller_instance_id":"root-instance",
                    "resolved_instance_id":"child-instance",
                    "body_status":"represented_body",
                    "activation_kind":"mir_call",
                    "icfg_node":"rust::root::bb0"
                })],
                icfg_callsite_links: vec![json!({
                    "call_key":"K",
                    "icfg_node":"rust::root::bb0"
                })],
                icfg_stitching_v1: None,
                context_execution_graph_v1: None,
            }),
        };

        let root = context_expand(&mut graph, "rust::root::bb0")
            .expect("exact activation must replace canonical unwind fanout");
        let child_exec = execution_state_node_id(&ExecutionStateIdentity {
            code: CanonicalCodeNodeIdentity {
                concrete_instance_id: "child-instance".into(),
                basic_block: 0,
            },
            context: vec!["K".into()],
        });
        let cleanup_exec = execution_state_node_id(&ExecutionStateIdentity {
            code: CanonicalCodeNodeIdentity {
                concrete_instance_id: "root-instance".into(),
                basic_block: 2,
            },
            context: vec![],
        });
        assert_eq!(
            root,
            execution_state_node_id(&ExecutionStateIdentity {
                code: CanonicalCodeNodeIdentity {
                    concrete_instance_id: "root-instance".into(),
                    basic_block: 0,
                },
                context: vec![],
            })
        );
        assert!(!graph.icfg_edges.iter().any(|edge| {
            edge.label.as_deref() == Some("Rust unwind propagate")
                || edge.label.as_deref() == Some("Rust drop unwind propagate")
        }));
        assert_eq!(
            graph
                .icfg_edges
                .iter()
                .filter(|edge| {
                    edge.source == child_exec
                        && edge.destination == cleanup_exec
                        && edge.label.as_deref()
                            == Some("DEP1 unwind exit -> matched caller cleanup")
                })
                .count(),
            1
        );
        let audit = graph
            .dependency_body_ingestion_v1
            .as_ref()
            .and_then(|dep| dep.context_execution_graph_v1.as_ref())
            .expect("context graph audit");
        assert_eq!(audit["capability"], "context_explicit_icfg_v1");
        assert_eq!(audit["call_graph_scc"], "acyclic");
        assert!(audit["recursive_context_policy"].is_null());
        assert_eq!(
            audit["certified_legacy_cross_body_unwind_templates"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
    }


    #[test]
    fn context_expand_dynamic_trait_drop_stays_opaque_and_does_not_create_rust_activation() {
        let drop_node = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:0, statements:vec![],
            terminator:Some(MirTerminator::Drop {
                details:String::new(), source_info:String::new(),
                return_target:"bb1".into(), unwind_target:"continue".into(),
                dropped_value:"Local(_1) -> *".into(), is_mutable:false,
                deallocator_evidence:None,
            }),
        });
        let ret = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:1, statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let mut graph = GlobalICFGOrdered {
            ordered_nodes:vec![("rust::root::bb0".into(),drop_node),("rust::root::bb1".into(),ret)],
            icfg_edges:vec![context_edge(
                "rust::root::bb0".into(),"rust::root::bb1".into(),
                Some("DYNAMIC_TRAIT_DROP_SUMMARY_V1".into())
            )],
            llvm_memory_effects:None, svf_solved_points_to:None,
            rust_functions:BTreeMap::from([("root".into(),RustFunctionMetadata {
                name:"root".into(),arg_count:1,entry_node:"rust::root::bb0".into(),
                return_nodes:vec!["rust::root::bb1".into()]
            })]),
            rust_calls:vec![],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),
                crate_correlations:vec![],package_scope:vec![],
                body_statuses:vec![json!({"instance_id":"root-instance","body_status":"represented_body","local":true})],
                exact_call_bindings:vec![json!({
                    "call_key":"dep1-drop-v1:root-instance:bb0:mir_drop:dyn-glue",
                    "caller_instance_id":"root-instance","resolved_instance_id":"dyn-glue",
                    "body_status":"dynamic_drop_summary","activation_kind":"mir_drop",
                    "required_program_body":false,"icfg_node":"rust::root::bb0",
                    "dynamic_drop_summary_v1":{
                        "semantics":"opaque_dynamic_destructor_may_effect_v1",
                        "normal_memory_effect":"widen_all_tracked_to_top",
                        "unwind_memory_effect":"widen_all_tracked_to_top"
                    }
                })],
                icfg_callsite_links:vec![json!({
                    "call_key":"dep1-drop-v1:root-instance:bb0:mir_drop:dyn-glue",
                    "icfg_node":"rust::root::bb0"
                })],
                icfg_stitching_v1:None,context_execution_graph_v1:None,
            }),
        };
        let root = context_expand(&mut graph,"rust::root::bb0")
            .expect("opaque dynamic Drop must preserve finite control without a fabricated callee");
        let continuation = execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:1},
            context:vec![],
        });
        assert!(graph.icfg_edges.iter().any(|edge|
            edge.source==root && edge.destination==continuation
                && edge.label.as_deref()==Some("DYNAMIC_TRAIT_DROP_SUMMARY_V1")));
        assert!(graph.rust_calls.is_empty(),"opaque dynamic Drop must not fabricate RustCallMetadata");
        let audit=graph.dependency_body_ingestion_v1.as_ref().unwrap().context_execution_graph_v1.as_ref().unwrap();
        assert_eq!(audit["capability"],"context_explicit_icfg_v1");
        assert!(audit["represented_call_activations"].as_array().unwrap().is_empty());
    }

    #[test]
    fn context_expand_dynamic_drop_continue_widens_before_matched_parent_cleanup() {
        let root_call = call_block("cleanup(bb2)");
        let root_ret = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:1, statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let root_cleanup = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:2, statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let child_drop = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:0, statements:vec![],
            terminator:Some(MirTerminator::Drop {
                details:String::new(),source_info:String::new(),return_target:"bb1".into(),
                unwind_target:"continue".into(),dropped_value:"Local(_1) -> *".into(),
                is_mutable:false,deallocator_evidence:None,
            }),
        });
        let child_ret = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:1, statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let mut graph = GlobalICFGOrdered {
            ordered_nodes:vec![
                ("rust::root::bb0".into(),root_call),
                ("rust::root::bb1".into(),root_ret),
                ("rust::root::bb2".into(),root_cleanup),
                ("rust::child::bb0".into(),child_drop),
                ("rust::child::bb1".into(),child_ret),
            ],
            icfg_edges:vec![
                context_edge("rust::child::bb0".into(),"rust::child::bb1".into(),
                    Some("DYNAMIC_TRAIT_DROP_SUMMARY_V1".into())),
            ],
            llvm_memory_effects:None,svf_solved_points_to:None,
            rust_functions:BTreeMap::from([
                ("root".into(),RustFunctionMetadata {name:"root".into(),arg_count:0,
                    entry_node:"rust::root::bb0".into(),return_nodes:vec!["rust::root::bb1".into(),"rust::root::bb2".into()]}),
                ("child".into(),RustFunctionMetadata {name:"child".into(),arg_count:1,
                    entry_node:"rust::child::bb0".into(),return_nodes:vec!["rust::child::bb1".into()]}),
            ]),
            rust_calls:vec![RustCallMetadata {
                caller_function:"root".into(),call_node:"rust::root::bb0".into(),callee_function:"child".into(),
                dummy_call_node:"dummyCall::old".into(),dummy_ret_node:"dummyRet::old".into(),
                arguments:vec![],return_place:"_0".into(),return_node:"rust::root::bb1".into(),is_closure:false,
            }],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),
                crate_correlations:vec![],package_scope:vec![],
                body_statuses:vec![
                    json!({"instance_id":"root-instance","body_status":"represented_body","local":true}),
                    json!({"instance_id":"child-instance","body_status":"represented_body","local":true}),
                ],
                exact_call_bindings:vec![
                    json!({"call_key":"K","caller_instance_id":"root-instance",
                        "resolved_instance_id":"child-instance","body_status":"represented_body",
                        "activation_kind":"mir_call","icfg_node":"rust::root::bb0"}),
                    json!({"call_key":"DYN","caller_instance_id":"child-instance",
                        "resolved_instance_id":"dyn-glue","body_status":"dynamic_drop_summary",
                        "activation_kind":"mir_drop","required_program_body":false,
                        "icfg_node":"rust::child::bb0",
                        "dynamic_drop_summary_v1":{"semantics":"opaque_dynamic_destructor_may_effect_v1",
                            "normal_memory_effect":"widen_all_tracked_to_top",
                            "unwind_memory_effect":"widen_all_tracked_to_top"}}),
                ],
                icfg_callsite_links:vec![
                    json!({"call_key":"K","icfg_node":"rust::root::bb0"}),
                    json!({"call_key":"DYN","icfg_node":"rust::child::bb0"}),
                ],
                icfg_stitching_v1:None,context_execution_graph_v1:None,
            }),
        };
        context_expand(&mut graph,"rust::root::bb0")
            .expect("opaque child Drop must unwind conservatively to exact parent cleanup");
        let child_drop_exec=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"child-instance".into(),basic_block:0},
            context:vec!["K".into()],
        });
        let cleanup_exec=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:2},
            context:vec![],
        });
        assert_eq!(graph.icfg_edges.iter().filter(|edge|
            edge.source==child_drop_exec && edge.destination==cleanup_exec
                && edge.label.as_deref()==Some("DYNAMIC_TRAIT_DROP_SUMMARY_UNWIND_V1")
        ).count(),1);
        assert!(!graph.icfg_edges.iter().any(|edge|
            edge.source==child_drop_exec && edge.destination==cleanup_exec
                && edge.label.as_deref()==Some("DEP1 unwind exit -> matched caller cleanup")));
    }

    #[test]
    fn context_expand_opaque_higher_order_continue_widens_before_matched_parent_cleanup() {
        let mut graph=spawn_callback_graph(false);
        if let Some((_,GlobalICFGNode::Mir(block)))=graph.ordered_nodes.iter_mut()
            .find(|(id,_)|id=="rust::root::bb0")
        {
            if let Some(MirTerminator::Call {unwind_target,..})=block.terminator.as_mut() {
                *unwind_target="continue".into();
            }
        }
        let parent_call=call_block("cleanup(bb2)");
        let ret=|block_id| GlobalICFGNode::Mir(MirBasicBlock {
            block_id,statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        graph.ordered_nodes.extend([
            ("rust::parent::bb0".into(),parent_call),
            ("rust::parent::bb1".into(),ret(1)),
            ("rust::parent::bb2".into(),ret(2)),
        ]);
        graph.rust_functions.insert("parent".into(),RustFunctionMetadata {
            name:"parent".into(),arg_count:0,entry_node:"rust::parent::bb0".into(),
            return_nodes:vec!["rust::parent::bb1".into(),"rust::parent::bb2".into()],
        });
        graph.rust_calls.push(RustCallMetadata {
            caller_function:"parent".into(),call_node:"rust::parent::bb0".into(),callee_function:"root".into(),
            dummy_call_node:"dummyCall::parent-old".into(),dummy_ret_node:"dummyRet::parent-old".into(),
            arguments:vec![],return_place:"_0".into(),return_node:"rust::parent::bb1".into(),is_closure:false,
        });
        let dep=graph.dependency_body_ingestion_v1.as_mut().unwrap();
        dep.body_statuses.push(json!({"instance_id":"parent-instance","body_status":"represented_body","local":true}));
        dep.exact_call_bindings.push(json!({
            "call_key":"K","caller_instance_id":"parent-instance","resolved_instance_id":"root-instance",
            "body_status":"represented_body","activation_kind":"mir_call","icfg_node":"rust::parent::bb0"
        }));
        dep.icfg_callsite_links.push(json!({"call_key":"K","icfg_node":"rust::parent::bb0"}));

        context_expand(&mut graph,"rust::parent::bb0")
            .expect("opaque higher-order provider unwind-continue must reach exact parent cleanup conservatively");
        let child_call_exec=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:0},
            context:vec!["K".into()],
        });
        let cleanup_exec=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"parent-instance".into(),basic_block:2},
            context:vec![],
        });
        assert_eq!(graph.icfg_edges.iter().filter(|edge|
            edge.source==child_call_exec && edge.destination==cleanup_exec
                && edge.label.as_deref()==Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1")
        ).count(),1);
        assert!(!graph.icfg_edges.iter().any(|edge|
            edge.source==child_call_exec && edge.destination==cleanup_exec
                && edge.label.as_deref()==Some("DEP1 unwind exit -> matched caller cleanup")));
    }

    #[test]
    fn opaque_dynamic_drop_continue_is_preserved_on_first_cross_activation_unwind_edge() {
        // This local regression protects the first cross-activation unwind
        // label for both opaque dynamic destructors and opaque higher-order
        // providers. The end-to-end fixtures above exercise graph construction.
        let opaque = BTreeSet::from(["rust::exec::opaque-drop".to_string()]);
        let opaque_hoc = BTreeSet::from(["rust::exec::opaque-hoc".to_string()]);
        assert_eq!(
            matched_unwind_relation_label("rust::exec::opaque-drop", &opaque, &opaque_hoc),
            "DYNAMIC_TRAIT_DROP_SUMMARY_UNWIND_V1",
        );
        assert_eq!(
            matched_unwind_relation_label("rust::exec::opaque-hoc", &opaque, &opaque_hoc),
            "OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1",
        );
        assert_eq!(
            matched_unwind_relation_label("rust::exec::ordinary-exit", &opaque, &opaque_hoc),
            "DEP1 unwind exit -> matched caller cleanup",
        );
    }

    #[test]
    fn context_expand_direct_recursion_converges_with_scc_local_k1() {
        let call=call_block("continue");
        let ret=GlobalICFGNode::Mir(MirBasicBlock {
            block_id:1,statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let old_call="dummyCall::recursive".to_string();
        let old_ret="dummyRet::recursive".to_string();
        let mut graph=GlobalICFGOrdered {
            ordered_nodes:vec![
                ("rust::rec::bb0".into(),call),
                ("rust::rec::bb1".into(),ret),
                (old_call.clone(),GlobalICFGNode::DummyCall(DummyNode {
                    dummy_node_name:"dummyCall".into(),incoming_edge:"rust::rec::bb0".into(),
                    outgoing_edge:"rust::rec::bb0".into(),id:old_call.clone(),mir_var:None,llvm_var:None,
                    argument_bindings:vec![],is_internal:Some(true)})),
                (old_ret.clone(),GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name:"dummyRet".into(),incoming_edge:"rust::rec::bb1".into(),
                    outgoing_edge:"rust::rec::bb1".into(),id:old_ret.clone(),mir_var:None,llvm_var:None,
                    argument_bindings:vec![],is_internal:Some(true)})),
            ],
            icfg_edges:vec![
                context_edge("rust::rec::bb0".into(),old_call.clone(),Some("Rust Call -> dummyCall".into())),
                context_edge(old_call.clone(),"rust::rec::bb0".into(),Some("dummyCall -> Rust Entry".into())),
                context_edge("rust::rec::bb1".into(),old_ret.clone(),Some("Rust Return -> dummyRet".into())),
                context_edge(old_ret.clone(),"rust::rec::bb1".into(),Some("dummyRet -> Rust Continuation".into())),
            ],
            llvm_memory_effects:None,svf_solved_points_to:None,
            rust_functions:BTreeMap::from([("rec".into(),RustFunctionMetadata {
                name:"rec".into(),arg_count:0,entry_node:"rust::rec::bb0".into(),return_nodes:vec!["rust::rec::bb1".into()]
            })]),
            rust_calls:vec![RustCallMetadata {
                caller_function:"rec".into(),call_node:"rust::rec::bb0".into(),callee_function:"rec".into(),
                dummy_call_node:old_call,dummy_ret_node:old_ret,arguments:vec![],return_place:"_0".into(),
                return_node:"rust::rec::bb1".into(),is_closure:false,
            }],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),crate_correlations:vec![],package_scope:vec![],
                body_statuses:vec![json!({"instance_id":"rec-instance","body_status":"represented_body","local":true})],
                exact_call_bindings:vec![json!({"call_key":"K","caller_instance_id":"rec-instance","resolved_instance_id":"rec-instance",
                    "body_status":"represented_body","activation_kind":"mir_call","icfg_node":"rust::rec::bb0"})],
                icfg_callsite_links:vec![json!({"call_key":"K","icfg_node":"rust::rec::bb0"})],
                icfg_stitching_v1:None,context_execution_graph_v1:None,
            }),
        };
        context_expand(&mut graph,"rust::rec::bb0").expect("direct recursion must converge");
        let audit=graph.dependency_body_ingestion_v1.as_ref().unwrap().context_execution_graph_v1.as_ref().unwrap();
        assert_eq!(audit["capability"],"context_explicit_icfg_v3");
        assert_eq!(audit["call_graph_scc"],"recursive_scc_local_k1");
        assert_eq!(audit["recursive_context_k"],1);
        let activations=audit["represented_call_activations"].as_array().unwrap();
        assert!(activations.iter().any(|a|a["context_transition"]=="recursive_scc_k1_replace"));
        let states=audit["states"].as_array().unwrap();
        let contexts=states.iter().filter(|s|s["concrete_instance_id"]=="rec-instance")
            .map(|s|s["context"].to_string()).collect::<BTreeSet<_>>();
        assert_eq!(contexts.len(),2,"root [] plus recursive [K], never unbounded [K,K,...]");
    }

    #[test]
    fn context_expand_local_to_dependency_emits_v4_and_exact_entry_frame() {
        let root_call=call_block("unreachable");
        let root_ret=GlobalICFGNode::Mir(MirBasicBlock {block_id:1,statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()})});
        let dep_ret=GlobalICFGNode::Mir(MirBasicBlock {block_id:0,statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()})});
        let old_call="dummyCall::dep".to_string();
        let old_ret="dummyRet::dep".to_string();
        let dep_scope="dep1-instance::dep-instance".to_string();
        let dep_entry=format!("rust::{dep_scope}::bb0");
        let mut graph=GlobalICFGOrdered {
            ordered_nodes:vec![
                ("rust::root::bb0".into(),root_call),("rust::root::bb1".into(),root_ret),
                (dep_entry.clone(),dep_ret),
                (old_call.clone(),GlobalICFGNode::DummyCall(DummyNode {
                    dummy_node_name:"dummyCall".into(),incoming_edge:"rust::root::bb0".into(),outgoing_edge:dep_entry.clone(),
                    id:old_call.clone(),mir_var:None,llvm_var:None,argument_bindings:vec![],is_internal:Some(true)})),
                (old_ret.clone(),GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name:"dummyRet".into(),incoming_edge:dep_entry.clone(),outgoing_edge:"rust::root::bb1".into(),
                    id:old_ret.clone(),mir_var:None,llvm_var:None,argument_bindings:vec![],is_internal:Some(true)})),
            ],
            icfg_edges:vec![
                context_edge("rust::root::bb0".into(),old_call.clone(),Some("Rust Call -> dummyCall".into())),
                context_edge(old_call.clone(),dep_entry.clone(),Some("dummyCall -> Rust Entry".into())),
                context_edge(dep_entry.clone(),old_ret.clone(),Some("Rust Return -> dummyRet".into())),
                context_edge(old_ret.clone(),"rust::root::bb1".into(),Some("dummyRet -> Rust Continuation".into())),
            ],
            llvm_memory_effects:None,svf_solved_points_to:None,
            rust_functions:BTreeMap::from([
                ("root".into(),RustFunctionMetadata {name:"root".into(),arg_count:0,entry_node:"rust::root::bb0".into(),return_nodes:vec!["rust::root::bb1".into()]}),
                (dep_scope.clone(),RustFunctionMetadata {name:dep_scope.clone(),arg_count:0,entry_node:dep_entry.clone(),return_nodes:vec![dep_entry.clone()]}),
            ]),
            rust_calls:vec![RustCallMetadata {caller_function:"root".into(),call_node:"rust::root::bb0".into(),
                callee_function:dep_scope,dummy_call_node:old_call,dummy_ret_node:old_ret,arguments:vec![],return_place:"_0".into(),
                return_node:"rust::root::bb1".into(),is_closure:false}],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(),target_triple:"test".into(),crate_correlations:vec![],package_scope:vec![],
                body_statuses:vec![
                    json!({"instance_id":"root-instance","body_status":"represented_body","local":true}),
                    json!({"instance_id":"dep-instance","body_status":"represented_body","local":false}),
                ],
                exact_call_bindings:vec![json!({"call_key":"K","caller_instance_id":"root-instance","resolved_instance_id":"dep-instance",
                    "body_status":"represented_body","activation_kind":"mir_call","icfg_node":"rust::root::bb0"})],
                icfg_callsite_links:vec![json!({"call_key":"K","icfg_node":"rust::root::bb0"})],
                icfg_stitching_v1:None,context_execution_graph_v1:None,
            }),
        };
        context_expand(&mut graph,"rust::root::bb0").expect("local -> dependency context expansion");
        let audit=graph.dependency_body_ingestion_v1.as_ref().unwrap().context_execution_graph_v1.as_ref().unwrap();
        assert_eq!(audit["capability"],"context_explicit_icfg_v4");
        assert_eq!(audit["local_context_policy"],"exact_call_string");
        assert_eq!(audit["dependency_context_policy"],"entry_exact_then_dependency_internal_call_string_k1");
        assert_eq!(audit["dependency_context_k"],1);
        assert_eq!(audit["activation_count"],2);
        let activations=audit["represented_call_activations"].as_array().unwrap();
        assert_eq!(activations.len(),1);
        assert_eq!(activations[0]["context_transition"],"dependency_region_entry_exact_push");
        assert_eq!(activations[0]["parent_context"],json!([]));
        assert_eq!(activations[0]["callee_context"],json!(["K"]));
        assert_eq!(activations[0]["dependency_entry_context"],json!(["K"]));
    }

    #[test]
    fn represented_diverging_dependency_call_retires_canonical_proxy_under_multiple_contexts() {
        let mk_call = |block_id: usize, return_target: Option<&str>| GlobalICFGNode::Mir(MirBasicBlock {
            block_id,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: String::new(),
                source_info: String::new(),
                function_called: "callee".into(),
                callee_def_path: None,
                deallocator_evidence: None,
                allocation_disposition_evidence: None,
                atomic_ptr_evidence: None,
                higher_order_evidence: None,
                callee_is_local: false,
                callback_def_paths: vec![],
                resolved_instance_callees: vec![],
                instance_dispatch_observed: true,
                instance_dispatch_external: false,
                instance_dispatch_unresolved: false,
                arguments: vec![],
                return_place: "_0".into(),
                return_target: return_target.map(str::to_owned),
                unwind_target: "unreachable".into(),
            }),
        });
        let ret = |block_id: usize| GlobalICFGNode::Mir(MirBasicBlock {
            block_id,
            statements: vec![],
            terminator: Some(MirTerminator::Return { details: String::new(), source_info: String::new() }),
        });

        let dep_scope = "dep1-instance::dep-caller".to_string();
        let target_scope = "dep1-instance::dep-target".to_string();
        let dep_entry = format!("rust::{dep_scope}::bb0");
        let target_entry = format!("rust::{target_scope}::bb0");
        let root_call_1 = "dummyCall::root-1".to_string();
        let root_ret_1 = "dummyRet::root-1".to_string();
        let root_call_2 = "dummyCall::root-2".to_string();
        let root_ret_2 = "dummyRet::root-2".to_string();
        let diverging_proxy = "dummyCall::dep-diverging".to_string();

        let dummy_call = |id: &str, incoming: &str, outgoing: &str| GlobalICFGNode::DummyCall(DummyNode {
            dummy_node_name: "dummyCall".into(),
            incoming_edge: incoming.into(),
            outgoing_edge: outgoing.into(),
            id: id.into(),
            mir_var: None,
            llvm_var: None,
            argument_bindings: vec![],
            is_internal: Some(true),
        });
        let dummy_ret = |id: &str, incoming: &str, outgoing: &str| GlobalICFGNode::DummyRet(DummyNode {
            dummy_node_name: "dummyRet".into(),
            incoming_edge: incoming.into(),
            outgoing_edge: outgoing.into(),
            id: id.into(),
            mir_var: None,
            llvm_var: None,
            argument_bindings: vec![],
            is_internal: Some(true),
        });

        let mut graph = GlobalICFGOrdered {
            ordered_nodes: vec![
                ("rust::root::bb0".into(), mk_call(0, Some("bb1"))),
                ("rust::root::bb1".into(), mk_call(1, Some("bb2"))),
                ("rust::root::bb2".into(), ret(2)),
                (dep_entry.clone(), mk_call(0, None)),
                (target_entry.clone(), ret(0)),
                (root_call_1.clone(), dummy_call(&root_call_1, "rust::root::bb0", &dep_entry)),
                (root_ret_1.clone(), dummy_ret(&root_ret_1, &dep_entry, "rust::root::bb1")),
                (root_call_2.clone(), dummy_call(&root_call_2, "rust::root::bb1", &dep_entry)),
                (root_ret_2.clone(), dummy_ret(&root_ret_2, &dep_entry, "rust::root::bb2")),
                (diverging_proxy.clone(), dummy_call(&diverging_proxy, &dep_entry, &target_entry)),
            ],
            icfg_edges: vec![
                context_edge("rust::root::bb0".into(), root_call_1.clone(), Some("Rust Call -> dummyCall".into())),
                context_edge(root_call_1.clone(), dep_entry.clone(), Some("dummyCall -> Rust Entry".into())),
                context_edge(root_ret_1.clone(), "rust::root::bb1".into(), Some("dummyRet -> Rust Continuation".into())),
                context_edge("rust::root::bb1".into(), root_call_2.clone(), Some("Rust Call -> dummyCall".into())),
                context_edge(root_call_2.clone(), dep_entry.clone(), Some("dummyCall -> Rust Entry".into())),
                context_edge(root_ret_2.clone(), "rust::root::bb2".into(), Some("dummyRet -> Rust Continuation".into())),
                context_edge(dep_entry.clone(), diverging_proxy.clone(), Some("Rust Call -> dummyCall".into())),
                context_edge(diverging_proxy.clone(), target_entry.clone(), Some("dummyCall -> Rust Entry".into())),
            ],
            llvm_memory_effects: None,
            svf_solved_points_to: None,
            rust_functions: BTreeMap::from([
                ("root".into(), RustFunctionMetadata {
                    name: "root".into(), arg_count: 0, entry_node: "rust::root::bb0".into(),
                    return_nodes: vec!["rust::root::bb2".into()],
                }),
                (dep_scope.clone(), RustFunctionMetadata {
                    name: dep_scope.clone(), arg_count: 0, entry_node: dep_entry.clone(), return_nodes: vec![],
                }),
                (target_scope.clone(), RustFunctionMetadata {
                    name: target_scope.clone(), arg_count: 0, entry_node: target_entry.clone(),
                    return_nodes: vec![target_entry.clone()],
                }),
            ]),
            rust_calls: vec![
                RustCallMetadata {
                    caller_function: "root".into(), call_node: "rust::root::bb0".into(),
                    callee_function: dep_scope.clone(), dummy_call_node: root_call_1, dummy_ret_node: root_ret_1,
                    arguments: vec![], return_place: "_0".into(), return_node: "rust::root::bb1".into(), is_closure: false,
                },
                RustCallMetadata {
                    caller_function: "root".into(), call_node: "rust::root::bb1".into(),
                    callee_function: dep_scope.clone(), dummy_call_node: root_call_2, dummy_ret_node: root_ret_2,
                    arguments: vec![], return_place: "_0".into(), return_node: "rust::root::bb2".into(), is_closure: false,
                },
            ],
            dependency_body_ingestion_v1: Some(DependencyBodyIngestionExport {
                capability: "dependency_body_ingestion_v1".into(),
                target_triple: "test".into(),
                crate_correlations: vec![],
                package_scope: vec![],
                body_statuses: vec![
                    json!({"instance_id":"root-instance","body_status":"represented_body","local":true}),
                    json!({"instance_id":"dep-caller","body_status":"represented_body","local":false}),
                    json!({"instance_id":"dep-target","body_status":"represented_body","local":false}),
                ],
                exact_call_bindings: vec![
                    json!({"call_key":"ROOT-1","caller_instance_id":"root-instance","resolved_instance_id":"dep-caller",
                        "body_status":"represented_body","activation_kind":"mir_call","icfg_node":"rust::root::bb0"}),
                    json!({"call_key":"ROOT-2","caller_instance_id":"root-instance","resolved_instance_id":"dep-caller",
                        "body_status":"represented_body","activation_kind":"mir_call","icfg_node":"rust::root::bb1"}),
                    json!({"call_key":"DIVERGE","caller_instance_id":"dep-caller","resolved_instance_id":"dep-target",
                        "body_status":"represented_body","activation_kind":"mir_call","icfg_node":dep_entry}),
                ],
                icfg_callsite_links: vec![
                    json!({"call_key":"ROOT-1","icfg_node":"rust::root::bb0"}),
                    json!({"call_key":"ROOT-2","icfg_node":"rust::root::bb1"}),
                    json!({"call_key":"DIVERGE","icfg_node":dep_entry}),
                ],
                icfg_stitching_v1: None,
                context_execution_graph_v1: None,
            }),
        };

        context_expand(&mut graph, "rust::root::bb0")
            .expect("represented diverging call must not resurrect canonical multi-context proxy plumbing");
        assert!(graph.ordered_nodes.iter().all(|(id, _)| id != &diverging_proxy),
            "canonical diverging dummyCall must be retired after context expansion");
        assert!(graph.icfg_edges.iter().all(|edge| edge.source != diverging_proxy && edge.destination != diverging_proxy));
        let audit = graph.dependency_body_ingestion_v1.as_ref().unwrap()
            .context_execution_graph_v1.as_ref().unwrap();
        let activations = audit["represented_call_activations"].as_array().unwrap();
        let diverging: Vec<_> = activations.iter().filter(|a| a["call_key"] == "DIVERGE").collect();
        assert_eq!(diverging.len(), 2, "both exact dependency entry contexts must retain a certified diverging activation");
        let contexts = diverging.iter().map(|a| a["callee_context"].to_string()).collect::<BTreeSet<_>>();
        assert_eq!(contexts.len(), 2, "the two dependency entry frames must remain distinct");
    }

    fn spawn_callback_graph(certified: bool) -> GlobalICFGOrdered {
        let root_call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id:0, statements:vec![],
            terminator:Some(MirTerminator::Call {
                details:String::new(), source_info:String::new(),
                function_called:"std::thread::spawn".into(),
                callee_def_path:Some("std::thread::spawn".into()),
                deallocator_evidence:None, allocation_disposition_evidence:None, atomic_ptr_evidence:None,
                higher_order_evidence:None, callee_is_local:false,
                callback_def_paths:vec!["root::{closure#0}".into()],
                resolved_instance_callees:vec![], instance_dispatch_observed:true,
                instance_dispatch_external:true, instance_dispatch_unresolved:false,
                arguments:vec![], return_place:"_handle".into(),
                return_target:Some("bb1".into()), unwind_target:"unreachable".into(),
            }),
        });
        let ret = |block_id| GlobalICFGNode::Mir(MirBasicBlock {
            block_id, statements:vec![],
            terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()}),
        });
        let old_call="dummyCall::root::bb0::callback0".to_string();
        let old_ret="dummyRet::root::bb0::callback0".to_string();
        let mut body_statuses=vec![json!({"instance_id":"root-instance","body_status":"represented_body","local":true})];
        let mut bindings=vec![json!({
            "call_key":"spawn-provider",
            "caller_instance_id":"root-instance",
            "resolved_instance_id":"std-spawn-instance",
            "body_status":"out_of_scope",
            "activation_kind":"mir_call",
            "required_program_body":false,
            "icfg_node":"rust::root::bb0"
        })];
        let mut links=vec![json!({"call_key":"spawn-provider","icfg_node":"rust::root::bb0"})];
        if certified {
            body_statuses.push(json!({"instance_id":"callback-instance","body_status":"represented_body","local":true}));
            bindings.push(json!({
                "call_key":"callback-K",
                "caller_instance_id":"root-instance",
                "resolved_instance_id":"callback-instance",
                "resolved_diagnostic_name":"root::{closure#0}",
                "body_status":"represented_body",
                "activation_kind":"higher_order_callback",
                "callback_semantics":"spawn_may_once",
                "required_program_body":true,
                "icfg_node":"rust::root::bb0"
            }));
            links.push(json!({"call_key":"callback-K","icfg_node":"rust::root::bb0"}));
        }
        GlobalICFGOrdered {
            ordered_nodes:vec![
                ("rust::root::bb0".into(),root_call),
                ("rust::root::bb1".into(),ret(1)),
                ("rust::root::{closure#0}::bb0".into(),ret(0)),
                (old_call.clone(),GlobalICFGNode::DummyCall(DummyNode {
                    dummy_node_name:"dummyCall".into(), incoming_edge:"rust::root::bb0".into(),
                    outgoing_edge:"rust::root::{closure#0}::bb0".into(), id:old_call.clone(),
                    mir_var:None, llvm_var:None, argument_bindings:vec![], is_internal:Some(true),
                })),
                (old_ret.clone(),GlobalICFGNode::DummyRet(DummyNode {
                    dummy_node_name:"dummyRet".into(), incoming_edge:"rust::root::{closure#0}::bb0".into(),
                    outgoing_edge:"rust::root::bb1".into(), id:old_ret.clone(),
                    mir_var:None, llvm_var:None, argument_bindings:vec![], is_internal:Some(true),
                })),
            ],
            icfg_edges:vec![
                context_edge("rust::root::bb0".into(),"rust::root::bb1".into(),Some("Spawn caller continuation".into())),
                context_edge("rust::root::bb0".into(),old_call.clone(),Some("Higher-order callback -> dummyCall".into())),
                context_edge(old_call.clone(),"rust::root::{closure#0}::bb0".into(),Some("dummyCall -> Rust Entry".into())),
                context_edge("rust::root::{closure#0}::bb0".into(),old_ret.clone(),Some("Return -> dummyRet".into())),
                context_edge(old_ret.clone(),"rust::root::bb1".into(),Some("Spawn callback -> caller continuation".into())),
            ],
            llvm_memory_effects:None, svf_solved_points_to:None,
            rust_functions:BTreeMap::from([
                ("root".into(),RustFunctionMetadata {name:"root".into(),arg_count:0,entry_node:"rust::root::bb0".into(),return_nodes:vec!["rust::root::bb1".into()]}),
                ("root::{closure#0}".into(),RustFunctionMetadata {name:"root::{closure#0}".into(),arg_count:1,entry_node:"rust::root::{closure#0}::bb0".into(),return_nodes:vec!["rust::root::{closure#0}::bb0".into()]}),
            ]),
            rust_calls:vec![RustCallMetadata {
                caller_function:"root".into(), call_node:"rust::root::bb0".into(),
                callee_function:"root::{closure#0}".into(), dummy_call_node:old_call,
                dummy_ret_node:old_ret, arguments:vec![], return_place:String::new(),
                return_node:"rust::root::bb1".into(), is_closure:true,
            }],
            dependency_body_ingestion_v1:Some(DependencyBodyIngestionExport {
                capability:"dependency_body_ingestion_v1".into(), target_triple:"test".into(),
                crate_correlations:vec![], package_scope:vec![], body_statuses, exact_call_bindings:bindings,
                icfg_callsite_links:links, icfg_stitching_v1:None, context_execution_graph_v1:None,
            }),
        }
    }

    #[test]
    fn context_expand_preserves_spawn_may_once_with_exact_callback_instance() {
        let mut graph=spawn_callback_graph(true);
        let root=context_expand(&mut graph,"rust::root::bb0")
            .expect("concrete callback binding must context-expand the spawn branch");
        let root_call=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:0},context:vec![]});
        let continuation=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:1},context:vec![]});
        let callback_entry=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"callback-instance".into(),basic_block:0},
            context:vec!["callback-K".into()]});
        assert_eq!(root,root_call);
        assert!(graph.ordered_nodes.iter().any(|(id,_)| id==&callback_entry));
        assert_eq!(graph.icfg_edges.iter().filter(|e|e.source==root_call && e.destination==continuation).count(),1);
        assert!(graph.icfg_edges.iter().any(|e|e.destination==callback_entry && e.label.as_deref()==Some("dummyCall -> Rust Entry")));
        assert!(!graph.icfg_edges.iter().any(|e|matches!(e.label.as_deref(),
            Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1")
                | Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1"))));
        let audit=graph.dependency_body_ingestion_v1.as_ref().unwrap().context_execution_graph_v1.as_ref().unwrap();
        assert!(audit["opaque_higher_order_summaries"].as_array().unwrap().is_empty());
        let callback=audit["represented_call_activations"].as_array().unwrap().iter()
            .find(|r|r["activation_kind"]=="higher_order_callback").expect("callback audit record");
        assert_eq!(callback["callback_semantics"],"spawn_may_once");
        assert_eq!(callback["direct_continuation_required"],true);
    }

    #[test]
    fn context_expand_uncertified_higher_order_callback_is_opaque_not_fabricated() {
        let mut graph=spawn_callback_graph(false);
        // Turn the canonical higher-order branch into the repeated-consumer
        // shape used by Iterator::collect(Map<..., closure>): zero callbacks
        // may take the consumer exit, while callback returns loop to the call.
        graph.icfg_edges.retain(|edge| edge.label.as_deref()!=Some("Spawn caller continuation")
            && edge.label.as_deref()!=Some("Spawn callback -> caller continuation"));
        graph.icfg_edges.push(context_edge(
            "rust::root::bb0".into(),"rust::root::bb1".into(),Some("Higher-order consumer exit".into())));
        graph.ordered_nodes.push((
            "rust::root::bb2".into(),
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 2,
                statements: vec![],
                terminator: Some(MirTerminator::Return {
                    details: String::new(),
                    source_info: String::new(),
                }),
            }),
        ));
        graph.icfg_edges.push(context_edge(
            "rust::root::bb0".into(),"rust::root::bb2".into(),Some("Call unwind".into())));
        if let Some((_,GlobalICFGNode::Mir(block)))=graph.ordered_nodes.iter_mut().find(|(id,_)|id=="rust::root::bb0") {
            if let Some(MirTerminator::Call {unwind_target,..})=block.terminator.as_mut() {
                *unwind_target="cleanup(bb2)".into();
            }
        }
        let old_ret="dummyRet::root::bb0::callback0".to_string();
        graph.icfg_edges.push(context_edge(
            old_ret.clone(),"rust::root::bb0".into(),Some("Higher-order callback loop".into())));
        if let Some((_,GlobalICFGNode::DummyRet(ret)))=graph.ordered_nodes.iter_mut().find(|(id,_)|id==&old_ret) {
            ret.outgoing_edge="rust::root::bb0".into();
        }
        if let Some(relation)=graph.rust_calls.iter_mut().find(|call|
            call.call_node=="rust::root::bb0" && call.dummy_ret_node==old_ret
        ) {
            relation.return_node="rust::root::bb0".into();
        }
        if let Some((_,GlobalICFGNode::Mir(block)))=graph.ordered_nodes.iter_mut().find(|(id,_)|id=="rust::root::bb0") {
            if let Some(MirTerminator::Call {function_called,callee_def_path,..})=block.terminator.as_mut() {
                *function_called="<Map<I,F> as Iterator>::collect".into();
                *callee_def_path=Some("std::iter::Iterator::collect".into());
            }
        }

        let root=context_expand(&mut graph,"rust::root::bb0")
            .expect("uncertified producer-recognized callback must receive an opaque summary");
        let continuation=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:1},context:vec![]});
        let cleanup=execution_state_node_id(&ExecutionStateIdentity {
            code:CanonicalCodeNodeIdentity {concrete_instance_id:"root-instance".into(),basic_block:2},context:vec![]});
        assert!(graph.icfg_edges.iter().any(|edge|
            edge.source==root && edge.destination==root
                && edge.label.as_deref()==Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1")));
        assert!(graph.icfg_edges.iter().any(|edge|
            edge.source==root && edge.destination==continuation
                && edge.label.as_deref()==Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_V1")),
            "zero-callback normal exit must still execute the opaque provider effect");
        assert!(graph.icfg_edges.iter().any(|edge|
            edge.source==root && edge.destination==cleanup
                && edge.label.as_deref()==Some("OPAQUE_HIGHER_ORDER_CALLBACK_SUMMARY_UNWIND_V1")),
            "provider unwind must be opaque and remain semantically typed as unwind");
        assert!(!graph.icfg_edges.iter().any(|edge|
            edge.label.as_deref()==Some("Higher-order callback -> dummyCall")
                || edge.label.as_deref()==Some("dummyCall -> Rust Entry")
                || edge.label.as_deref()==Some("Higher-order callback loop")
                || edge.label.as_deref().is_some_and(|label| label.contains("Return -> dummyRet"))));
        assert!(!graph.ordered_nodes.iter().any(|(id,_)|
            id=="dummyCall::root::bb0::callback0" || id=="dummyRet::root::bb0::callback0"
                || id=="rust::root::{closure#0}::bb0"
        ), "opaque HOC summary must retire the complete uncertified callback plumbing");
        assert!(!graph.icfg_edges.iter().any(|edge|
            edge.source=="dummyCall::root::bb0::callback0"
                || edge.destination=="dummyCall::root::bb0::callback0"
                || edge.source=="dummyRet::root::bb0::callback0"
                || edge.destination=="dummyRet::root::bb0::callback0"
        ));
        let audit=graph.dependency_body_ingestion_v1.as_ref().unwrap()
            .context_execution_graph_v1.as_ref().unwrap();
        assert!(!audit["represented_call_activations"].as_array().unwrap().iter()
            .any(|record|record["activation_kind"]=="higher_order_callback"));
        let summaries=audit["opaque_higher_order_summaries"].as_array().unwrap();
        assert_eq!(summaries.len(),1);
        assert_eq!(summaries[0]["semantics"],"opaque_higher_order_callback_zero_or_more_v1");
        assert_eq!(summaries[0]["provider_effect"],"opaque_at_least_once_per_call");
        assert_eq!(summaries[0]["callback_multiplicity"],"zero_or_more");
        assert_eq!(summaries[0]["canonical_callback_return_proxies"],json!(["dummyRet::root::bb0::callback0"]));
        assert_eq!(summaries[0]["execution_state"],root);
    }

    #[test]
    fn mixed_exact_and_uncertified_higher_order_callbacks_fail_closed() {
        let mut graph=spawn_callback_graph(true);
        let second_call="dummyCall::root::bb0::callback1".to_string();
        graph.ordered_nodes.push((second_call.clone(),GlobalICFGNode::DummyCall(DummyNode {
            dummy_node_name:"dummyCall".into(),incoming_edge:"rust::root::bb0".into(),
            outgoing_edge:"rust::root::{closure#0}::bb0".into(),id:second_call.clone(),
            mir_var:None,llvm_var:None,argument_bindings:vec![],is_internal:Some(true),
        })));
        graph.icfg_edges.push(context_edge(
            "rust::root::bb0".into(),second_call.clone(),Some("Higher-order callback -> dummyCall".into())));
        graph.icfg_edges.push(context_edge(
            second_call,"rust::root::{closure#0}::bb0".into(),Some("dummyCall -> Rust Entry".into())));
        let error=context_expand(&mut graph,"rust::root::bb0").unwrap_err();
        assert!(error.contains("mixes exact and uncertified callback proxies"));
    }

    #[test]
    fn ordinary_uncertified_cross_body_call_still_fails_closed() {
        let mut graph=spawn_callback_graph(false);
        graph.icfg_edges.iter_mut().for_each(|edge| {
            if edge.label.as_deref()==Some("Higher-order callback -> dummyCall") {
                edge.label=Some("Rust Call -> dummyCall".into());
            }
        });
        if let Some((_,GlobalICFGNode::Mir(block)))=graph.ordered_nodes.iter_mut().find(|(id,_)|id=="rust::root::bb0") {
            if let Some(MirTerminator::Call {callback_def_paths,..})=block.terminator.as_mut() {
                callback_def_paths.clear();
            }
        }
        let error=context_expand(&mut graph,"rust::root::bb0").unwrap_err();
        assert!(error.contains("nonrepresented call rust::root::bb0 still points directly to a Rust body"));
    }
}

#[cfg(test)]
mod dg2_required_body_tests {
    use super::*;
    fn graph(required: bool) -> GlobalICFGOrdered {
        serde_json::from_value(json!({"ordered_nodes":[],"icfg_edges":[],"rust_functions":{},"rust_calls":[],
            "dependency_body_ingestion_v1":{"capability":"dependency_body_ingestion_v1","target_triple":"test",
            "crate_correlations":[],"package_scope":[],"body_statuses":[],
            "exact_call_bindings":[{"required_program_body":required,"resolved_instance_id":"child","body_status":"out_of_scope"}]}})).unwrap()
    }
    #[test]
    fn dg2_missing_required_body_fails_closed() {
        let error=context_expand(&mut graph(true),"rust::root::bb0").unwrap_err();
        assert!(error.starts_with("required_program_body_unrepresented:"));
    }
    #[test]
    fn dg2_foreign_cross_body_guard_remains_fail_closed() {
        let mut g=graph(false);
        let dep=g.dependency_body_ingestion_v1.as_mut().unwrap();
        dep.exact_call_bindings.clear();
        dep.body_statuses.push(json!({"instance_id":"root-instance","body_status":"represented_body","local":true}));
        g.rust_functions.insert("root".into(),RustFunctionMetadata {name:"root".into(),arg_count:0,entry_node:"rust::root::bb0".into(),return_nodes:vec!["rust::root::bb0".into()]});
        let block=GlobalICFGNode::Mir(MirBasicBlock {block_id:0,statements:vec![],terminator:Some(MirTerminator::Return {details:String::new(),source_info:String::new()})});
        g.ordered_nodes=vec![("rust::root::bb0".into(),block.clone()),("rust::foreign::bb0".into(),block)];
        g.icfg_edges.push(context_edge("rust::root::bb0".into(),"rust::foreign::bb0".into(),Some("Goto".into())));
        assert!(context_expand(&mut g,"rust::root::bb0").unwrap_err().contains("unrepresented cross-body Rust edge"));
    }
    #[test]
    fn dg2_uncovered_leaf_is_not_required_body_failure() {
        let error=context_expand(&mut graph(false),"rust::root::bb0").unwrap_err();
        assert!(!error.starts_with("required_program_body_unrepresented:"));
    }
}
#[cfg(test)]
mod rust_activation_unwind_metadata_execution_tests {
    use super::*;
    use crate::structs::*;

    fn fixture(policy: Option<&str>, action: &str, with_relation: bool) -> GlobalICFGOrdered {
        let call = RustCallMetadata { caller_function:"parent".into(), call_node:"opaque-call".into(),
            dummy_call_node:"opaque-proxy".into(), callee_function:"child".into(), dummy_ret_node:"opaque-return".into(),
            return_node:"opaque-continuation".into(), return_place:String::new(), arguments:vec![], is_closure:true };
        let caller: GlobalICFGNode = serde_json::from_value(json!({"node_type":"Mir","node_data":{
            "block_id":0,"statements":[],"terminator":{"kind":"Call","details":"","source_info":"",
            "function_called":"opaque-provider","arguments":[],"return_place":"_0","return_target":"bb1",
            "unwind_target":"terminate"}}})).unwrap();
        let child: GlobalICFGNode = GlobalICFGNode::Mir(MirBasicBlock { block_id:0,statements:vec![],
            terminator:Some(MirTerminator::UnwindResume {details:String::new(), source_info:String::new()}) });
        let mut activation=json!({"call_key":"native-key","activation_kind":"higher_order_callback",
            "caller_call_state":"opaque-call","dummy_call_state":"opaque-proxy","callee_entry_state":"opaque-entry",
            "unwind_action":"terminate", "unwind_return_relations":[]});
        if let Some(policy)=policy {activation["callback_semantics"]=json!(policy);}
        if with_relation {activation["unwind_return_relations"]=json!([
            {"callee_unwind_state":"opaque-entry","caller_unwind_state":"opaque-cleanup","unwind_action":action}]);}
        let dep: DependencyBodyIngestionExport = serde_json::from_value(json!({
            "capability":"dependency_body_ingestion_v1","target_triple":"","crate_correlations":[],
            "package_scope":[],"body_statuses":[],"exact_call_bindings":[],"icfg_callsite_links":[],
            "context_execution_graph_v1":{"represented_call_activations":[activation]}})).unwrap();
        GlobalICFGOrdered { ordered_nodes:vec![("opaque-call".into(),caller), ("opaque-entry".into(),child.clone()),
            ("opaque-cleanup".into(),child)],icfg_edges:vec![IcfgEdge {source:"opaque-entry".into(),
                destination:"opaque-cleanup".into(),label:Some("DEP1 unwind exit -> matched caller cleanup".into()),
                source_label:None,destination_label:None}], llvm_memory_effects:None,svf_solved_points_to:None,
            rust_functions:BTreeMap::from([("child".into(),RustFunctionMetadata {name:"child".into(),arg_count:0,
                entry_node:"opaque-entry".into(),return_nodes:vec![]})]),rust_calls:vec![call],dependency_body_ingestion_v1:Some(dep) }
    }
    #[test]
    fn rust_activation_unwind_metadata_synchronous_callback_dep1_evidence() {
        let graph=fixture(Some("conditional_callback_once"),"cleanup(bb2)",true);
        let before=serde_json::to_value(&graph).unwrap();
        let table=rust_activation_unwind_metadata_from_execution_graph(&graph).unwrap().unwrap();
        let r=&table.activations[0];
        assert_eq!(r.activation_kind,RustActivationKind::SynchronousCallback);
        assert_eq!(r.source,RustActivationMetadataSource::ExecutionActivation {call_key:"native-key".into()});
        assert_eq!(r.unwind,RustActivationUnwindSemantics::CertifiedCallback {
            action:RustActivationUnwindAction::Cleanup {target:"bb2".into()},
            unwind_edges:vec![RustActivationUnwindEdge {source_node:"opaque-entry".into(),destination_node:"opaque-cleanup".into()}] });
        // The provider MIR and ledger top-level action both say Terminate.
        // Callback evidence, rather than either provider action, says Cleanup.
        assert_eq!(serde_json::to_value(&graph).unwrap(),before);
        assert_eq!(RustActivationUnwindMetadataV1::from_operational_json(&before).unwrap(),Some(table));
    }
    #[test]
    fn rust_activation_unwind_metadata_missing_dep1_callback_evidence() {
        let graph=fixture(Some("repeated_callbacks"),"cleanup(bb2)",false);
        let table=rust_activation_unwind_metadata_from_execution_graph(&graph).unwrap().unwrap();
        assert_eq!(table.activations[0].activation_kind,RustActivationKind::SynchronousCallback);
        assert!(matches!(table.activations[0].unwind,RustActivationUnwindSemantics::Unresolved {
            reason:RustActivationUnresolvedReason::MissingCallbackUnwindEvidence }));
        let graph=fixture(None,"cleanup(bb2)",true);
        let table=rust_activation_unwind_metadata_from_execution_graph(&graph).unwrap().unwrap();
        assert_eq!(table.activations[0].activation_kind,RustActivationKind::Unresolved);
    }
    #[test]
    fn rust_activation_unwind_metadata_detached_dep1_no_caller_cleanup() {
        let graph=fixture(Some("spawn_may_once"),"cleanup(bb2)",false);
        let table=rust_activation_unwind_metadata_from_execution_graph(&graph).unwrap().unwrap();
        assert_eq!(table.activations[0].activation_kind,RustActivationKind::DetachedCallback);
        assert_eq!(table.activations[0].unwind,RustActivationUnwindSemantics::DetachedNoCallerUnwind);
        assert!(rust_activation_unwind_metadata_from_execution_graph(
            &fixture(Some("spawn_may_once"),"cleanup(bb2)",true)).is_err());
    }
    #[test]
    fn rust_activation_unwind_metadata_dep1_identity_ambiguity_fail_closed() {
        let mut graph=fixture(Some("conditional_callback_once"),"cleanup(bb2)",true);
        let records=graph.dependency_body_ingestion_v1.as_mut().unwrap().context_execution_graph_v1.as_mut()
            .unwrap().get_mut("represented_call_activations").unwrap().as_array_mut().unwrap();
        records.push(records[0].clone());
        assert!(rust_activation_unwind_metadata_from_execution_graph(&graph).is_err());
    }
    #[test]
    fn rust_activation_unwind_metadata_dep1_wrong_flow_fail_closed() {
        let mut graph=fixture(Some("conditional_callback_once"),"cleanup(bb2)",true);
        graph.icfg_edges[0].label=Some("normal".into());
        assert!(rust_activation_unwind_metadata_from_execution_graph(&graph).is_err());
    }
}
