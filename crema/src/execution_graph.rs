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

fn tarjan_cycle(graph: &BTreeMap<String, BTreeSet<String>>, root: &str) -> Option<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        state: &mut BTreeMap<String, u8>,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        state.insert(node.to_owned(), 1);
        stack.push(node.to_owned());
        for next in graph.get(node).into_iter().flatten() {
            match state.get(next).copied().unwrap_or(0) {
                0 => if let Some(cycle) = visit(next, graph, state, stack) { return Some(cycle); },
                1 => {
                    let at = stack.iter().position(|item| item == next).unwrap_or(0);
                    let mut cycle = stack[at..].to_vec();
                    cycle.push(next.clone());
                    return Some(cycle);
                }
                _ => {}
            }
        }
        stack.pop();
        state.insert(node.to_owned(), 2);
        None
    }
    visit(root, graph, &mut BTreeMap::new(), &mut Vec::new())
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

/// Pinned MIR's cleanup target is serialized as `cleanup(bbN)`.  `continue`
/// means that unwinding escapes the current body; it is not a normal return.
fn cleanup_block(target: &str) -> Option<&str> {
    target.strip_prefix("cleanup(")?.strip_suffix(')')
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
/// Returns the graph and its root execution-node identity.  A reachable
/// recursive SCC or a missing exact body/call correlation is rejected.
pub fn context_expand(icfg: &mut GlobalICFGOrdered, selected_entry: &str) -> Result<String, String> {
    let Some(dep) = icfg.dependency_body_ingestion_v1.clone() else {
        return Ok(selected_entry.to_owned());
    };
    let represented = represented_statuses(&dep)?;
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
    for binding in &dep.exact_call_bindings {
        let (Some(key), Some(caller)) = (
            binding.get("call_key").and_then(Value::as_str),
            binding.get("caller_instance_id").and_then(Value::as_str),
        ) else { continue; };
        let Some(node) = binding.get("icfg_node").and_then(Value::as_str)
            .or_else(|| callsite_node_by_key.get(key).map(String::as_str)) else { continue; };
        let slot = (caller.to_owned(), node.to_owned());
        if let Some((prior_key, prior_binding)) = binding_by_node.insert(slot.clone(), (key.to_owned(), binding)) {
            if prior_key != key || prior_binding != binding {
                return Err(format!("multiple compiler call bindings claim the same caller/call MIR node: {} {}", slot.0, slot.1));
            }
        }
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
    if let Some(cycle) = tarjan_cycle(&call_graph, root_instance) {
        let value = json!({"status":"recursive_call_context_unsupported","p2_control_flow_complete":false,"recursive_instance_cycle":cycle});
        if let Some(dep) = icfg.dependency_body_ingestion_v1.as_mut() { dep.context_execution_graph_v1 = Some(value); }
        return Err("recursive_call_context_unsupported: reachable represented Rust call cycle".into());
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

    let mut queue = VecDeque::from([Activation { instance: root_instance.clone(), context: vec![] }]);
    let mut activations = BTreeSet::new();
    let mut generated_nodes = BTreeMap::<String, GlobalICFGNode>::new();
    let mut generated_edges = Vec::<IcfgEdge>::new();
    let mut generated_functions = BTreeMap::<String, RustFunctionMetadata>::new();
    let mut generated_calls = Vec::<RustCallMetadata>::new();
    let mut state_records = Vec::<Value>::new();
    let mut represented_call_records = Vec::<Value>::new();
    let mut static_exec_ids = BTreeMap::<String, BTreeSet<String>>::new();
    let mut proxy_remaps = BTreeMap::<String, BTreeSet<String>>::new();
    let mut entry_by_activation = BTreeMap::<Activation, String>::new();

    while let Some(activation) = queue.pop_front() {
        if !activations.insert(activation.clone()) { continue; }
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
            state_records.push(json!({"execution_node":exec_id,"canonical_code_node":{"concrete_instance_id":activation.instance,"basic_block":block.block_id},"canonical_node_id":static_id,"concrete_instance_id":activation.instance,"basic_block":block.block_id,"context":activation.context}));
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
            let Some(old_call_proxy) = edge_map.get(static_id).into_iter().flatten()
                .find(|edge| node_map.get(&edge.destination).is_some_and(|n| matches!(n, GlobalICFGNode::DummyCall(_))))
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
            let is_return = matches!(node, GlobalICFGNode::Mir(MirBasicBlock { terminator: Some(MirTerminator::Return { .. }), .. }));
            for edge in edge_map.get(static_id).into_iter().flatten() {
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
                generated_edges.push(context_edge(exec_id.clone(), destination, edge.label.clone()));
            }

            let Some((call_key, callee_id, binding, relation)) = represented_call else { continue; };
            let MirTerminator::Call { return_target, unwind_target, .. } =
                (match node { GlobalICFGNode::Mir(block) => block.terminator.as_ref().ok_or("call node without terminator")?, _ => unreachable!() })
            else { return Err(format!("exact represented relation {call_key} does not point to a MIR Call")); };
            let old_call_proxy = relation.map(|r| r.dummy_call_node.as_str()).or_else(|| edge_map.get(static_id).into_iter().flatten().find(|e| e.label.as_deref()==Some("Rust Call -> dummyCall")).map(|e| e.destination.as_str()));
            let call_proxy = encoded_proxy_id("dummyCall", call_key, &activation.context);
            let mut call_dummy = old_call_proxy.and_then(|id| node_map.get(id)).and_then(|n| match n { GlobalICFGNode::DummyCall(d) => Some(d.clone()), _ => None })
                .unwrap_or(DummyNode { dummy_node_name:"dummyCall".into(), incoming_edge:static_id.clone(), outgoing_edge:String::new(), id:call_proxy.clone(), mir_var:None, llvm_var:None, argument_bindings:vec![], is_internal:Some(true) });
            let callee_context = activation.context.iter().cloned().chain(std::iter::once(call_key.clone())).collect::<Vec<_>>();
            let callee_activation = Activation { instance: callee_id.clone(), context: callee_context.clone() };
            let callee_function = instance_function.get(callee_id.as_str())
                .ok_or_else(|| format!("missing callee function for {callee_id}"))?;
            let callee_entry_bb = icfg.rust_functions[callee_function].entry_node.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid callee entry")?;
            let callee_entry = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: callee_id.clone(), basic_block: callee_entry_bb }, context: callee_context });
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
                call_meta.callee_function = execution_scope(&callee_id, &activation.context.iter().cloned().chain(std::iter::once(call_key.clone())).collect::<Vec<_>>()).trim_start_matches("rust::").to_owned();
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
                    let return_exec = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: callee_id.clone(), basic_block: return_bb }, context: activation.context.iter().cloned().chain(std::iter::once(call_key.clone())).collect() });
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
            // Preserve MIR unwind action as provenance. Cleanup edges are added
            // only when the original call carries a concrete cleanup target.
            if let Some(target) = cleanup_block(unwind_target) {
                let caller_scope = scope_of_node(static_id).ok_or("invalid caller scope")?;
                let cleanup_static = format!("rust::{caller_scope}::{target}");
                if let Some(cleanup_exec) = state_ids.get(&cleanup_static) {
                    let caller_unwind_context = activation.context.iter().cloned().chain(std::iter::once(call_key.clone())).collect::<Vec<_>>();
                    let callee_body = nodes_by_scope.get(callee_function).ok_or("callee body nodes missing for unwind stitching")?;
                    for (exit_static, exit_node) in callee_body {
                        if !is_unwind_exit(exit_node) { continue; }
                        let bb = exit_static.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid callee unwind exit")?;
                        let exit_id = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: callee_id.clone(), basic_block: bb }, context: caller_unwind_context.clone() });
                        generated_edges.push(context_edge(exit_id, cleanup_exec.clone(), Some("DEP1 unwind exit -> matched caller cleanup".into())));
                    }
                }
            }
            represented_call_records.push(json!({
                "call_key":call_key,
                "caller_instance_id":activation.instance,
                "caller_call_state":exec_id,
                "callee_instance_id":callee_id,
                "callee_entry_state":callee_entry,
                "parent_context":activation.context,
                "callee_context":activation.context.iter().cloned().chain(std::iter::once(call_key.clone())).collect::<Vec<_>>(),
                "dummy_call_state":call_proxy,
                "dummy_ret_state":if return_target.is_some() {call_meta.as_ref().map(|m|m.dummy_ret_node.clone()).unwrap_or_default()} else {String::new()},
                "return_target":return_target,
                "continuation_state":continuation_record,
                "normal_return_relations":return_records,
                "unwind_action":unwind_target,
                "callee_body_status":"represented_body"
            }));
            if let Some(call_meta) = call_meta { generated_calls.push(call_meta); }
            let _ = binding;
        }
    }

    let root_function = instance_function[root_instance].clone();
    let root_meta = &icfg.rust_functions[&root_function];
    let root_bb = root_meta.entry_node.rsplit_once("::bb").and_then(|(_, bb)| bb.parse::<u32>().ok()).ok_or("invalid root entry block")?;
    let root_exec = execution_state_node_id(&ExecutionStateIdentity { code: CanonicalCodeNodeIdentity { concrete_instance_id: root_instance.clone(), basic_block: root_bb }, context:vec![] });
    let mut replaced_proxy_ids = BTreeSet::<String>::new();
    for (_, _, _, relation) in represented_calls.values() {
        if let Some(relation) = relation {
            replaced_proxy_ids.insert(relation.dummy_call_node.clone());
            if !relation.dummy_ret_node.is_empty() { replaced_proxy_ids.insert(relation.dummy_ret_node.clone()); }
        }
    }
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
            if generated_edges.iter().any(|e| e.source == call_state && e.destination == continuation) {
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
    let execution_state_count = state_records.len();
    let count = generated_nodes.len();
    let canonical_count = state_records.iter().map(|r| r["canonical_code_node"].to_string()).collect::<BTreeSet<_>>().len();
    icfg.ordered_nodes = generated_nodes.into_iter().collect();
    icfg.icfg_edges = generated_edges;
    icfg.rust_functions = generated_functions;
    icfg.rust_calls = generated_calls;
    if let Some(dep) = icfg.dependency_body_ingestion_v1.as_mut() {
        dep.context_execution_graph_v1 = Some(json!({
            "capability":"context_explicit_icfg_v1","status":"complete_for_acyclic_represented_rust_calls",
            "acquired_bodies":represented.len(),"canonical_code_nodes":canonical_count,"execution_states":execution_state_count,
            "proxy_and_non_mir_nodes":count.saturating_sub(execution_state_count),
            "states":state_records,"represented_call_activations":represented_call_records,
            "call_graph_scc":"acyclic"
        }));
    }
    Ok(root_exec)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
