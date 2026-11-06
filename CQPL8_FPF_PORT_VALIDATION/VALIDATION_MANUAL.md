# Manual validation protocol

Nothing in this document was executed by Codex. Use fresh output directories;
never overwrite a frozen campaign.

```bash
export PORT=/mnt/wsx-storage/af/a-phd-ml-fn-port
export KIT="$PORT/CQPL8_FPF_PORT_VALIDATION"
export BOOT=/mnt/wsx-storage/af/cqpl8-fpf-port-bootstrap-manual
export RQ1_OUT=/mnt/wsx-storage/af/cqpl8-fpf-port-rq1-manual
export RQ2_OUT=/mnt/wsx-storage/af/cqpl8-fpf-port-rq2-manual
export ORACLE=/home/af/Documenti/a-phd/RQ1_CORRECTED_ORACLE/RQ1_ORACLE_175.csv
export RECON="$PORT/RQ1_RQ2_CQPL8_RECONCILIATION"
export BASE=/mnt/wsx-storage/af/CREMA_CQPL_VALIDATED_INTEGRATION
```

## 1. Compile from the exact worktree into dedicated target directories

```bash
bash "$KIT/scripts/bootstrap.sh" "$BOOT" build
```

The bootstrap refuses an unexpected HEAD, records source/binary hashes, uses
`nightly-2024-11-21`, external Cargo target directories, an isolated `TMPDIR`,
`CARGO_INCREMENTAL=0`, and the matching sysroot `LD_LIBRARY_PATH`.

After any Rust source modification, choose a **new** bootstrap directory. Do
not reuse `/mnt/wsx-storage/af/cqpl8-fpf-port-bootstrap-manual`.

### Focused export-recovery smoke

After rebuilding into a fresh bootstrap, execute only the formerly failing
crate in another fresh output directory:

```bash
python3 "$KIT/scripts/run_rq1_final.py" \
  --worktree "$PORT" \
  --bootstrap /mnt/wsx-storage/af/cqpl8-cstringcargo-fix-bootstrap-manual \
  --oracle "$ORACLE" \
  --out /mnt/wsx-storage/af/cqpl8-cstringcargo-fix-smoke-manual \
  --only a-code_full_rust/cstringcargo_enum_df_only_rust
```

Success requires `producer/annotated_icfg_v2.json`, valid query and explanation
JSON files, and four primary rows with `execution_status=OK` in
`PRIMARY_RESULTS.tsv`; `DF_NORMAL` must be the sole primary DF row. Review the
actual results, supporting findings and witnesses without imposing synthetic
expected verdicts. In `crema-export.log`, verify the typed marker
`A3_BODYLESS_ENUM_CONSTRUCTOR_CALLBACK_V1: callback=log::MaybeStaticStr::Static`
and the absence of `UNRESOLVED_HIGHER_ORDER: local callback MIR body missing`.
Also verify that the checkpoint's `producer/crema-command.json` contains
`--a3-bodyless-enum-constructor-callback-v1`; the capability is selected from
`A3_TARGET_CAPABILITIES.tsv` only for the source-audited target.

For a later full campaign, `scripts/rq1_paper_pipeline.sh --full` always rejects
an existing bootstrap/output path, recompiles, runs the FFI guard and lets the
runner verify source and binary hashes. It therefore cannot silently reuse the
pre-fix bootstrap.

## 2. Unit tests

Use the same external target directories created by the bootstrap:

```bash
RUSTUP_TOOLCHAIN=nightly-2024-11-21 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$BOOT/targets/crema" TMPDIR="$BOOT/tmp" \
  cargo test --manifest-path "$PORT/crema/Cargo.toml"

RUSTUP_TOOLCHAIN=nightly-2024-11-21 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$BOOT/targets/cqpl" TMPDIR="$BOOT/tmp" \
  cargo test --manifest-path "$PORT/cqpl/cqpl_checker/Cargo.toml"
```

## 3. FFI provenance smoke

```bash
bash "$KIT/scripts/bootstrap.sh" "$BOOT" ffi-smoke
```

This refuses binary-hash drift and aborts if the stale FFI sentinel survives.

## 4. Focused closure, UAF, DF and ML smokes

Run each with a new output directory (the final runner accepts `--only`):

```bash
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-closure-df --only a-code_full_rust/closure_df
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-real-uaf --only a-code_full_rust/uaf_read_ptr
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-real-df --only a-code_full_rust/struct_point_df_only_rust
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-ml-malloc --only a-code_c_to_rust_alloc/c_malloc_leak_ml
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-ml-calloc --only a-code_c_to_rust_alloc/c_calloc_leak_ml
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-b20b --only a-code_c_ffi_bodyless_gate/b20b_realloc_result_redefined_before_free
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-b20c --only a-code_c_ffi_bodyless_gate/b20c_realloc_zero_size_fail_closed
python3 "$KIT/scripts/run_rq1_final.py" --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out /mnt/wsx-storage/af/smoke-b39 --only a-code_c_ffi_bodyless_gate/b39_realloc_may_null_or_malloc_not_rn1
python3 "$KIT/scripts/validate_smokes.py" --root /mnt/wsx-storage/af
```

Manually verify positive finding allocation identities, that `b20b` retains
both ML and DF, and that `b20c` has no unjustified positive ML finding.

## 5. Full RQ1 (DF_NORMAL primary, DF_FULL diagnostic)

```bash
python3 "$KIT/scripts/run_rq1_final.py" \
  --worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" \
  --out "$RQ1_OUT" --resume

python3 "$RECON/scripts/audit_rq1_oracle_policy.py" \
  --raw "$RQ1_OUT/PRIMARY_RESULTS.tsv" --oracle "$ORACLE" \
  --policy evidence-qualified \
  --out /mnt/wsx-storage/af/cqpl8-fpf-port-rq1-policy-audit

python3 "$RECON/scripts/compare_rq1_branches.py" \
  --baseline-raw "$BASE/RQ1_OPERATIONAL_CONFUSION_MATRIX.tsv" \
  --baseline-classification "$BASE/RQ1_OPERATIONAL_CONFUSION_MATRIX.tsv" \
  --candidate-raw "$RQ1_OUT/PRIMARY_RESULTS.tsv" \
  --candidate-classification /mnt/wsx-storage/af/cqpl8-fpf-port-rq1-policy-audit/RQ1_CLASSIFICATION.tsv \
  --baseline-oracle "$ORACLE" --candidate-oracle "$ORACLE" \
  --candidate-provenance "$RQ1_OUT/PROVENANCE.json" \
  --baseline-policy evidence-qualified --candidate-policy evidence-qualified \
  --out /mnt/wsx-storage/af/cqpl8-fpf-port-rq1-comparison
```

Check 175 inventory entries, 696 historically evaluable primary cells, the
separate DF_FULL diagnostic rows, and every `(target_id, property)` transition.
Do not accept aggregate equality without witness review.

## 6. Frozen RQ2 (nine projects, five queries, 45 results)

```bash
python3 "$KIT/scripts/run_rq2_final.py" \
  --worktree "$PORT" --bootstrap "$BOOT" --out "$RQ2_OUT" --jobs 4 --resume

python3 /mnt/wsx-storage/af/RQ1_FPF002_DF_REDUCTION/scripts/compare_rq2.py \
  "$RQ2_OUT" --output /mnt/wsx-storage/af/cqpl8-fpf-port-rq2-45.tsv
```

Required gate: 45/45 with result, subresult, findings, allocation identities,
certificates and semantically equivalent witnesses preserved.

## 7. Table 5 (21 real ML findings)

```bash
python3 /mnt/wsx-storage/af/RQ1_FPF002_DF_REDUCTION/scripts/compare_table5.py \
  "$RQ2_OUT" \
  --golden /mnt/wsx-storage/af/RQ1_FP_REDUCTION/TABLE5_GOLDEN_BASELINE.tsv \
  --output /mnt/wsx-storage/af/cqpl8-fpf-port-table5.tsv
```

Required gate: 21/21 source-level leaks with valid allocation identities and
witnesses. The unrelated historical `shared-register` DF alarm is not a golden
real leak.

## 8. Final acceptance

Inspect `git diff --check`, the unit logs, FFI smoke, all focused findings,
row-level RQ1 comparison, RQ2 45/45 and Table 5 21/21. Only then decide whether
to commit or integrate. No command here updates a golden baseline or oracle.

## 9. Final normalized-fixture RQ1 and paper tables

The CStringCargo source used here is a documented source-level variant. Its
`src/main.rs` must hash to
`583745a43c7d5c90e33413e0cd7814e53476bb43cfd56cad94b1032f4d118386`.
The runner records that hash, per-target source-tree hashes and the
target-specific A3 capability manifest in `PROVENANCE.json`.

Choose paths which do not exist:

```bash
export FINAL_RQ1=/mnt/wsx-storage/af/cqpl8-cstringcargo-normalized-rq1-full-20261106
export FINAL_TABLES=/mnt/wsx-storage/af/cqpl8-cstringcargo-normalized-rq1-tables-20261106
export FINAL_COMPARE=/mnt/wsx-storage/af/cqpl8-cstringcargo-normalized-rq1-vs-696-20261106
```

For unit regressions, first create a dedicated bootstrap and use its external
target directories:

```bash
export TEST_BOOT=/mnt/wsx-storage/af/cqpl8-cstringcargo-normalized-test-bootstrap-20261106
bash "$KIT/scripts/bootstrap.sh" "$TEST_BOOT" build

RUSTUP_TOOLCHAIN=nightly-2024-11-21 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$TEST_BOOT/targets/crema" TMPDIR="$TEST_BOOT/tmp" \
  cargo test --manifest-path "$PORT/crema/Cargo.toml"

RUSTUP_TOOLCHAIN=nightly-2024-11-21 CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$TEST_BOOT/targets/cqpl" TMPDIR="$TEST_BOOT/tmp" \
  cargo test --manifest-path "$PORT/cqpl/cqpl_checker/Cargo.toml"
```

The next command performs a fresh build, FFI smoke and complete RQ1 replay; it
is not reporting-only:

```bash
bash "$KIT/scripts/rq1_paper_pipeline.sh" --full \
  /mnt/wsx-storage/af/cqpl8-cstringcargo-normalized-rq1-pipeline-bootstrap-20261106 \
  "$FINAL_RQ1"
```

Verify all 700 primary cells before reporting:

```bash
jq -e '
  .primary_rows == 700 and .successful_primary_rows == 700 and
  .five_query_rows == 875 and .successful_five_query_rows == 875 and
  .df_primary_query_ids == ["DF_NORMAL"] and .status == "PASS"
' "$FINAL_RQ1/EXECUTION_SUMMARY.json"

awk -F '\t' 'NR>1 {k=$1 SUBSEP $2; if (++seen[k]>1) exit 1; n++; if ($5!="OK") bad++}
  END {exit !(n==700 && bad==0)}' "$FINAL_RQ1/PRIMARY_RESULTS.tsv"
```

Generate the two CSV/LaTeX tables without executing the analyzer:

```bash
python3 "$KIT/scripts/report_rq1_tables.py" \
  --campaign "$FINAL_RQ1" --oracle "$ORACLE" \
  --capabilities "$KIT/A3_TARGET_CAPABILITIES.tsv" \
  --fixture-source "$PORT/tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs" \
  --out "$FINAL_TABLES"
```

This creates `RQ1_PER_QUERY.csv`, `RQ1_PER_QUERY.tex`,
`RQ1_GLOBAL_MICRO.csv`, `RQ1_GLOBAL_MICRO.tex`, the 700-row
`RQ1_CLASSIFICATION.tsv` and `RQ1_REPORT_PROVENANCE.json`. `DF_FULL` is never
included.

Compare the 696 historical common cells. The four candidate-only cells remain
explicit in `RQ1_MISSING_KEYS.tsv`:

```bash
python3 "$RECON/scripts/compare_rq1_branches.py" \
  --baseline-raw /mnt/wsx-storage/af/CREMA_CQPL_VALIDATED_INTEGRATION/RQ1_OPERATIONAL_CONFUSION_MATRIX.tsv \
  --baseline-classification /mnt/wsx-storage/af/CREMA_CQPL_VALIDATED_INTEGRATION/RQ1_OPERATIONAL_CONFUSION_MATRIX.tsv \
  --candidate-raw "$FINAL_RQ1/PRIMARY_RESULTS.tsv" \
  --candidate-classification "$FINAL_TABLES/RQ1_CLASSIFICATION.tsv" \
  --candidate-provenance "$FINAL_RQ1/PROVENANCE.json" \
  --baseline-policy evidence-qualified --candidate-policy evidence-qualified \
  --out "$FINAL_COMPARE"
```

Validate arithmetic and provenance:

```bash
awk -F, 'NR>1 {e+=$3; x+=$4; tp+=$5; fp+=$6; tn+=$7; fn+=$8}
  END {print e,x,tp,fp,tn,fn; exit !((e+x)==700 && (tp+fp+tn+fn)==e)}' \
  "$FINAL_TABLES/RQ1_PER_QUERY.csv"

test "$(wc -l < "$FINAL_TABLES/RQ1_GLOBAL_MICRO.csv")" -eq 2
jq -e '.expected_cells==700 and .df_primary=="DF_NORMAL" and
  .df_full_included==false and (.evaluable_cells+.excluded_cells)==700 and
  .policy=="evidence-qualified"' "$FINAL_TABLES/RQ1_REPORT_PROVENANCE.json"
sha256sum "$ORACLE" "$KIT/A3_TARGET_CAPABILITIES.tsv" \
  "$PORT/tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs"
```
