# svf_solved_points_to_v1

Solved `AndersenWaveDiff` evidence exported from the same pinned SVF run used to
build the ICFG.  The sidecar intentionally exports the boundary-relevant formal
sets rather than every internal PAG node.

Every points-to set is explicitly **MAY**.  Singleton sets are never promoted to
MUST identity.  Records are positional only under
`svf_formal_arg_index_v1`. CREMA correlates a set with a Rust FFI actual only
when the sidecar formal index and `svf_var_id` exactly match the producer's
formal-parameter certificate; otherwise it fails closed and exposes no PTA
basis on that binding.

The complete artifact is carried into annotated ICFG v2. CQPL requires the
`svf_solved_points_to_v1` capability and payload to appear together and
revalidates `analysis=AndersenWaveDiff`, `semantics=may`, positional indices,
unique formal VarIDs, and sorted unique points-to sets.

## Empty sets and explanation strength

An empty solved formal set is permitted, especially for a C function that is entered from Rust rather than from a C `CallBase` in the analyzed LLVM module.

Therefore:

- empty set != MUST-no-alias;
- empty set != negative allocation evidence;
- an empty set does not contribute `pta_basis` to `cqpl_result_assessment_v1`;
- non-empty memberships remain MAY, including singleton sets.

Cross-language allocation identity at Rust->C boundaries is represented separately by `ffi_argument_identity_v1`.

See `../ANALYSIS_PIPELINE.md`.
