# external_deallocation_effects_v1

`external_deallocation_effects_v1` is the Bcontract-ND1 producer/consumer
boundary for external C-call deallocation effects.

The capability is additive and does not change the frozen CQPL query language
or upgrade MAY evidence to MUST evidence.  It makes negative external-effect
knowledge auditable and fail-closed.

## Record vocabulary

Each record is keyed by the external `dummyCall` node and contains:

- `callee`: the C function selected by the existing Rust->SVF bridge;
- `status`: one of `certified_absent`, `observed_may_deallocate`, `unresolved`;
- `basis`: a closed producer-evidence basis.

Closed tuples:

- `certified_absent` / `svf_leaf_no_call_deallocation_v1`
- `observed_may_deallocate` / `structural_c_free_v1`
- `unresolved` / `svf_call_effect_unresolved_v1`
- `unresolved` / `svf_body_unavailable_v1`

## Soundness boundary

`certified_absent` is deliberately narrow.  CREMA emits it only when the
materialized SVF body exists and contains no `FunCallBlock` at all.  This proves
absence only for the currently modeled call-based deallocation vocabulary.  A
wrapper containing any direct, indirect, opaque, intrinsic, or otherwise
unclassified call remains `unresolved` in v1.

A direct structural `@free` call produces `observed_may_deallocate`; it does not
assert that every execution deallocates, and it does not create MUST truth.

Absence of a record is never interpreted as proof of no deallocation.

## Non-goals

v1 does not perform transitive call-graph closure and does not classify arbitrary
custom allocators or inline assembly.  These remain fail-closed.  The capability
is intended as a trustworthy negative-effect boundary that later CQPL
refinements may consume without reconstructing intent from pretty-printed code.
