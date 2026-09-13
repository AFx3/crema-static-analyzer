use rustc_driver::Callbacks;
use rustc_interface::Queries;
use rustc_middle::mir::{Place, PlaceElem, Statement, StatementKind, Terminator, TerminatorKind, Operand};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{Write, Read};
use serde_json;
use std::error::Error;
use rustc_middle::mir::{Local, LocalDecl, Mutability};
use rustc_index::IndexVec;
use std::fs::read_dir;
use rustc_hir::def::DefKind;
use rustc_middle::ty::{self, TyCtxt, TyKind};

use crate::structs::{MirStatement, MirTerminator, MirBasicBlock, MirRepresentation, SourceInfoData,
    LlvmRepresentation, LlvmFunction, LlvmJson, LlvmJsonNode, SvfStatement, LlvmEdge, IcfgEdge, DummyNode, GlobalICFGNode, GlobalICFGOrdered, MirCallArgument,
    RustFunctionMetadata, RustCallMetadata, TerminalNode };
use crate::utils::{unwind_action_to_string,compute_hash, load_ffi_functions};

// NOTE: rustc unwind Terminate is materialized as an explicit terminal ICFG node; from 1.86, unwind actions are:
/* 
Continue
No action is to be taken. Continue unwinding.

This is similar to Cleanup(bb) where bb does nothing but Resume, but they are not equivalent, as presence of Cleanup(_) will make a frame non-POF.

Unreachable
Triggers undefined behavior if unwind happens.

Terminate(UnwindTerminateReason)
Terminates the execution if unwind happens.

Depending on the platform and situation this may cause a non-unwindable panic or abort.

Cleanup(BasicBlock)
Cleanups to be done.
*/
/// Empirical bridge for the currently supported single-relevant-parameter
/// FFI wrappers.
///
/// In the pinned SVF JSON, a pointer formal appears at FunEntry as a StoreStmt
/// whose rhs VarID is the incoming pointer value and whose lhs is its stack
/// slot.  We intentionally do not guess beyond this evidence.  General
/// multi-parameter mapping is deferred to a later phase.
fn svf_first_formal_param_var_id(function: &LlvmFunction) -> Option<usize> {
    // Real SVF output from the pinned producer does not guarantee that the
    // parameter-spill StoreStmt is attached to FunEntryBlock.  Clang/SVF can
    // place the alloca and store in the first IntraBlock instead.  Recover the
    // first supported formal from structured SVF statements across the whole
    // function: a StoreStmt whose lhs is a stack object created by AddrStmt.
    //
    // Phase 5 deliberately supports only the first relevant formal.  General
    // argument-index -> formal mapping remains future work.
    let stack_slots: HashSet<usize> = function
        .nodes
        .iter()
        .flat_map(|node| node.svf_statements.iter())
        .filter(|stmt| stmt.stmt_type == "AddrStmt")
        .filter_map(|stmt| stmt.lhs_var_id)
        .collect();

    function
        .nodes
        .iter()
        .flat_map(|node| node.svf_statements.iter())
        .find(|stmt| {
            stmt.stmt_type == "StoreStmt"
                && stmt
                    .lhs_var_id
                    .is_some_and(|lhs| stack_slots.contains(&lhs))
        })
        .and_then(|stmt| stmt.rhs_var_id)
}

/// Empirical C -> Rust pointer-return bridge.
///
/// Under the pinned SVF pipeline, the externally visible pointer returned from
/// the C wrapper reaches FunExit through a PhiStmt.  Its lhs VarID is the value
/// bridged to MIR `return_place`.
///
/// If this evidence is absent we return None: guessing another lhs would be
/// unsound and could manufacture allocator provenance.
fn svf_function_return_var_id(exit_node: &LlvmJsonNode) -> Option<usize> {
    exit_node
        .svf_statements
        .iter()
        .filter(|stmt| stmt.stmt_type == "PhiStmt")
        .find_map(SvfStatement::result_var_id)
}

pub struct MirExtractor {
    pub mir_representation: MirRepresentation,
    pub llvm_representation: Option<LlvmRepresentation>, // store parsed LLVM IR
    /// Per-CREMA-run SVF output directory.  This is explicit so pure-Rust
    /// targets cannot accidentally ingest JSON artifacts from earlier runs.
    pub llvm_output_dir: String,
    /// Explicit MIR argument count per local Rust function/closure.
    pub rust_function_arg_counts: HashMap<String, usize>,
    /// User-selected concrete entry used only to seed v6L rustc Instance
    /// propagation. It is not a heuristic target selector.
    pub instance_entry_hint: String,
}

impl MirExtractor {
    
    pub fn new(llvm_output_dir: String, instance_entry_hint: String) -> Self {
        MirExtractor {
            mir_representation: MirRepresentation { functions: BTreeMap::new() },
            llvm_representation: None,
            llvm_output_dir,
            rust_function_arg_counts: HashMap::new(),
            instance_entry_hint,
        }
    }
    
    // NOTE: now pass the local declarations so that we can check a place’s mutability, store this info also in mir terminator's call arguments
    pub fn convert_statement(&self, statement: &Statement<'_>, local_decls: &IndexVec<Local, LocalDecl>) -> MirStatement {
        let source_info = statement.source_info.clone();

        let source_info_data = SourceInfoData {
            span: format!("{:?}", source_info.span),
            scope: format!("{:?}", source_info.scope),
        };

        let statement_kind = match &statement.kind {
            StatementKind::Assign(..) => "Assign",
            StatementKind::FakeRead(..) => "FakeRead",
            StatementKind::SetDiscriminant { .. } => "SetDiscriminant",
            StatementKind::Deinit(..) => "Deinit",
            StatementKind::StorageLive(..) => "StorageLive",
            StatementKind::StorageDead(..) => "StorageDead",
            StatementKind::Retag(..) => "Retag",
            StatementKind::PlaceMention(..) => "PlaceMention",
            StatementKind::AscribeUserType(..) => "AscribeUserType",
            StatementKind::Coverage(..) => "Coverage",
            StatementKind::Intrinsic(..) => "Intrinsic",
            StatementKind::ConstEvalCounter => "ConstEvalCounter",
            StatementKind::Nop => "Nop",
            StatementKind::BackwardIncompatibleDropHint { .. } => "BackwardIncompatibleDropHint",
        };

        // defaults for when a place isn’t present:
        let mut place_info: Option<String> = None;
        let mut rvalue: Option<String> = None;
        let mut is_mutable: Option<bool> = None;

        if let StatementKind::Assign(box (lhs, rhs)) = &statement.kind {
            let (place_desc, mutable_flag) = self.describe_place(lhs, local_decls);
            place_info = Some(place_desc);
            rvalue = Some(format!("{:?}", rhs));
            is_mutable = Some(mutable_flag);
        }

        MirStatement {
            source_info: source_info_data,
            kind: statement_kind.to_string(),
            details: format!("{:?}", statement.kind),
            place: place_info,
            rvalue,
            is_mutable,
        }
    }

    // modify the helper to also check the local’s mutability.
    // returns a tuple: (description string, is_mutable flag).
    fn describe_place(&self, place: &Place<'_>, local_decls: &IndexVec<Local, LocalDecl>) -> (String, bool) {
        // start by describing the local (e.g. Local(3))
        let mut description = format!("Local({:?})", place.local);
        // look up the local declaration and determine its mutability
        let is_mut = local_decls.get(place.local)
            .map(|local_decl| local_decl.mutability == Mutability::Mut)
            .unwrap_or(false);
        if is_mut {
            description.push_str(" [mutable]");
        }
        // continue describing the projections
        for elem in place.projection {
            description.push_str(" -> ");
            match elem {
                PlaceElem::Deref => description.push_str("*"),
                PlaceElem::Field(field_idx, ty) => {
                    description.push_str(&format!("Field({:?}, Type: {:?})", field_idx, ty))
                }
                PlaceElem::Index(local) => {
                    description.push_str(&format!("Index({:?})", local))
                }
                PlaceElem::ConstantIndex { offset, from_end, .. } => {
                    description.push_str(&format!("ConstantIndex(offset: {}, from_end: {})", offset, from_end))
                }
                PlaceElem::Subslice { from, to, from_end } => {
                    description.push_str(&format!(
                        "Subslice(from: {}, to: {}, from_end: {})",
                        from, to, from_end
                    ));
                }
                PlaceElem::Downcast(name, variant_idx) => {
                    description.push_str(&format!("Downcast(name: {:?}, variant: {:?})", name, variant_idx))
                }
                PlaceElem::OpaqueCast(ty) => {
                    description.push_str(&format!("OpaqueCast(Type: {:?})", ty));
                }
                PlaceElem::Subtype(ty) => {
                    description.push_str(&format!("Subtype(Type: {:?})", ty));
                }
        
            }
        }
        (description, is_mut)
    }

    // pass the local declarations here as well so that any place conversions can include mutability
    pub fn convert_terminator<'tcx>(
        &self,
        terminator: &Option<Terminator<'tcx>>,
        local_decls: &IndexVec<Local, LocalDecl<'tcx>>,
        tcx: TyCtxt<'tcx>,
    ) -> Option<MirTerminator> {
        terminator.as_ref().map(|t| match &t.kind {
            TerminatorKind::Goto { target } => MirTerminator::Goto {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
                target: format!("{:?}", target),
            },
            TerminatorKind::SwitchInt { discr, targets, .. } => {
                let discr_str = format!("{:?}", discr);
                let target_blocks = targets
                    .iter()
                    .map(|(val, bb)| format!("({}, {:?})", val, bb))
                    .collect::<Vec<String>>();
                let otherwise_block = Some(format!("{:?}", targets.otherwise()));

                MirTerminator::SwitchInt {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    discr: discr_str,
                    targets: target_blocks,
                    otherwise: otherwise_block,
                }
            },
            TerminatorKind::Return => MirTerminator::Return {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::UnwindResume => MirTerminator::UnwindResume {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::Unreachable => MirTerminator::Unreachable {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::Drop { place, target, unwind, .. } => {
                let (dropped_desc, is_mut) = self.describe_place(place, local_decls);
                MirTerminator::Drop {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    return_target: format!("{:?}", target),
                    unwind_target: unwind_action_to_string(unwind),
                    dropped_value: dropped_desc,
                    is_mutable: is_mut,
                }
            },
            // --- Modified Call terminator for args local mut ---
            TerminatorKind::Call { func, args, destination, target, unwind, .. } => {
                // for each arg, extract the inner operand from the spanned wrapper
                let call_arguments: Vec<MirCallArgument> = args.iter().map(|spanned_arg| {
                    let operand = &spanned_arg.node;
                    match operand {
                        // if the operand is a Copy or Move, then extract the place and check its mutability
                        Operand::Copy(place) | Operand::Move(place) => {
                            let (desc, is_mut) = self.describe_place(place, local_decls);
                            MirCallArgument { arg: desc, is_mutable: Some(is_mut) }
                        },
                        // for other kinds of operands (e.g. constants) not record mutability
                        _ => MirCallArgument { arg: format!("{:?}", operand), is_mutable: None },
                    }
                }).collect();
                
                // The textual MIR spelling is useful for diagnostics and for
                // library summaries, but it is not a stable semantic identity.
                // Constant function items carry an exact rustc DefId; record its
                // canonical DefPath separately and resolve local calls from it.
                let func_ty = func.ty(local_decls, tcx);
                let (callee_def_path, callee_is_local) = match func_ty.kind() {
                    TyKind::FnDef(def_id, _) => {
                        (Some(tcx.def_path_str(*def_id)), def_id.is_local())
                    }
                    _ => (None, false),
                };

                // Recover closure identities structurally from the operand type
                // tree.  This sees closures nested inside iterator/adaptor types
                // as well as direct closure operands, and is independent of
                // HashMap order and Debug-format source-location strings.
                let mut callback_paths = BTreeSet::new();
                for spanned_arg in args {
                    let operand = &spanned_arg.node;
                    let arg_ty = operand.ty(local_decls, tcx);
                    for generic_arg in arg_ty.walk() {
                        let Some(nested_ty) = generic_arg.as_type() else { continue; };
                        if let TyKind::Closure(def_id, _) = nested_ty.kind() {
                            callback_paths.insert(tcx.def_path_str(*def_id));
                        }
                    }
                }

                MirTerminator::Call {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    function_called: format!("{:?}", func),
                    callee_def_path,
                    callee_is_local,
                    callback_def_paths: callback_paths.into_iter().collect(),
                    resolved_instance_callees: Vec::new(),
                    instance_dispatch_observed: false,
                    instance_dispatch_external: false,
                    instance_dispatch_unresolved: false,
                    arguments: call_arguments,
                    return_place: format!("{:?}", destination),
                    return_target: target.map(|t| format!("{:?}", t)),
                    unwind_target: unwind_action_to_string(unwind),
                }
            },
            TerminatorKind::Assert { cond, expected, msg, target, unwind } => {
                let cond_str = format!("{:?}", cond);
                let msg_str = format!("{:?}", msg);

                MirTerminator::Assert {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    return_target: format!("{:?}", target),
                    unwind_target: unwind_action_to_string(unwind),
                    cond: cond_str,
                    expected: *expected,
                    msg: msg_str,
                }
            },
            TerminatorKind::InlineAsm { template, operands, options, line_spans, unwind, .. } => {
                MirTerminator::InlineAsm {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    template: template.iter().map(|s| s.to_string()).collect(),
                    operands: operands.iter().map(|op| format!("{:?}", op)).collect(),
                    options: format!("{:?}", options),
                    line_spans: line_spans.iter().map(|span| format!("{:?}", span)).collect(),
                    unwind_target: Some(unwind_action_to_string(unwind)),
                }
            },
            // catch-all for unhandled variants
            _ => MirTerminator::Unhandled {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
        })
    }
}






#[derive(Debug, Clone, Default)]
struct ConcreteCallDispatch {
    local_callees: BTreeSet<String>,
    observed: bool,
    has_external: bool,
    unresolved: bool,
}

/// Resolve reachable generic/trait calls in a monomorphic rustc context.
///
/// This deliberately mirrors codegen's Instance discipline: start from one
/// concrete entry, instantiate MIR generic arguments in each caller Instance,
/// and ask rustc to resolve the precise function Instance.  Results are keyed
/// by the *generic MIR body + bb*, so two concrete instantiations of the same
/// body contribute a deterministic union rather than overwriting each other.
fn resolve_reachable_instance_dispatch<'tcx>(
    tcx: TyCtxt<'tcx>,
    requested_entry: &str,
) -> BTreeMap<(String, usize), ConcreteCallDispatch> {
    let normalized = requested_entry
        .strip_prefix("rust::")
        .unwrap_or(requested_entry)
        .trim_end_matches("::bb0");

    let body_owners: Vec<_> = tcx
        .hir()
        .body_owners()
        .filter(|local| matches!(tcx.def_kind(*local), DefKind::Fn | DefKind::AssocFn | DefKind::Closure))
        .collect();
    let local_body_paths: BTreeSet<String> = body_owners
        .iter()
        .map(|local| tcx.def_path_str(local.to_def_id()))
        .collect();

    let mut candidates: Vec<_> = body_owners
        .iter()
        .copied()
        .filter(|local| {
            let path = tcx.def_path_str(local.to_def_id());
            path == normalized || path.ends_with(&format!("::{normalized}"))
        })
        .collect();
    candidates.sort_by_key(|local| tcx.def_path_str(local.to_def_id()));
    candidates.dedup();

    if candidates.len() != 1 {
        eprintln!(
            "v6L Instance dispatch: entry '{}' resolved to {} local MIR bodies; concrete dispatch propagation disabled (CQPL remains fail-closed on unresolved local calls)",
            requested_entry,
            candidates.len()
        );
        return BTreeMap::new();
    }

    let entry_def = candidates[0].to_def_id();
    // The public CLI does not carry generic arguments.  Therefore an entry that
    // cannot be represented by Instance::mono is outside this v6L feature's
    // contract; the supported/default `main` entry is monomorphic.
    let entry = ty::Instance::mono(tcx, entry_def);
    // Every caller carried by this worklist is a concrete rustc Instance.
    // Resolution therefore belongs to rustc's fully-monomorphized, post-typeck
    // environment rather than a bare ParamEnv.  This is the same environment
    // used by rustc's public/stable-MIR bridge when resolving a concrete
    // Instance and is required by the pinned 3fee0f12e rustc-private API.
    let typing_env = ty::TypingEnv::fully_monomorphized();
    let mut worklist = std::collections::VecDeque::from([entry]);
    // Preserve full rustc Instance identity (including instance kind and args).
    // A debug-string key is not a semantic identity and could conflate distinct
    // concrete codegen instances.  The reachable set is expected to be small,
    // so a Vec membership check is deliberately preferred over inventing a
    // hash/ordering surrogate for rustc's unstable internal type.
    let mut seen: Vec<ty::Instance<'tcx>> = Vec::new();
    let mut out: BTreeMap<(String, usize), ConcreteCallDispatch> = BTreeMap::new();

    while let Some(caller) = worklist.pop_front() {
        if seen.contains(&caller) {
            continue;
        }
        seen.push(caller);
        if !caller.def_id().is_local() {
            continue;
        }
        let caller_path = tcx.def_path_str(caller.def_id());
        if !local_body_paths.contains(&caller_path) {
            continue;
        }
        let body = tcx.optimized_mir(caller.def_id());
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let Some(term) = data.terminator.as_ref() else { continue; };
            let TerminatorKind::Call { func, .. } = &term.kind else { continue; };
            let func_ty = func.ty(&body.local_decls, tcx);
            let (def_id, args) = match func_ty.kind() {
                TyKind::FnDef(def_id, args) => (*def_id, *args),
                _ => continue,
            };

            let dispatch = out.entry((caller_path.clone(), bb.index())).or_default();
            dispatch.observed = true;
            let concrete_args = match caller.try_instantiate_mir_and_normalize_erasing_regions(
                tcx,
                typing_env,
                ty::EarlyBinder::bind(args),
            ) {
                Ok(args) => args,
                Err(_) => {
                    // Normalization failure means that this concrete callsite is
                    // outside the proven Instance-dispatch fragment.  Do not
                    // panic or silently fall back to a generic DefId: retain an
                    // explicit unresolved marker so CQPL export fails closed.
                    dispatch.unresolved = true;
                    continue;
                }
            };
            match ty::Instance::try_resolve(tcx, typing_env, def_id, concrete_args) {
                Ok(Some(callee)) => {
                    if callee.def_id().is_local() {
                        let callee_path = tcx.def_path_str(callee.def_id());
                        if local_body_paths.contains(&callee_path) {
                            dispatch.local_callees.insert(callee_path);
                            worklist.push_back(callee);
                        } else {
                            // A local shim/item without a canonical extracted MIR
                            // body cannot be silently summarized as if complete.
                            dispatch.unresolved = true;
                        }
                    } else {
                        dispatch.has_external = true;
                    }
                }
                Ok(None) | Err(_) => dispatch.unresolved = true,
            }
        }
    }
    out
}

fn resolved_local_targets(
    function_called: &str,
    callee_def_path: &Option<String>,
    callback_def_paths: &[String],
    resolved_instance_callees: &[String],
    instance_dispatch_observed: bool,
    functions: &BTreeMap<String, Vec<MirBasicBlock>>,
) -> Vec<(String, bool)> {
    if instance_dispatch_observed {
        let mut targets = resolved_instance_callees.to_vec();
        targets.sort();
        targets.dedup();
        return targets.into_iter().map(|callee| (callee, false)).collect();
    }
    let direct_local = resolve_local_callee(function_called, callee_def_path, functions);
    let direct_closure = if direct_local.is_none() {
        resolve_direct_closure_callee(function_called, callee_def_path, callback_def_paths, functions)
    } else {
        None
    };
    if let Some(callee) = direct_local {
        vec![(callee, false)]
    } else if let Some(callee) = direct_closure {
        vec![(callee, true)]
    } else {
        Vec::new()
    }
}

/// Resolve a local Rust callee from rustc's canonical DefPath.  The textual
/// MIR spelling is retained only as a backwards-compatible fallback for unit
/// fixtures created before v6K; production extraction always supplies
/// `callee_def_path` for constant FnDef operands.
fn resolve_local_callee(
    function_called: &str,
    callee_def_path: &Option<String>,
    functions: &BTreeMap<String, Vec<MirBasicBlock>>,
) -> Option<String> {
    if let Some(path) = callee_def_path {
        if functions.contains_key(path) {
            return Some(path.clone());
        }
    }
    if functions.contains_key(function_called) {
        return Some(function_called.to_string());
    }
    None
}

/// A closure body is entered directly only for the language-level Fn/FnMut/
/// FnOnce call traits.  Seeing a closure type inside another generic call
/// (Iterator::map/filter/product, callback registration, etc.) is *not* enough
/// to claim that the callback executes at that callsite.
fn is_direct_closure_invoke(callee_def_path: Option<&str>, function_called: &str) -> bool {
    let name = callee_def_path.unwrap_or(function_called);
    name.ends_with("::Fn::call")
        || name.ends_with("::FnMut::call_mut")
        || name.ends_with("::FnOnce::call_once")
        || name.contains(" as std::ops::Fn<") && name.ends_with(">::call")
        || name.contains(" as std::ops::FnMut<") && name.ends_with(">::call_mut")
        || name.contains(" as std::ops::FnOnce<") && name.ends_with(">::call_once")
}

fn resolve_direct_closure_callee(
    function_called: &str,
    callee_def_path: &Option<String>,
    callback_def_paths: &[String],
    functions: &BTreeMap<String, Vec<MirBasicBlock>>,
) -> Option<String> {
    if !is_direct_closure_invoke(callee_def_path.as_deref(), function_called) {
        return None;
    }
    let mut candidates: Vec<String> = callback_def_paths
        .iter()
        .filter(|path| functions.contains_key(*path))
        .cloned()
        .collect();
    candidates.sort();
    candidates.dedup();
    if candidates.len() == 1 { candidates.pop() } else { None }
}

fn return_nodes_for(function: &str, functions: &BTreeMap<String, Vec<MirBasicBlock>>) -> Vec<String> {
    let mut out: Vec<String> = functions
        .get(function)
        .into_iter()
        .flat_map(|blocks| blocks.iter())
        .filter(|block| matches!(block.terminator, Some(MirTerminator::Return { .. })))
        .map(|block| format!("rust::{function}::bb{}", block.block_id))
        .collect();
    out.sort();
    out
}

/// Materialize rustc unwind-terminate destinations as explicit maximal states.
/// A dangling edge is not a transition relation over the serialized node domain;
/// dropping it at CQPL export time would also erase a concrete maximal path.
fn materialize_terminal_nodes(
    nodes: &mut Vec<(String, GlobalICFGNode)>,
    edges: &[IcfgEdge],
) {
    let existing: BTreeSet<String> = nodes.iter().map(|(id, _)| id.clone()).collect();
    let terminals: BTreeSet<String> = edges
        .iter()
        .filter(|edge| edge.destination.ends_with("::terminate"))
        .map(|edge| edge.destination.clone())
        .filter(|id| !existing.contains(id))
        .collect();

    for id in terminals {
        nodes.push((
            id,
            GlobalICFGNode::Terminal(TerminalNode {
                reason: "unwind_terminate".to_string(),
            }),
        ));
    }
}

fn validate_closed_edge_domain(
    nodes: &[(String, GlobalICFGNode)],
    edges: &[IcfgEdge],
) -> Result<(), String> {
    let ids: BTreeSet<&str> = nodes.iter().map(|(id, _)| id.as_str()).collect();
    let mut dangling = BTreeSet::new();
    for edge in edges {
        if !ids.contains(edge.source.as_str()) {
            dangling.insert(format!("missing source '{}' for edge '{} -> {}'", edge.source, edge.source, edge.destination));
        }
        if !ids.contains(edge.destination.as_str()) {
            dangling.insert(format!("missing destination '{}' for edge '{} -> {}'", edge.destination, edge.source, edge.destination));
        }
    }
    if dangling.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "v6K canonical ICFG is not closed over its edge relation: {}",
            dangling.into_iter().collect::<Vec<_>>().join("; ")
        ))
    }
}

impl Callbacks for MirExtractor {fn after_analysis<'tcx>(&mut self, _compiler: &rustc_interface::interface::Compiler, queries: &'tcx Queries<'tcx>) -> rustc_driver::Compilation {
    // helpers fn per costruire ID univoci
    fn get_dummy_call_id(rust_func: &str, bb: usize, call_suffix: &str, internal: bool) -> String {
        if internal {
            format!("dummyCall::rust::{}::bb{}::{}_internal", rust_func, bb, call_suffix)
        } else {
            format!("dummyCall::rust::{}::bb{}::{}", rust_func, bb, call_suffix)
        }
    }
    fn get_dummy_ret_id(rust_func: &str, ret_target: &str, call_suffix: &str, internal: bool) -> String {
        if internal {
            format!("dummyRet::rust::{}::{}::{}_internal", rust_func, ret_target, call_suffix)
        } else {
            format!("dummyRet::rust::{}::{}::{}", rust_func, ret_target, call_suffix)
        }
    }
    // hashmap per memorizzare, per ciascuna  FFI, tutti i call suffix (cioè le chiamate)
    let mut ffi_call_sites: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    queries.global_ctxt().unwrap().enter(|tcx| {
        // --- 1. build concrete rustc Instance dispatch from the selected entry ---
        let concrete_dispatch = resolve_reachable_instance_dispatch(tcx, &self.instance_entry_hint);

        // --- 2. costruisco la MIR per ogni funzione ---
        for def_id in tcx.hir().body_owners() {
            let _function_name = tcx.def_path_str(def_id.to_def_id());

            ///// QUESTO CI STO ANCORA LAVORANDO, E' IL CASO DELLA SPAZZATURA NELLA MIR...
            if !def_id.to_def_id().is_local() {
                continue;
            }
            let function_name = tcx.def_path_str(def_id);
            // filter
            if function_name.contains("::RE") {
                            println!("Skipping compiler-generated function: {}", function_name);
                                continue;
                            }
                          

             // 1) skip mir opitimized for constants
            match tcx.def_kind(def_id) {
                DefKind::Fn | DefKind::AssocFn | DefKind::Closure => { 
                     // ok, process
                     }
                _ => continue,
            }
         
            //println!("Processing function: {}", function_name);
            let body = tcx.optimized_mir(def_id.to_def_id());
            let mut function_blocks = Vec::new();
            for (bb, data) in body.basic_blocks.iter_enumerated() {
                let mut terminator = self.convert_terminator(&data.terminator, &body.local_decls, tcx);
                if let Some(MirTerminator::Call {
                    resolved_instance_callees,
                    instance_dispatch_observed,
                    instance_dispatch_external,
                    instance_dispatch_unresolved,
                    ..
                }) = terminator.as_mut() {
                    if let Some(dispatch) = concrete_dispatch.get(&(function_name.clone(), bb.index())) {
                        *resolved_instance_callees = dispatch.local_callees.iter().cloned().collect();
                        *instance_dispatch_observed = dispatch.observed;
                        *instance_dispatch_external = dispatch.has_external;
                        *instance_dispatch_unresolved = dispatch.unresolved;
                    }
                }
                let mir_basic_block = MirBasicBlock {
                    block_id: bb.index(),
                    statements: data.statements.iter().map(|stmt| self.convert_statement(stmt, &body.local_decls)).collect(),
                    terminator,
                };
                function_blocks.push(mir_basic_block);
            }
            self.rust_function_arg_counts
                .insert(function_name.clone(), body.arg_count);
            self.mir_representation.functions.insert(function_name, function_blocks);
        }
        // --- 2. LOAD LLVM IR (SVF) from this CREMA invocation only ---
        match load_all_llvm_json(&self.llvm_output_dir) {
            Ok(parsed) => self.llvm_representation = Some(parsed),
            Err(e) => eprintln!(
                "Failed to parse LLVM JSON files from {}: {:?}",
                self.llvm_output_dir, e
            ),
        }
        // --- 3. LOAD FFI functions da JSON ---
        let ffi_functions = match load_ffi_functions("./ffi_functions.json") {
            Ok(set) => set,
            Err(e) => {
                eprintln!("Failed to load FFI functions: {:?}", e);
                std::collections::HashSet::new()
            }
        };
        // --- 4. filters LLVM functions to reatins only the one marked as ffi ---
        if let Some(ref mut llvm_repr) = self.llvm_representation {
            llvm_repr.functions.retain(|func_name, _| ffi_functions.contains(func_name));
        }
        // --- 5. BUILD ICFG EDGES  ---
        let mut icfg_edges = Vec::new();
        let mut rust_calls: Vec<RustCallMetadata> = Vec::new();
        for (rust_func, blocks) in &self.mir_representation.functions {
            for block in blocks {
                if let Some(MirTerminator::Call {
                    function_called,
                    callee_def_path,
                    callee_is_local,
                    callback_def_paths,
                    resolved_instance_callees,
                    instance_dispatch_observed,
                    instance_dispatch_external,
                    instance_dispatch_unresolved,
                    return_target,
                    unwind_target,
                    arguments,
                    return_place,
                    ..
                }) = &block.terminator {
                    // definition of del call suffix as MIR caller node (es. "rust::main::bb2")
                    let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                    // if function called is FFI, registr call suffix for the function

                    if ffi_functions.contains(function_called) {
                        ffi_call_sites.entry(function_called.clone()).or_default().push(call_suffix.clone());
                        let mir_arg = arguments.first().map(|arg| arg.arg.clone());
                        let rust_call_site = format!("rust::{}::bb{}", rust_func, block.block_id);
                        let rust_return_node = if let Some(rt) = return_target {
                            format!("rust::{}::{}", rust_func, rt)
                        } else {
                            format!("rust::{}::end", rust_func)
                        };
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                if !llvm_func.nodes.is_empty() {
                                    let dummy_call_id = get_dummy_call_id(rust_func, block.block_id, &call_suffix, false);
                                    if let Some(entry_node) = llvm_func
                                        .nodes
                                        .iter()
                                        .find(|node| {
                                            node.node_kind_string == "FunEntryBlock"
                                        })
                                        .or_else(|| llvm_func.nodes.first())
                                    {
                                        // set call_suffix to rename the entry node
                                        let llvm_entry = format!("llvm::{}::node{}::{}", function_called, entry_node.node_id, call_suffix);
                                        // MODIFIED: concatate  call_suffix to original var name
                                        let _llvm_var = entry_node.svf_statements.first().and_then(|stmt| {
                                            stmt.rhs_var_id.map(|id| format!("{}@{}", id, call_suffix))
                                        });
                                        icfg_edges.push(IcfgEdge {
                                            source: rust_call_site.clone(),
                                            destination: dummy_call_id.clone(),
                                            label: Some("FFI Call".to_string()),
                                            source_label: None,
                                            destination_label: mir_arg.clone(),
                                        });
                                        icfg_edges.push(IcfgEdge {
                                            source: dummy_call_id.clone(),
                                            destination: llvm_entry.clone(),
                                            label: Some("dummyCall->LLVM Entry".to_string()),
                                            source_label: mir_arg.clone(),
                                            destination_label: None,
                                        });
                                        if let Some(exit_node) = llvm_func.nodes.iter().find(|node| {
                                            node.node_kind_string == "FunExitBlock"
                                                && node.outgoing_edges.is_empty()
                                        }) {
                                            let llvm_exit = format!("llvm::{}::node{}::{}", function_called, exit_node.node_id, call_suffix);
                                            let ret_target = return_target.as_ref().unwrap_or(&"end".to_string()).clone();
                                            let dummy_ret_id = get_dummy_ret_id(rust_func, &ret_target, &call_suffix, false);
                                            icfg_edges.push(IcfgEdge {
                                                source: llvm_exit.clone(),
                                                destination: dummy_ret_id.clone(),
                                                label: Some("LLVM Exit->dummyRet".to_string()),
                                                source_label: Some(exit_node.basic_block_info.clone().unwrap_or_else(|| "llvm_param".to_string())),
                                                destination_label: None,
                                            });
                                            icfg_edges.push(IcfgEdge {
                                                source: dummy_ret_id.clone(),
                                                destination: rust_return_node.clone(),
                                                label: Some("dummyRet->MIR Return".to_string()),
                                                source_label: Some(exit_node.basic_block_info.clone().unwrap_or_else(|| "llvm_param".to_string())),
                                                destination_label: None,
                                            });
                                        }
                                    }
                                } else {
                                    icfg_edges.push(IcfgEdge {
                                        source: rust_call_site.clone(),
                                        destination: rust_return_node.clone(),
                                        label: Some("FFI Call (stdlib)".to_string()),
                                        source_label: Some(format!("Mir bb{}", block.block_id)),
                                        destination_label: None,
                                    });
                                }
                            } else {
                                icfg_edges.push(IcfgEdge {
                                    source: rust_call_site.clone(),
                                    destination: rust_return_node.clone(),
                                    label: Some("FFI Call (stdlib)".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                        } else {
                            icfg_edges.push(IcfgEdge {
                                source: rust_call_site.clone(),
                                destination: rust_return_node.clone(),
                                label: Some("FFI Call (no LLVM info)".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }
                        let effective_unwind = extract_target(unwind_target);
                        if effective_unwind != "unreachable" && effective_unwind != "continue" {
                            let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                            icfg_edges.push(IcfgEdge {
                                source: src,
                                destination: dst,
                                label: Some("Call unwind".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }

                        } else {
                            // v6L: if this callsite was reached under concrete rustc
                            // Instances, use the deterministic union of concrete local
                            // targets. Otherwise preserve the v6K direct resolver.
                            let resolved_targets = resolved_local_targets(
                                function_called,
                                callee_def_path,
                                callback_def_paths,
                                resolved_instance_callees,
                                *instance_dispatch_observed,
                                &self.mir_representation.functions,
                            );
                            let multi_target = resolved_targets.len() > 1;
                            let call_site = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let caller_return = return_target
                                .as_ref()
                                .map(|rt| format!("rust::{}::{}", rust_func, rt))
                                .unwrap_or_else(|| format!("rust::{}::end", rust_func));

                            for (ordinal, (callee, is_closure_call)) in resolved_targets.iter().enumerate() {
                                let branch = if multi_target { Some(ordinal) } else { None };
                                let base_call = get_dummy_call_id(rust_func, block.block_id, &call_site, true);
                                let base_ret = get_dummy_ret_id(rust_func, &block.block_id.to_string(), &call_site, true);
                                let dummy_call_id = branch.map(|n| format!("{}::instance{}", base_call, n)).unwrap_or(base_call);
                                let dummy_ret_id = branch.map(|n| format!("{}::instance{}", base_ret, n)).unwrap_or(base_ret);

                                rust_calls.push(RustCallMetadata {
                                    caller_function: rust_func.clone(),
                                    call_node: call_site.clone(),
                                    callee_function: callee.clone(),
                                    dummy_call_node: dummy_call_id.clone(),
                                    dummy_ret_node: dummy_ret_id.clone(),
                                    arguments: arguments.clone(),
                                    return_place: return_place.clone(),
                                    return_node: caller_return.clone(),
                                    is_closure: *is_closure_call,
                                });

                                icfg_edges.push(IcfgEdge {
                                    source: call_site.clone(),
                                    destination: dummy_call_id.clone(),
                                    label: Some("Rust Call -> dummyCall".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                                icfg_edges.push(IcfgEdge {
                                    source: dummy_call_id.clone(),
                                    destination: format!("rust::{}::bb0", callee),
                                    label: Some("dummyCall -> Rust Entry".to_string()),
                                    source_label: None,
                                    destination_label: None,
                                });
                                for ret in return_nodes_for(callee, &self.mir_representation.functions) {
                                    icfg_edges.push(IcfgEdge {
                                        source: ret,
                                        destination: dummy_ret_id.clone(),
                                        label: Some("Rust Return -> dummyRet".to_string()),
                                        source_label: None,
                                        destination_label: None,
                                    });
                                }
                                icfg_edges.push(IcfgEdge {
                                    source: dummy_ret_id,
                                    destination: caller_return.clone(),
                                    label: Some("dummyRet -> Rust Continuation".to_string()),
                                    source_label: None,
                                    destination_label: None,
                                });
                            }

                            // Preserve external branches of different concrete
                            // monomorphizations. Any unresolved concrete Instance
                            // remains an explicit fail-closed edge.
                            let need_summary = resolved_targets.is_empty()
                                || *instance_dispatch_external
                                || *instance_dispatch_unresolved;
                            if need_summary {
                                if let Some(rt) = return_target {
                                    let src = call_site.clone();
                                    let dst = format!("rust::{}::{}", rust_func, rt);
                                    let label = if *instance_dispatch_unresolved {
                                        "UNRESOLVED_LOCAL_CALL: concrete rustc Instance resolution incomplete"
                                    } else if !callback_def_paths.is_empty()
                                        && resolved_targets.is_empty()
                                    {
                                        // Resolving the *external callee* Instance does not prove
                                        // that callbacks passed to it are never invoked.  Until
                                        // the higher-order semantics itself is represented, retain
                                        // the existing fail-closed boundary (e.g. shared-register).
                                        "UNRESOLVED_HIGHER_ORDER: callback semantics not modeled"
                                    } else if *instance_dispatch_observed && *instance_dispatch_external {
                                        "Resolved external Instance summary return"
                                    } else if *instance_dispatch_observed {
                                        "External/summary call return"
                                    } else if *callee_is_local {
                                        "UNRESOLVED_LOCAL_CALL: local FnDef has no canonical MIR body"
                                    } else {
                                        "External/summary call return"
                                    };
                                    icfg_edges.push(IcfgEdge {
                                        source: src,
                                        destination: dst,
                                        label: Some(label.to_string()),
                                        source_label: Some(format!("Mir bb{}", block.block_id)),
                                        destination_label: None,
                                    });
                                }
                            }

                            let effective_unwind = extract_target(unwind_target);
                            if effective_unwind != "unreachable" && effective_unwind != "continue" {
                                let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                                let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                                icfg_edges.push(IcfgEdge {
                                    source: src,
                                    destination: dst,
                                    label: Some("Call unwind".to_string()),
                                    source_label: Some(format!("Mir bb{}", block.block_id)),
                                    destination_label: None,
                                });
                            }
                        }
                    }
                //////////////////////////////////////////////////////////////////////////////////////////////////////
                // GOTO terminator
                if let Some(MirTerminator::Goto { target, details, source_info }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, target);
                    icfg_edges.push(IcfgEdge {
                        source: src,
                        destination: dst,
                        label: Some("Goto".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                }
                ///////////////////////////////////////////////////////////////////////////////////////////////////////
                // handle others terminators (SwitchInt, Assert, Drop) 
                if let Some(MirTerminator::SwitchInt { targets, otherwise, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    for target in targets {
                        let trimmed = target.trim_matches(|c| c == '(' || c == ')');
                        let parts: Vec<&str> = trimmed.split(',').collect();
                        if parts.len() >= 2 {
                            let target_block = parts[1].trim();
                            let dst = format!("rust::{}::{}", rust_func, target_block);
                            icfg_edges.push(IcfgEdge {
                                source: src.clone(),
                                destination: dst,
                                label: Some("SwitchInt target".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }
                    }
                    if let Some(otherwise_target) = otherwise {
                        let dst = format!("rust::{}::{}", rust_func, otherwise_target);
                        icfg_edges.push(IcfgEdge {
                            source: src.clone(),
                            destination: dst,
                            label: Some("SwitchInt otherwise".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
                if let Some(MirTerminator::Assert { return_target, unwind_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let rt = return_target.clone();
                    let dst = format!("rust::{}::{}", rust_func, rt);
                    icfg_edges.push(IcfgEdge {
                        source: src.clone(),
                        destination: dst,
                        label: Some("Assert success".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                    let effective_unwind = extract_target(unwind_target);
                    if effective_unwind != "unreachable" && effective_unwind != "continue" {
                        let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                        icfg_edges.push(IcfgEdge {
                            source: src,
                            destination: dst,
                            label: Some("Assert unwind".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
                if let Some(MirTerminator::Drop { return_target, unwind_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, return_target);
                    icfg_edges.push(IcfgEdge {
                        source: src.clone(),
                        destination: dst,
                        label: Some("Drop return".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                    let effective_unwind = extract_target(unwind_target);
                    if effective_unwind != "unreachable" && effective_unwind != "continue" {
                        let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                        icfg_edges.push(IcfgEdge {
                            source: src,
                            destination: dst,
                            label: Some("Drop unwind".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
            }
        }
        // --- 6. BUILD GLOBAL ICFG ordered NODES ---
        let mut ordered_icfg_nodes: Vec<(String, GlobalICFGNode)> = Vec::new();
        for (rust_func, blocks) in &self.mir_representation.functions {
            let mut sorted_blocks = blocks.clone();
            sorted_blocks.sort_by_key(|b| b.block_id);
            for block in sorted_blocks {
                let mir_node_id = format!("rust::{}::bb{}", rust_func, block.block_id);
                ordered_icfg_nodes.push((mir_node_id.clone(), GlobalICFGNode::Mir(block.clone())));
                if let Some(MirTerminator::Call {
                    function_called,
                    callee_def_path,
                    callee_is_local,
                    callback_def_paths,
                    resolved_instance_callees,
                    instance_dispatch_observed,
                    instance_dispatch_external: _,
                    instance_dispatch_unresolved: _,
                    return_target,
                    arguments,
                    return_place,
                    ..
                }) = &block.terminator {
                    if ffi_functions.contains(function_called) {
                        // --- FFI CALL DUMMY NODES ---
                        let mir_arg =
                            arguments.first().map(|arg| arg.arg.clone());
                        let call_suffix =
                            format!("rust::{}::bb{}", rust_func, block.block_id);
                        let dummy_call_id = get_dummy_call_id(
                            rust_func,
                            block.block_id,
                            &call_suffix,
                            false,
                        );

                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) =
                                llvm_repr.functions.get(function_called).cloned()
                            {
                                if let Some(entry_node) = llvm_func
                                    .nodes
                                    .iter()
                                    .find(|node| {
                                        node.node_kind_string == "FunEntryBlock"
                                    })
                                    .or_else(|| llvm_func.nodes.first())
                                {
                                    let llvm_entry = format!(
                                        "llvm::{}::node{}::{}",
                                        function_called,
                                        entry_node.node_id,
                                        call_suffix
                                    );

                                    let llvm_var = if mir_arg.is_some() {
                                        svf_first_formal_param_var_id(&llvm_func)
                                            .map(|id| {
                                                format!("{}@{}", id, call_suffix)
                                            })
                                    } else {
                                        None
                                    };

                                    ordered_icfg_nodes.push((
                                        dummy_call_id.clone(),
                                        GlobalICFGNode::DummyCall(DummyNode {
                                            dummy_node_name:
                                                "dummyCall".to_string(),
                                            incoming_edge: mir_node_id.clone(),
                                            outgoing_edge: llvm_entry.clone(),
                                            id: compute_hash(&(
                                                mir_node_id.clone(),
                                                llvm_entry.clone(),
                                            )),
                                            mir_var: mir_arg.clone(),
                                            llvm_var,
                                            is_internal: Some(false),
                                        }),
                                    ));
                                }
                            }
                        }
                        // replicate llvm nodes to each FFI call: each call has its body copy
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                                for llvm_node in &llvm_func.nodes {
                                    let new_node_id = format!("llvm::{}::node{}::{}", function_called, llvm_node.node_id, call_suffix);
                                    ordered_icfg_nodes.push((new_node_id, GlobalICFGNode::Llvm(llvm_node.clone())));
                                }
                            }
                        }
                        let rust_return_node = if let Some(ret) = return_target {
                            format!("rust::{}::{}", rust_func, ret)
                        } else {
                            format!("rust::{}::end", rust_func)
                        };
                        let ret_target = return_target.as_ref().unwrap_or(&"end".to_string()).clone();
                        let dummy_ret_id = get_dummy_ret_id(rust_func, &ret_target, &format!("rust::{}::bb{}", rust_func, block.block_id), false);
                        if let Some(llvm_repr) = &self.llvm_representation {
                            if let Some(llvm_func) = llvm_repr.functions.get(function_called).cloned() {
                                if let Some(exit_node) = llvm_func.nodes.iter().find(|node| {
                                    node.node_kind_string == "FunExitBlock"
                                        && node.outgoing_edges.is_empty()
                                }) {
                                    let call_suffix = format!("rust::{}::bb{}", rust_func, block.block_id);
                                    let llvm_exit = format!("llvm::{}::node{}::{}", function_called, exit_node.node_id, call_suffix);
                                    let llvm_return_var =
                                        svf_function_return_var_id(exit_node)
                                            .map(|id| {
                                                format!("{}@{}", id, call_suffix)
                                            });

                                    ordered_icfg_nodes.push((
                                        dummy_ret_id.clone(),
                                        GlobalICFGNode::DummyRet(DummyNode {
                                            dummy_node_name:
                                                "dummyRet".to_string(),
                                            incoming_edge: llvm_exit.clone(),
                                            outgoing_edge:
                                                rust_return_node.clone(),
                                            id: compute_hash(&(
                                                rust_return_node.clone(),
                                                llvm_exit.clone(),
                                            )),
                                            mir_var:
                                                Some(return_place.clone()),
                                            llvm_var: llvm_return_var,
                                            is_internal: Some(false),
                                        }),
                                    ));
                                }
                            }
                        }
                        } else {
                            let resolved_targets = resolved_local_targets(
                                function_called,
                                callee_def_path,
                                callback_def_paths,
                                resolved_instance_callees,
                                *instance_dispatch_observed,
                                &self.mir_representation.functions,
                            );
                            let multi_target = resolved_targets.len() > 1;
                            let call_site = format!("rust::{}::bb{}", rust_func, block.block_id);
                            let caller_return = return_target
                                .as_ref()
                                .map(|rt| format!("rust::{}::{}", rust_func, rt))
                                .unwrap_or_else(|| format!("rust::{}::end", rust_func));
                            for (ordinal, (callee, _)) in resolved_targets.iter().enumerate() {
                                let branch = if multi_target { Some(ordinal) } else { None };
                                let base_call = get_dummy_call_id(rust_func, block.block_id, &call_site, true);
                                let base_ret = get_dummy_ret_id(rust_func, &block.block_id.to_string(), &call_site, true);
                                let dummy_call_id = branch.map(|n| format!("{}::instance{}", base_call, n)).unwrap_or(base_call);
                                let dummy_ret_id = branch.map(|n| format!("{}::instance{}", base_ret, n)).unwrap_or(base_ret);
                                let callee_entry = format!("rust::{}::bb0", callee);

                                ordered_icfg_nodes.push((
                                    dummy_call_id.clone(),
                                    GlobalICFGNode::DummyCall(DummyNode {
                                        dummy_node_name: "dummyCall".to_string(),
                                        incoming_edge: mir_node_id.clone(),
                                        outgoing_edge: callee_entry,
                                        id: compute_hash(&(mir_node_id.clone(), dummy_call_id.clone())),
                                        mir_var: arguments.first().map(|arg| arg.arg.clone()),
                                        llvm_var: None,
                                        is_internal: Some(true),
                                    }),
                                ));
                                ordered_icfg_nodes.push((
                                    dummy_ret_id.clone(),
                                    GlobalICFGNode::DummyRet(DummyNode {
                                        dummy_node_name: "dummyRet".to_string(),
                                        incoming_edge: callee.clone(),
                                        outgoing_edge: caller_return.clone(),
                                        id: compute_hash(&(caller_return.clone(), dummy_ret_id.clone())),
                                        mir_var: Some(return_place.clone()),
                                        llvm_var: Some("Local _0".to_string()),
                                        is_internal: Some(true),
                                    }),
                                ));
                            }
                        }
                    }
                }
            }
        // --- 7. REPLICATE global LLVM edges for ech FFI call ---
        let mut final_edges = icfg_edges.clone();
        let ordered_ids: std::collections::HashSet<String> =
            ordered_icfg_nodes.iter().map(|(id, _)| id.clone()).collect();
        if let Some(llvm_repr) = &self.llvm_representation {
            // for each replicated ffi, use the registered call suffixes
            for (func_name, call_sites) in &ffi_call_sites {
                // FILTER global edges related to the current function
                for edge in &llvm_repr.global_edges {
                    // edge.source and edge.destination are the original IDs (without call_suffix)
                    for call_suffix in call_sites {
                        let src_str = format!("llvm::{}::node{}::{}", func_name, edge.source, call_suffix);
                        let dst_str = format!("llvm::{}::node{}::{}", func_name, edge.destination, call_suffix);
                        if ordered_ids.contains(&src_str) && ordered_ids.contains(&dst_str) {
                            if !final_edges.iter().any(|e| e.source == src_str && e.destination == dst_str) {
                                final_edges.push(IcfgEdge {
                                    source: src_str,
                                    destination: dst_str,
                                    label: Some("LLVM".to_string()),
                                    source_label: None,
                                    destination_label: None,
                                });
                            }
                        }
                    }
                }
            }
        }
        materialize_terminal_nodes(&mut ordered_icfg_nodes, &final_edges);
        if let Err(err) = validate_closed_edge_domain(&ordered_icfg_nodes, &final_edges) {
            panic!("{err}");
        }

        let mut node_label_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (node_id, node) in &ordered_icfg_nodes {
            let label = match node {
                GlobalICFGNode::Mir(mir) => format!("Mir bb{}", mir.block_id),
                GlobalICFGNode::Llvm(llvm) => llvm.info.clone(),
                GlobalICFGNode::DummyCall(dummy) => format!("{} (id: {})", dummy.dummy_node_name, dummy.id),
                GlobalICFGNode::DummyRet(dummy) => format!("{} (id: {})", dummy.dummy_node_name, dummy.id),
                GlobalICFGNode::Terminal(terminal) => format!("Terminal ({})", terminal.reason),
            };
            node_label_map.insert(node_id.clone(), label);
        }
        let mut updated_edges: Vec<IcfgEdge> = final_edges.iter().map(|edge| {
            let source_label = node_label_map.get(&edge.source).cloned();
            let destination_label = node_label_map.get(&edge.destination).cloned();
            IcfgEdge {
                source: edge.source.clone(),
                destination: edge.destination.clone(),
                label: edge.label.clone(),
                source_label,
                destination_label,
            }
        }).collect();
        updated_edges.sort_by(|a, b| {
            (&a.source, &a.destination, &a.label).cmp(&(&b.source, &b.destination, &b.label))
        });
        updated_edges.dedup_by(|a, b| {
            a.source == b.source && a.destination == b.destination && a.label == b.label
        });
        ordered_icfg_nodes.sort_by(|a, b| a.0.cmp(&b.0));
        let rust_functions = self
            .mir_representation
            .functions
            .iter()
            .map(|(name, blocks)| {
                let mut return_nodes: Vec<String> = blocks
                    .iter()
                    .filter(|block| {
                        matches!(
                            block.terminator,
                            Some(MirTerminator::Return { .. })
                        )
                    })
                    .map(|block| format!("rust::{}::bb{}", name, block.block_id))
                    .collect();
                return_nodes.sort();
                return_nodes.dedup();

                (
                    name.clone(),
                    RustFunctionMetadata {
                        name: name.clone(),
                        arg_count: self
                            .rust_function_arg_counts
                            .get(name)
                            .copied()
                            .unwrap_or(0),
                        entry_node: format!("rust::{}::bb0", name),
                        return_nodes,
                    },
                )
            })
            .collect();

        rust_calls.sort_by(|a, b| {
            (
                &a.call_node,
                &a.callee_function,
                &a.dummy_call_node,
                &a.dummy_ret_node,
            )
                .cmp(&(
                    &b.call_node,
                    &b.callee_function,
                    &b.dummy_call_node,
                    &b.dummy_ret_node,
                ))
        });
        rust_calls.dedup_by(|a, b| {
            a.call_node == b.call_node
                && a.callee_function == b.callee_function
                && a.dummy_call_node == b.dummy_call_node
                && a.dummy_ret_node == b.dummy_ret_node
        });

        let global_icfg_ordered = GlobalICFGOrdered {
            ordered_nodes: ordered_icfg_nodes,
            icfg_edges: updated_edges,
            rust_functions,
            rust_calls,
        };
        let output_filename = "global_icfg.json";
        let mut file = File::create(output_filename)
            .expect("Failed to create output file");
        let json_output =
            serde_json::to_string_pretty(&global_icfg_ordered).expect("Failed to serialize JSON");
        file.write_all(json_output.as_bytes())
            .expect("Failed to write JSON output");
        println!("Global ICFG saved to {}", output_filename);
    });
    rustc_driver::Compilation::Stop
}
}



// ---------------------------------------------------------------------
// Helper functions 
// ---------------------------------------------------------------------
// extracts a clean target from an unwind_target string
fn extract_target<'a>(target: &'a str) -> &'a str {
    if target.starts_with("cleanup(") && target.ends_with(")") {
        &target["cleanup(".len()..target.len() - 1]
    } else {
        target
    }
}

pub fn load_all_llvm_json(dir: &str) -> Result<LlvmRepresentation, Box<dyn Error>> {
    let mut combined_functions: HashMap<String, LlvmFunction> = HashMap::new();
    let mut combined_global_edges: Vec<LlvmEdge> = Vec::new();
    
    let mut entries = read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            if let Some(fname) = path.file_name().and_then(|f| f.to_str()) {
                if fname.ends_with("_A_FINAL_ICFG.json") {
                    println!("Loading LLVM JSON file: {}", fname);
                    let file_path = path.to_str().unwrap();
                    let representation = parse_llvm_json(file_path)?;
                    for (func_name, llvm_function) in representation.functions {
                        combined_functions
                            .entry(func_name.clone())
                            .and_modify(|existing_function| {
                                existing_function.nodes.extend(llvm_function.nodes.clone());
                            })
                            .or_insert(llvm_function);
                    }
                    combined_global_edges.extend(representation.global_edges);
                }
            }
        }
    }
    
    combined_global_edges.sort_by(|a, b| {
        a.source.cmp(&b.source)
            .then(a.destination.cmp(&b.destination))
            .then(a.edge_type.cmp(&b.edge_type))
    });
    combined_global_edges.dedup();
    
    Ok(LlvmRepresentation {
        functions: combined_functions,
        global_edges: combined_global_edges,
    })
}

pub fn parse_llvm_json(file_path: &str) -> Result<LlvmRepresentation, Box<dyn Error>> {
    let mut file = File::open(file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let llvm_json: LlvmJson = serde_json::from_str(&contents)?;
    
    let mut functions: HashMap<String, LlvmFunction> = HashMap::new();
    
    for node in llvm_json.nodes.into_iter() {
        if let Some(func_name) = node.function_name.clone() {
            functions
                .entry(func_name.clone())
                .and_modify(|func| {
                    func.nodes.push(node.clone());
                })
                .or_insert_with(|| LlvmFunction {
                    function_name: func_name,
                    nodes: vec![node],
                });
        }
    }
    
    let mut global_edges = llvm_json.edges;
    global_edges.sort_by(|a, b| a.source.cmp(&b.source).then(a.destination.cmp(&b.destination)));
    global_edges.dedup();
    
    Ok(LlvmRepresentation { functions, global_edges })
}

#[cfg(test)]
mod phase6k_callee_resolution_tests {
    use super::*;

    fn empty_functions(names: &[&str]) -> BTreeMap<String, Vec<MirBasicBlock>> {
        names.iter().map(|name| ((*name).to_string(), Vec::new())).collect()
    }

    #[test]
    fn generic_pretty_print_resolves_by_canonical_def_path() {
        let functions = empty_functions(&["MyStruct::new"]);
        assert_eq!(
            resolve_local_callee(
                "MyStruct::new::<[u8; 3]>",
                &Some("MyStruct::new".into()),
                &functions,
            ),
            Some("MyStruct::new".into())
        );
    }

    #[test]
    fn higher_order_carrier_does_not_execute_callback_at_constructor_callsite() {
        let functions = empty_functions(&["main::{closure#0}"]);
        assert_eq!(
            resolve_direct_closure_callee(
                "Iterator::map::<_, {closure@x}>",
                &Some("core::iter::traits::iterator::Iterator::map".into()),
                &["main::{closure#0}".into()],
                &functions,
            ),
            None
        );
    }

    #[test]
    fn concrete_instance_targets_form_sorted_deduplicated_union() {
        let functions = empty_functions(&["impl_a::new", "impl_b::new", "Trait::new"]);
        let got = resolved_local_targets(
            "<T as Trait>::new",
            &Some("Trait::new".into()),
            &[],
            &["impl_b::new".into(), "impl_a::new".into(), "impl_b::new".into()],
            true,
            &functions,
        );
        assert_eq!(
            got,
            vec![("impl_a::new".into(), false), ("impl_b::new".into(), false)]
        );
    }

    #[test]
    fn observed_instance_dispatch_does_not_fallback_to_generic_trait_item() {
        let functions = empty_functions(&["Trait::new"]);
        let got = resolved_local_targets(
            "<T as Trait>::new",
            &Some("Trait::new".into()),
            &[],
            &[],
            true,
            &functions,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn direct_fn_once_call_resolves_exact_single_callback() {
        let functions = empty_functions(&["main::{closure#0}", "main::{closure#1}"]);
        assert_eq!(
            resolve_direct_closure_callee(
                "<main::{closure#1} as std::ops::FnOnce<()>>::call_once",
                &Some("core::ops::function::FnOnce::call_once".into()),
                &["main::{closure#1}".into()],
                &functions,
            ),
            Some("main::{closure#1}".into())
        );
    }

    #[test]
    fn unwind_terminate_destination_is_materialized_as_terminal_node() {
        let mut nodes = vec![(
            "rust::main::bb0".to_string(),
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 0,
                statements: vec![],
                terminator: None,
            }),
        )];
        let edges = vec![IcfgEdge {
            source: "rust::main::bb0".into(),
            destination: "rust::main::terminate".into(),
            label: Some("Call unwind".into()),
            source_label: None,
            destination_label: None,
        }];

        materialize_terminal_nodes(&mut nodes, &edges);
        assert!(matches!(
            nodes.iter().find(|(id, _)| id == "rust::main::terminate").map(|(_, n)| n),
            Some(GlobalICFGNode::Terminal(TerminalNode { reason })) if reason == "unwind_terminate"
        ));
        assert!(validate_closed_edge_domain(&nodes, &edges).is_ok());
    }

    #[test]
    fn canonical_edge_domain_rejects_nonterminal_dangling_destination() {
        let nodes = vec![(
            "rust::main::bb0".to_string(),
            GlobalICFGNode::Mir(MirBasicBlock {
                block_id: 0,
                statements: vec![],
                terminator: None,
            }),
        )];
        let edges = vec![IcfgEdge {
            source: "rust::main::bb0".into(),
            destination: "rust::main::missing".into(),
            label: None,
            source_label: None,
            destination_label: None,
        }];
        assert!(validate_closed_edge_domain(&nodes, &edges).is_err());
    }
}

#[cfg(test)]
mod phase5_ffi_bridge_tests {
    use super::{
        svf_first_formal_param_var_id, svf_function_return_var_id,
        LlvmFunction, LlvmJsonNode, SvfStatement,
    };

    fn stmt(
        stmt_type: &str,
        lhs: Option<usize>,
        rhs: Option<usize>,
        operands: Option<Vec<usize>>,
    ) -> SvfStatement {
        SvfStatement {
            stmt_id: 1,
            stmt_type: stmt_type.to_string(),
            stmt_info: String::new(),
            edge_id: None,
            pta_edge: None,
            lhs_var_id: lhs,
            rhs_var_id: rhs,
            res_var_id: None,
            operand_var_ids: operands,
            operand_vars: None,
            call_inst: None,
            is_conditional: None,
            condition_var_id: None,
            successors: None,
        }
    }

    fn node(kind: &str, statements: Vec<SvfStatement>) -> LlvmJsonNode {
        LlvmJsonNode {
            node_id: 1,
            node_type: false,
            info: String::new(),
            node_kind_string: kind.to_string(),
            node_kind: 0,
            node_source_loc: String::new(),
            function_name: Some("f".to_string()),
            basic_block: None,
            basic_block_name: None,
            basic_block_info: None,
            svf_statements: statements,
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
        }
    }

    fn function(nodes: Vec<LlvmJsonNode>) -> LlvmFunction {
        LlvmFunction {
            function_name: "f".to_string(),
            nodes,
        }
    }

    #[test]
    fn ffi_formal_parameter_is_store_rhs_not_alloca_object() {
        let f = function(vec![node(
            "FunEntryBlock",
            vec![
                stmt("AddrStmt", Some(8), Some(9), None),
                stmt("StoreStmt", Some(8), Some(7), None),
            ],
        )]);

        assert_eq!(svf_first_formal_param_var_id(&f), Some(7));
    }

    #[test]
    fn ffi_formal_bridge_is_deliberately_first_store_only() {
        let f = function(vec![node(
            "FunEntryBlock",
            vec![
                stmt("AddrStmt", Some(8), Some(80), None),
                stmt("AddrStmt", Some(10), Some(100), None),
                stmt("StoreStmt", Some(8), Some(7), None),
                stmt("StoreStmt", Some(10), Some(9), None),
            ],
        )]);

        // Phase 5 supports only the first relevant formal bridge. A later phase
        // must derive argument-index -> formal-VarID mapping explicitly.
        assert_eq!(svf_first_formal_param_var_id(&f), Some(7));
    }

    #[test]
    fn ffi_formal_parameter_accepts_real_svf_shape_with_empty_funentry() {
        let f = function(vec![
            node("FunEntryBlock", vec![]),
            node("IntraBlock", vec![stmt("AddrStmt", Some(37), Some(38), None)]),
            node("IntraBlock", vec![stmt("StoreStmt", Some(37), Some(36), None)]),
        ]);

        // Real c_free_i32 output observed in the focus corpus:
        //   Var37 = alloca ptr
        //   StoreStmt: [Var37 <-- Var36]
        assert_eq!(svf_first_formal_param_var_id(&f), Some(36));
    }

    #[test]
    fn ffi_return_value_is_funexit_phi_lhs() {
        let exit = node(
            "FunExitBlock",
            vec![stmt("PhiStmt", Some(6), None, Some(vec![30]))],
        );

        assert_eq!(svf_function_return_var_id(&exit), Some(6));
    }

    #[test]
    fn ffi_return_value_accepts_legacy_svf_phi_schema() {
        // This is the schema emitted by the pre-Phase-5 SVF exporter:
        // Phi result in `res_var_id`, incoming values in `operand_vars`.
        let raw = r#"{
            "node_id": 2,
            "node_type": false,
            "info": "",
            "node_kind_string": "FunExitBlock",
            "node_kind": 0,
            "node_source_loc": "",
            "function_name": "c_alloc",
            "basic_block": null,
            "basic_block_name": null,
            "basic_block_info": null,
            "svf_statements": [{
                "stmt_id": 1,
                "stmt_type": "PhiStmt",
                "stmt_info": "PhiStmt: [Var6 <-- ([Var30, ICFGNode20],)]",
                "edge_id": null,
                "pta_edge": true,
                "res_var_id": 6,
                "operand_vars": [{"op_var_id": 30, "icfg_node": 20}],
                "call_inst": null,
                "is_conditional": null,
                "condition_var_id": null,
                "successors": null
            }],
            "incoming_edges": [],
            "outgoing_edges": []
        }"#;

        let exit: LlvmJsonNode = serde_json::from_str(raw).unwrap();
        assert_eq!(svf_function_return_var_id(&exit), Some(6));
        assert_eq!(
            exit.svf_statements[0].normalized_operand_var_ids(),
            vec![30]
        );
    }
    #[test]
    fn ffi_return_bridge_does_not_guess_non_phi_lhs() {
        let exit = node(
            "FunExitBlock",
            vec![stmt("CopyStmt", Some(77), Some(30), None)],
        );

        assert_eq!(svf_function_return_var_id(&exit), None);
    }

}

