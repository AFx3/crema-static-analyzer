# External deallocation call provenance V1 (D4-P0)

This capability is a proof sidecar for an already classified Rust MIR call. It creates no drop, allocation, deallocation, points-to, lifecycle, allocation-label, or allocation-disposition event or state. Existing free-call interpretation remains authoritative.

The closed callee set is `{free}`. Admission requires a real Rust MIR `Call` terminator whose structured callee identity is the exact selected-crate external FFI declaration `free`, with arity 1, formal index 0, and no represented C body. Pretty call text, substring matching, drop labels, allocation labels, and C-frontier `external_deallocation_effect` records are not positive evidence.

Each record has `callee=free`, `arity=1`, `formal_index=0`, `family=c_malloc`, `operation=free`, `language=c`, `body_status=bodyless`, `certainty=may_effect`, and `basis=rust_mir_exact_external_free_call_v1`. Its node is the exact Rust call node. `actual_variable` is the canonical same-function Rust actual when that ID is already present in the serialized variable catalog. It is null only when the structured MIR operand has no corresponding canonical scoped catalog entry. The producer never adds or rewrites variables to fill this field. The gate audits the MIR operand and catalog for every null. No allocation identity is required. `free(NULL)` can therefore carry a record without an allocation-specific drop.

The baseline has 44 eligible calls: 28 with allocation-specific drop labels, 8 without such labels and with CR1 corroboration, and 8 without such labels or CR1 corroboration. All 44 receive one record. CR1 result-deallocation evidence proves a separate relation between a realloc result and a later free call; it is corroboration, not another direct-free semantic event.

D4 consumes this call-local proof as its deallocation-family source after independent review. D4-P0 itself changes no query truth or assessment.
