# llvm_memory_effects_v1

Proof-carrying LLVM **16.0.4** memory/effect evidence.

The producer first requires LLVM Verifier success for both the parsed input
module and the isolated TLI clone (`input_ir_verified` / `tli_clone_verified`).
It then preserves two distinct evidence origins:

- `explicit_input_ir` / `llvm16_explicit_input_ir_v1`: contracts represented on
  the LLVM16-parsed input module before any inference pass. This is a
  consumer-normalized input view (e.g. LLVM16 may normalize older IR syntax),
  not a claim about the exact textual spelling emitted by Clang;
- `llvm_tli_inferred` / `llvm16_tli_libfunc_attrs_v1`: attributes
  inferred on an isolated clone using LLVM 16 `TargetLibraryInfo` plus
  `inferNonMandatoryLibFuncAttrs`.

The inferred clone is never used to build SVFIR, ICFG, or Andersen.  The
sidecar records both snapshots and a structural delta bit, so downstream code
cannot confuse frontend evidence with a library-model inference.

Exported dimensions include function `nofree`, `nosync`, `willreturn`,
`memory(...)`, allocator kind/family/size and return `noalias`; formal
`nofree`, `nocapture`, `returned`, `readnone`, `readonly`, `writeonly`,
`allocptr`, and `allocalign`; and explicit/inferred effective callsite memory
effects.

Missing attributes are unconstrained except that LLVM's `MemoryEffects` API
represents the documented implicit read/write default. `nocapture`, parameter
`nofree`, and parameter access attributes remain formal-copy scoped.
`returned` is alias evidence, not ownership transfer. `allockind("free")` plus
`allocptr` describes deallocation semantics but does not by itself prove which
CREMA abstract allocation is MUST-freed.

The complete sidecar is embedded into annotated ICFG v2 when the capability is
advertised. CQPL rejects a naked capability, a naked payload, unknown nested
fields, a false verifier certificate, inconsistent TLI recognition/libfunc
identity, or a `tli_changed` bit that disagrees with the structural snapshots.

## How this evidence is used in R2

This payload is evidence, not a separate truth engine.

For external deallocation classification, a structured direct `CallBase` to a callee with explicit/TLI `allockind(free|realloc)+allocptr` may corroborate a historical structural MAY-deallocation basis.

The historical basis is not replaced solely to obtain a newer-looking provenance string. R2-R1.2 serializes the independent LLVM basis in `corroborating_bases`.

Official semantics are pinned to the LLVM 16 release documentation:
<https://releases.llvm.org/16.0.0/docs/LangRef.html>.

See `../ANALYSIS_PIPELINE.md`.
