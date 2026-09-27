# `reallocation_boundaries_v2`

`reallocation_boundaries_v2` is an additive extension of
`reallocation_boundaries_v1`.  An artifact declaring v2 MUST also declare
`reallocation_boundaries_v1`, `allocation_contracts_v1`, and `mir_semantics_v2`.

The v2 extension permits the proof basis
`rust_foreign_decl_c_realloc_allocptr_family_v2` in the existing
`reallocation_boundaries` payload.  Such a record certifies only the following
allocator-consumer fact:

- the record is anchored at a producer MIR call node representing a bodyless foreign `realloc` boundary;
- `source_variable` is bound at that node to `source_allocation`;
- the `realloc` allocptr consumer requires allocator family `c_malloc`;
- the source allocation keeps its own producer-supplied allocator contract.

The record does **not** assert that the source is a valid `realloc` origin, does
not assert that reallocation succeeds, does not synthesize a deallocation, and
does not transfer or invalidate lifecycle state.  If the source allocator
family is known and differs from `c_malloc`, the checker may expose
`allocator_mismatch_l(a)` as MAY/UNKNOWN evidence at that node.  If the source
family is unresolved, the predicate remains MAY/UNKNOWN but diagnostics must
remain unoriented.

The legacy basis `rust_foreign_decl_c_realloc_boundary_v1` retains its original
meaning: the source is a producer-certified `c_malloc` allocation created by
`malloc` or `calloc`, and is eligible for the separate conditional-reallocation
(CR1) proof surface.  CR1 MUST ignore v2 family-consumer records.

This separation is intentional: allocator-family compatibility is a precondition
on the `realloc` consumer; it is not a deallocation event.
