# `conditional_reallocations_v1`

`conditional_reallocations_v1` is an additive, proof-carrying refinement for a
bodyless C `realloc` whose success/failure outcome is tested immediately by an
exact raw-pointer `is_null` predicate.

The capability does **not** invent a C CFG body and does not mutate CREMA's
abstract fixed point.  The producer emits a sidecar relation; CQPL may consume
that relation to materialize one checker-local semantic outcome state on the
certified successful branch.

## Certified preconditions

A record is emitted only when all of the following are proven:

1. `reallocation_boundaries_v1` identifies a bodyless `realloc` call on one
   `c_malloc` allocation identity.
2. `allocation_existence_guards_v1` proves that the source pointer is non-null
   and that the source allocation exists on the edge entering the `realloc`.
3. The size argument is a compile-time `usize` constant strictly greater than
   zero.  `realloc(p, 0)` is deliberately outside v1 because a null result does
   not certify preservation of the old object.
4. The `realloc` result variable is tested by an exact raw-pointer `is_null`
   call and the predicate result drives the next boolean `SwitchInt`.
5. Both failure (`q == NULL`) and success (`q != NULL`) successors are exact
   canonical ICFG edges.
6. Any `result_deallocations` record is a direct bodyless C `free(q)` call on a
   success-reachable path.  The v1 checker accepts only the closed proof basis
   `rust_foreign_decl_c_free_result_v1`.

If any condition is ambiguous, the capability/payload is absent and RBF1
continues to fail closed.

## Semantics

For a certified record with positive non-zero size:

- failure (`q == NULL`) preserves the old allocation and no successful-result
  object is assumed;
- success (`q != NULL`) invalidates/deallocates the old object before control
  enters the success successor, and transfers the live allocation obligation to
  the result `q`.

CQPL represents only the first clause needed for temporal memory-error truth by
inserting a **checker-local** success-outcome state carrying a MAY
`drop(old-allocation)` event.  This state is not serialized by CREMA, is not a
fake foreign body, and is never added to `typed_edge_flow_v1` or source
provenance.

The result object is not assigned a fabricated `AbstractAllocId` in v1.
Leak refutation therefore uses the proof-carrying `result_deallocations` path
coverage rather than pretending that `q` is the old object.

## Closed wire vocabulary

A top-level record has:

- `family = "c_malloc"`
- `operation = "realloc"`
- `certainty = "may_abstract"`
- `size_semantics = "positive_nonzero_constant"`
- `status = "conditional_guarded"`
- `basis = "rust_foreign_decl_c_realloc_is_null_switch_v1"`

A result-deallocation record has:

- `family = "c_malloc"`
- `operation = "free"`
- `basis = "rust_foreign_decl_c_free_result_v1"`

## Dependencies

The capability requires:

- `allocation_state_v1`
- `allocation_existence_guards_v1`
- `reallocation_boundaries_v1`
- `mir_semantic_labels_v1`
- `typed_edge_flow_v1`

Capability and payload are atomic: either both are present or both are absent.
