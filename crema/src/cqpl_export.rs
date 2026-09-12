use crate::abstract_domain::{AbstractMemory, AbstractState, CellValue, Name};
use crate::structs::{GlobalICFGNode, GlobalICFGOrdered, MirTerminator, SvfStatement};
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

#[derive(Debug, Clone, Serialize)]
struct AnnotatedIcfg {
    schema_version: u32,
    entry: String,
    variables: Vec<ProgramVariable>,
    nodes: Vec<AnnotatedNode>,
}

#[derive(Debug, Clone, Serialize)]
struct ProgramVariable {
    id: String,
    language: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct AnnotatedNode {
    id: String,
    successors: Vec<String>,
    labels: Vec<EventLabel>,
    /// CREMA Phase 5 stores one converged per-node state after applying the
    /// node transformer. It does not retain a separate stable Pi#_pre map.
    /// Do not fabricate one: v1 exports an explicit empty pre-memory.
    pre: AbstractMemoryAnnotation,
    post: AbstractMemoryAnnotation,
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
        if node_ids.contains(&edge.source) && node_ids.contains(&edge.destination) {
            successors
                .entry(edge.source.clone())
                .or_default()
                .insert(edge.destination.clone());
        }
    }

    // CREMA intentionally keeps ordinary Rust->Rust calls out of icfg_edges:
    // fixed_point_analysis follows those calls implicitly by pushing the callee
    // bb0 into its worklist and propagating Return states to the internal
    // dummyRet.  CQPL, however, reasons only over the exported transition
    // relation R.  Materialize those already-existing analysis transitions here
    // so K# has the same interprocedural reachability as the abstract analysis.
    // This is read-only: the CREMA ICFG itself is not mutated.
    materialize_internal_rust_call_edges(icfg, &node_ids, &mut successors);

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

        let mut labels = labels_for_node(node_id, node, &llvm_names, &ffi_functions);
        expand_closure_event_labels(node_id, &mut labels, &closure_event_aliases);
        for label in &labels {
            variable_ids.insert(label.variable.clone());
        }

        nodes.push(AnnotatedNode {
            id: node_id.clone(),
            successors: successors
                .get(node_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            labels,
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

    let output = AnnotatedIcfg {
        schema_version: CQPL_ANNOTATED_ICFG_SCHEMA_VERSION,
        entry: entry.to_string(),
        variables,
        nodes,
    };

    let file = File::create(output_path)?;
    serde_json::to_writer_pretty(file, &output)?;
    Ok(())
}

/// Materialize ordinary Rust-to-Rust call/return transitions that CREMA's
/// fixed-point engine follows implicitly rather than storing in `icfg_edges`.
///
/// For an internal call, the stored graph currently contains
///
/// caller -> dummyCall -> dummyRet -> caller-return
///
/// while `fixed_point_analysis` additionally evaluates the callee from bb0 and
/// propagates each callee Return to that dummyRet.  For CQPL we replace only the
/// synthetic dummyCall->dummyRet bypass in the *exported* relation with
///
/// caller -> dummyCall -> callee-bb0 -> ... -> callee-Return -> dummyRet
///        -> caller-return.
///
/// If several call sites invoke the same callee, a callee Return has one edge to
/// each corresponding dummyRet.  This is the usual finite, context-insensitive
/// ICFG over-approximation; it can introduce spurious return paths but does not
/// hide a real Rust call.  CREMA's analysis state and legacy detector are not
/// modified.
fn materialize_internal_rust_call_edges(
    icfg: &GlobalICFGOrdered,
    node_ids: &BTreeSet<String>,
    successors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let node_by_id: BTreeMap<&str, &GlobalICFGNode> = icfg
        .ordered_nodes
        .iter()
        .map(|(id, node)| (id.as_str(), node))
        .collect();

    let mut calls: Vec<(String, String, String, Vec<String>)> = Vec::new();

    for (caller_id, node) in &icfg.ordered_nodes {
        let GlobalICFGNode::Mir(bb) = node else { continue; };
        let Some(MirTerminator::Call { function_called, .. }) = &bb.terminator else {
            continue;
        };

        // Closure calls already have explicit call/return edges in CREMA's ICFG.
        if function_called.contains("{closure") || function_called.contains("closure#") {
            continue;
        }

        let callee_entry = format!("rust::{function_called}::bb0");
        if !node_ids.contains(&callee_entry) {
            continue;
        }

        // An ordinary local Rust call is identified exactly as in the current
        // ICFG/fixed-point implementation: the call-site has an internal
        // DummyCall successor whose outgoing edge names the paired DummyRet.
        let Some((dummy_call_id, dummy_ret_id)) = successors
            .get(caller_id)
            .and_then(|succs| {
                succs.iter().find_map(|sid| match node_by_id.get(sid.as_str()) {
                    Some(GlobalICFGNode::DummyCall(dummy))
                        if dummy.is_internal.unwrap_or(false) =>
                    {
                        Some((sid.clone(), dummy.outgoing_edge.clone()))
                    }
                    _ => None,
                })
            })
        else {
            continue;
        };

        if !node_ids.contains(&dummy_ret_id) {
            continue;
        }

        let callee_prefix = format!("rust::{function_called}::bb");
        let return_nodes: Vec<String> = icfg
            .ordered_nodes
            .iter()
            .filter_map(|(id, candidate)| {
                if !id.starts_with(&callee_prefix) {
                    return None;
                }
                match candidate {
                    GlobalICFGNode::Mir(callee_bb)
                        if matches!(&callee_bb.terminator, Some(MirTerminator::Return { .. })) =>
                    {
                        Some(id.clone())
                    }
                    _ => None,
                }
            })
            .collect();

        // Keep the original bypass if the callee has no representable Return;
        // never make the exported graph less connected based on a failed match.
        if !return_nodes.is_empty() {
            calls.push((dummy_call_id, dummy_ret_id, callee_entry, return_nodes));
        }
    }

    for (dummy_call_id, dummy_ret_id, callee_entry, return_nodes) in calls {
        let call_succs = successors.entry(dummy_call_id).or_default();
        call_succs.remove(&dummy_ret_id);
        call_succs.insert(callee_entry);

        for return_node in return_nodes {
            successors
                .entry(return_node)
                .or_default()
                .insert(dummy_ret_id.clone());
        }
    }
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
    if id.starts_with("Local(") || id.starts_with("Leak(Local(") {
        "rust"
    } else if id.starts_with('%')
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
                    MirTerminator::Goto { details, .. }
                    | MirTerminator::UnwindResume { details, .. }
                    | MirTerminator::Return { details, .. }
                    | MirTerminator::Unreachable { details, .. }
                    | MirTerminator::InlineAsm { details, .. }
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

                        if is_deallocation_call(function_called, call_text, ffi_functions) {
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
        GlobalICFGNode::DummyCall(_) | GlobalICFGNode::DummyRet(_) => {}
    }
    labels.into_iter().collect()
}

fn is_fresh_allocation_call(
    function_called: &str,
    call_text: &str,
    ffi_functions: &HashSet<String>,
) -> bool {
    is_box_new_call(function_called)
        || is_box_new_call(call_text)
        || is_raw_alloc_call(function_called)
        || is_raw_alloc_call(call_text)
        || is_raw_alloc_zeroed_call(function_called)
        || is_raw_alloc_zeroed_call(call_text)
        || is_c_malloc_call(function_called, ffi_functions)
        || is_c_malloc_call(call_text, ffi_functions)
        // Legacy Phase-5 allocation cases retained by the current transfer.
        || call_text.contains("std::ffi::CString::new")
        || call_text.contains("<std::ffi::CString as std::convert::From<&std::ffi::CStr>>::from")
        || call_text.contains("std::slice::<impl [") && call_text.contains(">::into_vec::<std::alloc::Global>")
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
    use crate::structs::{DummyNode, IcfgEdge, MirBasicBlock, MirCallArgument, SourceInfoData, MirStatement};

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
    fn exporter_materializes_internal_rust_call_and_return_edges_for_cqpl() {
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
            outgoing_edge: dummy_ret_id.into(),
            id: "dc".into(),
            mir_var: None,
            llvm_var: None,
            is_internal: Some(true),
        });
        let dummy_ret = GlobalICFGNode::DummyRet(DummyNode {
            dummy_node_name: "dummyRet".into(),
            incoming_edge: dummy_call_id.into(),
            outgoing_edge: caller_return.into(),
            id: "dr".into(),
            mir_var: None,
            llvm_var: None,
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
                edge(dummy_call_id, dummy_ret_id),
                edge(dummy_ret_id, caller_return),
                edge(callee_entry, callee_return),
            ],
        };

        let mut state = AbstractState::default();
        let mut mem = AbstractMemory::default();
        mem.set_cell_value(&"Local(_1)".to_string(), CellValue::ALLOC);
        state.insert(callee_entry.into(), mem);

        let path = std::env::temp_dir().join(format!(
            "crema-cqpl-internal-call-export-{}.json",
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

        assert_eq!(successors_of(dummy_call_id), vec![callee_entry.to_string()]);
        assert_eq!(successors_of(callee_return), vec![dummy_ret_id.to_string()]);
        assert_eq!(successors_of(dummy_ret_id), vec![caller_return.to_string()]);
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
    fn cstring_from_raw_call_gets_read_summary_but_not_drop_at_call_site() {
        let call = GlobalICFGNode::Mir(MirBasicBlock {
            block_id: 10,
            statements: vec![],
            terminator: Some(MirTerminator::Call {
                details: "_16 = std::ffi::CString::from_raw(copy _4)".into(),
                source_info: "<cqpl-test>".into(),
                function_called: "std::ffi::CString::from_raw".into(),
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

}
