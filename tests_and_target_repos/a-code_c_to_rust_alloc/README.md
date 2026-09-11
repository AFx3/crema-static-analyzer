# Phase 5 C-origin allocation micro-corpus

This corpus is intentionally separate from the frozen 92-target Phase-4.4
regression suite.

## Question under test

Can CREMA preserve a positive `malloc`/`calloc` allocation-family provenance
through SVF/LLVM, bridge a pointer returned by C into the Rust MIR local, and
then reason consistently about matching `free`, leaks, double free,
use-after-free, and Rust ownership APIs?

## Allocator-family model

Positive family represented in this phase:

- C `malloc` / `calloc` family.

Matching deallocator:

- C `free`, whether executed inside an inlined C wrapper or invoked from a Rust
  MIR call site through an actual foreign declaration.

The source language of the call site is not the allocator family.

## Rust APIs

Positive C-malloc provenance reaching `CString::from_raw`,
`Box::from_raw`, `Vec::from_raw_parts`, `String::from_raw_parts`, or
`std::alloc::dealloc` is reported as a potential allocator/ownership-contract
violation. For the general allocator APIs the analysis does not claim that
plain C provenance proves physical allocator incompatibility on every concrete
platform/configuration; it claims the Rust API safety preconditions are not
established by that provenance.

`CStr::from_ptr` is borrow-only and is a negative ownership-transfer control.

## Important LLVM/SVF controls

The 16-target suite includes:
- matching free inside inlined C;
- free-inside-C followed by Rust use;
- two inlined instances of the same C free wrapper;
- direct foreign `free` called from Rust;
- direct double free and UAF;
- malloc/calloc leak;
- dedicated `String::from_raw_parts` ownership-contract target;
- non-malloc static-storage pointer.

## Deliberately deferred

- C `realloc` success/failure disjunction;
- `aligned_alloc`, `strdup`, custom allocator families;
- C++ `new/delete`;
- proof that a foreign allocator is identical to Rust `Global`;
- general multi-parameter MIR-argument -> SVF-formal mapping.
