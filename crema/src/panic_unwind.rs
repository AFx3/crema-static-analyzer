use crate::structs::IcfgEdge;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

/// Opt-in A3 semantic profile for panic/unwind-sensitive lifecycle propagation.
///
/// The frozen baseline remains unchanged unless the CLI enables this profile.
static PANIC_UNWIND_LIFECYCLE_V1: AtomicBool = AtomicBool::new(false);
const INTERNAL_ENV: &str = "CREMA_INTERNAL_PANIC_UNWIND_LIFECYCLE_V1";

pub fn set_panic_unwind_lifecycle_v1_enabled(enabled: bool) {
    PANIC_UNWIND_LIFECYCLE_V1.store(enabled, Ordering::SeqCst);
    if enabled {
        env::set_var(INTERNAL_ENV, "1");
    } else {
        env::remove_var(INTERNAL_ENV);
    }
}

pub fn panic_unwind_lifecycle_v1_enabled() -> bool {
    PANIC_UNWIND_LIFECYCLE_V1.load(Ordering::SeqCst)
        || env::var(INTERNAL_ENV).ok().as_deref() == Some("1")
}

/// Semantic edge class used by the A3 transfer layer.
///
/// Existing frozen ICFG JSON stores human-readable labels rather than a typed
/// enum.  This adapter gives the analysis a typed view without changing the
/// serialized v1/v2 ICFG format.  A future schema revision can serialize this
/// enum directly and keep this function only for backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeFlowKind {
    Normal,
    Unwind,
}

pub fn edge_flow_kind(edge: &IcfgEdge) -> EdgeFlowKind {
    match edge.label.as_deref() {
        Some("Call unwind")
        | Some("Drop unwind")
        | Some("Assert unwind")
        | Some("InlineAsm unwind")
        | Some("Rust unwind propagate")
        | Some("Rust drop unwind propagate") => EdgeFlowKind::Unwind,
        _ => EdgeFlowKind::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(label: &str) -> IcfgEdge {
        IcfgEdge {
            source: "rust::main::bb0".into(),
            destination: "rust::main::bb1".into(),
            label: Some(label.into()),
            source_label: None,
            destination_label: None,
        }
    }

    #[test]
    fn canonical_unwind_labels_are_typed_as_unwind() {
        for label in [
            "Call unwind",
            "Drop unwind",
            "Assert unwind",
            "InlineAsm unwind",
            "Rust unwind propagate",
            "Rust drop unwind propagate",
        ] {
            assert_eq!(edge_flow_kind(&edge(label)), EdgeFlowKind::Unwind);
        }
    }

    #[test]
    fn ordinary_edges_are_normal() {
        assert_eq!(edge_flow_kind(&edge("Drop return")), EdgeFlowKind::Normal);
        assert_eq!(edge_flow_kind(&edge("Goto")), EdgeFlowKind::Normal);
    }
}
