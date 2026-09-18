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

R2-R0 emission policy:

- `tt` -> `subresult=tt`, `direction=true`, `strength=abstract_established`;
- `ff` -> `subresult=ff`, `direction=false`, `strength=abstract_established`;
- `unk` with a **directionally positive** finding -> `unk_true`, with strength inherited from the strongest directional finding;
- `unk` with no directional finding -> `unk_unoriented`, `strength=unresolved`;
- an `unresolved_allocator_contract_candidate` is explicitly non-directional: an unknown allocator/deallocator family can resolve either to equality or inequality, so its presence must not orient the result toward true.

`unk_false` and `unk_mixed` are reserved but are not emitted until an explicit, dual refuting-evidence pipeline exists. Absence of a positive witness must never be reinterpreted as negative evidence.

`basis` is constructed from the same supporting findings, allocation/deallocation contracts, disposition evidence, FFI positional identity certificates, and external-effect evidence already serialized in the explanation. The assessment is therefore presentation/epistemology over the existing model-checking result, not an independent classifier.

## R2-R1.2 provenance rules

The assessment basis is a canonical projection of the serialized explanation evidence.

Rules:

- the token used in `assessment.basis` MUST match the JSON wire token for the same evidence;
- canonical CString tokens are `producer_certified_c_string_into_raw` and `producer_certified_c_string_from_raw`;
- `pta_basis:svf_andersen_wave_diff_may_v1` is emitted only when the attached FFI record has a non-empty `svf_may_points_to` set;
- an empty SVF points-to set may remain serialized in the FFI certificate as analysis context, but it is not positive PTA support;
- a historical primary external-effect basis may be accompanied by `external_effect_corroborating_basis:<basis>` entries; corroboration never changes the status or truth value.

Current FINAL112 acceptance for provenance-only releases freezes both truth and assessment counts:

```text
ff=705  unk=413  tt=226
unk_true=273  unk_unoriented=140
strong_abstract_evidence=73
observational_candidate=200
unresolved=140
```

See `../ANALYSIS_PIPELINE.md` for the end-to-end interpretation.
