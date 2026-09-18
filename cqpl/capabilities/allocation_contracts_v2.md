# `allocation_contracts_v2`

`allocation_contracts_v2` is a fail-closed refinement of
`allocation_contracts_v1`. An artifact declaring v2 MUST also declare v1.

## Scope

v1 remains the frozen source of `allocator_contract` origin summaries. v2 adds
producer-certified **deallocator** contracts to every schema-v2 `drop` allocation
event. The CQPL checker compares allocator/deallocator `family`; it never infers
a family from diagnostic strings.

The v2 contract fields are:

- `family`: `rust_global | c_malloc | unknown`;
- `operation`: the modeled release operation;
- `language`: `rust | c | unknown`;
- `basis`: one closed proof-basis value;
- `owner_def_path`, `allocator_def_path`: audit-only rustc provenance required
  only for structurally typed `Box<_, Global>` / `Vec<_, Global>` drops;
- `callee_def_path`: audit-only canonical provenance required for a producer-proven
  `std::alloc::dealloc`/`alloc::alloc::dealloc` call.

## Normative family mapping

### `rust_global`

Rust's standard-library allocation documentation states that a program has one
standard-library "global" allocator and that it is used, for example, by
`Box<T>` and `Vec<T>`. `std::alloc::dealloc` deallocates with that global
allocator. The concrete implementation behind the global allocator may be
replaced via `#[global_allocator]`; therefore `rust_global` denotes the Rust
**global allocator API family**, not a particular operating-system allocator.

Official references:

- https://doc.rust-lang.org/std/alloc/index.html
- https://doc.rust-lang.org/std/alloc/fn.dealloc.html
- https://doc.rust-lang.org/std/boxed/index.html#memory-layout
- https://doc.rust-lang.org/std/vec/index.html#memory-layout
- https://doc.rust-lang.org/std/alloc/struct.Global.html

For compiler-side identity, CREMA uses rustc semantic identifiers rather than
hard-coded pretty paths. rustc's development guide explicitly recommends
`rustc_diagnostic_item`/`TyCtxt::is_diagnostic_item` to avoid path-based
misclassification. `Box` and `Global` are identified by language items; `Vec`
is identified by its diagnostic item.

Official rustc references:

- https://rustc-dev-guide.rust-lang.org/diagnostics/diagnostic-items.html
- https://doc.rust-lang.org/nightly/nightly-rustc/rustc_hir/lang_items/struct.LanguageItems.html
- https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/symbol/sym/constant.Vec.html

### `c_malloc`

LLVM's Language Reference defines function attribute
`"alloc-family"="malloc"` as the common allocation family for
`malloc/calloc/realloc/free`, and `allockind("free")` as releasing the block
passed through `allocptr`. CREMA serializes that LLVM family name as
`c_malloc` so that the abstract family is not confused with the concrete
`malloc` operation.

Official reference:

- https://llvm.org/docs/LangRef.html#alloc-family
- https://llvm.org/docs/LangRef.html#allockind

**Current evidence boundary:** the historical allocation-contract basis remains
separate from EFX1. `structural_c_free_v1` continues to identify the frozen
producer proof used for the allocation event itself. Separately,
`llvm_memory_effects_v1` now carries explicit LLVM16 and isolated-TLI evidence,
including `alloc-family`, `allockind` and `allocptr` where available.

When both proofs support the same external MAY-deallocation effect, R2-R1.2
preserves the historical primary basis and exposes the LLVM proof as
`external_deallocation_effects_v1.corroborating_bases`. The checker does not
silently rewrite one provenance class into the other.

## Closed v2 proof bases

`rust_box_global_drop`
: The MIR drop place is structurally a rustc `owned_box` ADT and its allocator
  type is the rustc `global_alloc_ty` lang item. Contract:
  `rust_global / drop / rust`.

`rust_vec_global_drop`
: The MIR drop place is structurally the `Vec` diagnostic item and its allocator
  type is the rustc `global_alloc_ty` lang item. Contract:
  `rust_global / drop / rust`.

`rust_cstring_global_drop`
: The MIR drop place is structurally the rustc `cstring_type` diagnostic item
  (`CString`) and the producer records the global allocator provenance. Contract:
  `rust_global / drop / rust`. This basis is producer-certified; the checker does
  not reconstruct it from pretty-printed paths. The ownership/deallocation
  protocol follows the official `CString` API documentation, including the
  requirement that pointers transferred by `CString::into_raw` are reclaimed by
  `CString::from_raw` rather than C `free`: <https://doc.rust-lang.org/std/ffi/struct.CString.html>.

`rust_global_dealloc_api`
: The rustc-side producer, while holding the call target `DefId`, proves an
  external item in crate `alloc`, parent module `alloc`, named `dealloc`.  The
  canonical DefPath is serialized only for audit.  The exporter/checker do not
  reconstruct the proof from that string. Contract: `rust_global / dealloc / rust`.

`structural_c_free_v1`
: CREMA has structurally recognized the modeled C `free` operation using the
  pre-existing v1 producer model. Contract: `c_malloc / free / c`. This basis is
  intentionally retained for historical comparability. If independent
  LLVM16/TLI evidence proves the same MAY effect, it is reported separately as
  external-effect corroboration rather than replacing this basis.

`unresolved`
: The producer has no supported structural proof. `family` MUST be `unknown`.

No other basis is valid in the v6Q-r1c frozen contract.

## Deliberate exclusions

v6Q-r1c does **not** promote generic MIR Drop, `String`, `CString`, `Rc`, `Arc`,
custom `Drop`, custom allocator parameters, or pretty-text-only allocator calls.
They remain `unknown` unless a separately specified structural proof is added in
a future version.

In particular, "this is Rust code" or absence of authored C files is never
sufficient evidence for `rust_global`.

## CQPL semantics

`allocator_mismatch_l(a)` remains family-level and MAY-only. v2 changes producer
precision, not the three-valued temporal semantics:

- known equal family: no mismatch witness at that release event;
- known different family: positive MAY mismatch witness (`unk`);
- any `unknown` family: possible mismatch witness (`unk`);
- absence of a witness can be refuted (`ff`).

A v2 query must declare:

```cqpl
requires allocation_contracts_v2;
```

Missing capability or malformed proof metadata is a hard error, never `ff`.

## Explainability provenance boundary

`allocation_contracts_v2` certifies **deallocator** evidence only. Allocator
origins remain the frozen `allocation_contracts_v1` summaries.

For that reason, an allocator-origin witness with no `basis` is not interpreted
as a failed v2 proof and is not labeled `unresolved`. Explainability records it
explicitly as:

```text
legacy_v1_allocator_summary
```

and renders the proof basis as `<not-applicable-v1>`. Producer-certified v2
deallocator witnesses are labeled `producer_certified_v2_deallocator`; an
explicit `basis = "unresolved"` is labeled
`explicitly_unresolved_v2_deallocator`.

These labels are diagnostic provenance only. They do not alter allocator-family
comparison or CQPL three-valued truth.
