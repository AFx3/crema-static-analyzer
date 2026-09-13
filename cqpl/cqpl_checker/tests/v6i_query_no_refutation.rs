use cqpl_checker::{parse_query, AnnotatedIcfg, Env, Kripke, ModelChecker, Truth};
use serde_json::{json, Value};

const DF_QUERY: &str = include_str!("../../queries_v2/double_free_alloc.cqpl");
const LEAK_QUERY: &str = include_str!("../../queries_v2/leak_alloc.cqpl");
const UAF_QUERY: &str = include_str!("../../queries_v2/use_after_free_alloc.cqpl");

fn allocation_label(predicate: &str) -> Value {
    json!({
        "predicate": predicate,
        "allocation": "A",
        "certainty": "may_abstract"
    })
}

fn node(id: &str, successors: &[&str], predicates: &[&str]) -> Value {
    json!({
        "id": id,
        "successors": successors,
        "labels": [],
        "allocation_labels": predicates.iter().map(|p| allocation_label(p)).collect::<Vec<_>>(),
        "identity": null,
        "pre": {"cells": []},
        "post": {"cells": []}
    })
}

fn mixed_kripke(nodes: Vec<Value>) -> Kripke {
    // The witness deliberately contains both Rust and C program-variable domains.
    // Allocation-event predicates are bound to A, so the theorem regression is
    // independent of accidental variable-name aliasing.
    let input: AnnotatedIcfg = serde_json::from_value(json!({
        "schema_version": 2,
        "entry": "entry",
        "variables": [
            {"id": "rust::main::_1", "language": "rust"},
            {"id": "c::c_alloc::1@rust::main::bb0", "language": "c"}
        ],
        "allocations": [{
            "id": "A",
            "display": "c-call:c_alloc:malloc",
            "site": {"kind": "c_call", "node_id": "c_alloc", "allocator": "malloc"},
            "context": ["rust::main::bb0"]
        }],
        "nodes": nodes
    })).expect("synthetic schema-v2 witness must deserialize");
    Kripke::from_annotated_icfg(input).expect("synthetic schema-v2 witness must form a Kripke")
}

fn eval(k: &Kripke, query: &str) -> Truth {
    let formula = parse_query(query).expect("official query must parse");
    ModelChecker::new(k)
        .evaluate(&formula, &Env::new())
        .expect("official query must type-check and evaluate")
}

#[test]
fn v6i_double_free_complete_query_is_unknown_not_false_on_observable_witness() {
    let k = mixed_kripke(vec![
        node("entry", &["c_alloc"], &[]),
        node("c_alloc", &["after_alloc"], &["alloc"]),
        node("after_alloc", &["c_free_1"], &[]),
        node("c_free_1", &["after_free"], &["drop"]),
        node("after_free", &["c_free_2"], &[]),
        node("c_free_2", &[], &["drop"]),
    ]);
    assert_eq!(eval(&k, DF_QUERY), Truth::Unknown);
}

#[test]
fn v6i_use_after_free_complete_query_is_unknown_not_false_on_cross_language_witness() {
    let k = mixed_kripke(vec![
        node("entry", &["c_alloc"], &[]),
        node("c_alloc", &["after_alloc"], &["alloc"]),
        node("after_alloc", &["c_free"], &[]),
        node("c_free", &["rust_after_free"], &["drop"]),
        node("rust_after_free", &["rust_use"], &[]),
        node("rust_use", &[], &["use"]),
    ]);
    assert_eq!(eval(&k, UAF_QUERY), Truth::Unknown);
}

#[test]
fn v6i_leak_complete_query_is_unknown_not_false_on_maximal_no_drop_path() {
    let k = mixed_kripke(vec![
        node("entry", &["c_alloc"], &[]),
        node("c_alloc", &["rust_live"], &["alloc"]),
        node("rust_live", &["exit"], &[]),
        node("exit", &[], &[]),
    ]);
    assert_eq!(eval(&k, LEAK_QUERY), Truth::Unknown);
}

#[test]
fn v6i_negative_allocation_event_guards_cannot_be_false_in_schema_v2() {
    // Schema v2 has no exact/MUST allocation-event certainty. Therefore a positive
    // allocation atom is either ff or unk, and its negation is respectively tt or unk.
    let with_alloc = mixed_kripke(vec![
        node("entry", &[], &["alloc", "drop"]),
    ]);
    let no_events = mixed_kripke(vec![
        node("entry", &[], &[]),
    ]);
    assert_eq!(eval(&with_alloc, "exists_alloc a. !alloc_l(a)"), Truth::Unknown);
    assert_eq!(eval(&with_alloc, "exists_alloc a. !drop_l(a)"), Truth::Unknown);
    assert_eq!(eval(&no_events, "exists_alloc a. !alloc_l(a)"), Truth::True);
    assert_eq!(eval(&no_events, "exists_alloc a. !drop_l(a)"), Truth::True);
}
