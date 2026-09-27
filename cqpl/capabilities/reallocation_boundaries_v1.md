# `reallocation_boundaries_v1`

## Purpose

`reallocation_boundaries_v1` is a diagnostic-only, proof-carrying boundary
capability for a bodyless external C `realloc` call whose branch-sensitive
allocation semantics are not yet represented in the abstract allocation
state.

It exists to make diagnostic reasoning fail closed.  A consumer MUST NOT treat
this capability as an allocation, deallocation, use, or truth-semantic event.
In particular it does not change CQPL `tt` / `ff` / `unk` results.

## Atomic payload

The capability and top-level `reallocation_boundaries` payload are atomic:
either both are present or both are absent.  The capability requires schema v2
and `allocation_state_v1`.

Each record has the closed proof tuple:

- `node`: Rust MIR call node for the unresolved bodyless `realloc` boundary.
- `source_allocation`: existing abstract allocation that may be passed as the
  first `realloc` argument.
- `source_variable`: canonical Rust program variable for the old pointer.
- `result_variable`: canonical Rust program variable receiving the nullable
  `realloc` result.
- `family = "c_malloc"`.
- `operation = "realloc"`.
- `certainty = "may_abstract"`.
- `status = "conditional_unmodeled"`.
- `basis = "rust_foreign_decl_c_realloc_boundary_v1"`.

The source allocation must already have a producer allocator contract in the
`c_malloc` family with operation `malloc` or `calloc`, and `source_variable`
must point to that allocation at `node` in the existing identity overlay.
Both variables and the allocation must be declared in the top-level catalogs.

## Producer rule

CREMA emits a record only for an external, bodyless Rust MIR call classified as
C `realloc` by foreign-declaration/libc evidence.  A represented external LLVM
body suppresses this boundary record.  The record is attached independently to
each source allocation in the existing MAY points-to set of the old-pointer
argument.

No fresh allocation identity is created by this capability.

## Consumer rule

CQPL may use a retained record only to block a diagnostic proof that would
otherwise conclude a negative leak assessment by crossing the unresolved
conditional reallocation.  The safe fallback is therefore `unk_unoriented`.

The record MUST NOT:

- satisfy `alloc_l`, `drop_l`, `read_l`, `write_l`, or `use_l`;
- mutate `allocation_post`;
- enter `CqplTruthModel`;
- prove that the old allocation survives or is freed;
- prove that the result is fresh, aliases the old allocation, or is non-null;
- orient double-free or use-after-free positively.

Those properties require a later reallocation-specific semantic refinement.
