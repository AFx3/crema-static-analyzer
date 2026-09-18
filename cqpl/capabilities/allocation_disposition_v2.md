# `allocation_disposition_v2` — B1.1 CString refinement

`allocation_disposition_v2` is an **additive, fail-closed refinement** of
`allocation_disposition_v1`.  An artifact declaring v2 MUST also declare v1.
The seven v1 `(kind, obligation_effect, basis)` tuples remain unchanged.

B1.1 adds exactly two producer-certified MAY observations:

| `kind` | `obligation_effect` | `basis` |
|---|---|---|
| `cstring_into_raw` | `preserve_manual_obligation` | `rustc_cstring_into_raw_v1` |
| `cstring_from_raw` | `restore_raii_obligation` | `rustc_cstring_from_raw_v1` |

Every record remains `certainty = "may_abstract"`.  This refinement does not
promote a CQPL result to `tt`, does not introduce MUST ownership, and does not
reinterpret the frozen twelve queries.

The JSON wire spellings are **exactly** `cstring_into_raw` and
`cstring_from_raw`. The spellings `c_string_into_raw` and `c_string_from_raw`
are not aliases and must be rejected. Rust producer/consumer enums pin these
wire names with per-variant `#[serde(rename = ...)]` instead of deriving the
artifact protocol from Rust identifier case conversion. Serde documents this
mechanism at <https://serde.rs/variant-attrs.html>.

## Official Rust semantic basis

The model follows the Rust standard-library contract for `CString`:

- <https://doc.rust-lang.org/std/ffi/struct.CString.html>
- <https://doc.rust-lang.org/src/alloc/ffi/c_str.rs.html>

`CString::into_raw` consumes the `CString` and transfers ownership to the C
caller.  The returned pointer must be returned to Rust and reconstructed with
`CString::from_raw` for proper Rust-side reclamation; the standard C `free()`
function must not be used for that pointer.  Failure to reconstruct the value
with `from_raw` leaks the allocation.  `from_raw` therefore restores a Rust
RAII ownership obligation; it is not itself modeled as a deallocation event.

## Producer proof boundary

CREMA classifies these calls while rustc semantic identity is still available.
The checker does **not** infer the event from pretty-printed DefPath text.
`callee_def_path` is audit provenance only.  The runtime and typed checker both
validate the exact `(kind, obligation_effect, basis)` tuple and reject CString
records unless `allocation_disposition_v2` is declared.

## Explainability contract

A MAY leak/UAF/DF result may remain `unk`.  When a CString disposition is on the
relevant witness, the explanation must preserve its producer-certified basis
(e.g. `producer_certified_c_string_into_raw`) rather than replacing the MAY with
a stronger conclusion.

The diagnostic projection uses `allocation_disposition_witness_v1` and two
ordered roles:

- `ownership_handoff` for `cstring_into_raw`;
- `ownership_reclaim` for `cstring_from_raw`.

For a CString double-free candidate involving a C `free` followed by Rust RAII
drop, the explanation should therefore be able to expose the causal ownership
chain:

```text
cstring_into_raw
  -> C free
  -> cstring_from_raw
  -> CString Drop
```

when those producer records are reachable in that order for the same
`AbstractAllocId`. This projection is read-only: it augments supporting evidence
and must not change the query result.
