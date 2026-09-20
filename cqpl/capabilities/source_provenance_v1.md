# `source_provenance_v1`

`source_provenance_v1` is an additive schema-v2 producer capability for **diagnostic-only source grounding**. It does not create events, allocation identities, aliases, lifecycle facts, control-flow edges, or MUST evidence.

When the capability is present, every annotated ICFG node carries a `source_provenance` record. The record contains:

- `language = rust | c | synthetic`;
- a deterministic set of syntactic `anchors` for that node;
- `allocation_events`, which attach a subset of those exact anchors to an already-existing allocation-centric event `(predicate, AbstractAllocId)`.

Rust anchors distinguish MIR statements from the MIR terminator. Statement anchors carry their zero-based `statement_index`. Their basis is `rustc_mir_source_info_v1`. C anchors use the producer's SVF/LLVM node source location with basis `svf_llvm_node_source_loc_v1`.

A Rust anchor may additionally contain a structured `parsed_span` (`file`, start/end line and column). `raw_span` is retained for audit. The parsed file string is producer-reported and is not a logical identity; it may be absolute on some rustc invocations.

## Semantic non-interference

The producer builds provenance from the **same raw syntactic event occurrences** used to construct `allocation_labels`. Allocation-event grounding is then joined through the already-computed MAY `event_identity` relation. Consequently:

```text
source provenance = existing event semantics + existing MAY identity + source occurrence
```

and never:

```text
source text -> newly inferred semantic event
```

The consumer validates that every provenance event has a matching `allocation_label` with the same predicate, allocation ID, and `may_abstract` certainty. Provenance therefore cannot strengthen an event or convert MAY to MUST.

The capability and payload are atomic. A schema-v2 artifact declaring `source_provenance_v1` must contain `source_provenance` on every node; source-provenance payload without the capability is rejected.

## Ordering rule

`anchors` and `allocation_events` are sets serialized deterministically. Their serialization order is **not** an execution order. In particular, two events grounded in the same basic block are not ordered merely by their position in these arrays. A `mir_statement.statement_index` is an intra-block syntactic location, while inter-node ordering remains the responsibility of the annotated ICFG witness path.

## Missing source

Synthetic nodes legitimately have no source anchor. LLVM/SVF nodes whose producer artifact carries no source location also remain ungrounded. The diagnostic layer must expose this explicitly rather than invent a line from node names or nearby events.
