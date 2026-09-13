#!/usr/bin/env bash
set -euo pipefail

CQPL_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CREMA_PHD_ROOT:-$(cd "$CQPL_DIR/.." && pwd)}"
NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${CQPL_RUN_ALL_OUT:-$ROOT/repro-results/cqpl-run-all-v6l-r8-$STAMP}"
SKIP="no_errors_projects/openapi-client-gen"
CLEAN_FLAG=()
if [[ "${CQPL_CLEAN_TARGETS:-0}" == "1" ]]; then CLEAN_FLAG=(--clean-targets); fi

fail(){ echo "CQPL RUN_ALL FAIL: $*" >&2; exit 1; }
command -v cargo >/dev/null || fail "cargo not found"
command -v python3 >/dev/null || fail "python3 not found"
[[ -d "$ROOT/tests_and_target_repos" ]] || fail "missing $ROOT/tests_and_target_repos"
[[ -f "$CQPL_DIR/regression/reference/entry_overrides.json" ]] || fail "missing entry_overrides.json"

mkdir -p "$OUT"
echo "=== CQPL run_all: schema-v2 corpus over active 109 (runner r8) ==="
echo "root=$ROOT"
echo "out=$OUT"
echo "toolchain=$NIGHTLY"
echo "skip=$SKIP"
echo "legacy comparison=DISABLED"
echo "historical result comparison=DISABLED"

python3 "$CQPL_DIR/scripts/run_corpus.py" \
  --root "$ROOT" \
  --out "$OUT" \
  --schema-version 2 \
  --expected-targets "$CQPL_DIR/artifact/EXPECTED_TARGETS_V6L.txt" \
  --target-config "$CQPL_DIR/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$CQPL_DIR/regression/reference/entry_overrides.json" \
  --toolchain "$NIGHTLY" \
  --skip "$SKIP" \
  --require-discovered 110 \
  --require-active 109 \
  "${CLEAN_FLAG[@]}"

python3 "$CQPL_DIR/scripts/render_ae_table.py" \
  "$OUT/results.json" \
  --full "$OUT/artifact-evaluation-table.tex" \
  --summary "$OUT/artifact-evaluation-summary.tex"

python3 - "$OUT/results.json" <<'PY'
import json, sys
p=json.load(open(sys.argv[1]))
assert p['discovered_targets']==110
assert p['active_targets']==109
assert p['modeled_complete']==109
assert p['failures']==0
assert p['skipped_targets']==['no_errors_projects/openapi-client-gen']
assert p['legacy_detector_comparison'] is False
assert p['historical_result_comparison'] is False
print('CQPL RUN_ALL SUMMARY: PASS active=109 complete=109 skipped=1')
PY

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS.FINAL -print0 \
    | sort -z \
    | xargs -0 sha256sum \
    | sed 's#  \./#  #' > SHA256SUMS.FINAL
)

echo "CQPL RUN_ALL: PASS"
echo "results=$OUT/results.json"
echo "status=$OUT/status.tsv"
echo "latex_full=$OUT/artifact-evaluation-table.tex"
echo "latex_summary=$OUT/artifact-evaluation-summary.tex"
echo "evidence=$OUT"
