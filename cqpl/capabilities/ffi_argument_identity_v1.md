# `ffi_argument_identity_v1`

`ffi_argument_identity_v1` is a proof-carrying, observational capability for the Rust -> C FFI boundary.

Each record certifies one zero-based argument position using the existing Bmulti positional certificate and the existing allocation-identity fixed point:

```text
Rust actual ProgramVarId
    -- arg_index -->
C/SVF formal ProgramVarId
    -- MAY identity -->
AbstractAllocId set
```

The producer emits a record only for external `DummyCall` nodes with a complete `svf_formal_arg_index_v1` positional binding **and a MIR actual that is representable as a function-scoped `ProgramVarId`**. MIR constants and other non-local operands are outside the allocation-identity variable domain and are skipped rather than treated as export errors. It reads both representable sides from the post-DummyCall identity state and fails closed if their MAY allocation sets disagree.

Closed fields:

- `certainty = "may_abstract"`
- `basis = "crema_bmulti_actual_formal_identity_v1"`
- `formal_mapping_basis = "svf_formal_arg_index_v1"`
- optional/non-empty SVF points-to memberships require `svf_points_to_basis = "svf_andersen_wave_diff_may_v1"`

Scientific restrictions:

- a non-local MIR operand (for example an integer constant passed to a C allocator) produces no `ffi_argument_identity_v1` record for that argument; this is non-applicability, not negative evidence;
- each emitted record carries at least one MAY `AbstractAllocId`; empty MAY identity sets are not serialized as negative evidence;
- this capability never upgrades MAY identity to MUST;
- an empty solved SVF formal points-to set is permitted at a Rust->C entry boundary;
- the cross-language identity certificate and Andersen points-to evidence are independent evidence sources;
- absence of an allocation from a record is not proof that the allocation cannot flow through another alias;
- the capability is explainability/provenance evidence in R2-R0 and does not change the frozen CQPL query semantics.

## PTA provenance in explanations

`svf_points_to_basis` names the analysis that produced `svf_may_points_to`. The certificate may carry that field even when the solved formal set is empty.

For `cqpl_result_assessment_v1`, however, `pta_basis:...` is positive supporting evidence **only when `svf_may_points_to` is non-empty**. This prevents an empty solved set at a Rust->C entry boundary from being presented as if it supported the queried bug pattern.

See `../ANALYSIS_PIPELINE.md`.
