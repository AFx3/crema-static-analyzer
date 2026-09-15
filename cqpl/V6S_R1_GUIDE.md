# v6S-r1 guide — allocation disposition and escape provenance

This document explains v6S-r1 in simple terms.  The normative machine boundary is `capabilities/allocation_disposition_v1.md`.

## 1. What changes compared with v6R?

v6R tells us **why a query is `unk`**.  For memory leaks, it showed that 105/112 subjects are inconclusive and all 105 depend on a MAY allocation fact.

v6S-r1 asks a different question:

> after an allocation is created, what lifecycle/ownership operation do we observe for that same abstract allocation?

Examples are `Box::into_raw`, `Box::from_raw`, a normal deallocation, an allocation returned to the caller, or `mem::drop` applied only to a raw pointer value.

The answer is stored in each graph node under `allocation_disposition`.

**The old leak query still returns exactly what it returned in v6R.**  v6S-r1 collects evidence; v6S-r2 will decide how to use it in a new leak property.

## 2. The three facts to keep separate

For leak reasoning, do not conflate these three statements:

1. **an allocation exists**;
2. **some pointer to it is dropped**;
3. **the allocation's cleanup obligation is discharged**.

They are not equivalent.

A raw pointer is just a pointer value.  The Rust Reference states that dropping a raw pointer does not affect the lifecycle of the pointee.  Therefore:

```rust
let b = Box::new(42);
let p = Box::into_raw(b);
std::mem::drop(p);
```

must **not** be modeled as freeing the Box allocation.

## 3. Example A: `boxed_bool__ml`

Target:

```text
tests_and_target_repos/
  a-code_full_rust/
    a-memory_leaks_full_rust_literals/
      boxed_bool/
```

Its essential code is:

```rust
let b1: Box<bool> = Box::new(false);
let raw: *mut bool = Box::into_raw(b1);
```

There is no `Box::from_raw(raw)` and no deallocation in the frozen source audit.  That audit also records one `forget` occurrence.  v6S-r1 does **not** guess its ownership meaning from the source counter: `mem_forget_owned_box` is emitted only if rustc proves that the forgotten argument itself has the exact `Box` ADT.  A `forget` applied to a raw pointer therefore does not become a Box-obligation event.

### What v6R sees

The frozen query `leak_alloc` is `unk`.  The explanation frontier is essentially:

```text
MAY_ALLOCATION
QUERY_THREE_VALUED_PROPAGATION
```

That is sound but imprecise: the positive allocation event is MAY, so the query cannot become `tt`.

### What v6S-r1 adds

On the `Box::into_raw` call, CREMA should emit a record similar to:

```json
{
  "kind": "box_into_raw",
  "certainty": "may_abstract",
  "obligation_effect": "preserve_manual_obligation"
}
```

Read it as:

```text
Box owns allocation A
        |
        | Box::into_raw
        v
raw pointer denotes A
cleanup responsibility still exists
```

It does **not** mean that A is definitely leaked.  It means that normal `Box` RAII cleanup was consumed and manual cleanup remains relevant.

Expected v6S-r1 result:

```text
old leak_alloc result:      unk   (unchanged)
new disposition evidence:   box_into_raw(A)
```

## 4. Example B: `clean_into_from_raw`

The clean target performs:

```rust
let b1 = Box::new(99);
let raw = Box::into_raw(b1);
unsafe { let _ = Box::from_raw(raw); }
```

v6S-r1 should observe:

```text
box_into_raw(A)
box_from_raw(A)
[then, if identity/drop evidence resolves]
may_deallocate(A)
```

The key difference from the leak example is that `Box::from_raw` reconstructs a `Box`.  Official Rust documentation states that the resulting Box owns the pointer and its destructor drops `T` and frees the Box allocation.

Again, r1 does not yet make a new logical claim.  It exposes the facts needed to distinguish this target from `boxed_bool__ml` in a later query.

## 5. Example C: `drop_raw_ptr_no_free`

Target:

```text
tests_and_target_repos/a-code_full_rust/drop_raw_ptr_no_free
```

Essential code:

```rust
let boxed = Box::new(42_i32);
let raw = Box::into_raw(boxed);
std::mem::drop(raw);
unsafe { println!("{}", *raw); }
```

Correct v6S behavior is:

```text
box_into_raw(A)
raw_pointer_drop_noop(A)
NO drop_l(A) from mem::drop(raw)
NO may_deallocate(A) from mem::drop(raw)
NO FREED transition for A from mem::drop(raw)
```

Why?  The raw pointer value is `Copy`; dropping it does not run the pointee destructor or release the allocation.  The later dereference is therefore not a use-after-free *because of that `mem::drop` call*.

This is different from:

```rust
unsafe { std::ptr::drop_in_place(raw); }
```

which runs the pointee destructor, but is still not by itself equivalent to deallocating the backing storage.

## 6. Meaning of every record field

- `allocation`: opaque abstract allocation identity used to correlate events across nodes;
- `kind`: which certified lifecycle event was observed;
- `certainty`: always `may_abstract` in v6S-r1;
- `obligation_effect`: conservative interpretation of the event for future leak reasoning;
- `basis`: stable proof-basis identifier checked by the consumer;
- `source_variable`: program variable from which the allocation identity was resolved, when available;
- `target_variable`: return/destination variable, when meaningful;
- `callee_def_path`: audit-only rustc DefPath.  It does not authorize the semantics by string matching.

## 7. What v6S-r1 deliberately does not do

It does not:

- introduce `must_alloc`;
- make `MAY_ALLOCATION` true;
- change the old leak query;
- classify an unknown external call as an ownership transfer;
- claim that `return_escape` proves safe cleanup;
- equate `mem::drop(raw)` with freeing the pointee.

## 8. Acceptance criterion

v6S-r1 is accepted only if:

```text
old v6R matrix: 112 x 12 = 1344
new v6S-r1 matrix: 112 x 12 = 1344
truth mismatches: 0
```

and the raw-pointer fixture satisfies the no-deallocation invariants above.

After that, `analyze_allocation_disposition.py` partitions the 105 leak-unknown subjects by observed disposition signature.  Those measured data, not intuition, determine v6S-r2.
