# `typed_edge_flow_v1`

`typed_edge_flow_v1` is an additive schema-v2 boundary capability that preserves the canonical CREMA ICFG edge relation when exporting to CQPL.

Each `typed_edges` record contains the canonical `source`, `destination`, original `label`, `source_label`, `destination_label`, and a closed `flow` classification: `normal` or `unwind`.

The capability is **observational in A/R2**: CTL/CQPL temporal operators continue to traverse `nodes[*].successors`. The checker therefore must not change query truth values merely because this capability is present.

The consumer validates all of the following:

- capability and payload are atomic: either both are present or both are absent;
- every typed edge endpoint belongs to the annotated node domain;
- exact duplicate typed edges are rejected;
- the typed `flow` agrees with the closed canonical unwind-label vocabulary;
- projecting `typed_edges` to `(source,destination)` yields exactly the legacy `successors` relation for every node.

The closed unwind labels in v1 are:

```text
Call unwind
Drop unwind
Assert unwind
InlineAsm unwind
Rust unwind propagate
Rust drop unwind propagate
```

Every other canonical edge label is classified as `normal` in v1. Adding a new unwind-producing label therefore requires an explicit protocol revision or synchronized producer/consumer update; it must not be guessed by the checker.

This capability intentionally does **not** scope allocation/deallocation events to an outcome. That is a separate semantic refinement (A/R3). In particular, `typed_edge_flow_v1` alone must never justify upgrading MAY allocation events to MUST/`tt`.
