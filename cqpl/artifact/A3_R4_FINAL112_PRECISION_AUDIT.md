# A3 R4 FINAL112 precision-delta audit

This file records the deliberate semantic delta between the immutable historical
FINAL112 freeze and the A3 R4 identity/lifecycle refinement.

The historical artifacts (`FINAL112_GRAPH_QUERY_AUDIT.tsv`,
`FINAL112_AUDIT_SUMMARY.json`, and related evidence) remain unchanged.  A3 is
accepted only if a fresh 112 x 12 run differs from that freeze in exactly the
seven cells listed in `A3_R4_FINAL112_PRECISION_DELTAS.tsv`.

All seven approved changes are `unk -> ff`; no `tt` changes are permitted.
They are oracle-consistent precision refinements:

- `skip-list-test` has source/reference class `ML` only.  The four approved
  changes refute unrelated double-free and use-after-free predicates after the
  refined identity analysis; the leak predicates remain unaffected.
- `unsafely-created-owned-type` has an empty reference-class set (clean target).
  The three approved changes refute allocator-mismatch/structural candidates
  that were previously unknown because ownership identity was less precise.

A real double-free target, `cstringcargo_enum_df_only_rust`, is intentionally
*not* allowlisted.  R4-r3 must preserve its historical `unk` results rather
than unsoundly refining them to `ff`.

The validator checks all of the following fail-closed:

1. 112 subjects and exactly the frozen 12-query surface;
2. the shipped historical audit is unchanged and remains the comparison base;
3. the source-oracle class for every allowlisted target matches the shipped
   `FINAL112_SOURCE_ORACLE_AUDIT.tsv`;
4. every allowlisted baseline value matches the historical cell;
5. every approved delta is observed in the fresh run;
6. no additional delta is observed;
7. result counts equal the historical counts transformed by exactly those seven
   approved deltas.

Expected A3 R4 counts after the approved refinements are therefore:

- `ff = 657`
- `unk = 461`
- `tt = 226`

This is a precision-improvement claim, not a claim that the historical freeze
was incorrect or should be overwritten.
