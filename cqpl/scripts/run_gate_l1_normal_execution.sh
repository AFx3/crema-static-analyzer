#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-$PWD}"; ROOT="$(cd "$ROOT" && pwd)"; cd "$ROOT"
TOOLCHAIN="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
BASE="$ROOT/cqpl/artifact/POST_R2_FINAL118"
ORACLE="$ROOT/cqpl/artifact/FINAL112_SOURCE_ORACLE_AUDIT.tsv"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/gate-l1-normal-execution-$STAMP"
MAIN="$OUT/main"; EXP="$OUT/experimental"; REGISTRY="$OUT/registry"; MATRIX="$OUT/query-matrix"; SUBJECTS="$OUT/subjects.tsv"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
fail(){ echo "GATE_L1_NORMAL_EXECUTION: FAIL: $*" >&2; exit 2; }

[[ -f "$BASE/QUERY_RESULTS_LONG.tsv" ]] || fail "missing POST-R2 baseline freeze"
[[ -f "$ORACLE" ]] || fail "missing source oracle"
grep -Fx 'requires typed_edge_flow_v1;' cqpl/queries_v2/leak_alloc_state.cqpl >/dev/null || fail "leak_alloc_state lacks typed_edge_flow_v1 requirement"
grep -Fx 'assessment_scope normal_execution;' cqpl/queries_v2/leak_alloc_state.cqpl >/dev/null || fail "leak_alloc_state lacks normal assessment scope"

cargo +"$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml
cargo +"$TOOLCHAIN" build --manifest-path cqpl/cqpl_checker/Cargo.toml

# Focused fresh source-to-query controls first.
for spec in \
  'clean:a-code_full_rust/clean_alloc_read_and_drop' \
  'boxed:a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool'
do
  tag="${spec%%:*}"; target="${spec#*:}"; dir="$OUT/focus-$tag"
  python3 cqpl/scripts/run_one_target_v6q_r1c.py --root "$ROOT" --relative-path "$target" --out "$dir" --toolchain "$TOOLCHAIN"
  "$CHECKER" "$dir/annotated_icfg_v2.json" cqpl/queries_v2/leak_alloc_state.cqpl --json > "$OUT/focus-$tag.json"
done
python3 - "$OUT/focus-clean.json" "$OUT/focus-boxed.json" <<'PY'
import json,sys
c=json.load(open(sys.argv[1])); b=json.load(open(sys.argv[2]))
assert c['result']=='unk' and c['assessment']['subresult']=='unk_false', c
assert 'assessment_scope:normal_execution' in c['assessment']['basis'], c
assert b['result']=='unk' and b['assessment']['subresult']=='unk_true', b
assert 'assessment_scope:normal_execution' in b['assessment']['basis'], b
print('FOCUSED: PASS clean=unk_false boxed_bool__ml=unk_true truth=unk/unk')
PY

EXPECTED_MAIN="$BASE/EXPECTED_MAIN_TARGETS.txt"; EXPECTED_EXP="$BASE/EXPECTED_EXPERIMENTAL_TARGETS.txt"
RUNNER="$ROOT/cqpl/scripts/run_corpus_allocator_contracts.py"
python3 "$RUNNER" --root "$ROOT" --tests-root "$ROOT/tests_and_target_repos" --out "$MAIN" --schema-version 2 --contract-capability v2 --expected-targets "$EXPECTED_MAIN" --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" --toolchain "$TOOLCHAIN" --skip no_errors_projects/openapi-client-gen --require-discovered 112 --require-active 111
python3 "$RUNNER" --root "$ROOT" --tests-root "$ROOT/test_and_target_repos_experimental" --out "$EXP" --schema-version 2 --contract-capability v2 --expected-targets "$EXPECTED_EXP" --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" --toolchain "$TOOLCHAIN" --require-discovered 4 --require-active 4
CREMA_PHD_ROOT="$ROOT" CREMA_RUST_TOOLCHAIN="$TOOLCHAIN" CQPL_REGISTRY_OUT="$REGISTRY" bash cqpl/scripts/run_registry_crates_v6q_r1c.sh

: > "$SUBJECTS"
find "$MAIN/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 | sort -z | while IFS= read -r -d '' artifact; do printf 'corpus\t%s\t%s\n' "$(basename "$(dirname "$artifact")")" "$artifact"; done >> "$SUBJECTS"
find "$EXP/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 | sort -z | while IFS= read -r -d '' artifact; do printf 'experimental\t%s\t%s\n' "$(basename "$(dirname "$artifact")")" "$artifact"; done >> "$SUBJECTS"
for name in unicode-ident-1.0.18 ryu-1.0.20 memchr-2.7.4; do artifact="$REGISTRY/$name/analysis/annotated_icfg_v2.json"; [[ -s "$artifact" ]] || fail "missing $artifact"; printf 'registry\t%s\t%s\n' "$name" "$artifact" >> "$SUBJECTS"; done
[[ "$(wc -l < "$SUBJECTS")" -eq 118 ]] || fail "expected 118 subjects"

python3 cqpl/scripts/run_queries_v2_matrix.py --checker "$CHECKER" --queries "$ROOT/cqpl/queries_v2" --subject-tsv "$SUBJECTS" --out "$MATRIX"
python3 cqpl/scripts/gate_l1_normal_execution.py --baseline "$BASE" --matrix "$MATRIX" --oracle "$ORACLE" --out "$OUT/gate"
echo "GATE_L1_NORMAL_EXECUTION: PASS out=$OUT"
