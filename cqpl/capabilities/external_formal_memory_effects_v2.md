# external_formal_memory_effects_v2

`external_formal_memory_effects_v2` (EFM2) is the strict, versioned successor to `external_formal_memory_effects_v1` (EFM1). EFM1 remains a frozen valid contract for legacy artifacts: its five-function tuple set, `size_argument_index` encoding, proof basis, and provenance vocabulary are unchanged. A single artifact must not declare both versions for the shared `external_formal_memory_effects` payload. New CREMA artifacts use EFM2.

EFM2 is a schema-v2 capability and requires `mir_semantics_v2`. Each record proves one `read` or `write` role for one formal of an exact selected-crate foreign declaration. CREMA binds that formal position to the exact Rust MIR actual and emits the corresponding ordinary event on the real Rust `term:call` node.

EFM2 does not create an allocation identity, a return alias, or an interior-pointer identity. Allocation-centric `read(A)` and `write(A)` facts arise only through the existing CREMA allocation-identity relation. Function-level `argmem` is not per-formal evidence and is never expanded across pointer arguments by EFM2.

## Closed EFM2 contract

Every record has `basis = crema_efm2_closed_contract_v1`.

| callee | arity | semantic class | formal effect | extent |
|---|---:|---|---|---|
| `strlen` | 1 | `strlen_read_c_string_v1` | `0: read` | `c_string_until_nul` |
| `memcmp` | 3 | `memcmp_v1` | `0: read` | `bytes_from_formal(2)` |
| `memcmp` | 3 | `memcmp_v1` | `1: read` | `bytes_from_formal(2)` |
| `memcpy` | 3 | `memcpy_v1` | `0: write` | `bytes_from_formal(2)` |
| `memcpy` | 3 | `memcpy_v1` | `1: read` | `bytes_from_formal(2)` |
| `memmove` | 3 | `memmove_v1` | `0: write` | `bytes_from_formal(2)` |
| `memmove` | 3 | `memmove_v1` | `1: read` | `bytes_from_formal(2)` |
| `memset` | 3 | `memset_v1` | `0: write` | `bytes_from_formal(2)` |
| `memchr` | 3 | `memchr_bounded_read_v1` | `0: read` | `bytes_from_formal(2)` |
| `strchr` | 2 | `strchr_read_c_string_v1` | `0: read` | `c_string_until_nul` |
| `write` | 3 | `posix_write_v1` | `1: read` | `bytes_from_formal(2)` |

No other callee, arity, formal, access, extent, semantic class, proof basis, or semantic-source tuple is part of EFM2.

## Extent semantics

`bytes_from_formal` requires `extent_argument_index`. If that exact MIR actual is `const 0_usize`, CREMA emits neither the node memory event nor an EFM2 record. A dynamic, unknown, or nonzero extent retains the MAY effect; EFM2 performs no symbolic range analysis.

`c_string_until_nul` forbids `extent_argument_index`. EFM2 does not compute a concrete string length.

## Admission and identity rules

Admission requires an exact external symbol, exact arity, and membership in the selected-crate FFI declaration inventory. Qualified names, substrings, wrappers, and generic LLVM function-level memory summaries do not establish an EFM2 tuple. If a C/LLVM body for the exact callee is represented, the bodyless summary is suppressed for all eight callees.

The formal index and Rust actual identity remain distinct. If CREMA cannot recover both the raw MIR event local and its same-function canonical Rust `ProgramVarId`, it omits the effect. EFM2 never equates an SVF variable with a Rust MIR local.

## Provenance

Existing EFM1 semantic-source tokens remain stable for the five inherited functions. EFM2 adds these closed tokens:

- `memmove`: `posix_memmove_n_byte_copy_semantics_v1`, `llvm16_memmove_formal_semantics_v1`, `llvm16_tli_memmove_recognition_v1`
- `memchr`: `posix_memchr_bounded_read_semantics_v1`, `llvm16_tli_memchr_recognition_v1`
- `strchr`: `posix_strchr_c_string_read_semantics_v1`

`semantic_sources` records the provider documentation from which CREMA's closed tuple was derived. It does not claim that TLI or SVF executed on the current bodyless call.

Normative references are POSIX `memmove` and `memchr`, ISO-C/POSIX `<string.h>` `strchr`, and LLVM 16 library semantics:

- https://pubs.opengroup.org/onlinepubs/9799919799/functions/memmove.html
- https://pubs.opengroup.org/onlinepubs/9799919799/functions/memchr.html
- https://releases.llvm.org/16.0.0/docs/LangRef.html
- https://llvm.org/doxygen/classllvm_1_1TargetLibraryInfo.html

EFM2 changes no CQPL syntax, CTL operator, truth value, connective, `exists_alloc`, or read/write query semantics. It enriches only the existing MAY event model.
