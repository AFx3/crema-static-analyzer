#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$PWD}"
ROOT="$(cd "$ROOT" && pwd)"
cd "$ROOT"

TOOLCHAIN="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
BASE="$ROOT/cqpl/artifact/POST_R2_FINAL118"
ASSESSMENT_BASE="$ROOT/cqpl/artifact/GATE_L1_NORMAL_EXECUTION_FINAL118"
ORACLE="$ROOT/cqpl/artifact/FINAL112_SOURCE_ORACLE_AUDIT.tsv"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/gate-full13-double-free-$STAMP"
MAIN="$OUT/main"
EXP="$OUT/experimental"
REGISTRY="$OUT/registry"
CANONICAL_MATRIX="$OUT/matrix-canonical12"
EXPERIMENTAL_MATRIX="$OUT/matrix-double-free-normal"
SUBJECTS="$OUT/subjects.tsv"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
NEW_QUERY="$ROOT/cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl"

fail(){ echo "FULL13_CORPUS_GATE: FAIL: $*" >&2; exit 2; }

[[ -f "$NEW_QUERY" ]] || fail "missing experimental query: $NEW_QUERY"
[[ -f "$ROOT/cqpl/scripts/run_single_query_matrix.py" ]] || fail "missing run_single_query_matrix.py"
[[ -f "$ROOT/cqpl/scripts/validate_full13_double_free.py" ]] || fail "missing validate_full13_double_free.py"
[[ -f "$ROOT/cqpl/scripts/collect_warning_inventory.py" ]] || fail "missing collect_warning_inventory.py"
[[ -f "$BASE/QUERY_RESULTS_LONG.tsv" ]] || fail "missing truth baseline"
[[ -f "$ASSESSMENT_BASE/UNKNOWN_EXPLANATIONS.tsv" ]] || fail "missing assessment baseline"
[[ -f "$ORACLE" ]] || fail "missing source oracle"
grep -q 'all_candidate_drop_suffixes_exclude_repeated_drop' cqpl/cqpl_checker/src/explain.rs || fail "double-free normal-execution implementation not installed"

mkdir -p "$OUT"
printf '%s\n' \
  "root=$ROOT" \
  "toolchain=$TOOLCHAIN" \
  "git_head=$(git rev-parse HEAD)" \
  "git_branch=$(git branch --show-current)" \
  "truth_baseline=$BASE" \
  "assessment_baseline=$ASSESSMENT_BASE" \
  "experimental_query=$NEW_QUERY" \
  > "$OUT/environment.txt"
git status --short > "$OUT/git-status-before.txt"

printf '\n[1/11] Unit tests + checker build\n'
cargo +"$TOOLCHAIN" test --manifest-path crema/Cargo.toml 2>&1 | tee "$OUT/crema-tests.log"
cargo +"$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml 2>&1 | tee "$OUT/cqpl-tests.log"
cargo +"$TOOLCHAIN" build --manifest-path cqpl/cqpl_checker/Cargo.toml 2>&1 | tee "$OUT/cqpl-build.log"

printf '\n[2/11] Fresh main corpus (111 active targets)\n'
EXPECTED_MAIN="$BASE/EXPECTED_MAIN_TARGETS.txt"
RUNNER="$ROOT/cqpl/scripts/run_corpus_allocator_contracts.py"
python3 "$RUNNER" \
  --root "$ROOT" \
  --tests-root "$ROOT/tests_and_target_repos" \
  --out "$MAIN" \
  --schema-version 2 \
  --contract-capability v2 \
  --expected-targets "$EXPECTED_MAIN" \
  --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" \
  --toolchain "$TOOLCHAIN" \
  --skip no_errors_projects/openapi-client-gen \
  --require-discovered 112 \
  --require-active 111

printf '\n[3/11] Fresh experimental corpus (4 targets)\n'
EXPECTED_EXP="$BASE/EXPECTED_EXPERIMENTAL_TARGETS.txt"
python3 "$RUNNER" \
  --root "$ROOT" \
  --tests-root "$ROOT/test_and_target_repos_experimental" \
  --out "$EXP" \
  --schema-version 2 \
  --contract-capability v2 \
  --expected-targets "$EXPECTED_EXP" \
  --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" \
  --toolchain "$TOOLCHAIN" \
  --require-discovered 4 \
  --require-active 4

printf '\n[4/11] Fresh registry controls (3 crates)\n'
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$TOOLCHAIN" \
CQPL_REGISTRY_OUT="$REGISTRY" \
  bash cqpl/scripts/run_registry_crates_v6q_r1c.sh 2>&1 | tee "$OUT/registry-console.log"

printf '\n[5/11] Build exact 118-subject manifest\n'
: > "$SUBJECTS"
find "$MAIN/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 \
  | sort -z \
  | while IFS= read -r -d '' artifact; do
      printf 'corpus\t%s\t%s\n' "$(basename "$(dirname "$artifact")")" "$artifact"
    done >> "$SUBJECTS"
find "$EXP/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 \
  | sort -z \
  | while IFS= read -r -d '' artifact; do
      printf 'experimental\t%s\t%s\n' "$(basename "$(dirname "$artifact")")" "$artifact"
    done >> "$SUBJECTS"
for name in unicode-ident-1.0.18 ryu-1.0.20 memchr-2.7.4; do
  artifact="$REGISTRY/$name/analysis/annotated_icfg_v2.json"
  [[ -s "$artifact" ]] || fail "missing $artifact"
  printf 'registry\t%s\t%s\n' "$name" "$artifact" >> "$SUBJECTS"
done
[[ "$(wc -l < "$SUBJECTS")" -eq 118 ]] || fail "expected 118 subjects, got $(wc -l < "$SUBJECTS")"

echo "subjects=118"

printf '\n[6/11] Canonical 118 x 12 = 1416 matrix\n'
python3 cqpl/scripts/run_queries_v2_matrix.py \
  --checker "$CHECKER" \
  --queries "$ROOT/cqpl/queries_v2" \
  --subject-tsv "$SUBJECTS" \
  --out "$CANONICAL_MATRIX"

printf '\n[7/11] Canonical L1 + W1 regression gates\n'
python3 cqpl/scripts/gate_l1_normal_execution.py \
  --baseline "$BASE" \
  --matrix "$CANONICAL_MATRIX" \
  --oracle "$ORACLE" \
  --out "$OUT/gate-l1"
python3 cqpl/scripts/gate_w1_diagnostic_witness.py \
  --truth-baseline "$BASE" \
  --assessment-baseline "$ASSESSMENT_BASE" \
  --matrix "$CANONICAL_MATRIX" \
  --out "$OUT/gate-w1" \
  --expected-attempts 1416

printf '\n[8/11] Experimental 118 x 1 normal-execution double-free matrix\n'
python3 cqpl/scripts/run_single_query_matrix.py \
  --checker "$CHECKER" \
  --query "$NEW_QUERY" \
  --subject-tsv "$SUBJECTS" \
  --out "$EXPERIMENTAL_MATRIX"

printf '\n[9/11] Full 13-query semantic validation + UAF unwind audit\n'
python3 cqpl/scripts/validate_full13_double_free.py \
  --canonical-matrix "$CANONICAL_MATRIX" \
  --experimental-matrix "$EXPERIMENTAL_MATRIX" \
  --out "$OUT/full13-validation"

printf '\n[10/11] Warning inventory (no warning fixes in this run)\n'
python3 cqpl/scripts/collect_warning_inventory.py \
  --root "$OUT" \
  --out "$OUT/warnings"

printf '\n[11/11] Freeze manifests\n'
python3 - "$OUT" <<'PY'
from pathlib import Path
import hashlib,sys
root=Path(sys.argv[1])
manifest=root/'ARTIFACT_SHA256SUMS'
files=sorted(p for p in root.rglob('*') if p.is_file() and p != manifest)
with manifest.open('w') as f:
    for p in files:
        h=hashlib.sha256(p.read_bytes()).hexdigest()
        f.write(f"{h}  ./{p.relative_to(root).as_posix()}\n")
PY

git status --short > "$OUT/git-status-after.txt"

python3 - "$OUT/gate-l1/GATE_L1_NORMAL_EXECUTION.json" "$OUT/gate-w1/GATE_W1_DIAGNOSTIC_WITNESS.json" "$OUT/full13-validation/FULL13_DOUBLE_FREE_REPORT.json" <<'PY'
import json,sys
l1=json.load(open(sys.argv[1])); w1=json.load(open(sys.argv[2])); f13=json.load(open(sys.argv[3]))
assert l1['status']=='PASS', l1
assert w1['status']=='PASS', w1
assert f13['status']=='PASS', f13
print('\nFULL13_CORPUS_GATE: PASS')
print('  canonical_attempts=1416')
print('  experimental_attempts=118')
print('  combined_attempts=1534')
print('  experimental_truth_counts=', f13['experimental_truth_counts'])
print('  experimental_unknown_assessment_counts=', f13['experimental_unknown_assessment_counts'])
print('  uaf_positive_certificates_requiring_non_normal_edge=', f13['uaf_unwind_audit']['positive_certificates_requiring_non_normal_edge'])
print('  legacy_double_free_positive_certificates_requiring_non_normal_edge=', f13['legacy_double_free_unwind_audit']['positive_certificates_requiring_non_normal_edge'])
PY

echo "out=$OUT"
