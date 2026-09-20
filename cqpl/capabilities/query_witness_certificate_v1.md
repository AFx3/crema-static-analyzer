# `query_witness_certificate_v1`

`query_witness_certificate_v1` is a read-only explainability output produced when an annotated ICFG declares `source_provenance_v1`. It is **not** a CQPL truth capability and need not appear in a query `requires` clause.

A certificate is generated once per existing directional `AllocationObligationFinding`; it does not run a second bug-finding algorithm. The pipeline is:

```text
CQPL truth result
  -> existing supporting/refuting findings
  -> cqpl_result_assessment_v1
  -> source-grounded certificate projection
```

This ordering is an implementation invariant: certificate construction cannot affect `ff | unk | tt` or `unk_true | unk_false | unk_mixed | unk_unoriented`.

## Canonical allocation site versus witness entry

The certificate intentionally separates two concepts that historical explanation fields could conflate:

- `allocation.node` is the canonical node encoded in `AbstractAllocId.site.node_id` for `rust_call` or `c_call` sites;
- `witness_entry` is the node chosen by the existing diagnostic path search and may merely be a later node whose abstract post-state still contains `Alloc`.

`witness_entry` must never be relabeled as the allocation origin. Synthetic allocation sites report `synthetic_no_source_anchor` rather than fabricating source provenance.

## Event grounding

Ordered diagnostic roles include, as applicable, ownership handoff, normal return, first/second deallocation, use after deallocation, allocator-family mismatch, modeled freed-state barrier, and non-returning discharge. For allocation events (`alloc`, `drop`, `read`, `write`, `use`), source anchors are taken only from the validated `(node, allocation, predicate)` provenance relation.

A role may report:

- `grounded` — exactly one source anchor;
- `multiple_candidate_anchors` — the abstract event corresponds to more than one syntactic occurrence;
- `source_unavailable` — no producer anchor is available;
- `synthetic_no_source_anchor` — canonical allocation identity is synthetic.

Multiple candidate anchors are preserved rather than arbitrarily selecting a line.

## Abstract witness semantics

`abstract_witness.model` is `annotated_abstract_icfg` and `concrete_execution` is always `false`. Every consecutive pair of nodes is validated against the ICFG successor relation. When `typed_edge_flow_v1` is present, the certificate also records the exact producer-certified flow (`normal` or `unwind`). Under `assessment_scope normal_execution`, every witness edge must include a `normal` typed edge.

The certificate therefore witnesses ordering in the **abstract transition system**; it is not presented as a concrete counterexample trace.

## Uncertainty frontier

The certificate preserves the CQPL reason frontier and classifies each reason by layer (`abstract_domain`, `abstract_control_flow`, `producer_or_contract`, or `cqpl_semantics`). Reasons are not forced to have a source location when no such location is semantically meaningful.

## Gate invariant

A W1 acceptance gate must retain the frozen semantic matrix exactly:

```text
truth delta      = 0 / 1416
assessment delta = 0 / 1416
```

and must additionally validate certificate structure, canonical allocation-site grounding, event/source consistency, typed-edge consistency, deterministic serialization, and explicit handling of missing or multiple source anchors.
