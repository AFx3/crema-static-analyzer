# Phase B-minimal — bodyless malloc-family fresh identities, existing TOP semantics

This checkpoint implements the selected strategy:

```text
new AbstractAllocId + existing CellValue::TOP
```

No new lattice element, no presence/lifecycle product, no CQPL truth change.

## Semantics

For a bodyless exact foreign `malloc`/`calloc`/`strdup` call, or for an exact bodyless `realloc` call whose first actual is producer-certified MUST-null, at Rust MIR callsite `c`:

```text
AbstractAllocId.site = CCall {
    node_id   = c,
    allocator = malloc | calloc | strdup | realloc(NULL, ·)
}

identity(return_local) includes AbstractAllocId
allocator family = c_malloc
legacy state(return_local) = TOP
alloc_l(AbstractAllocId) certainty = may_abstract
```

`TOP` intentionally preserves the frozen nullable abstraction:

```text
NULL or live allocation -> TOP
```

Thus CQPL allocation-state truth remains conservative (`unk`) while allocation
identity enables correlation with later drop/read/write/mismatch events.

## Certification boundary

Bare `malloc`, `calloc`, `strdup`, and the RN1 `realloc` specialization are recognized only if they occur in the
producer-generated foreign declaration set (`ffi_functions.json`).

Direct `libc::malloc`/`libc::calloc`/`libc::strdup`/`libc::realloc` canonical paths remain supported; the `realloc` case still requires the independent MUST-null proof.

The following do not materialize:

```text
my_malloc
malloc_wrapper
arbitrary pointer-returning extern
getenv
```

A represented external LLVM body suppresses MIR-side materialization so the
existing LLVM/SVF `CCall` identity remains authoritative.

## No schema change

The existing structures are reused:

```text
AllocationSiteId::CCall
allocation_contracts_v1/v2/v3
allocation_state_v1
schema-v2 may_abstract labels
```

The exporter is not weakened. The prior fail-closed condition still rejects a
modeled allocation event if identity is absent.

## Expected focused behavior

B01 bodyless malloc leak:

```text
producer export = pass
one CCall allocation at rust::main::bb*
state = TOP
alloc_l certainty = may_abstract
family = c_malloc
truth = unk
assessment = unk_true
```

B08b bodyless malloc -> Rust dealloc mismatch:

```text
producer export = pass
same CCall allocation reaches Rust deallocator
family = c_malloc
mismatch truth = unk
assessment = unk_true
```

B16: two static malloc callsites -> two distinct AbstractAllocIds.

B17: one static malloc site revisited in loop -> no positive double-free
orientation solely from static-site reuse.

B18: `getenv` pointer return -> no CCall allocation.

B19: bodyless calloc -> one CCall/calloc allocation, TOP, c_malloc family.


## `strdup` extension

`strdup` is modeled as a fresh nullable allocation, never as an alias-return.
The returned allocation contract is `family=c_malloc`, `operation=strdup`,
`language=c`; its initial abstract state remains TOP for the same success/null
reason as `malloc`.  This extension does not imply that arbitrary pointer-returning
foreign functions allocate.


## RN1 — `realloc(NULL, n)`

RN1 models the POSIX/C special case in which formal 0 is definitely null.  The
producer uses a separate intraprocedural MUST-null dataflow (intersection at
joins), seeded only by the canonical rustc DefPaths `core::ptr::null_mut` and
`std::ptr::null_mut`.  Direct local copy/move and supported pointer casts may
propagate the fact; unsupported assignments/calls kill it.  Empty points-to is
never interpreted as null.

When that proof reaches an exact bodyless foreign `realloc` call, the result is
a fresh `CCall{allocator=realloc}` identity with allocator contract
`family=c_malloc, operation=realloc, language=c` and lifecycle `TOP`.  No old
allocation exists, so RN1 emits no `reallocation_boundaries_v1/v2`, no CR1
record, and no synthetic deallocation.

The rule applies also to size zero.  POSIX specifies the null-source case as
equivalent to `malloc(size)`; for size zero the implementation may return null
or a distinct allocation-like pointer.  RN1 therefore models MAY resource
existence/ownership, but does not infer positive dereference validity or any
read/write event from the zero-size result.

A source that is merely MAY-null, unknown, integer-cast, or obtained through an
unsupported flow does not enter RN1.
