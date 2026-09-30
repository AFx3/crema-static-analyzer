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
        | Some("Rust drop unwind propagate")
        | Some("DEP1 unwind exit -> matched caller cleanup") => EdgeFlowKind::Unwind,
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
            "DEP1 unwind exit -> matched caller cleanup",
        ] {
            assert_eq!(edge_flow_kind(&edge(label)), EdgeFlowKind::Unwind);
        }
    }

    #[test]
    fn ordinary_edges_are_normal() {
        assert_eq!(edge_flow_kind(&edge("Drop return")), EdgeFlowKind::Normal);
        assert_eq!(edge_flow_kind(&edge("Goto")), EdgeFlowKind::Normal);
    }

    #[test]
    fn diagnostic_unwind_substring_is_not_a_semantic_edge_kind() {
        assert_eq!(edge_flow_kind(&edge("not-unwind-diagnostic")), EdgeFlowKind::Normal);
        assert_eq!(edge_flow_kind(&edge("Rust Return -> dummyRet")), EdgeFlowKind::Normal);
        assert_eq!(edge_flow_kind(&edge("dummyRet -> Rust Continuation")), EdgeFlowKind::Normal);
        assert_eq!(edge_flow_kind(&edge("cleanup internal edge")), EdgeFlowKind::Normal);
    }

    /// Run separately against the real production graph, not a mock fixture.
    #[test]
    #[ignore = "requires UW1_PRODUCTION_GRAPH and UW1_PRODUCTION_ANNOTATED artifacts"]
    fn matched_cleanup_is_consumed_by_real_lifecycle_on_production_graph() {
        let graph_path = env::var("UW1_PRODUCTION_GRAPH").expect("production G path");
        let annotated_path = env::var("UW1_PRODUCTION_ANNOTATED").expect("production A path");
        let graph: crate::structs::GlobalICFGOrdered = serde_json::from_str(
            &std::fs::read_to_string(graph_path).unwrap()).unwrap();
        let annotated: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(annotated_path).unwrap()).unwrap();
        let entry = annotated["entry"].as_str().expect("production entry");
        let identity = crate::identity::fixed_point_identity_analysis(&graph, entry);
        let lifecycle = crate::panic_lifecycle_domain::fixed_point_real_panic_lifecycle(
            &graph, &identity, entry).expect("real lifecycle accepts final G");
        let observed = std::cell::RefCell::new(std::collections::BTreeSet::new());
        crate::panic_lifecycle_domain::fixed_point_lifecycle_with_transfer(
            &graph, entry, &crate::panic_lifecycle_domain::PanicLifecycleMemory::default(),
            |edge, state| {
                if edge.label.as_deref() == Some("DEP1 unwind exit -> matched caller cleanup") {
                    assert_eq!(edge_flow_kind(edge), EdgeFlowKind::Unwind);
                    assert!(lifecycle.contains_block(&edge.destination));
                    observed.borrow_mut().insert((edge.source.clone(), edge.destination.clone()));
                }
                state.clone()
            }).unwrap();
        assert!(!observed.borrow().is_empty());
        println!("UW1_LIFECYCLE_CONSUMED={}", serde_json::to_string(&*observed.borrow()).unwrap());
    }
}
