use crate::structs::{MirBasicBlock, MirTerminator};
use std::collections::BTreeSet;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

/// v6P opt-in semantic profile.  The frozen v6O/v6N paths remain unchanged
/// unless `--mir-semantics-v2` explicitly enables this profile.
static MIR_SEMANTICS_V2: AtomicBool = AtomicBool::new(false);
const INTERNAL_ENV: &str = "CREMA_INTERNAL_MIR_SEMANTICS_V2";

pub fn set_mir_semantics_v2_enabled(enabled: bool) {
    MIR_SEMANTICS_V2.store(enabled, Ordering::SeqCst);
    if enabled {
        env::set_var(INTERNAL_ENV, "1");
    } else {
        env::remove_var(INTERNAL_ENV);
    }
}

pub fn mir_semantics_v2_enabled() -> bool {
    MIR_SEMANTICS_V2.load(Ordering::SeqCst)
        || env::var(INTERNAL_ENV).ok().as_deref() == Some("1")
}

/// Stable CQPL statement vocabulary for `mir_semantic_labels_v1`.
/// Names are intentionally independent of rustc Debug formatting.
pub fn statement_category(kind: &str) -> &'static str {
    match kind {
        "Assign" => "assign",
        "FakeRead" => "fake_read",
        "SetDiscriminant" => "set_discriminant",
        "Deinit" => "deinit",
        "StorageLive" => "storage_live",
        "StorageDead" => "storage_dead",
        "Retag" => "retag",
        "PlaceMention" => "place_mention",
        "AscribeUserType" => "ascribe_user_type",
        "Coverage" => "coverage",
        "Intrinsic" => "intrinsic",
        "ConstEvalCounter" => "const_eval_counter",
        "Nop" => "nop",
        "BackwardIncompatibleDropHint" => "backward_incompatible_drop_hint",
        _ => "other",
    }
}

/// Classify the *observed pinned-toolchain Debug spelling* into a stable
/// Rvalue vocabulary.  The semantics of each category are taken from rustc's
/// MIR definitions; the textual adapter itself is deliberately versioned and
/// must be revalidated on a compiler migration.
pub fn rvalue_category(raw: &str) -> &'static str {
    let t = raw.trim();
    if t.starts_with("copy ") || t.starts_with("move ") {
        return "use";
    }
    if t.starts_with("const ") {
        return "const";
    }
    if t.starts_with("AddWithOverflow(")
        || t.starts_with("SubWithOverflow(")
        || t.starts_with("MulWithOverflow(")
    {
        return "checked_binary_op";
    }
    if t.starts_with("PtrMetadata(") {
        return "ptr_metadata";
    }
    if t.starts_with("Discriminant(") || t.starts_with("discriminant(") {
        return "discriminant";
    }
    if t.starts_with("Len(") {
        return "len";
    }
    if t.starts_with("SizeOf(") || t.starts_with("AlignOf(") {
        return "nullary_op";
    }
    if t.starts_with("deref_copy ") || t.starts_with("CopyForDeref(") {
        return "copy_for_deref";
    }
    if t.starts_with("&raw ") {
        return "address_of";
    }
    if t.starts_with('&') {
        return "ref";
    }
    if t.contains(" as ") {
        return "cast";
    }
    const BINARY: [&str; 16] = [
        "Add(", "Sub(", "Mul(", "Div(", "Rem(", "BitXor(", "BitAnd(", "BitOr(",
        "Shl(", "Shr(", "Eq(", "Lt(", "Le(", "Ne(", "Ge(", "Gt(",
    ];
    if BINARY.iter().any(|p| t.starts_with(p)) {
        return "binary_op";
    }
    if t.starts_with("Neg(") || t.starts_with("Not(") {
        return "unary_op";
    }
    if t.starts_with("Repeat(") || t.starts_with('[') {
        return "repeat";
    }
    if t.starts_with("ThreadLocalRef(") {
        return "thread_local_ref";
    }
    if t.starts_with("ShallowInitBox(") {
        return "shallow_init_box";
    }

    // rustc's Debug form for Aggregate often starts directly with the ADT
    // constructor (`core::option::Option::<T>::Some`, `Range`, user structs).
    // Such values are not field-sensitive in CellValue and are therefore one
    // stable conservative category here.
    if t.contains("::") || t.contains(" { ") || t.ends_with('}') {
        return "aggregate";
    }
    "other"
}

pub fn terminator_category(term: &MirTerminator) -> &'static str {
    match term {
        MirTerminator::Goto { .. } => "goto",
        MirTerminator::SwitchInt { .. } => "switch_int",
        MirTerminator::UnwindResume { .. } => "unwind_resume",
        MirTerminator::UnwindTerminate { .. } => "unwind_terminate",
        MirTerminator::Return { .. } => "return",
        MirTerminator::Unreachable { .. } => "unreachable",
        MirTerminator::Drop { .. } => "drop",
        MirTerminator::Call { .. } => "call",
        MirTerminator::TailCall { .. } => "tail_call",
        MirTerminator::Assert { .. } => "assert",
        MirTerminator::Yield { .. } => "yield",
        MirTerminator::CoroutineDrop { .. } => "coroutine_drop",
        MirTerminator::FalseEdge { .. } => "false_edge",
        MirTerminator::FalseUnwind { .. } => "false_unwind",
        MirTerminator::InlineAsm { .. } => "inline_asm",
        MirTerminator::Unhandled { .. } => "unhandled",
    }
}

pub fn semantic_labels_for_block(block: &MirBasicBlock) -> Vec<String> {
    let mut labels = BTreeSet::new();
    for stmt in &block.statements {
        labels.insert(format!("stmt:{}", statement_category(&stmt.kind)));
        if let Some(rvalue) = stmt.rvalue.as_deref() {
            labels.insert(format!("rvalue:{}", rvalue_category(rvalue)));
        }
    }
    if let Some(term) = block.terminator.as_ref() {
        labels.insert(format!("term:{}", terminator_category(term)));
    }
    labels.into_iter().collect()
}

#[cfg(test)]
pub fn is_option_map_or_def_path(path: &str) -> bool {
    path.starts_with("core::option::Option") && path.ends_with("::map_or")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_rvalue_adapter_covers_real_crate_families() {
        assert_eq!(rvalue_category("copy (_3.0: usize)"), "use");
        assert_eq!(rvalue_category("const 4_usize"), "const");
        assert_eq!(rvalue_category("AddWithOverflow(copy _1, copy _2)"), "checked_binary_op");
        assert_eq!(rvalue_category("PtrMetadata(copy _3)"), "ptr_metadata");
        assert_eq!(rvalue_category("discriminant(_4)"), "discriminant");
        assert_eq!(rvalue_category("core::option::Option::<usize>::Some(copy _1)"), "aggregate");
    }

    #[test]
    fn option_map_or_classifier_uses_canonical_core_namespace() {
        assert!(is_option_map_or_def_path("core::option::Option<T>::map_or"));
        assert!(is_option_map_or_def_path("core::option::Option::map_or"));
        assert!(!is_option_map_or_def_path("third_party::Option::map_or"));
    }
}
