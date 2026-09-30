# DEP1-P1-SC1: Status Contract Correction

**Status:** proposed protocol amendment for independent review  
**Applies to:** `dependency_body_ingestion_v1`  
**Pinned compiler evidence:** `nightly-2024-11-21` (`rustc 1.84.0-nightly`, commit `3fee0f12e4f595948f8f54f57c8b7a7a58127124`)

This amendment supersedes only the conflicting status-taxonomy and failure-semantics clauses identified below. The historical protocol files remain unchanged and are part of the amendment history. This document does not claim full DEP1-P1 completion.

## 1. Reason for the correction

The P1 production protocol listed `compiler_query_failed` as a per-call/body status. In the pinned compiler, `optimized_mir(DefId)` is a rustc query whose value is `&Body`, not `Result<&Body, Error>`. The corresponding `is_mir_available(DefId)` query returns `bool`. rustc's query engine poisons a query that panics; subsequent access raises a fatal error. There is no sound per-callee recovery value after a fatal `optimized_mir` query, and catching the panic and continuing would violate query-engine failure semantics.

Consequently, `compiler_query_failed` is **not implementable soundly as a recoverable per-body status** on this toolchain. It is replaced here by the run/session-level terminal state `compiler_session_failed`. A failed compiler session must never be represented as an ordinary external call or a successful, complete analysis.

Pinned source locations and the captured API excerpt are listed in the SC1 evidence package's `PINNED_RUSTC_QUERY_FAILURE_REPORT.json` and `pinned-rustc-query-api.txt`.

## 2. Recoverable per-call/per-body taxonomy

The following are the only per-call/per-body statuses in the corrected contract:

| Status | Meaning |
|---|---|
| `represented_body` | The call resolved to an eligible body, MIR was available, and that concrete body/Instance was admitted to traversal. |
| `body_unavailable` | Scope and call resolution succeeded, but `is_mir_available(resolved_def_id)` returned false. No `optimized_mir` query or body traversal follows for that callee. |
| `out_of_scope` | The call resolved, but its package/unit is intentionally outside the selected runtime scope (for example sysroot or host-only code). Preserve the scope reason separately. |
| `unresolved_scope_identity` | The compiler crate could not be joined uniquely to a Cargo unit/PackageId. This is fail-closed and cannot be treated as an ordinary external call. |
| `unresolved_instance` | A statically represented `FnDef` was present, but compiler Instance resolution/normalization did not produce a concrete Instance. |
| `virtual_or_dynamic_unresolved` | The call operand has no statically resolvable `FnDef`/Instance (for example an unresolved virtual/dynamic call). |

`is_mir_available` must dominate every dependency `optimized_mir` query. Only `represented_body` callees may be enqueued. The resolved `Instance.def_id()` is the query key for trait implementations.

### Body availability and semantic coverage

`body_unavailable` is not itself a completeness verdict. Coverage is assessed over reachable calls and must carry machine-observable provenance. The accepted semantic paths are:

| Coverage path | Meaning | DEP1 completeness effect |
|---|---|---|
| `dep1_represented_rust_body` | Eligible Rust dependency Instance has available MIR and is represented by DEP1. | Covered. |
| `dep1_expected_rust_body_unavailable` | Eligible Rust callable is expected to have a Rust body, but `is_mir_available(resolved_def_id)` is false. | Incomplete. |
| `delegated_crema_ffi_body` | The exact FFI call is represented by CREMA's existing imported FFI body/inlining path. | Covered by CREMA. |
| `delegated_cqpl_external_library_model` | The exact bodyless external call is represented by existing CQPL/ELE1 external-library semantics. | Covered by CQPL/ELE1. |

The two FFI paths are distinct. Do not treat every foreign declaration as an ELE1 call, and do not treat a body-backed FFI call as bodyless. DEP1 must not duplicate either implementation. A foreign declaration by itself proves only that the item is a compiler-classified foreign item; it does not prove which accepted semantic layer covers a particular call.

For compiler classification, the pinned `tcx.is_foreign_item(def_id)` structurally checks whether the item's parent is `DefKind::ForeignMod`. This can establish that a resolved item is a foreign declaration, but cannot establish that an exact call site has a CREMA FFI body or an ELE1 binding. Coverage must be linked to the existing consumer's exact call/node record: a represented external body is witnessed by the existing ICFG FFI body/inlining relation; the bodyless route is witnessed by the existing ELE1 `ExternalCallBindingV1`/effect provenance. Names or source text may not be used to infer delegation.

The current DEP1 body-status export contains only `body_status` and the compiler-derived `body_unavailable_reason`. It does not contain a call-linked semantic coverage/delegation field, and its dependency-body records are not yet stitched into the existing ICFG call nodes. Therefore current evidence may establish `foreign_item` / bodyless-by-rustc-construction, but must not fabricate `delegated_crema_ffi_body` or `delegated_cqpl_external_library_model`. The minimal future producer addition is a call-linked `semantic_coverage_path` field (plus exact existing ICFG/ELE1 binding reference) populated only after the existing consumer confirms one of those paths. Until then, whole-run completeness for such a reachable boundary is unproven; it is not licensed to be reported complete merely because the item is foreign.

For a non-foreign eligible Rust callable where `is_mir_available(resolved_def_id)` is false, record the candidate path `dep1_expected_rust_body_unavailable`; this makes analysis incomplete. For a foreign declaration with unavailable Rust MIR, preserve the low-level reason `rust_mir_unavailable` only as a compiler fact plus `foreign_item=true`; do not use the old generic `intentionally_bodyless_foreign` value as a coverage claim. The eventual call record must say which existing FFI semantic path covers it, or leave run completeness unresolved/incomplete.

An eligible `InstanceKind::Virtual` is `virtual_or_dynamic_unresolved`, not a body-unavailable case: it has no callable MIR body by compiler definition. Unknown/missing coverage data fails closed and cannot be treated as covered.

### `opaque` and `intentionally_opaque`

The prior documents use `opaque` and `intentionally_opaque` inconsistently: as a synonym for unavailable MIR, as a synonym for out-of-scope code, or as a generic external-call boundary. The current semantic core has no independent observable condition that distinguishes a further `opaque` state from the six statuses above. SC1 therefore does not add an `opaque` alias.

If a future policy intentionally withholds an otherwise eligible, MIR-available body, it must first define a distinct machine-observable reason and an independently reviewable policy. Until then, use the applicable status above and retain its reason; do not synthesize `intentionally_opaque` merely to satisfy historical wording.

## 3. Run/session failure and completeness model

Per-body recoverable states are separate from compiler-process and whole-analysis outcomes.

The DEP1 run report should expose, subject to schema review:

```json
{
  "compiler_session_status": "completed | failed | not_started",
  "analysis_complete": true,
  "incomplete_reasons": []
}
```

`incomplete_reasons` is a set/list of applicable run-level reasons, including:

```text
dependency_body_unavailable
dependency_body_capture_failed
dynamic_target_unresolved
resource_limit_reached
compiler_session_failed
compiler_error
```

The report may additionally use `complete_for_reachable_supported_bodies` as a positive completeness classification. `analysis_complete_for_reachable_semantics` is true only when every reachable call is covered by exactly one accepted semantic path or is explicitly classified as an incompleteness/failure, and the selected runtime-scope fixed point completed. A reachable eligible Rust call with unavailable expected MIR, unresolved scope identity, unresolved concrete Instance, unresolved dynamic target, resource limit, compiler error, or fatal compiler session makes the analysis incomplete. A foreign declaration alone does not establish coverage: the exact call must be delegated to either CREMA's FFI body/inlining path or CQPL/ELE1's external-library model. Deliberately out-of-scope code does not by itself make analysis incomplete relative to the declared DEP1 runtime scope; its `out_of_scope` status and reason remain visible.

`resource_limit_reached` is run-level because a traversal/resource cap truncates the analysis, not one body's semantic classification. Any introduced limit must stop/mark the run incomplete; it must not silently return a complete-looking artifact.

`compiler_session_failed` is terminal. If rustc returns a fatal error, panics, or aborts during a compiler query, the wrapper/orchestrator must fail the DEP1 phase and must not publish the partial graph as complete. Where the supervisor can persist a failure sidecar, it records `compiler_session_status: "failed"`, `analysis_complete: false`, and `compiler_session_failed` (or the more specific `compiler_error`). No per-body `compiler_query_failed` record is fabricated. If the compiler process cannot persist a sidecar, its non-success exit remains a failed run and must not be interpreted as a successful result.

## 4. Fail-closed behavior

- Missing or ambiguous Cargo PackageId/crate correlation for an eligible ordinary dependency is `unresolved_scope_identity` and fails the Phase-B analysis closed.
- `body_unavailable` is emitted only after successful call resolution and positive runtime-scope eligibility, with the false `is_mir_available` result. The callee is not enqueued and no optimized MIR is queried. Completeness is then decided by accepted semantic coverage, not by body presence alone.
- `unresolved_instance` and `virtual_or_dynamic_unresolved` remain explicit. They are not converted to `out_of_scope`.
- A compiler fatal/query failure terminates the compiler session. It is not caught to continue semantic traversal.
- Any known omitted reachable body or truncation is reflected in run-level incompleteness; absence from the output is never presented as proof of absent behavior.

## 5. Historical clauses amended

This amendment preserves, rather than deletes, the earlier requirements and their rationale:

- `DEP1_P1_PRODUCTION_IMPLEMENTATION.md` §12 listed `compiler_query_failed` and `resource_limit_reached` among per-call/body states and paired `intentionally_opaque` with `out_of_scope` in the provider algorithm. SC1 moves fatal compiler-query failure and resource limits to run-level reporting and removes `intentionally_opaque` absent a distinct observable state.
- `DEP1_P1_RESUME_AFTER_ID0.md` listed `compiler_query_failed` as a body status. SC1 replaces that per-body requirement with terminal `compiler_session_failed`.
- `DEP1_DEPENDENCY_BODY_INGESTION_V1.md` §§2, 14, and 15 already require explicit unavailable/opaque outcomes, fail-closed behavior, and run completeness. SC1 makes those levels precise: recoverable body status versus run/session status, and does not weaken its completeness requirement.

SC1-R1 further corrects SC1's initially overbroad statement that every `body_unavailable` makes analysis incomplete. It separates the DEP1 missing-Rust-MIR condition from semantic delegation. CREMA's body-backed FFI path and CQPL/ELE1's bodyless external-library path remain distinct existing implementations. The current producer's foreign-item reason alone proves neither delegation; an exact call-linked coverage reference is a future producer requirement before whole-run completeness can be claimed for such calls.

The historical text and original `W1_R1_INCOMPLETE` report remain preserved. The R1 incomplete verdict accurately records the old contract mismatch. The SC1 reevaluation judges the proven body-provider behavior against this corrected taxonomy; it does not retroactively alter the evidence or claim final P1 acceptance.

## 6. Compatibility and scope

This is a producer/protocol status correction only. It does not change CQPL checker semantics, three-valued logic, canonical queries, resource propagation, ICFG linkage, or any subject-specific behavior. Any new serialized completeness fields require a separately reviewed schema change before production implementation. The taxonomy is general Cargo/rustc behavior and is not fitted to RustSec or `emap`.
