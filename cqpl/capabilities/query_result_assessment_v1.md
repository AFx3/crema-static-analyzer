# `cqpl_result_assessment_v1`

`cqpl_result_assessment_v1` is an output/explainability protocol. It is not a fourth/fifth truth logic and does not change CQPL's frozen three-valued result:

```text
result = tt | ff | unk
```

It adds an orthogonal epistemic assessment:

```text
subresult = tt | ff | unk_true | unk_false | unk_mixed | unk_unoriented
direction = true | false | mixed | none
strength  = abstract_established | strong_abstract_evidence | observational_candidate | unresolved
```

R2-R0 / Gate-L1 emission policy:

- `tt` -> `subresult=tt`, `direction=true`, `strength=abstract_established`;
- `ff` -> `subresult=ff`, `direction=false`, `strength=abstract_established`;
- `unk` with a **directionally positive** finding -> `unk_true`, with strength inherited from the strongest directional finding;
- `unk` with an explicit **directionally negative** Gate-L1 finding -> `unk_false`; the initial leak-state rule is observational only and never changes CQPL truth;
- `unk` with both positive and negative directional findings -> `unk_mixed`; neither direction overrides the other;
- `unk` with no directional finding -> `unk_unoriented`, `strength=unresolved`;
- an `unresolved_allocator_contract_candidate` is explicitly non-directional: an unknown allocator/deallocator family can resolve either to equality or inequality, so its presence must not orient the result toward true.

Gate L1 activates the previously reserved `unk_false` and `unk_mixed` values for the exact allocation-state leak shape

```text
exists_alloc a. EF (alloc(a) && EX EG !drop(a))
```

only when an explicit refuting finding is constructed. Absence of a positive witness is still never reinterpreted as negative evidence.

The initial refuting finding is `all_candidate_suffixes_cross_modeled_drop`. It requires complete coverage of the represented allocation candidates and explicit allocation origins, rejects allocation-site reuse, rejects unresolved ownership/external-effect evidence, and rejects every terminal or cyclic suffix that can avoid an exact `CellValue::Freed` barrier. `Top` is never accepted as a deallocation barrier.

This remains an **observational assessment**, not a proof of memory safety: `CellValue::Freed` still makes the CQPL MAY predicate `drop(a)` evaluate to `unk`, not `tt`. Therefore Gate L1 may produce `subresult=unk_false` while `result` remains exactly `unk`.

`basis` is constructed from the same supporting findings, allocation/deallocation contracts, disposition evidence, FFI positional identity certificates, and external-effect evidence already serialized in the explanation. The assessment is therefore presentation/epistemology over the existing model-checking result, not an independent classifier.

## Query-declared assessment scope

Gate L1 may declare `assessment_scope normal_execution;` together with an explicit `requires typed_edge_flow_v1;`. The declaration changes only traversal used to construct directional assessment findings; the CQPL `ff|unk|tt` result continues to be evaluated on the complete successor relation.

A normal-scoped assessment carries `assessment_scope:normal_execution` and `capability:typed_edge_flow_v1` in `basis`, plus an explicit caveat that unwind edges remain part of truth semantics. See `../ASSESSMENT_SCOPES.md`.


## R2-R1.2 provenance rules

The assessment basis is a canonical projection of the serialized explanation evidence.

Rules:

- the token used in `assessment.basis` MUST match the JSON wire token for the same evidence;
- canonical CString tokens are `producer_certified_c_string_into_raw` and `producer_certified_c_string_from_raw`;
- `pta_basis:svf_andersen_wave_diff_may_v1` is emitted only when the attached FFI record has a non-empty `svf_may_points_to` set;
- an empty SVF points-to set may remain serialized in the FFI certificate as analysis context, but it is not positive PTA support;
- a historical primary external-effect basis may be accompanied by `external_effect_corroborating_basis:<basis>` entries; corroboration never changes the status or truth value.

The historical FINAL112 provenance-only acceptance froze both truth and assessment counts:

```text
ff=705  unk=413  tt=226
unk_true=273  unk_unoriented=140
strong_abstract_evidence=73
observational_candidate=200
unresolved=140
```

See `../ANALYSIS_PIPELINE.md` for the end-to-end interpretation.

The normal-execution Gate L1 has a separate acceptance script, `../scripts/run_gate_l1_normal_execution.sh`. Its scientific acceptance requires zero truth deltas, zero non-target assessment deltas, no negative-only orientation of known true memory leaks, `clean_alloc_read_and_drop -> unk_false`, and `boxed_bool__ml -> unk_true`. The query-level scope must remain explicit and the complete-model CQPL truth must remain unchanged.
