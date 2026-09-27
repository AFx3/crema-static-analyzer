# external_formal_memory_effects_v1

`external_formal_memory_effects_v1` (EFM1) is a schema-v2 producer capability for declaration-only/bodyless external memory operations. It requires `mir_semantics_v2` (and therefore `mir_semantic_labels_v1`) because each proof record is anchored to the real MIR `term:call` node.

It does **not** equate Rust MIR variables/functions with SVF variables/functions.  The capability composes two independent facts:

1. a closed, versioned semantic contract assigns `read` or `write` to a formal parameter position of a recognized external API;
2. CREMA's Rust-side MIR/allocation-identity analysis resolves the actual argument occupying that formal position.

The resulting event is an ordinary existing CQPL `read(v)` or `write(v)` label on the real Rust MIR call node.  Allocation-centric `read(A)`/`write(A)` labels are obtained only by the existing `AllocationIdentityState`; EFM1 creates no aliases or allocation identities.

## Closed v1 contract

| semantic class | callee | formal effect | extent formal | CREMA basis | frozen semantic sources |
|---|---|---:|---:|---|---|
| `strlen_read_c_string_v1` | `strlen` | `0: read` | — | `crema_efm1_closed_contract_v1` | SVF AbsExtAPI strlen semantics; LLVM16 TLI argmem-read semantics |
| `memset_v1` | `memset` | `0: write` | 2 | `crema_efm1_closed_contract_v1` | SVF MEMSET semantics; LLVM16 TLI arg0-writeonly semantics |
| `memcpy_v1` | `memcpy` | `0: write` | 2 | `crema_efm1_closed_contract_v1` | SVF MEMCPY semantics; LLVM16 TLI arg0-writeonly semantics |
| `memcpy_v1` | `memcpy` | `1: read` | 2 | `crema_efm1_closed_contract_v1` | SVF MEMCPY semantics; LLVM16 TLI arg1-readonly semantics |
| `memcmp_v1` | `memcmp` | `0: read` | 2 | `crema_efm1_closed_contract_v1` | LLVM16 TLI argmem-read; MemoryLocation memcmp arg semantics |
| `memcmp_v1` | `memcmp` | `1: read` | 2 | `crema_efm1_closed_contract_v1` | LLVM16 TLI argmem-read; MemoryLocation memcmp arg semantics |
| `posix_write_v1` | `write` | `1: read` | 2 | `crema_efm1_closed_contract_v1` | POSIX write buffer semantics; LLVM16 TLI arg1-readonly semantics |

The contract is intentionally fail-closed. EFM1 v1 admits only exact selected-crate foreign declaration names from CREMA's HIR extraction and exact arity. A C function whose body is represented in the loaded LLVM/SVF ICFG is excluded from EFM1 so observed represented-body events are never duplicated by a bodyless summary.

`size_argument_index` identifies the byte-count formal for bounded-memory APIs. EFM1 suppresses the event only when the corresponding MIR actual is the exact constant `const 0_usize`; otherwise the event remains MAY. No symbolic range reasoning is claimed.

## Official semantic sources frozen by v1

The semantic roles above mirror documented behavior of the analysis/library interfaces rather than SVF program variables:

- SVF `ExtAPI` annotations and `is_memcpy` / `is_memset`: https://svf-tools.github.io/SVF-doxygen/html/classSVF_1_1ExtAPI.html
- SVF `AbsExtAPI` handlers including `strlen`, `memcpy`, and `memset`: https://svf-tools.github.io/SVF-doxygen/html/classSVF_1_1AbsExtAPI.html
- LLVM `TargetLibraryInfo`, whose `getLibFunc` recognition includes library-function type/prototype validation and target availability: https://llvm.org/doxygen/classllvm_1_1TargetLibraryInfo.html
- LLVM `BuildLibCalls` / inferred libcall attributes: https://llvm.org/doxygen/BuildLibCalls_8h.html
- LLVM `MemoryLocation::getForArgument`, including per-argument memory locations for recognized libcalls such as `memcmp`: https://llvm.org/doxygen/classllvm_1_1MemoryLocation.html
- POSIX `write()`, specified as writing bytes from the buffer supplied by the caller: https://pubs.opengroup.org/onlinepubs/9690949599/functions/write.html

`basis = crema_efm1_closed_contract_v1` means that CREMA applied this frozen contract. `semantic_sources` records which official provider semantics motivated the tuple; those tokens are provenance references, **not** claims that SVF/TLI executed on the bodyless Rust call. No synthetic SVF variable is matched to a Rust local.

## Consumer obligations

A conforming checker must reject an EFM1 payload unless all of the following hold:

- capability and non-empty payload appear atomically, with `mir_semantics_v2` declared;
- the record references a real `rust::...` node carrying `term:call` and a declared Rust actual variable;
- `actual_variable` is a Rust variable whose canonical serialized ID belongs to the same Rust function scope as the node;
- `event_variable` is the same MIR local as the function-scoped `actual_variable`;
- the referenced node already contains the corresponding ordinary `read`/`write` event;
- `(callee, semantic_class, formal_index, access, extent, basis, semantic_sources)` is one of the closed v1 tuples above;
- duplicate records for the same node/callee/formal/access are rejected.

EFM1 changes no CQPL syntax and no three-valued truth rule. It only supplies producer events that were previously absent at declaration-only external boundaries.
