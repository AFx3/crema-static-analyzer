# `allocation_disposition_v1`

`allocation_disposition_v1` is the v6S-r1 producer capability for **allocation-lifecycle provenance**.

It does **not** change the CQPL truth lattice, the 12 frozen queries, or the meaning of `alloc_l` / `drop_l`.  Every v6S-r1 disposition record is a **MAY abstract observation**.  The purpose of r1 is to measure which ownership/lifecycle facts are available before designing a new leak query in v6S-r2.

## Why this capability exists

The v6R explainability run showed that both leak queries are `unk` on 105/112 subjects and that all 105 unknowns contain `MAY_ALLOCATION` in their dependency frontier.  That fact alone is not enough to safely promote leak results to `tt`: a live allocation may have been returned, handed to a raw-pointer cleanup obligation, reconstructed into an owner, deliberately leaked, or deallocated later.

v6S-r1 therefore records *what happened to the allocation obligation* without yet using that information as a CQPL predicate.

## Record shape

Each schema-v2 node contains `allocation_disposition: []` when the capability is declared.  A record has:

```json
{
  "allocation": "<opaque AbstractAllocId>",
  "kind": "box_into_raw",
  "certainty": "may_abstract",
  "obligation_effect": "preserve_manual_obligation",
  "basis": "rustc_box_into_raw_v1",
  "source_variable": "rust::main::Local(_1)",
  "target_variable": "rust::main::Local(_2)",
  "callee_def_path": "alloc::boxed::Box::<T>::into_raw"
}
```

`allocation` is an opaque `AbstractAllocId`.  `source_variable` / `target_variable` are audit provenance only.  `callee_def_path` is also audit provenance: classification is completed in CREMA while the rustc `DefId` and argument type are still available; the consumer does not infer the event from this string.

## Closed v6S-r1 vocabulary

| `kind` | `obligation_effect` | meaning |
|---|---|---|
| `box_into_raw` | `preserve_manual_obligation` | `Box` ownership is consumed; cleanup responsibility remains with the caller/raw-pointer owner |
| `box_from_raw` | `restore_raii_obligation` | a raw pointer may be reconstructed into a `Box`; subsequent `Box` drop can perform cleanup |
| `box_leak` | `preserve_persistent_obligation` | `Box::leak` deliberately prevents normal Box cleanup |
| `mem_forget_owned_box` | `preserve_unreclaimed_obligation` | `mem::forget(Box)` consumes the Box without running its destructor |
| `raw_pointer_drop_noop` | `no_pointee_lifecycle_effect` | dropping the raw pointer value does not drop/deallocate the pointee |
| `return_escape` | `may_escape_to_caller` | the function return place may denote the allocation |
| `may_deallocate` | `may_discharge` | mirrors an existing allocation-centric `drop_l` MAY event |

All records have `certainty = "may_abstract"` in r1.  A singleton identity set is not promoted to MUST.

## Rust semantic basis

The model follows the official Rust documentation:

- raw pointers: <https://doc.rust-lang.org/reference/types/pointer.html> — copying or dropping a raw pointer has no effect on the lifecycle of another value;
- `std::mem::drop`: <https://doc.rust-lang.org/std/mem/fn.drop.html> — for `Copy` types the call effectively does nothing; the function itself is a normal generic function;
- `Box::into_raw`, `Box::from_raw`, `Box::leak`: <https://doc.rust-lang.org/std/boxed/struct.Box.html>;
- `std::mem::forget`: <https://doc.rust-lang.org/std/mem/fn.forget.html>.

A particularly important distinction is:

```text
std::mem::drop(raw: *mut T)
    -> drop the pointer value only
    -> NO pointee destructor
    -> NO pointee deallocation
```

This is different from `ptr::drop_in_place(raw)`, which may run the destructor of `T` but still does not, by itself, establish allocator deallocation.  v6S-r1 does not collapse these operations.

## Producer proof boundary

`Box::{into_raw,from_raw,leak}` are recognized only when rustc proves that the associated item belongs to an inherent impl whose self ADT is the `owned_box` lang item.  `mem::drop` is recognized as `raw_pointer_drop_noop` only for the exact `core::mem::drop` item with a first MIR argument of `TyKind::RawPtr`.  `mem::forget` is recorded only when its first argument is the exact Box ADT.

Unknown external calls are deliberately **not** assigned an ownership-transfer event in r1.  That will require a separate, explicit external-summary model.

## Soundness contract

The capability is observational in v6S-r1:

1. it cannot turn an old `ff`, `unk`, or `tt` into another truth value;
2. it exports only MAY observations;
3. lack of a disposition record is not proof that an event did not happen;
4. `raw_pointer_drop_noop` must never create `drop_l`, `may_deallocate`, or a `FREED` pointee state;
5. consumers validate the closed `(kind, obligation_effect, basis)` tuple and fail closed on malformed records.
