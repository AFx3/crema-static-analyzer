# DEP1-P1-COV1: Reachable Call Semantic Coverage Provenance

Status: producer provenance bridge. This does not claim full DEP1-P1 completion and does not change CQPL truth or assessment semantics.

## Producers retained

- DEP1 Rust body records and resolved-call records remain in `dependency_body_ingestion_v1.body_statuses` and `exact_call_bindings`.
- CREMA's existing imported FFI body path remains the ICFG relation from the MIR call node through `FFI Call`, `dummyCall->LLVM Entry`, the imported LLVM body, `LLVM Exit->dummyRet`, and `dummyRet->MIR Return`.
- CQPL/ELE1 remains the external-library model path. A bodyless model is accepted only when the existing `ExternalCallBindingV1` and its `ExternalLibraryEffectsV1` envelope identify the same call node and at least one effect family.

No coverage effect is reimplemented by the ledger.

## Deterministic call key

For a resolved call, the key is:

```text
dep1-call-v1:<caller Instance identity>:bb<basic-block>:<resolved callee Instance identity>
```

If rustc cannot resolve an Instance, the final component is the compiler-stable resolved definition identity, operand definition identity, or the literal `unresolved`, in that order. The value is assembled from stable compiler identity fields and the MIR block index; diagnostic names are not key material.

The callback emits `icfg_callsite_links` while it has both compiler identity and the existing selected-crate ICFG naming context. Each link repeats the caller definition identity, caller Instance identity, block index, and callee identity so the exporter can validate the join. Dependency callers without an existing ICFG call node get no link and cannot receive a CREMA/ELE1 delegation by name inference.

## Exported ledger

When DEP1 is enabled, annotated ICFG output includes:

```json
{
  "call_semantic_coverage": [
    {
      "call_key": "...",
      "semantic_coverage_path": "dep1_rust_mir | crema_ffi_body | cqpl_external_library_model | uncovered",
      "coverage_reference": ["exact producer artifact reference"],
      "coverage_status": "covered | uncovered | out_of_scope | conflicting_coverage | invalid_call_key"
    }
  ],
  "call_semantic_coverage_complete": true
}
```

`dep1_rust_mir` requires one matching `represented_body` record for the resolved concrete Instance and refers to both the exact call binding and body instance. `crema_ffi_body` requires the exact ICFG entry and return relations around an imported LLVM body. `cqpl_external_library_model` requires exactly one matching ELE1 binding and its nonempty effect envelope. A foreign-item classification, ABI, or displayed name alone is never a delegation proof. For local callers, the producer requires exactly one compiler-identity-validated callsite link; every linked reachable call is checked for existing CREMA and ELE1 paths, even when DEP1 already represents its callee. This makes duplicate or conflicting coverage incomplete instead of silently preferring one path.

Zero accepted paths is `uncovered`. Multiple paths, duplicate producer relations, or mismatched identity/link fields are rejected as conflicts or uncovered incomplete results. No precedence is inferred. Out-of-scope callees remain explicitly visible and are outside this scoped call-coverage Boolean. An eligible expected Rust body that is unavailable, unresolved scope identity/Instance/dispatch, and any reachable in-scope call without accepted evidence make `call_semantic_coverage_complete` false even if another path is present. Fatal compiler failures remain run-level failures under SC1.

The ledger is producer provenance only. CQPL does not consume it, and this call-only Boolean is not resource completeness or full analysis completeness.
