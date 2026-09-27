# `conditional_reallocations_v2`

`conditional_reallocations_v2` is the hardening layer for the v1 conditional
`realloc` relation. An artifact declaring v2 MUST also declare
`conditional_reallocations_v1` and uses the same `conditional_reallocations`
payload.

v2 adds producer-certified proof obligations that are deliberately
consumer-visible.  They refine the proof surface only; the v1 conditional
reallocation semantics are unchanged.

## Outcome correlation

Each record carries:

1. `outcome_correlation_basis = "direct_cfg_edge_realloc_to_is_null_v1"`.
   The certified raw-pointer `is_null` call must be the immediate canonical
   ICFG successor of the `realloc` call.
2. `outcome_argument_variable`, the MIR local actually consumed by `is_null`.
3. `outcome_value_flow_basis`, drawn from the closed vocabulary:
   - `rust_mir_direct_result_operand_v1`: the `is_null` operand is exactly the
     realloc result local and that local is not redefined by statements in the
     predicate block;
   - `rust_mir_single_local_copy_result_operand_v1`: the operand local has
     exactly one definition in the predicate block and that definition is the
     exact MIR form `operand = copy result`; the realloc result local is not
     redefined in the predicate block.

The producer deliberately rejects casts, dereferences, projections, `move`,
transitive copy chains, multiple definitions of the operand temporary, and any
same-block redefinition of the realloc-result local.  This closes the gap that
a direct inter-block CFG edge alone cannot exclude.

## Result deallocation

Every `result_deallocations` entry carries:

1. `variable`, the logical realloc-result local whose allocation obligation is
   being discharged;
2. `argument_variable`, the MIR local actually passed to `free`;
3. `argument_correlation_basis`, drawn from the same closed operand vocabulary:
   - `rust_mir_direct_result_operand_v1`, or
   - `rust_mir_single_local_copy_result_operand_v1`;
4. `value_flow_basis = "rust_mir_result_no_redefinition_all_paths_v1"`.

The last proof is distinct from argument correlation.  It requires every
canonical CFG path from the certified success successor to the candidate
`free` to keep the logical realloc-result local unchanged and to avoid an
earlier deallocator/bodyless `realloc` consumption.  Statements in the target
`free` block are included in this check; only the target terminator itself is
the permitted final consumer.  Consequently, a block such as

```text
result = old_source
arg = copy result
free(arg)
```

may satisfy the local argument-correlation proof but MUST fail the
no-redefinition proof.

The checker validates the direct CFG edge, the closed proof-basis strings, the
consumer-visible argument variables, matching structural MIR labels for a
single-copy proof, and the raw `drop` event against the observed free argument.
The producer certifies the exact MIR local-copy and no-redefinition properties;
they are never reconstructed from pretty-printed source text.

The semantics of v1 are unchanged: failure preserves the old allocation,
success invalidates the old allocation, no fresh C allocation identity is
fabricated for the result, and the producer ICFG is not rewritten with
checker-local semantic nodes.

Cases outside v2 deliberately fail closed, including delayed outcome checks,
zero/dynamic sizes, missing source-existence proof, non-local/cast/projected or
transitively copied predicate operands, and result deallocation whose logical
result local is redefined or whose obligation is consumed before `free`.
