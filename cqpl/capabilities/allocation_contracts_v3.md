# allocation_contracts_v3

`allocation_contracts_v3` is an additive refinement of the frozen
`allocation_contracts_v2` producer-evidence vocabulary.  An artifact declaring
v3 MUST also declare `allocation_contracts_v1` and `allocation_contracts_v2`.

The purpose of v3 is deliberately narrow: certify the allocator/deallocator
family for an explicit `core::mem::drop(Box<T, Global>)` call when rustc still
provides the semantic `DefId` and argument type.

## New closed proof basis

`rust_mem_drop_owned_box_global_v1`
: The producer proves all of the following before emitting the basis:

  1. the call target is a non-local item in crate `core`, parent module `mem`,
     named `drop` (the canonical `core::mem::drop` item on the pinned toolchain);
  2. the moved first argument normalizes to an ADT whose definition is rustc's
     `owned_box` lang item;
  3. the Box allocator type argument is an ADT whose definition is rustc's
     `global_alloc_ty` lang item.

  The resulting deallocator contract is:

  `rust_global / drop / rust`

  and carries all three audit fields:
  `callee_def_path`, `owner_def_path`, and `allocator_def_path`.

The checker validates the basis/family/operation/language tuple and requires all
three provenance fields, but it never reconstructs semantic identity from their
strings.

## Fail-closed exclusions

The new basis MUST NOT be emitted for:

- `mem::drop` of a raw pointer;
- `mem::drop` of `Vec`, `CString`, `String`, `Rc`, `Arc`, or arbitrary user
  types;
- `Box<T, A>` when `A` is not rustc's canonical `Global` allocator;
- local functions named `drop`;
- pretty-printed/canonical-looking call text without producer evidence;
- unresolved or generic allocator types.

All such cases retain the pre-existing v1/v2 behavior.  In particular, the
historical `allocation_contracts_v2` vocabulary is not redefined.

## Semantic basis

Rust documents `mem::drop<T>` as moving its argument into `drop`, after which
that value is automatically dropped before the function returns.  Rust's Box
memory-layout contract states that a non-zero-sized `Box` using its default
allocator uses `Global`, and `Box::from_raw` restores ownership so that the Box
destructor performs cleanup.

Normative references:

- https://doc.rust-lang.org/std/mem/fn.drop.html
- https://doc.rust-lang.org/std/boxed/
- https://doc.rust-lang.org/std/boxed/struct.Box.html#method.from_raw

## CQPL semantics

v3 adds producer precision only.  It does not introduce MUST allocation or MUST
deallocation facts.  Existing allocation-event labels remain `may_abstract`.
Therefore the only intended truth strengthening is removal of an
`UNRESOLVED_CONTRACT` frontier when a `mem::drop(Box<T, Global>)` event was
already present and the allocator family can now be proved.
