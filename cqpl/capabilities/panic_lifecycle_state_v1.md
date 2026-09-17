# `panic_lifecycle_state_v1`

`panic_lifecycle_state_v1` exposes sparse positive A3.7 panic/unwind lifecycle
facts on schema-v2 nodes. It refines `panic_unwind_lifecycle_v1` and therefore
requires that capability as well.

Each node carries a `panic_lifecycle` array keyed by opaque `AbstractAllocId`.
The v1 certainty vocabulary contains only `may_abstract`.

A record contains these MAY facts:

- `may_own`
- `may_partial_drop`
- `may_stale_owner`
- `may_committed`
- `may_complete`

The positive repeat-drop witness is
`may_own && may_partial_drop && may_stale_owner`. A matching `may_abstract`
record can justify only `unk`, never `tt`.

Version 1 does not encode producer completeness. Therefore v1 remains the
sparse evidence layer; the queryable `repeat_drop(a)` contract is versioned by
`panic_lifecycle_state_v2`, which adds an explicit node-local coverage frontier
so absence can be distinguished from unresolved analysis.
