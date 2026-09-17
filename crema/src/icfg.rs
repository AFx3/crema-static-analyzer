use rustc_driver::Callbacks;
use rustc_interface::Queries;
use rustc_middle::mir::{Place, PlaceElem, Statement, StatementKind, Terminator, TerminatorKind, Operand, NonDivergingIntrinsic};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{Write, Read};
use serde_json;
use std::error::Error;
use rustc_middle::mir::{Local, LocalDecl, Mutability};
use rustc_index::IndexVec;
use std::fs::read_dir;
use rustc_hir::def::DefKind;
use rustc_hir::{ForeignItemKind, ItemKind};
use rustc_hir::def_id::DefId;
use rustc_middle::ty::{self, TyCtxt, TyKind};
use rustc_span::symbol::sym;

use crate::structs::{MirStatement, MirTerminator, MirBasicBlock, MirRepresentation, SourceInfoData,
    LlvmRepresentation, LlvmFunction, LlvmJson, LlvmJsonNode, SvfStatement, LlvmEdge, IcfgEdge, DummyNode, GlobalICFGNode, GlobalICFGOrdered, MirCallArgument,
    RustFunctionMetadata, RustCallMetadata, TerminalNode, RustDropAllocatorEvidence, RustDropAllocatorEvidenceKind,
    RustCallDeallocatorEvidence, RustCallDeallocatorEvidenceKind, RustAllocationDispositionEvidence,
    RustAllocationDispositionEvidenceKind, RustHigherOrderCallEvidence,
    RustHigherOrderCallEvidenceKind };
use crate::utils::{unwind_action_to_string,compute_hash, load_ffi_functions};
use crate::mir_semantics::mir_semantics_v2_enabled;
use crate::panic_unwind::panic_unwind_lifecycle_v1_enabled;


/// A3.2 import boundary for external MIR.
///
/// `panic_unwind_lifecycle_v1` needs MIR from application dependencies such as
/// `aligned_box`, because their panic-sensitive ownership protocol is exactly
/// what the analysis must observe.  Recursively expanding sysroot crates, on
/// the other hand, defeats CREMA's existing library summaries and eventually
/// reaches generic compiler-internal shims such as `ptr::drop_in_place`, where
/// concrete Instance resolution is intentionally incomplete.  Those calls must
/// remain external/summary boundaries rather than becoming partially imported
/// local MIR.
fn a3_is_sysroot_crate_name(crate_name: &str) -> bool {
    matches!(
        crate_name,
        "std"
            | "core"
            | "alloc"
            | "proc_macro"
            | "test"
            | "panic_unwind"
            | "panic_abort"
            | "unwind"
            | "compiler_builtins"
    )
}

fn a3_external_mir_import_allowed<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> bool {
    let crate_name = tcx.crate_name(def_id.krate).to_string();
    !a3_is_sysroot_crate_name(&crate_name)
}


/// v6N-r1/B1.1-r1: prove the allocator family of a generic MIR `Drop`
/// from rustc semantic identity, never from `Debug`/pretty-printed type text.
///
/// Soundness basis (official Rust documentation/source):
/// - `Box<T, A = Global>` uses `Global` for non-ZST backing allocations:
///   https://doc.rust-lang.org/std/boxed/index.html#memory-layout
/// - `Vec<T, A = Global>` uses `Global` for non-ZST/nonzero-capacity storage:
///   https://doc.rust-lang.org/std/vec/index.html#memory-layout
/// - `CString` is rustc diagnostic item `cstring_type` and stores its owned
///   bytes in `Box<[u8]>`; dropping the owner therefore releases the backing
///   allocation through Rust's Global allocator contract:
///   https://doc.rust-lang.org/src/alloc/ffi/c_str.rs.html
/// - `Global` is the standard-library global allocator abstraction:
///   https://doc.rust-lang.org/std/alloc/struct.Global.html
///
/// Identity mechanism (official rustc documentation):
/// - diagnostic items are the supported rustc mechanism for identifying
///   standard-library types without hard-coded paths:
///   https://rustc-dev-guide.rust-lang.org/diagnostics/diagnostic-items.html
/// - `LanguageItems::{owned_box,global_alloc_ty}` identify Box and Global;
///   `sym::cstring_type` identifies CString via `TyCtxt::is_diagnostic_item`.
///
/// The evidence remains intentionally narrow. String/Rc/Arc, custom Drop,
/// custom allocators, aliases that do not normalize to an ADT, and unresolved
/// generic allocator parameters remain `None` and therefore `unknown`.
fn rust_drop_allocator_evidence<'tcx>(
    place: &Place<'tcx>,
    local_decls: &IndexVec<Local, LocalDecl<'tcx>>,
    tcx: TyCtxt<'tcx>,
) -> Option<RustDropAllocatorEvidence> {
    let dropped_ty = place.ty(local_decls, tcx).ty;
    let TyKind::Adt(owner_def, args) = dropped_ty.kind() else {
        return None;
    };

    // CString does not expose an allocator type parameter.  Its official
    // implementation stores the owned byte buffer as Box<[u8]>, so the
    // allocator evidence is the canonical Global lang item.  We prove the
    // owner identity with rustc's `cstring_type` diagnostic item rather than
    // the rendered path `alloc::ffi::c_str::CString`.
    if tcx.is_diagnostic_item(sym::cstring_type, owner_def.did()) {
        let allocator_def_id = tcx.lang_items().global_alloc_ty()?;
        return Some(RustDropAllocatorEvidence {
            kind: RustDropAllocatorEvidenceKind::CStringGlobal,
            owner_def_path: tcx.def_path_str(owner_def.did()),
            allocator_def_path: tcx.def_path_str(allocator_def_id),
        });
    }

    let kind = if tcx.lang_items().owned_box() == Some(owner_def.did()) {
        RustDropAllocatorEvidenceKind::BoxGlobal
    } else if tcx.is_diagnostic_item(sym::Vec, owner_def.did()) {
        RustDropAllocatorEvidenceKind::VecGlobal
    } else {
        return None;
    };

    // Box<T, A> and Vec<T, A> both place the allocator type after T.  We do
    // not rely on its printed name: the allocator ADT must be rustc's
    // `global_alloc_ty` lang item.  Any custom/parametric allocator fails
    // closed to `None`.
    let allocator_ty = args.types().nth(1)?;
    let TyKind::Adt(allocator_def, _) = allocator_ty.kind() else {
        return None;
    };
    if tcx.lang_items().global_alloc_ty() != Some(allocator_def.did()) {
        return None;
    }

    Some(RustDropAllocatorEvidence {
        kind,
        owner_def_path: tcx.def_path_str(owner_def.did()),
        allocator_def_path: tcx.def_path_str(allocator_def.did()),
    })
}


/// v6N-r1a: classify the public Rust global deallocation API while the rustc
/// `DefId` is available.  This intentionally avoids exporter-side matching of
/// `function_called` or rendered MIR strings.
///
/// For the pinned nightly, `std::alloc::dealloc` resolves to the public item
/// `alloc::alloc::dealloc`.  We establish this structurally from DefId metadata:
/// external crate `alloc`, parent module named `alloc`, item named `dealloc`.
/// The canonical DefPath is retained only as audit provenance.
fn rust_call_deallocator_evidence<'tcx>(
    def_id: DefId,
    tcx: TyCtxt<'tcx>,
) -> Option<RustCallDeallocatorEvidence> {
    if def_id.is_local() {
        return None;
    }
    if tcx.crate_name(def_id.krate).as_str() != "alloc"
        || tcx.item_name(def_id).as_str() != "dealloc"
    {
        return None;
    }
    let parent = tcx.parent(def_id);
    // `TyCtxt::item_name` is only valid for definitions that actually carry a
    // name.  In particular, an `Impl` parent has no item name on the pinned
    // compiler and asking for one can ICE rustc.  Prove the parent is a module
    // before reading its name.
    if !matches!(tcx.def_kind(parent), DefKind::Mod)
        || tcx.item_name(parent).as_str() != "alloc"
    {
        return None;
    }
    Some(RustCallDeallocatorEvidence {
        kind: RustCallDeallocatorEvidenceKind::GlobalDeallocApi,
        callee_def_path: tcx.def_path_str(def_id),
    })
}

/// v6S-r1: producer-certify ownership/disposition operations while rustc
/// semantic identity and argument types are available.
///
/// This evidence is deliberately narrower than the historical textual transfer
/// summaries.  Its purpose is observational provenance for future leak
/// precision work; it MUST NOT change the truth value of the frozen v6R query
/// set.
///
/// Box/CString methods are accepted only when the associated item belongs to
/// an inherent impl whose exact self ADT is a producer-certified rustc identity:
/// `owned_box` for Box and diagnostic item `cstring_type` for CString.  The
/// rendered DefPath is stored only for auditability.
///
/// `mem::drop` is special-cased only for an exact core::mem::drop DefId whose
/// first MIR argument type is a raw pointer.  Rust's Reference states that
/// dropping a raw pointer has no effect on the lifecycle of the pointee, so
/// this evidence certifies a *no-op on the pointee*, not a deallocation.
///
/// Official documentation:
/// - Box::into_raw / from_raw / leak:
///   https://doc.rust-lang.org/std/boxed/struct.Box.html
/// - CString::into_raw / from_raw (including the explicit prohibition on C
///   `free` for pointers returned by into_raw):
///   https://doc.rust-lang.org/std/ffi/struct.CString.html#method.into_raw
///   https://doc.rust-lang.org/std/ffi/struct.CString.html#method.from_raw
/// - CString rustc identity and storage representation (`cstring_type`, Box<[u8]>):
///   https://doc.rust-lang.org/src/alloc/ffi/c_str.rs.html
/// - raw pointers: https://doc.rust-lang.org/reference/types/pointer.html#raw-pointers-const-and-mut
/// - mem::drop: https://doc.rust-lang.org/std/mem/fn.drop.html
/// - mem::forget: https://doc.rust-lang.org/std/mem/fn.forget.html
fn rust_allocation_disposition_evidence<'tcx>(
    def_id: DefId,
    first_arg_ty: Option<ty::Ty<'tcx>>,
    tcx: TyCtxt<'tcx>,
) -> Option<RustAllocationDispositionEvidence> {
    let item_symbol = tcx.item_name(def_id);
    let item = item_symbol.as_str();

    if matches!(item, "into_raw" | "from_raw" | "leak") {
        let assoc_item = tcx.opt_associated_item(def_id)?;
        let impl_id = assoc_item.impl_container(tcx)?;
        if tcx.impl_trait_ref(impl_id).is_some() {
            return None;
        }
        let self_ty = tcx.type_of(impl_id).instantiate_identity();
        let TyKind::Adt(owner_def, _) = self_ty.kind() else {
            return None;
        };

        let kind = if tcx.lang_items().owned_box() == Some(owner_def.did()) {
            match item {
                "into_raw" => RustAllocationDispositionEvidenceKind::BoxIntoRaw,
                "from_raw" => RustAllocationDispositionEvidenceKind::BoxFromRaw,
                "leak" => RustAllocationDispositionEvidenceKind::BoxLeak,
                _ => unreachable!(),
            }
        } else if tcx.is_diagnostic_item(sym::cstring_type, owner_def.did()) {
            match item {
                "into_raw" => RustAllocationDispositionEvidenceKind::CStringIntoRaw,
                "from_raw" => RustAllocationDispositionEvidenceKind::CStringFromRaw,
                // CString has no `leak` API; fail closed if a future inherent
                // method with this spelling appears.
                "leak" => return None,
                _ => unreachable!(),
            }
        } else {
            return None;
        };

        return Some(RustAllocationDispositionEvidence {
            kind,
            callee_def_path: tcx.def_path_str(def_id),
            owner_def_path: Some(tcx.def_path_str(owner_def.did())),
        });
    }

    // Public std::mem::{drop,forget} resolve to core::mem items on the pinned
    // toolchain.  We classify from DefId metadata, not from rendered call text.
    let parent = tcx.parent(def_id);
    if !def_id.is_local()
        && tcx.crate_name(def_id.krate).as_str() == "core"
        && matches!(tcx.def_kind(parent), DefKind::Mod)
        && tcx.item_name(parent).as_str() == "mem"
    {
        match item {
            "drop" => {
                let arg_ty = first_arg_ty?;
                if matches!(arg_ty.kind(), TyKind::RawPtr(..)) {
                    return Some(RustAllocationDispositionEvidence {
                        kind: RustAllocationDispositionEvidenceKind::MemDropRawPointer,
                        callee_def_path: tcx.def_path_str(def_id),
                        owner_def_path: None,
                    });
                }
            }
            "forget" => {
                let arg_ty = first_arg_ty?;
                if let TyKind::Adt(owner_def, _) = arg_ty.kind() {
                    if tcx.lang_items().owned_box() == Some(owner_def.did()) {
                        return Some(RustAllocationDispositionEvidence {
                            kind: RustAllocationDispositionEvidenceKind::MemForgetOwnedBox,
                            callee_def_path: tcx.def_path_str(def_id),
                            owner_def_path: Some(tcx.def_path_str(owner_def.did())),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// v6Q/A3.3: producer-certify bounded higher-order `Option` and `Result`
/// combinators while rustc's semantic identity is still available.
///
/// The classification is deliberately structural: associated-item + inherent
/// impl + exact owner ADT identity.  It never infers higher-order semantics
/// from pretty-printed paths.
///
/// Official rustc identity APIs:
/// - `TyCtxt::opt_associated_item` / `AssocItem::impl_container` identify the
///   associated function and its impl container;
/// - `TyCtxt::is_diagnostic_item` compares the self ADT with rustc's canonical
///   `Result` diagnostic item;
/// - `LanguageItems::option_type` identifies the canonical `Option` ADT.
///
/// Official documentation:
/// https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/context/struct.TyCtxt.html
/// https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/symbol/sym/index.html
/// https://doc.rust-lang.org/core/option/enum.Option.html
/// https://doc.rust-lang.org/core/result/enum.Result.html
/// https://doc.rust-lang.org/core/ops/trait.FnOnce.html
///
/// The callback cardinalities below follow the public API contracts.  These
/// methods take `FnOnce` callbacks, so each callback is invoked at most once;
/// branch-selecting methods invoke the callback only for the documented enum
/// variant.  In particular `Result::map_err` applies its callback only to
/// `Err`, leaving `Ok` untouched.
///
/// `callback_argument_indices` are MIR call argument positions (receiver is
/// argument 0).  Recording them producer-side prevents unrelated closure-typed
/// arguments from being mistaken for callbacks by the ICFG consumer.
fn rust_higher_order_call_evidence<'tcx>(
    def_id: DefId,
    tcx: TyCtxt<'tcx>,
) -> Option<RustHigherOrderCallEvidence> {
    let assoc_item = tcx.opt_associated_item(def_id)?;
    let impl_id = assoc_item.impl_container(tcx)?;
    if tcx.impl_trait_ref(impl_id).is_some() {
        return None;
    }
    let self_ty = tcx.type_of(impl_id).instantiate_identity();
    let TyKind::Adt(owner_def, _) = self_ty.kind() else {
        return None;
    };

    enum CertifiedOwner {
        Option,
        Result,
    }
    let owner = if tcx.lang_items().option_type() == Some(owner_def.did()) {
        CertifiedOwner::Option
    } else if tcx.is_diagnostic_item(sym::Result, owner_def.did()) {
        CertifiedOwner::Result
    } else {
        return None;
    };

    // Keep the rustc `Symbol` binding alive while borrowing its underlying str.
    let method_symbol = tcx.item_name(def_id);
    let method = method_symbol.as_str();
    let (kind, callback_argument_indices) = match owner {
        CertifiedOwner::Option => match method {
            "map" => (RustHigherOrderCallEvidenceKind::OptionMap, vec![1]),
            "map_or" => (RustHigherOrderCallEvidenceKind::OptionMapOr, vec![2]),
            "map_or_else" => (RustHigherOrderCallEvidenceKind::OptionMapOrElse, vec![1, 2]),
            "and_then" => (RustHigherOrderCallEvidenceKind::OptionAndThen, vec![1]),
            "filter" => (RustHigherOrderCallEvidenceKind::OptionFilter, vec![1]),
            "inspect" => (RustHigherOrderCallEvidenceKind::OptionInspect, vec![1]),
            "or_else" => (RustHigherOrderCallEvidenceKind::OptionOrElse, vec![1]),
            "unwrap_or_else" => (RustHigherOrderCallEvidenceKind::OptionUnwrapOrElse, vec![1]),
            "ok_or_else" => (RustHigherOrderCallEvidenceKind::OptionOkOrElse, vec![1]),
            "is_some_and" => (RustHigherOrderCallEvidenceKind::OptionIsSomeAnd, vec![1]),
            "is_none_or" => (RustHigherOrderCallEvidenceKind::OptionIsNoneOr, vec![1]),
            _ => return None,
        },
        CertifiedOwner::Result => match method {
            "map" => (RustHigherOrderCallEvidenceKind::ResultMap, vec![1]),
            "map_err" => (RustHigherOrderCallEvidenceKind::ResultMapErr, vec![1]),
            "map_or" => (RustHigherOrderCallEvidenceKind::ResultMapOr, vec![2]),
            "map_or_else" => (RustHigherOrderCallEvidenceKind::ResultMapOrElse, vec![1, 2]),
            "and_then" => (RustHigherOrderCallEvidenceKind::ResultAndThen, vec![1]),
            "or_else" => (RustHigherOrderCallEvidenceKind::ResultOrElse, vec![1]),
            "unwrap_or_else" => (RustHigherOrderCallEvidenceKind::ResultUnwrapOrElse, vec![1]),
            "inspect" => (RustHigherOrderCallEvidenceKind::ResultInspect, vec![1]),
            "inspect_err" => (RustHigherOrderCallEvidenceKind::ResultInspectErr, vec![1]),
            "is_ok_and" => (RustHigherOrderCallEvidenceKind::ResultIsOkAnd, vec![1]),
            "is_err_and" => (RustHigherOrderCallEvidenceKind::ResultIsErrAnd, vec![1]),
            _ => return None,
        },
    };

    Some(RustHigherOrderCallEvidence {
        kind,
        callee_def_path: tcx.def_path_str(def_id),
        owner_def_path: tcx.def_path_str(owner_def.did()),
        callback_argument_indices,
    })
}


/// A3.6: resolve the user-written `Drop::drop` body associated with a MIR
/// `Drop(place)` from rustc type identity.
///
/// Rust defines a destructor as first invoking `<T as Drop>::drop` when the
/// dropped ADT implements `Drop`, followed by recursive field destruction.  We
/// therefore resolve only the explicit user destructor here; ordinary drop
/// glue for fields remains represented by CREMA's existing Drop completion
/// summary.  No path/name matching is involved.
fn a3_user_destructor_for_drop<'tcx>(
    tcx: TyCtxt<'tcx>,
    terminator: &Option<Terminator<'tcx>>,
    local_decls: &IndexVec<Local, LocalDecl<'tcx>>,
) -> Option<(String, DefId)> {
    let term = terminator.as_ref()?;
    let TerminatorKind::Drop { place, .. } = &term.kind else {
        return None;
    };
    let dropped_ty = place.ty(local_decls, tcx).ty;
    let TyKind::Adt(adt, _) = dropped_ty.kind() else {
        return None;
    };
    let destructor = adt.destructor(tcx)?;
    Some((tcx.def_path_str(destructor.did), destructor.did))
}


/// A3.4: discover concrete callback bodies referenced by a producer-certified
/// higher-order call in an imported application-dependency MIR body.
///
/// This is deliberately a *body-availability* operation, not an invocation
/// heuristic.  A discovered closure/FnDef is merely made available in the
/// represented function domain; control-flow edges to it are still created
/// only by `higher_order_call_semantics` after producer certification.
///
/// Scientific basis (official Rust documentation):
/// - every closure has a distinct anonymous closure type and all closures
///   implement `FnOnce`; some also implement `FnMut`/`Fn` depending on capture
///   use: https://doc.rust-lang.org/reference/types/closure.html#call-traits-and-coercions
/// - `Result::map_err` accepts `O: FnOnce(E) -> F` and invokes it only for
///   `Err`: https://doc.rust-lang.org/std/result/enum.Result.html#method.map_err
/// - foreign `optimized_mir(DefId)` is read from dependency metadata when MIR
///   is encoded: https://rustc-dev-guide.rust-lang.org/mir/passes.html
///
/// Only callback argument positions certified by
/// `rust_higher_order_call_evidence` are inspected.  This prevents an
/// unrelated closure-typed argument from causing callback-body materialization.
fn a3_certified_external_callback_bodies<'tcx>(
    tcx: TyCtxt<'tcx>,
    terminator: &Option<Terminator<'tcx>>,
    local_decls: &IndexVec<Local, LocalDecl<'tcx>>,
) -> BTreeMap<String, DefId> {
    let mut out = BTreeMap::new();
    let Some(term) = terminator.as_ref() else {
        return out;
    };
    let TerminatorKind::Call { func, args, .. } = &term.kind else {
        return out;
    };

    let func_ty = func.ty(local_decls, tcx);
    let TyKind::FnDef(callee_def, _) = func_ty.kind() else {
        return out;
    };
    let Some(evidence) = rust_higher_order_call_evidence(*callee_def, tcx) else {
        return out;
    };

    for callback_index in evidence.callback_argument_indices {
        let Some(spanned_arg) = args.get(callback_index) else {
            continue;
        };
        let callback_ty = spanned_arg.node.ty(local_decls, tcx);
        for generic_arg in callback_ty.walk() {
            let Some(nested_ty) = generic_arg.as_type() else {
                continue;
            };
            let callback_def = match nested_ty.kind() {
                TyKind::Closure(def_id, _) | TyKind::FnDef(def_id, _) => *def_id,
                _ => continue,
            };

            // Local closures are already collected by `tcx.hir().body_owners()`.
            // For external callbacks, retain exactly the same A3 application-
            // dependency/sysroot boundary as ordinary imported MIR.
            if callback_def.is_local()
                || !tcx.is_mir_available(callback_def)
                || !a3_external_mir_import_allowed(tcx, callback_def)
            {
                continue;
            }

            out.entry(tcx.def_path_str(callback_def))
                .or_insert(callback_def);
        }
    }

    out
}


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
    /// Structural closure-object binding recovered from the MIR destination
    /// place whose type is the corresponding local closure DefId.  Higher-order
    /// library summaries use this only to bind the closure environment (`_1`)
    /// when entering a callback body; iterator item arguments remain unknown.
    pub closure_bindings: BTreeMap<String, MirCallArgument>,
    /// A3.6 exact user `Drop::drop` target for each MIR Drop terminator.
    /// Keyed by canonical rustc function DefPath and MIR basic-block index.
    pub drop_destructors: BTreeMap<(String, usize), String>,
    /// Drop sites whose application/local destructor body is required by the
    /// A3 profile.  If materialization fails, schema-v2 must fail closed rather
    /// than silently reverting to a summary edge.
    pub drop_destructor_requires_body: BTreeSet<(String, usize)>,
    /// User-selected concrete entry used only to seed v6L rustc Instance
    /// propagation. It is not a heuristic target selector.
    pub instance_entry_hint: String,
    /// Reserved semantic boundary discovered while preparing concrete Instance
    /// dispatch. Generic entries are not a global boundary: they are analyzed
    /// parametrically, with genuinely unresolved local trait dispatch rejected
    /// at the callsite instead of rejecting the whole entry.
    pub instance_dispatch_boundary: Option<String>,
    /// v6O operational paths.  These are explicit so a Cargo rustc wrapper may
    /// run from the analyzed package directory without reading/writing stale
    /// files from an unrelated current working directory.
    pub ffi_functions_path: String,
    pub icfg_output_path: String,
    /// Legacy direct RunCompiler invocations stop after MIR extraction.  Cargo
    /// wrapper invocations must continue through codegen so Cargo receives the
    /// artifact it requested.
    pub continue_after_analysis: bool,
}

impl MirExtractor {
    
    pub fn new(llvm_output_dir: String, instance_entry_hint: String) -> Self {
        MirExtractor {
            mir_representation: MirRepresentation { functions: BTreeMap::new() },
            llvm_representation: None,
            llvm_output_dir,
            rust_function_arg_counts: HashMap::new(),
            closure_bindings: BTreeMap::new(),
            drop_destructors: BTreeMap::new(),
            drop_destructor_requires_body: BTreeSet::new(),
            instance_entry_hint,
            instance_dispatch_boundary: None,
            ffi_functions_path: "./ffi_functions.json".to_string(),
            icfg_output_path: "global_icfg.json".to_string(),
            continue_after_analysis: false,
        }
    }

    pub fn new_with_operational_paths(
        llvm_output_dir: String,
        instance_entry_hint: String,
        ffi_functions_path: String,
        icfg_output_path: String,
        continue_after_analysis: bool,
    ) -> Self {
        let mut extractor = Self::new(llvm_output_dir, instance_entry_hint);
        extractor.ffi_functions_path = ffi_functions_path;
        extractor.icfg_output_path = icfg_output_path;
        extractor.continue_after_analysis = continue_after_analysis;
        extractor
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

        // Typed producer-side statement evidence.  v6P-r1d records the primary
        // affected place for every pinned MIR statement that has one; consumers
        // never need to recover it from rustc Debug text.  `details` remains
        // diagnostic provenance, except for Intrinsic where a stable producer
        // prefix records which of the two pinned semantic variants was observed.
        let mut place_info: Option<String> = None;
        let mut rvalue: Option<String> = None;
        let mut is_mutable: Option<bool> = None;
        let mut details = format!("{:?}", statement.kind);

        match &statement.kind {
            StatementKind::Assign(box (lhs, rhs)) => {
                let (place_desc, mutable_flag) = self.describe_place(lhs, local_decls);
                place_info = Some(place_desc);
                rvalue = Some(format!("{:?}", rhs));
                is_mutable = Some(mutable_flag);
            }
            StatementKind::FakeRead(data) => {
                let (_, place) = &**data;
                let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                place_info = Some(place_desc);
                is_mutable = Some(mutable_flag);
            }
            StatementKind::SetDiscriminant { place, .. }
            | StatementKind::Deinit(place)
            | StatementKind::PlaceMention(place) => {
                let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                place_info = Some(place_desc);
                is_mutable = Some(mutable_flag);
            }
            StatementKind::Retag(_, place) => {
                let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                place_info = Some(place_desc);
                is_mutable = Some(mutable_flag);
            }
            StatementKind::AscribeUserType(data, _) => {
                let (place, _) = &**data;
                let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                place_info = Some(place_desc);
                is_mutable = Some(mutable_flag);
            }
            StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                place_info = Some(format!("Local({:?})", local));
                is_mutable = Some(
                    local_decls
                        .get(*local)
                        .map(|decl| decl.mutability == Mutability::Mut)
                        .unwrap_or(false),
                );
            }
            StatementKind::BackwardIncompatibleDropHint { place, .. } => {
                let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                place_info = Some(place_desc);
                is_mutable = Some(mutable_flag);
            }
            StatementKind::Intrinsic(intrinsic) => match &**intrinsic {
                NonDivergingIntrinsic::Assume(_) => {
                    details = format!("Intrinsic::Assume {:?}", intrinsic);
                }
                NonDivergingIntrinsic::CopyNonOverlapping(copy) => {
                    details = format!("Intrinsic::CopyNonOverlapping {:?}", intrinsic);
                    if let Operand::Copy(place) | Operand::Move(place) = &copy.dst {
                        let (place_desc, mutable_flag) = self.describe_place(place, local_decls);
                        place_info = Some(place_desc);
                        is_mutable = Some(mutable_flag);
                    }
                }
            },
            StatementKind::Coverage(..)
            | StatementKind::ConstEvalCounter
            | StatementKind::Nop => {}
        }

        MirStatement {
            source_info: source_info_data,
            kind: statement_kind.to_string(),
            details,
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
            TerminatorKind::UnwindTerminate(reason) => MirTerminator::UnwindTerminate {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
                reason: format!("{:?}", reason),
            },
            TerminatorKind::Unreachable => MirTerminator::Unreachable {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::Drop { place, target, unwind, .. } => {
                let (dropped_desc, is_mut) = self.describe_place(place, local_decls);
                let deallocator_evidence = rust_drop_allocator_evidence(place, local_decls, tcx);
                MirTerminator::Drop {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    return_target: format!("{:?}", target),
                    unwind_target: unwind_action_to_string(unwind),
                    dropped_value: dropped_desc,
                    is_mutable: is_mut,
                    deallocator_evidence,
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
                let first_arg_ty = args.first().map(|arg| arg.node.ty(local_decls, tcx));
                let (callee_def_path, callee_is_local, deallocator_evidence, allocation_disposition_evidence, higher_order_evidence) =
                    match func_ty.kind() {
                        TyKind::FnDef(def_id, _) => {
                            (
                                Some(tcx.def_path_str(*def_id)),
                                def_id.is_local(),
                                rust_call_deallocator_evidence(*def_id, tcx),
                                if mir_semantics_v2_enabled() {
                                    rust_allocation_disposition_evidence(*def_id, first_arg_ty, tcx)
                                } else {
                                    None
                                },
                                if mir_semantics_v2_enabled() {
                                    rust_higher_order_call_evidence(*def_id, tcx)
                                } else {
                                    None
                                },
                            )
                        }
                        _ => (None, false, None, None, None),
                    };

                if let Some(evidence) = &higher_order_evidence {
                    eprintln!(
                        "V6Q_HIGHER_ORDER_PRODUCER_EVIDENCE: kind={:?} callee={} owner={} callback_args={:?}",
                        evidence.kind,
                        evidence.callee_def_path,
                        evidence.owner_def_path,
                        evidence.callback_argument_indices,
                    );
                }

                if let Some(evidence) = &allocation_disposition_evidence {
                    eprintln!(
                        "V6S_ALLOCATION_DISPOSITION_PRODUCER_EVIDENCE: kind={:?} callee={} owner={}",
                        evidence.kind,
                        evidence.callee_def_path,
                        evidence.owner_def_path.as_deref().unwrap_or("<none>"),
                    );
                }

                // Recover callback identities from rustc types, never from MIR
                // Debug strings.  For producer-certified APIs, inspect ONLY the
                // callback-bearing MIR argument positions recorded by the producer.
                // This prevents unrelated closure-typed arguments from acquiring
                // callback control-flow edges.  Legacy non-certified summaries keep
                // their historical all-argument scan for compatibility.
                //
                // `FnDef` is included in addition to `Closure`: a real crate may
                // pass a named local function as an `FnOnce` callback.  A coerced
                // `fn` pointer no longer carries a unique DefId; if certification
                // exists but no callback identity can be recovered, schema-v2 will
                // fail closed below rather than silently treating the call as plain
                // external control flow.
                let certified_callback_indices = higher_order_evidence
                    .as_ref()
                    .map(|e| e.callback_argument_indices.as_slice());
                let mut callback_paths = BTreeSet::new();
                for (arg_index, spanned_arg) in args.iter().enumerate() {
                    if let Some(indices) = certified_callback_indices {
                        if !indices.contains(&arg_index) {
                            continue;
                        }
                    }
                    let operand = &spanned_arg.node;
                    let arg_ty = operand.ty(local_decls, tcx);
                    for generic_arg in arg_ty.walk() {
                        let Some(nested_ty) = generic_arg.as_type() else { continue; };
                        match nested_ty.kind() {
                            TyKind::Closure(def_id, _) | TyKind::FnDef(def_id, _) => {
                                callback_paths.insert(tcx.def_path_str(*def_id));
                            }
                            _ => {}
                        }
                    }
                }

                MirTerminator::Call {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    function_called: format!("{:?}", func),
                    callee_def_path,
                    deallocator_evidence,
                    allocation_disposition_evidence,
                    higher_order_evidence,
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
            TerminatorKind::TailCall { func, args, .. } => {
                let call_arguments: Vec<MirCallArgument> = args.iter().map(|spanned_arg| {
                    let operand = &spanned_arg.node;
                    match operand {
                        Operand::Copy(place) | Operand::Move(place) => {
                            let (desc, is_mut) = self.describe_place(place, local_decls);
                            MirCallArgument { arg: desc, is_mutable: Some(is_mut) }
                        },
                        _ => MirCallArgument { arg: format!("{:?}", operand), is_mutable: None },
                    }
                }).collect();
                let func_ty = func.ty(local_decls, tcx);
                let (callee_def_path, callee_is_local) = match func_ty.kind() {
                    TyKind::FnDef(def_id, _) => (Some(tcx.def_path_str(*def_id)), def_id.is_local()),
                    _ => (None, false),
                };
                MirTerminator::TailCall {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    function_called: format!("{:?}", func),
                    callee_def_path,
                    callee_is_local,
                    arguments: call_arguments,
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
            TerminatorKind::Yield { value, resume, resume_arg, drop } => {
                let (resume_desc, _) = self.describe_place(resume_arg, local_decls);
                MirTerminator::Yield {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    resume_target: format!("{:?}", resume),
                    drop_target: drop.map(|bb| format!("{:?}", bb)),
                    resume_arg: resume_desc,
                    value: format!("{:?}", value),
                }
            },
            TerminatorKind::CoroutineDrop => MirTerminator::CoroutineDrop {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
            },
            TerminatorKind::FalseEdge { real_target, imaginary_target } => MirTerminator::FalseEdge {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
                real_target: format!("{:?}", real_target),
                imaginary_target: format!("{:?}", imaginary_target),
            },
            TerminatorKind::FalseUnwind { real_target, unwind } => MirTerminator::FalseUnwind {
                details: format!("{:?}", t),
                source_info: format!("{:?}", t.source_info.span),
                real_target: format!("{:?}", real_target),
                unwind_target: unwind_action_to_string(unwind),
            },
            TerminatorKind::InlineAsm { template, operands, options, line_spans, targets, unwind, .. } => {
                MirTerminator::InlineAsm {
                    details: format!("{:?}", t),
                    source_info: format!("{:?}", t.source_info.span),
                    template: template.iter().map(|s| s.to_string()).collect(),
                    operands: operands.iter().map(|op| format!("{:?}", op)).collect(),
                    options: format!("{:?}", options),
                    line_spans: line_spans.iter().map(|span| format!("{:?}", span)).collect(),
                    targets: targets.iter().map(|bb| format!("{:?}", bb)).collect(),
                    unwind_target: Some(unwind_action_to_string(unwind)),
                }
            },
            // Any future pinned variant lands here and remains visible in telemetry.
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
    // Historical field name retained: under the A3 profile this set also
    // contains external DefPaths whose MIR is available and imported into the
    // represented function domain.
    local_callees: BTreeSet<String>,
    observed: bool,
    has_external: bool,
    unresolved: bool,
}

#[derive(Debug, Clone, Default)]
struct ReachableInstanceDispatch {
    calls: BTreeMap<(String, usize), ConcreteCallDispatch>,
    /// External dependency bodies admitted only when rustc proves MIR is
    /// available in crate metadata. Keyed by canonical DefPath for stable
    /// serialization/lookup; DefId is used only inside this rustc session.
    external_mir_bodies: BTreeMap<String, DefId>,
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
    include_external_mir: bool,
) -> Result<ReachableInstanceDispatch, String> {
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
        return Ok(ReachableInstanceDispatch::default());
    }

    let entry_def = candidates[0].to_def_id();
    // A generic CLI entry denotes the generic MIR body itself, as it did before
    // v6L.  Do not invent a type argument and do not call Instance::mono on a
    // definition that still requires monomorphization.  Instead, analyze that
    // MIR body parametrically and retain the v6K canonical direct-call resolver.
    // Concrete Instance enrichment is simply unavailable for this root.
    //
    // Soundness boundary: a local trait-item call reached from the parametric
    // body cannot be assigned an arbitrary implementation.  Such callsites are
    // marked unresolved below, so schema-v2 export still fails closed locally
    // when concrete dispatch is genuinely required.
    if tcx.generics_of(entry_def).requires_monomorphization(tcx) {
        eprintln!(
            "v6L Instance dispatch: selected entry '{}' ({}) is generic; analyzing generic MIR parametrically without choosing concrete type/const arguments",
            requested_entry,
            tcx.def_path_str(entry_def)
        );
        return Ok(ReachableInstanceDispatch {
            calls: resolve_parametric_generic_entry_dispatch(
                tcx,
                entry_def,
                &local_body_paths,
            ),
            external_mir_bodies: BTreeMap::new(),
        });
    }
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
    let mut external_mir_bodies: BTreeMap<String, DefId> = BTreeMap::new();

    while let Some(caller) = worklist.pop_front() {
        if seen.contains(&caller) {
            continue;
        }
        seen.push(caller);
        let caller_def = caller.def_id();
        let caller_path = tcx.def_path_str(caller_def);
        if caller_def.is_local() {
            if !local_body_paths.contains(&caller_path) {
                continue;
            }
        } else if !include_external_mir
            || !tcx.is_mir_available(caller_def)
            || !a3_external_mir_import_allowed(tcx, caller_def)
        {
            continue;
        }
        let body = tcx.optimized_mir(caller_def);
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
                    } else if include_external_mir
                        && tcx.is_mir_available(callee.def_id())
                        && a3_external_mir_import_allowed(tcx, callee.def_id())
                    {
                        let callee_path = tcx.def_path_str(callee.def_id());
                        dispatch.local_callees.insert(callee_path.clone());
                        external_mir_bodies
                            .entry(callee_path)
                            .or_insert(callee.def_id());
                        worklist.push_back(callee);
                    } else {
                        // Sysroot MIR deliberately remains behind CREMA's
                        // existing semantic/library-summary boundary.
                        dispatch.has_external = true;
                    }
                }
                Ok(None) | Err(_) => dispatch.unresolved = true,
            }
        }
    }
    Ok(ReachableInstanceDispatch {
        calls: out,
        external_mir_bodies,
    })
}

/// Traverse a generic entry without inventing monomorphization arguments.
///
/// Direct local functions/closures are left to the canonical DefPath resolver,
/// which analyzes their generic MIR bodies parametrically.  Local *trait-item*
/// calls are different: choosing one implementation would require a concrete
/// receiver/type context, so those callsites are explicitly marked unresolved.
/// External calls remain summaries and existing callback fail-closed handling
/// continues to apply.
fn resolve_parametric_generic_entry_dispatch<'tcx>(
    tcx: TyCtxt<'tcx>,
    entry_def: DefId,
    local_body_paths: &BTreeSet<String>,
) -> BTreeMap<(String, usize), ConcreteCallDispatch> {
    let mut out: BTreeMap<(String, usize), ConcreteCallDispatch> = BTreeMap::new();
    let mut worklist = std::collections::VecDeque::from([entry_def]);
    let mut seen: Vec<DefId> = Vec::new();

    while let Some(caller_def) = worklist.pop_front() {
        if !caller_def.is_local() || seen.contains(&caller_def) {
            continue;
        }
        seen.push(caller_def);
        let caller_path = tcx.def_path_str(caller_def);
        if !local_body_paths.contains(&caller_path) {
            continue;
        }
        let body = tcx.optimized_mir(caller_def);
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let Some(term) = data.terminator.as_ref() else { continue; };
            let TerminatorKind::Call { func, .. } = &term.kind else { continue; };
            let func_ty = func.ty(&body.local_decls, tcx);
            let (callee_def, _) = match func_ty.kind() {
                TyKind::FnDef(def_id, args) => (*def_id, *args),
                _ => continue,
            };

            if !callee_def.is_local() {
                continue;
            }

            // A trait declaration item is not a concrete implementation.  In a
            // generic entry its dispatch may depend on an unknown type parameter;
            // never connect it to an arbitrary generic trait body.
            if matches!(tcx.def_kind(callee_def), DefKind::AssocFn)
                && matches!(tcx.def_kind(tcx.parent(callee_def)), DefKind::Trait)
            {
                let dispatch = out.entry((caller_path.clone(), bb.index())).or_default();
                dispatch.observed = true;
                dispatch.unresolved = true;
                continue;
            }

            let callee_path = tcx.def_path_str(callee_def);
            if local_body_paths.contains(&callee_path) {
                worklist.push_back(callee_def);
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


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HigherOrderCallSemantics {
    /// Iterator adaptor construction is lazy: carrying a closure is not an
    /// invocation.  The closure DefId remains visible in the adaptor type and
    /// is recovered again at a later consuming call.
    LazyCarrier,
    /// A consuming iterator operation may invoke the callbacks carried by the
    /// iterator zero or more times.  The ICFG therefore contains an exit edge
    /// plus callback-return loops back to the consumer callsite.
    RepeatedCallbacks,
    /// `std::thread::spawn` may overlap callback execution with the caller.
    /// CREMA does not claim a full concurrency/interleaving semantics; it
    /// conservatively exposes both caller continuation and one callback branch.
    SpawnMayOnce,
    /// A branch-selecting combinator such as `Option::map_or` invokes its
    /// callback zero or one time depending on the receiver discriminant.  The
    /// ICFG overapproximates both branches and rejoins at the caller continuation.
    ConditionalCallbackOnce,
    /// A callback-bearing external API without a proven summary stays closed.
    Unknown,
}

/// Consume only producer-certified higher-order evidence. DefPath strings in
/// the evidence are audit provenance and are not reinterpreted here.
fn producer_certified_higher_order_semantics(
    evidence: Option<&RustHigherOrderCallEvidence>,
) -> Option<HigherOrderCallSemantics> {
    match evidence.map(|e| &e.kind) {
        Some(
            RustHigherOrderCallEvidenceKind::OptionMap
            | RustHigherOrderCallEvidenceKind::OptionMapOr
            | RustHigherOrderCallEvidenceKind::OptionMapOrElse
            | RustHigherOrderCallEvidenceKind::OptionAndThen
            | RustHigherOrderCallEvidenceKind::OptionFilter
            | RustHigherOrderCallEvidenceKind::OptionInspect
            | RustHigherOrderCallEvidenceKind::OptionOrElse
            | RustHigherOrderCallEvidenceKind::OptionUnwrapOrElse
            | RustHigherOrderCallEvidenceKind::OptionOkOrElse
            | RustHigherOrderCallEvidenceKind::OptionIsSomeAnd
            | RustHigherOrderCallEvidenceKind::OptionIsNoneOr
            | RustHigherOrderCallEvidenceKind::ResultMap
            | RustHigherOrderCallEvidenceKind::ResultMapErr
            | RustHigherOrderCallEvidenceKind::ResultMapOr
            | RustHigherOrderCallEvidenceKind::ResultMapOrElse
            | RustHigherOrderCallEvidenceKind::ResultAndThen
            | RustHigherOrderCallEvidenceKind::ResultOrElse
            | RustHigherOrderCallEvidenceKind::ResultUnwrapOrElse
            | RustHigherOrderCallEvidenceKind::ResultInspect
            | RustHigherOrderCallEvidenceKind::ResultInspectErr
            | RustHigherOrderCallEvidenceKind::ResultIsOkAnd
            | RustHigherOrderCallEvidenceKind::ResultIsErrAnd,
        ) => Some(HigherOrderCallSemantics::ConditionalCallbackOnce),
        None => None,
    }
}

fn higher_order_call_semantics(
    callee_def_path: Option<&str>,
    callback_def_paths: &[String],
    higher_order_evidence: Option<&RustHigherOrderCallEvidence>,
) -> Option<HigherOrderCallSemantics> {
    // v6Q-r1: certified API identity is considered before callback recovery.
    // If rustc proves that this is a supported higher-order Option API but the
    // callback value has lost its unique DefId (e.g. a coerced function pointer),
    // return Unknown so schema-v2 remains fail-closed.  Never silently downgrade
    // a certified higher-order call to an ordinary external summary.
    // Producer-certified evidence is itself the capability proof that this call
    // was classified on the opt-in MIR-semantics-v2 path.  The producer only
    // constructs `higher_order_evidence` while `mir_semantics_v2_enabled()` is
    // true, so re-reading the process environment here is redundant and makes
    // this pure consumer inconsistent under direct unit testing.  Consume the
    // evidence whenever it is present; the legacy path remains unchanged because
    // it serializes/constructs `None`.
    if let Some(semantics) = producer_certified_higher_order_semantics(higher_order_evidence) {
        if callback_def_paths.is_empty() {
            return Some(HigherOrderCallSemantics::Unknown);
        }
        return Some(semantics);
    }

    if callback_def_paths.is_empty() {
        return None;
    }

    let Some(path) = callee_def_path else {
        return Some(HigherOrderCallSemantics::Unknown);
    };

    let lazy = [
        "std::iter::Iterator::map",
        "std::iter::Iterator::filter",
        "core::iter::traits::iterator::Iterator::map",
        "core::iter::traits::iterator::Iterator::filter",
    ];
    if lazy.contains(&path) {
        return Some(HigherOrderCallSemantics::LazyCarrier);
    }

    let repeated = [
        "std::iter::Iterator::for_each",
        "std::iter::Iterator::collect",
        "std::iter::Iterator::product",
        "core::iter::traits::iterator::Iterator::for_each",
        "core::iter::traits::iterator::Iterator::collect",
        "core::iter::traits::iterator::Iterator::product",
    ];
    if repeated.contains(&path) {
        return Some(HigherOrderCallSemantics::RepeatedCallbacks);
    }

    if path == "std::thread::spawn" {
        return Some(HigherOrderCallSemantics::SpawnMayOnce);
    }

    Some(HigherOrderCallSemantics::Unknown)
}

fn local_higher_order_callbacks(
    callback_def_paths: &[String],
    functions: &BTreeMap<String, Vec<MirBasicBlock>>,
) -> Vec<String> {
    let mut out: Vec<String> = callback_def_paths
        .iter()
        .filter(|path| functions.contains_key(*path))
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

fn higher_order_callback_return_node(
    semantics: HigherOrderCallSemantics,
    call_site: &str,
    caller_return: &str,
) -> String {
    match semantics {
        HigherOrderCallSemantics::RepeatedCallbacks => call_site.to_string(),
        HigherOrderCallSemantics::SpawnMayOnce
        | HigherOrderCallSemantics::ConditionalCallbackOnce => caller_return.to_string(),
        HigherOrderCallSemantics::LazyCarrier | HigherOrderCallSemantics::Unknown => {
            caller_return.to_string()
        }
    }
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

/// MIR nodes through which an exception leaves `function` and must continue in
/// the caller.  This is deliberately distinct from normal `Return`: unwind can
/// leave through an explicit `UnwindResume`, or directly from a potentially
/// panicking terminator whose rustc unwind action is `Continue`.
fn unwind_exit_nodes_for(
    function: &str,
    functions: &BTreeMap<String, Vec<MirBasicBlock>>,
) -> Vec<String> {
    let mut out: Vec<String> = functions
        .get(function)
        .into_iter()
        .flat_map(|blocks| blocks.iter())
        .filter(|block| {
            match block.terminator.as_ref() {
                Some(MirTerminator::UnwindResume { .. }) => true,
                Some(MirTerminator::Call { unwind_target, .. })
                | Some(MirTerminator::Drop { unwind_target, .. })
                | Some(MirTerminator::Assert { unwind_target, .. }) => {
                    extract_target(unwind_target) == "continue"
                }
                Some(MirTerminator::InlineAsm { unwind_target: Some(unwind_target), .. }) => {
                    extract_target(unwind_target) == "continue"
                }
                _ => false,
            }
        })
        .map(|block| format!("rust::{function}::bb{}", block.block_id))
        .collect();
    out.sort();
    out.dedup();
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
        .filter(|edge| edge.destination.ends_with("::terminate") || edge.destination.ends_with("::unresolved_tail_call"))
        .map(|edge| edge.destination.clone())
        .filter(|id| !existing.contains(id))
        .collect();

    for id in terminals {
        let reason = if id.ends_with("::unresolved_tail_call") {
            "unresolved_tail_call".to_string()
        } else {
            "unwind_terminate".to_string()
        };
        nodes.push((
            id,
            GlobalICFGNode::Terminal(TerminalNode { reason }),
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
        // v6O-r1a: in Cargo-wrapper mode, discover foreign declarations from
        // the exact selected rustc invocation.  This replaces the historical
        // standalone ffi_extraction pre-pass for the opt-in v6O path.  The
        // legacy path remains unchanged because continue_after_analysis=false.
        //
        // Scientific boundary: this records declarations present in the
        // selected crate HIR only. It does not synthesize summaries for
        // external libraries or dependency crates and therefore cannot create
        // allocator/deallocator facts that are absent from the selected crate.
        if self.continue_after_analysis {
            let hir = tcx.hir();
            let mut ffi_names = BTreeSet::new();
            for item_id in hir.items() {
                let item = hir.item(item_id);
                if let ItemKind::ForeignMod { items, .. } = &item.kind {
                    for foreign_item_ref in *items {
                        let foreign_item = hir.foreign_item(foreign_item_ref.id);
                        if matches!(&foreign_item.kind, ForeignItemKind::Fn(..)) {
                            ffi_names.insert(foreign_item.ident.to_string());
                        }
                    }
                }
            }
            let payload = serde_json::json!({
                "ffi_functions": ffi_names.iter().cloned().collect::<Vec<_>>()
            });
            let serialized = serde_json::to_string_pretty(&payload)
                .expect("Failed to serialize v6O Cargo-selected FFI declarations");
            std::fs::write(&self.ffi_functions_path, serialized)
                .unwrap_or_else(|e| panic!(
                    "Failed to write v6O Cargo-selected FFI declarations {}: {e}",
                    self.ffi_functions_path
                ));
            println!(
                "V6O_CARGO_FFI_DECLARATIONS: PASS functions={} names={}",
                ffi_names.len(),
                ffi_names.iter().cloned().collect::<Vec<_>>().join(",")
            );
        }

        // --- 1. build concrete rustc Instance dispatch from the selected entry ---
        let reachable_dispatch = match resolve_reachable_instance_dispatch(
            tcx,
            &self.instance_entry_hint,
            panic_unwind_lifecycle_v1_enabled(),
        ) {
            Ok(dispatch) => dispatch,
            Err(boundary) => {
                self.instance_dispatch_boundary = Some(boundary);
                ReachableInstanceDispatch::default()
            }
        };
        let concrete_dispatch = &reachable_dispatch.calls;

        // --- 2. costruisco la MIR per ogni funzione ---
        // A3.6 seeds external user-destructor materialization from Drop
        // terminators found in local bodies, then closes the same set while
        // importing dependency MIR below.
        let mut a3_external_drop_bodies: BTreeMap<String, DefId> = BTreeMap::new();
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
                if panic_unwind_lifecycle_v1_enabled() {
                    if let Some((drop_path, drop_def)) = a3_user_destructor_for_drop(
                        tcx,
                        &data.terminator,
                        &body.local_decls,
                    ) {
                        self.drop_destructors
                            .insert((function_name.clone(), bb.index()), drop_path.clone());

                        let body_required = drop_def.is_local()
                            || a3_external_mir_import_allowed(tcx, drop_def);
                        if body_required {
                            self.drop_destructor_requires_body
                                .insert((function_name.clone(), bb.index()));
                        }
                        if !drop_def.is_local()
                            && tcx.is_mir_available(drop_def)
                            && a3_external_mir_import_allowed(tcx, drop_def)
                        {
                            a3_external_drop_bodies
                                .entry(drop_path)
                                .or_insert(drop_def);
                        }
                    }
                }

                // Recover the local object that stores each closure environment
                // from the destination place type, not from debug/source text.
                // This binding survives lazy iterator adaptor construction in
                // the MAY abstraction and lets a later consumer enter the
                // callback with its captured environment.
                for stmt in &data.statements {
                    if let StatementKind::Assign(assign) = &stmt.kind {
                        let (place, _) = &**assign;
                        if !place.projection.is_empty() {
                            continue;
                        }
                        let place_ty = body.local_decls[place.local].ty;
                        if let TyKind::Closure(def_id, _) = place_ty.kind() {
                            let (desc, is_mut) = self.describe_place(place, &body.local_decls);
                            self.closure_bindings.entry(tcx.def_path_str(*def_id)).or_insert(
                                MirCallArgument { arg: desc, is_mutable: Some(is_mut) }
                            );
                        }
                    }
                }
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
        // --- A3: import reachable external dependency MIR when rustc metadata
        // explicitly reports it as available.  This is opt-in so the frozen
        // corpus boundary remains unchanged. Generic dependency functions are
        // the primary target: rustc must encode their MIR for downstream
        // monomorphization.
        if panic_unwind_lifecycle_v1_enabled() {
            // A3.4 keeps body materialization closed over callback bodies that
            // are explicitly referenced by producer-certified higher-order
            // calls inside an imported dependency body.  This fixes the case
            // where `aligned_box::allocate` contains an inline `map_err` closure:
            // rustc exposes the closure DefId in the callback operand type, but
            // it is not a direct call target and therefore is absent from the
            // ordinary Instance-dispatch worklist.
            //
            // Importing the body does NOT assert invocation.  Invocation remains
            // governed by the certified higher-order summary below.
            let mut initial_external_mir: BTreeMap<String, DefId> = reachable_dispatch
                .external_mir_bodies
                .iter()
                .map(|(path, def_id)| (path.clone(), *def_id))
                .collect();
            initial_external_mir.extend(a3_external_drop_bodies.iter().map(
                |(path, def_id)| (path.clone(), *def_id),
            ));

            let mut external_mir_worklist: std::collections::VecDeque<(String, DefId)> =
                initial_external_mir
                    .iter()
                    .map(|(path, def_id)| (path.clone(), *def_id))
                    .collect();
            let mut external_mir_scheduled: BTreeSet<String> = initial_external_mir
                .keys()
                .cloned()
                .collect();

            while let Some((function_name, def_id)) = external_mir_worklist.pop_front() {
                if self.mir_representation.functions.contains_key(&function_name) {
                    continue;
                }
                if !tcx.is_mir_available(def_id)
                    || !a3_external_mir_import_allowed(tcx, def_id)
                {
                    continue;
                }
                match tcx.def_kind(def_id) {
                    DefKind::Fn | DefKind::AssocFn | DefKind::Closure => {}
                    _ => continue,
                }

                let body = tcx.optimized_mir(def_id);

                // Discover callback and custom-destructor bodies before building
                // the ICFG. The queue is deterministic by DefPath and
                // deduplicated by the scheduled set. Missing metadata remains
                // fail-closed later: we never manufacture a body.
                for data in body.basic_blocks.iter() {
                    for (callback_path, callback_def) in
                        a3_certified_external_callback_bodies(
                            tcx,
                            &data.terminator,
                            &body.local_decls,
                        )
                    {
                        if external_mir_scheduled.insert(callback_path.clone()) {
                            eprintln!(
                                "A3_EXTERNAL_CALLBACK_MIR_DISCOVERED: caller={} callback={}",
                                function_name, callback_path
                            );
                            external_mir_worklist
                                .push_back((callback_path, callback_def));
                        }
                    }
                    if let Some((drop_path, drop_def)) = a3_user_destructor_for_drop(
                        tcx,
                        &data.terminator,
                        &body.local_decls,
                    ) {
                        if !drop_def.is_local()
                            && tcx.is_mir_available(drop_def)
                            && a3_external_mir_import_allowed(tcx, drop_def)
                            && external_mir_scheduled.insert(drop_path.clone())
                        {
                            eprintln!(
                                "A3_EXTERNAL_DROP_MIR_DISCOVERED: caller={} destructor={}",
                                function_name, drop_path
                            );
                            external_mir_worklist.push_back((drop_path, drop_def));
                        }
                    }
                }

                let mut function_blocks = Vec::new();
                for (bb, data) in body.basic_blocks.iter_enumerated() {
                    if let Some((drop_path, drop_def)) = a3_user_destructor_for_drop(
                        tcx,
                        &data.terminator,
                        &body.local_decls,
                    ) {
                        self.drop_destructors
                            .insert((function_name.clone(), bb.index()), drop_path);
                        if drop_def.is_local()
                            || a3_external_mir_import_allowed(tcx, drop_def)
                        {
                            self.drop_destructor_requires_body
                                .insert((function_name.clone(), bb.index()));
                        }
                    }
                    for stmt in &data.statements {
                        if let StatementKind::Assign(assign) = &stmt.kind {
                            let (place, _) = &**assign;
                            if place.projection.is_empty() {
                                let place_ty = body.local_decls[place.local].ty;
                                if let TyKind::Closure(closure_def, _) = place_ty.kind() {
                                    let (desc, is_mut) = self.describe_place(place, &body.local_decls);
                                    self.closure_bindings
                                        .entry(tcx.def_path_str(*closure_def))
                                        .or_insert(MirCallArgument { arg: desc, is_mutable: Some(is_mut) });
                                }
                            }
                        }
                    }

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
                    function_blocks.push(MirBasicBlock {
                        block_id: bb.index(),
                        statements: data.statements.iter()
                            .map(|stmt| self.convert_statement(stmt, &body.local_decls))
                            .collect(),
                        terminator,
                    });
                }
                self.rust_function_arg_counts.insert(function_name.clone(), body.arg_count);
                self.mir_representation
                    .functions
                    .insert(function_name.clone(), function_blocks);
                println!("A3_EXTERNAL_MIR_IMPORTED: {}", function_name);
            }
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
        let ffi_functions = match load_ffi_functions(&self.ffi_functions_path) {
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
                    higher_order_evidence,
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

                            // Higher-order standard-library calls are modeled by
                            // API semantics over canonical DefPaths, never by the
                            // target name or pretty-printed MIR. Lazy iterator
                            // adaptors carry callbacks but do not invoke them.
                            // Consuming iterator operations overapproximate zero-or-
                            // more callback invocations with callback-return loops.
                            // thread::spawn exposes both caller continuation and a
                            // callback branch; this is callback reachability, not a
                            // claim of full concurrency/interleaving semantics.
                            let higher_order = if resolved_targets.is_empty() {
                                higher_order_call_semantics(
                                    callee_def_path.as_deref(),
                                    callback_def_paths,
                                    higher_order_evidence.as_ref(),
                                )
                            } else {
                                None
                            };
                            let callback_targets = match higher_order {
                                Some(HigherOrderCallSemantics::RepeatedCallbacks)
                                | Some(HigherOrderCallSemantics::SpawnMayOnce)
                                | Some(HigherOrderCallSemantics::ConditionalCallbackOnce) => {
                                    local_higher_order_callbacks(
                                        callback_def_paths,
                                        &self.mir_representation.functions,
                                    )
                                }
                                _ => Vec::new(),
                            };
                            let callback_bodies_complete = callback_targets.len()
                                == callback_def_paths.iter().collect::<BTreeSet<_>>().len();

                            if matches!(
                                higher_order,
                                Some(HigherOrderCallSemantics::RepeatedCallbacks)
                                    | Some(HigherOrderCallSemantics::SpawnMayOnce)
                                    | Some(HigherOrderCallSemantics::ConditionalCallbackOnce)
                            ) && callback_bodies_complete
                            {
                                let semantics = higher_order.unwrap();
                                for (ordinal, callee) in callback_targets.iter().enumerate() {
                                    let base_call = get_dummy_call_id(
                                        rust_func,
                                        block.block_id,
                                        &call_site,
                                        true,
                                    );
                                    let base_ret = get_dummy_ret_id(
                                        rust_func,
                                        &block.block_id.to_string(),
                                        &call_site,
                                        true,
                                    );
                                    let dummy_call_id = format!("{}::callback{}", base_call, ordinal);
                                    let dummy_ret_id = format!("{}::callback{}", base_ret, ordinal);
                                    let callback_return = higher_order_callback_return_node(
                                        semantics,
                                        &call_site,
                                        &caller_return,
                                    );
                                    let callback_arguments = self
                                        .closure_bindings
                                        .get(callee)
                                        .cloned()
                                        .into_iter()
                                        .collect();

                                    rust_calls.push(RustCallMetadata {
                                        caller_function: rust_func.clone(),
                                        call_node: call_site.clone(),
                                        callee_function: callee.clone(),
                                        dummy_call_node: dummy_call_id.clone(),
                                        dummy_ret_node: dummy_ret_id.clone(),
                                        arguments: callback_arguments,
                                        // A callback return value is not the return
                                        // value of spawn/collect/product/for_each.
                                        return_place: String::new(),
                                        return_node: callback_return.clone(),
                                        is_closure: true,
                                    });

                                    icfg_edges.push(IcfgEdge {
                                        source: call_site.clone(),
                                        destination: dummy_call_id.clone(),
                                        label: Some("Higher-order callback -> dummyCall".to_string()),
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
                                        destination: callback_return,
                                        label: Some(match semantics {
                                            HigherOrderCallSemantics::RepeatedCallbacks =>
                                                "Higher-order callback loop",
                                            HigherOrderCallSemantics::SpawnMayOnce =>
                                                "Spawn callback -> caller continuation",
                                            HigherOrderCallSemantics::ConditionalCallbackOnce =>
                                                "Conditional callback -> caller continuation",
                                            _ => unreachable!(),
                                        }.to_string()),
                                        source_label: None,
                                        destination_label: None,
                                    });
                                }
                            }

                            // Preserve the external/library return branch.  For a
                            // repeated consumer this is the zero/additional-exit
                            // branch; for spawn it represents caller continuation
                            // before the spawned callback's effects.
                            let need_summary = resolved_targets.is_empty()
                                || *instance_dispatch_external
                                || *instance_dispatch_unresolved;
                            if need_summary {
                                if let Some(rt) = return_target {
                                    let src = call_site.clone();
                                    let dst = format!("rust::{}::{}", rust_func, rt);
                                    let label = if *instance_dispatch_unresolved {
                                        "UNRESOLVED_LOCAL_CALL: concrete rustc Instance resolution incomplete"
                                    } else {
                                        match higher_order {
                                            Some(HigherOrderCallSemantics::LazyCarrier) =>
                                                "Higher-order lazy adaptor summary return",
                                            Some(HigherOrderCallSemantics::RepeatedCallbacks)
                                                if callback_bodies_complete =>
                                                    "Higher-order consumer exit",
                                            Some(HigherOrderCallSemantics::SpawnMayOnce)
                                                if callback_bodies_complete =>
                                                    "Spawn caller continuation",
                                            Some(HigherOrderCallSemantics::ConditionalCallbackOnce)
                                                if callback_bodies_complete =>
                                                    "Higher-order conditional callback exit",
                                            Some(HigherOrderCallSemantics::RepeatedCallbacks)
                                            | Some(HigherOrderCallSemantics::SpawnMayOnce)
                                            | Some(HigherOrderCallSemantics::ConditionalCallbackOnce) =>
                                                "UNRESOLVED_HIGHER_ORDER: local callback MIR body missing",
                                            Some(HigherOrderCallSemantics::Unknown) if higher_order_evidence.is_some() =>
                                                "UNRESOLVED_HIGHER_ORDER: producer-certified callback identity not recoverable",
                                            Some(HigherOrderCallSemantics::Unknown) =>
                                                "UNRESOLVED_HIGHER_ORDER: callback API semantics not modeled",
                                            None if *instance_dispatch_observed && *instance_dispatch_external =>
                                                "Resolved external Instance summary return",
                                            None if *instance_dispatch_observed =>
                                                "External/summary call return",
                                            None if *callee_is_local =>
                                                "UNRESOLVED_LOCAL_CALL: local FnDef has no canonical MIR body",
                                            None => "External/summary call return",
                                        }
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
                                let dst = format!("rust::{}::{}", rust_func, effective_unwind);

                                // A3: once a Rust callee body is represented completely,
                                // exceptional control returns from the callee's unwind exits,
                                // not directly from the caller callsite.  Keeping the old
                                // callsite->cleanup edge here would join an imprecise summary
                                // with the callee state and erase exactly the partial lifecycle
                                // information this capability is intended to preserve.
                                if panic_unwind_lifecycle_v1_enabled()
                                    && !resolved_targets.is_empty()
                                    && !need_summary
                                {
                                    for (callee, _) in &resolved_targets {
                                        for unwind_exit in unwind_exit_nodes_for(
                                            callee,
                                            &self.mir_representation.functions,
                                        ) {
                                            icfg_edges.push(IcfgEdge {
                                                source: unwind_exit,
                                                destination: dst.clone(),
                                                label: Some("Rust unwind propagate".to_string()),
                                                source_label: Some(format!(
                                                    "callee={} caller_bb={}",
                                                    callee, block.block_id
                                                )),
                                                destination_label: None,
                                            });
                                        }
                                    }
                                } else {
                                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
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
                if let Some(MirTerminator::Drop {
                    return_target,
                    unwind_target,
                    dropped_value,
                    ..
                }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let key = (rust_func.clone(), block.block_id);
                    let resolved_destructor = self
                        .drop_destructors
                        .get(&key)
                        .filter(|callee| self.mir_representation.functions.contains_key(*callee));

                    if let Some(callee) = resolved_destructor {
                        // A3.6: a MIR Drop of an ADT with user Drop is an
                        // interprocedural call to the exact rustc destructor
                        // body before drop-glue completion.  The completion
                        // summary itself is applied after the destructor's
                        // normal return by the abstract interpreter.
                        let call_site = src.clone();
                        let caller_return = format!("rust::{}::{}", rust_func, return_target);
                        let dummy_call_id = get_dummy_call_id(
                            rust_func,
                            block.block_id,
                            &call_site,
                            true,
                        );
                        let dummy_ret_id = get_dummy_ret_id(
                            rust_func,
                            &block.block_id.to_string(),
                            &call_site,
                            true,
                        );
                        rust_calls.push(RustCallMetadata {
                            caller_function: rust_func.clone(),
                            call_node: call_site.clone(),
                            callee_function: callee.clone(),
                            dummy_call_node: dummy_call_id.clone(),
                            dummy_ret_node: dummy_ret_id.clone(),
                            arguments: vec![MirCallArgument {
                                arg: dropped_value.clone(),
                                is_mutable: Some(true),
                            }],
                            return_place: String::new(),
                            return_node: caller_return.clone(),
                            is_closure: false,
                        });
                        icfg_edges.push(IcfgEdge {
                            source: call_site,
                            destination: dummy_call_id.clone(),
                            label: Some("Rust Drop -> dummyCall".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                        icfg_edges.push(IcfgEdge {
                            source: dummy_call_id,
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
                            destination: caller_return,
                            label: Some("dummyRet -> Rust Drop Continuation".to_string()),
                            source_label: Some(dropped_value.clone()),
                            destination_label: None,
                        });

                        // If the destructor itself unwinds into an explicit
                        // caller cleanup/terminate target, propagate from the
                        // represented body rather than from the Drop site.
                        let effective_unwind = extract_target(unwind_target);
                        if effective_unwind != "unreachable" && effective_unwind != "continue" {
                            let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                            for unwind_exit in unwind_exit_nodes_for(
                                callee,
                                &self.mir_representation.functions,
                            ) {
                                icfg_edges.push(IcfgEdge {
                                    source: unwind_exit,
                                    destination: dst.clone(),
                                    label: Some("Rust drop unwind propagate".to_string()),
                                    source_label: Some(format!(
                                        "destructor={} caller_bb={}",
                                        callee, block.block_id
                                    )),
                                    destination_label: None,
                                });
                            }
                        }
                    } else if self.drop_destructor_requires_body.contains(&key) {
                        let dst = format!("rust::{}::{}", rust_func, return_target);
                        icfg_edges.push(IcfgEdge {
                            source: src.clone(),
                            destination: dst,
                            label: Some(
                                "UNRESOLVED_DROP_DISPATCH: user Drop::drop MIR body missing"
                                    .to_string(),
                            ),
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
                    } else {
                        // No represented user destructor (including sysroot
                        // drop glue): preserve the established Drop summary.
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
                // TailCall has a distinct stack discipline: rustc pops the
                // current frame and calls with the current caller's return address.
                // The current ICFG cannot encode that without changing call/return
                // matching, so preserve the terminator structurally but force
                // schema-v2 CQPL export to fail closed on a reachable occurrence.
                if let Some(MirTerminator::TailCall { .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::bb{}::unresolved_tail_call", rust_func, block.block_id);
                    icfg_edges.push(IcfgEdge {
                        source: src,
                        destination: dst,
                        label: Some("UNRESOLVED_TAIL_CALL: caller-pop return-address semantics not modeled".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                }
                // Borrow-checking-only false edges are runtime gotos to the
                // real target; the imaginary edge is deliberately not put in
                // the executable ICFG.
                if let Some(MirTerminator::FalseEdge { real_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, real_target);
                    icfg_edges.push(IcfgEdge {
                        source: src,
                        destination: dst,
                        label: Some("FalseEdge real target".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                }
                if let Some(MirTerminator::FalseUnwind { real_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let dst = format!("rust::{}::{}", rust_func, real_target);
                    icfg_edges.push(IcfgEdge {
                        source: src,
                        destination: dst,
                        label: Some("FalseUnwind real target".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                }
                // Coroutine Yield is represented as a MAY abstraction over the
                // future resume continuation and optional drop continuation.
                if let Some(MirTerminator::Yield { resume_target, drop_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    let resume_dst = format!("rust::{}::{}", rust_func, resume_target);
                    icfg_edges.push(IcfgEdge {
                        source: src.clone(),
                        destination: resume_dst,
                        label: Some("Yield MAY resume".to_string()),
                        source_label: Some(format!("Mir bb{}", block.block_id)),
                        destination_label: None,
                    });
                    if let Some(drop_target) = drop_target {
                        let drop_dst = format!("rust::{}::{}", rust_func, drop_target);
                        icfg_edges.push(IcfgEdge {
                            source: src,
                            destination: drop_dst,
                            label: Some("Yield MAY drop".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                }
                // InlineAsm may have multiple normal targets.  Preserve every
                // rustc-provided target plus a real cleanup unwind target.
                if let Some(MirTerminator::InlineAsm { targets, unwind_target, .. }) = &block.terminator {
                    let src = format!("rust::{}::bb{}", rust_func, block.block_id);
                    for target in targets {
                        let dst = format!("rust::{}::{}", rust_func, target);
                        icfg_edges.push(IcfgEdge {
                            source: src.clone(),
                            destination: dst,
                            label: Some("InlineAsm target".to_string()),
                            source_label: Some(format!("Mir bb{}", block.block_id)),
                            destination_label: None,
                        });
                    }
                    if let Some(unwind_target) = unwind_target {
                        let effective_unwind = extract_target(unwind_target);
                        if effective_unwind != "unreachable" && effective_unwind != "continue" {
                            let dst = format!("rust::{}::{}", rust_func, effective_unwind);
                            icfg_edges.push(IcfgEdge {
                                source: src,
                                destination: dst,
                                label: Some("InlineAsm unwind".to_string()),
                                source_label: Some(format!("Mir bb{}", block.block_id)),
                                destination_label: None,
                            });
                        }
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
                    higher_order_evidence,
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

                            let higher_order = if resolved_targets.is_empty() {
                                higher_order_call_semantics(
                                    callee_def_path.as_deref(),
                                    callback_def_paths,
                                    higher_order_evidence.as_ref(),
                                )
                            } else {
                                None
                            };
                            if matches!(
                                higher_order,
                                Some(HigherOrderCallSemantics::RepeatedCallbacks)
                                    | Some(HigherOrderCallSemantics::SpawnMayOnce)
                                    | Some(HigherOrderCallSemantics::ConditionalCallbackOnce)
                            ) {
                                let semantics = higher_order.unwrap();
                                let callback_targets = local_higher_order_callbacks(
                                    callback_def_paths,
                                    &self.mir_representation.functions,
                                );
                                if callback_targets.len()
                                    == callback_def_paths.iter().collect::<BTreeSet<_>>().len()
                                {
                                    for (ordinal, callee) in callback_targets.iter().enumerate() {
                                        let base_call = get_dummy_call_id(
                                            rust_func,
                                            block.block_id,
                                            &call_site,
                                            true,
                                        );
                                        let base_ret = get_dummy_ret_id(
                                            rust_func,
                                            &block.block_id.to_string(),
                                            &call_site,
                                            true,
                                        );
                                        let dummy_call_id = format!("{}::callback{}", base_call, ordinal);
                                        let dummy_ret_id = format!("{}::callback{}", base_ret, ordinal);
                                        let callee_entry = format!("rust::{}::bb0", callee);
                                        let callback_return = higher_order_callback_return_node(
                                            semantics,
                                            &call_site,
                                            &caller_return,
                                        );

                                        ordered_icfg_nodes.push((
                                            dummy_call_id.clone(),
                                            GlobalICFGNode::DummyCall(DummyNode {
                                                dummy_node_name: "dummyCall".to_string(),
                                                incoming_edge: mir_node_id.clone(),
                                                outgoing_edge: callee_entry,
                                                id: compute_hash(&(mir_node_id.clone(), dummy_call_id.clone())),
                                                mir_var: self
                                                    .closure_bindings
                                                    .get(callee)
                                                    .map(|arg| arg.arg.clone()),
                                                llvm_var: None,
                                                is_internal: Some(true),
                                            }),
                                        ));
                                        ordered_icfg_nodes.push((
                                            dummy_ret_id.clone(),
                                            GlobalICFGNode::DummyRet(DummyNode {
                                                dummy_node_name: "dummyRet".to_string(),
                                                incoming_edge: callee.clone(),
                                                outgoing_edge: callback_return.clone(),
                                                id: compute_hash(&(callback_return, dummy_ret_id.clone())),
                                                // Callback result is not the external
                                                // higher-order API's return value.
                                                mir_var: None,
                                                llvm_var: None,
                                                is_internal: Some(true),
                                            }),
                                        ));
                                    }
                                }
                            }
                        }
                    }

                if let Some(MirTerminator::Drop {
                    return_target,
                    dropped_value,
                    ..
                }) = &block.terminator {
                    let key = (rust_func.clone(), block.block_id);
                    if let Some(callee) = self
                        .drop_destructors
                        .get(&key)
                        .filter(|callee| self.mir_representation.functions.contains_key(*callee))
                    {
                        let call_site = mir_node_id.clone();
                        let caller_return = format!("rust::{}::{}", rust_func, return_target);
                        let dummy_call_id = get_dummy_call_id(
                            rust_func,
                            block.block_id,
                            &call_site,
                            true,
                        );
                        let dummy_ret_id = get_dummy_ret_id(
                            rust_func,
                            &block.block_id.to_string(),
                            &call_site,
                            true,
                        );
                        ordered_icfg_nodes.push((
                            dummy_call_id.clone(),
                            GlobalICFGNode::DummyCall(DummyNode {
                                dummy_node_name: "dummyCall".to_string(),
                                incoming_edge: mir_node_id.clone(),
                                outgoing_edge: format!("rust::{}::bb0", callee),
                                id: compute_hash(&(mir_node_id.clone(), dummy_call_id.clone())),
                                mir_var: Some(dropped_value.clone()),
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
                                id: compute_hash(&(caller_return, dummy_ret_id.clone())),
                                mir_var: None,
                                llvm_var: None,
                                is_internal: Some(true),
                            }),
                        ));
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
        let output_filename = self.icfg_output_path.clone();
        let mut file = File::create(&output_filename)
            .expect("Failed to create output file");
        let json_output =
            serde_json::to_string_pretty(&global_icfg_ordered).expect("Failed to serialize JSON");
        file.write_all(json_output.as_bytes())
            .expect("Failed to write JSON output");
        println!("Global ICFG saved to {}", output_filename);
    });
    if self.continue_after_analysis {
        rustc_driver::Compilation::Continue
    } else {
        rustc_driver::Compilation::Stop
    }
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
    fn a3_external_mir_boundary_keeps_sysroot_behind_summaries() {
        for name in [
            "std",
            "core",
            "alloc",
            "proc_macro",
            "test",
            "panic_unwind",
            "panic_abort",
            "unwind",
            "compiler_builtins",
        ] {
            assert!(a3_is_sysroot_crate_name(name), "expected sysroot crate: {name}");
        }
    }

    #[test]
    fn a3_external_mir_boundary_allows_application_dependencies() {
        for name in ["aligned_box", "serde", "my_dependency"] {
            assert!(!a3_is_sysroot_crate_name(name), "unexpected sysroot classification: {name}");
        }
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
    fn result_map_err_producer_evidence_is_conditional_once() {
        let evidence = RustHigherOrderCallEvidence {
            kind: RustHigherOrderCallEvidenceKind::ResultMapErr,
            callee_def_path: "core::result::Result::<T, E>::map_err".to_string(),
            owner_def_path: "core::result::Result".to_string(),
            callback_argument_indices: vec![1],
        };
        assert_eq!(
            producer_certified_higher_order_semantics(Some(&evidence)),
            Some(HigherOrderCallSemantics::ConditionalCallbackOnce)
        );
    }

    #[test]
    fn higher_order_standard_library_semantics_are_classified_structurally() {
        let callbacks = vec!["main::{closure#0}".to_string()];
        for path in [
            "std::iter::Iterator::map",
            "std::iter::Iterator::filter",
        ] {
            assert_eq!(
                higher_order_call_semantics(Some(path), &callbacks, None),
                Some(HigherOrderCallSemantics::LazyCarrier)
            );
        }
        for path in [
            "std::iter::Iterator::collect",
            "std::iter::Iterator::product",
            "std::iter::Iterator::for_each",
        ] {
            assert_eq!(
                higher_order_call_semantics(Some(path), &callbacks, None),
                Some(HigherOrderCallSemantics::RepeatedCallbacks)
            );
        }
        assert_eq!(
            higher_order_call_semantics(Some("std::thread::spawn"), &callbacks, None),
            Some(HigherOrderCallSemantics::SpawnMayOnce)
        );
        assert_eq!(
            higher_order_call_semantics(Some("third_party::register_callback"), &callbacks, None),
            Some(HigherOrderCallSemantics::Unknown)
        );
    }

    #[test]
    fn producer_certified_option_family_maps_to_conditional_once() {
        for kind in [
            RustHigherOrderCallEvidenceKind::OptionMap,
            RustHigherOrderCallEvidenceKind::OptionMapOr,
            RustHigherOrderCallEvidenceKind::OptionMapOrElse,
            RustHigherOrderCallEvidenceKind::OptionAndThen,
            RustHigherOrderCallEvidenceKind::OptionFilter,
            RustHigherOrderCallEvidenceKind::OptionInspect,
            RustHigherOrderCallEvidenceKind::OptionOrElse,
            RustHigherOrderCallEvidenceKind::OptionUnwrapOrElse,
            RustHigherOrderCallEvidenceKind::OptionOkOrElse,
            RustHigherOrderCallEvidenceKind::OptionIsSomeAnd,
            RustHigherOrderCallEvidenceKind::OptionIsNoneOr,
        ] {
            let evidence = RustHigherOrderCallEvidence {
                kind,
                callee_def_path: "audit-only".into(),
                owner_def_path: "core::option::Option".into(),
                callback_argument_indices: vec![1],
            };
            assert_eq!(
                producer_certified_higher_order_semantics(Some(&evidence)),
                Some(HigherOrderCallSemantics::ConditionalCallbackOnce)
            );
        }
        assert_eq!(producer_certified_higher_order_semantics(None), None);
    }

    #[test]
    fn certified_higher_order_without_callback_identity_fails_closed() {
        let evidence = RustHigherOrderCallEvidence {
            kind: RustHigherOrderCallEvidenceKind::OptionMap,
            callee_def_path: "core::option::{impl#0}::map".into(),
            owner_def_path: "core::option::Option".into(),
            callback_argument_indices: vec![1],
        };
        assert_eq!(
            higher_order_call_semantics(Some("audit-only"), &[], Some(&evidence)),
            Some(HigherOrderCallSemantics::Unknown)
        );
    }

    #[test]
    fn higher_order_repeated_callbacks_loop_but_spawn_returns_to_continuation() {
        assert_eq!(
            higher_order_callback_return_node(
                HigherOrderCallSemantics::RepeatedCallbacks,
                "rust::main::bb7",
                "rust::main::bb8",
            ),
            "rust::main::bb7"
        );
        assert_eq!(
            higher_order_callback_return_node(
                HigherOrderCallSemantics::SpawnMayOnce,
                "rust::main::bb7",
                "rust::main::bb8",
            ),
            "rust::main::bb8"
        );
    }

    #[test]
    fn higher_order_callback_targets_are_local_sorted_and_deduplicated() {
        let functions = empty_functions(&[
            "main::{closure#0}",
            "main::{closure#1}",
        ]);
        let got = local_higher_order_callbacks(
            &[
                "main::{closure#1}".into(),
                "missing::{closure#0}".into(),
                "main::{closure#0}".into(),
                "main::{closure#1}".into(),
            ],
            &functions,
        );
        assert_eq!(
            got,
            vec![
                "main::{closure#0}".to_string(),
                "main::{closure#1}".to_string(),
            ]
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
    fn unresolved_tail_call_destination_is_materialized_and_labeled_fail_closed() {
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
            destination: "rust::main::bb0::unresolved_tail_call".into(),
            label: Some("UNRESOLVED_TAIL_CALL: caller-pop return-address semantics not modeled".into()),
            source_label: None,
            destination_label: None,
        }];

        materialize_terminal_nodes(&mut nodes, &edges);
        assert!(matches!(
            nodes.iter().find(|(id, _)| id.ends_with("::unresolved_tail_call")).map(|(_, n)| n),
            Some(GlobalICFGNode::Terminal(TerminalNode { reason })) if reason == "unresolved_tail_call"
        ));
        assert!(edges[0]
            .label
            .as_deref()
            .is_some_and(|label| label.starts_with("UNRESOLVED_")));
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

