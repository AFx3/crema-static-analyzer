# CQPL queries_v3 — stable user-facing memory-safety suite

`queries_v3` is the user-facing detection suite for the current CREMA/CQPL
schema-v2 producer. It intentionally separates user-visible bug classes from
internal capability-version history.

The suite contains exactly four primary memory-safety properties:

- `use_after_free_full.cqpl`
- `double_free_full.cqpl`
- `allocator_mismatch_full.cqpl`
- `leak_normal_completion.cqpl`

## Capability policy

A `requires` clause is an applicability contract, not an extra model-checking
pass and not a request to union results from older versions.

UAF, double-free and allocator-mismatch explicitly require
`allocation_contracts_v2`, `allocation_contracts_v3` and
`allocation_contracts_v4`. The redundancy is deliberate and audit-friendly:
current CREMA advertises all three on schema-v2 artifacts, while v3 and v4 add
different producer-certified deallocation vocabularies.

The leak query is deliberately unchanged from the validated v2 normal-completion
formulation:

```
requires allocation_state_v1;
requires typed_edge_flow_v1;
assessment_scope normal_execution;
```

Adding allocator-contract capabilities to that query would not strengthen its
formula and would only reduce applicability.

## Bodyless FFI

Bodyless-FFI capabilities such as `external_formal_memory_effects_v2`,
`external_deallocation_effects_v1`, `llvm_memory_effects_v1`, and
`ffi_argument_identity_v1` are producer evidence. They are intentionally not
required by every v3 query: when present, their admitted read/write/drop/family
evidence is already reflected in the predicates consumed by these formulas.
Requiring them globally would reject valid pure-Rust targets and would reduce,
not increase, detection coverage.

## Compatibility invariant

For a single artifact that satisfies both suites, the v3 UAF/DF/mismatch
formula body is identical to its v2 counterpart, and the leak query is identical
in both requirements and formula. Therefore a v2/v3 truth-value difference on
the same artifact is a consistency failure, not an intended semantic change.
