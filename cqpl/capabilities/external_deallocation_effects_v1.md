# external_deallocation_effects_v1

`external_deallocation_effects_v1` is an additive, proof-carrying producer/consumer boundary for external C-call deallocation effects. It remains three-valued and fail-closed: `certified_absent`, `observed_may_deallocate`, or `unresolved`. The capability by itself does not change a frozen query result and does not upgrade MAY evidence to MUST.

Absence of a record is never proof of absence of deallocation. Likewise, a positive MAY record says only that the classified call may cause deallocation under the stated basis; it does not say that every path deallocates, that a particular CREMA allocation is definitely freed, or that ownership was transferred.

Each record remains keyed by the external `dummyCall` node and contains the selected `callee`, a closed `status`, and a closed evidence `basis`. The existing Rust→SVF bridge remains responsible for selecting the external callee identity.

## Frozen ND1 bases

- `certified_absent` / `svf_leaf_no_call_deallocation_v1`
- `observed_may_deallocate` / `structural_c_free_v1`
- `unresolved` / `svf_call_effect_unresolved_v1`
- `unresolved` / `svf_body_unavailable_v1`

These retain their historical meaning. In particular, `svf_leaf_no_call_deallocation_v1` is only a narrow call-structure certificate for a body-backed leaf under the frozen producer vocabulary. It is not a proof about arbitrary inline assembly, custom allocators, hidden callbacks, ownership, or concurrent lifetime behavior. `structural_c_free_v1` remains positive MAY evidence and does not imply that every path reaches `free`.

## EFX1 additive bases

When—and only when—the annotated artifact also declares `llvm_memory_effects_v1` and carries a schema-valid matching payload, the following additional bases are accepted:

- `certified_absent` / `llvm16_explicit_nofree_v1`
- `certified_absent` / `llvm16_tli_nofree_v1`
- `certified_absent` / `llvm16_explicit_nonmodifying_memory_v1`
- `certified_absent` / `llvm16_tli_nonmodifying_memory_v1`
- `observed_may_deallocate` / `llvm16_explicit_allockind_deallocation_v1`
- `observed_may_deallocate` / `llvm16_tli_allockind_deallocation_v1`
- `observed_may_deallocate` / `llvm16_explicit_direct_callee_allockind_deallocation_v1`
- `observed_may_deallocate` / `llvm16_tli_direct_callee_allockind_deallocation_v1`

`nofree` denotes absence of deallocation caused by the classified callee; it is not promoted to a global post-call lifetime theorem. A non-modifying explicit `memory(...)` contract is sufficient only for the same narrow no-deallocation conclusion.

`allockind("free"|"realloc")` is positive MAY-deallocation evidence only when an `allocptr` formal is present. It does not identify a CREMA `AbstractAllocId` as MUST-freed. That requires separate identity/points-to correlation, and the solved SVF evidence remains MAY even when the points-to set is a singleton.

## Structured-boundary rule

EFX1 evidence is consumed only when the external callee identity is already carried structurally by the `DummyCall -> llvm::<callee>` boundary. For a body-backed wrapper, positive closure to a nested deallocator is permitted only through the producer's structured LLVM `CallBase` records and a matching explicit/TLI `allockind(free|realloc)+allocptr` contract. New EFX1 reasoning does not parse `node.info` or other pretty-printed SVF diagnostics to discover nested callees. Absence of a structured direct deallocator call is never a negative certificate.

The TLI route is provenance-distinct from explicit input IR. LLVM library inference is run on a clone and never mutates the module used to construct SVFIR/ICFG/Andersen. The evidence payload records both origins separately and the checker requires the capability and payload atomically.

## Primary basis versus corroboration

R2-R1.2 keeps historical provenance stable. A producer may therefore keep:

```text
basis = structural_c_free_v1
```

and add independent LLVM16 evidence in:

```text
corroborating_bases = [
  llvm16_tli_direct_callee_allockind_deallocation_v1
]
```

`corroborating_bases` is optional, sorted, unique, and must be compatible with the same `status`. LLVM corroboration requires the `llvm_memory_effects_v1` capability.

Corroboration is additive only:

```text
same status
same MAY/MUST interpretation
same CQPL truth
stronger audit trail
```

It must never be used to silently rewrite historical primary provenance.

In the frozen FINAL112 corpus used for R2-R1.2, the 20 `structural_c_free_v1` positive records are independently corroborated by structured LLVM16 direct-callee evidence. The final audit checks this fact.

See `../ANALYSIS_PIPELINE.md` for the complete flow.
