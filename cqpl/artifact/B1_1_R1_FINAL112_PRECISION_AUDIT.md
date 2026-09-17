# B1.1-r1 FINAL112 precision-delta audit

This audit records the reviewed B1.1-r1 precision delta over the immutable
historical FINAL112 graph/query audit. The historical baseline and the existing
A3 R4 seven-cell allowlist remain unchanged.

A fresh 112 x 12 candidate run after B1.1-r1 produced exactly twelve additional
changes, all `unk -> ff`, with no `tt` change:

- four allocator-mismatch refinements: the clean CString target and `rusant`
  each refine both allocator-mismatch queries after typed `CString` Drop becomes
  producer-certified as `rust_global`;
- eight `mir_structural_allocator_example` refinements. That query requires
  `term_l(drop) && allocator_mismatch_l(a)` at the same node. The affected MIR
  Drop nodes are typed `CString` drops whose deallocator family is now
  `rust_global`, so the same-node mismatch is refuted. Where a target contains
  a real C `free` mismatch, its dedicated allocator-mismatch query remains
  `unk`; B1.1 does not erase that evidence.

The reviewed cells are listed in `B1_1_R1_FINAL112_PRECISION_DELTAS.tsv`.

## Evidence invariants

The candidate run used exactly 112 subjects and 12 frozen queries (1344 cells).
It produced counts:

- `ff = 669`
- `unk = 449`
- `tt = 226`

These equal the immutable historical counts transformed by the seven previously
approved A3 refinements plus the twelve reviewed B1.1 refinements. No approved
A3 delta was missing and no wrong approved transition was observed.

B1.1 changes contract/provenance precision only; it does not promote MAY
allocation/deallocation evidence to MUST/TRUE.

## Policy

The B1.1 allowlist is additive and versioned. Do not edit the historical
`FINAL112_GRAPH_QUERY_AUDIT.tsv` and do not rewrite
`A3_R4_FINAL112_PRECISION_DELTAS.tsv`. A future change must receive its own
reviewed delta file rather than mutating either prior evidence set.
