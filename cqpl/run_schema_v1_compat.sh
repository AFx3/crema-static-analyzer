#!/usr/bin/env bash
set -euo pipefail

CQPL_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CREMA_PHD_ROOT:-$(cd "$CQPL_DIR/.." && pwd)}"
NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${CQPL_SCHEMA_V1_OUT:-$ROOT/repro-results/cqpl-schema-v1-compat-$STAMP}"
SKIP="no_errors_projects/openapi-client-gen"
CLEAN_FLAG=()
if [[ "${CQPL_CLEAN_TARGETS:-0}" == "1" ]]; then CLEAN_FLAG=(--clean-targets); fi

mkdir -p "$OUT"
echo "=== CQPL schema-v1 operational compatibility sweep ==="
echo "This is NOT a comparison against legacy detector labels or schema-v2 truth values."

python3 "$CQPL_DIR/scripts/run_corpus.py" \
  --root "$ROOT" \
  --out "$OUT" \
  --schema-version 1 \
  --expected-targets "$CQPL_DIR/artifact/EXPECTED_TARGETS_V6L.txt" \
  --target-config "$CQPL_DIR/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$CQPL_DIR/regression/reference/entry_overrides.json" \
  --toolchain "$NIGHTLY" \
  --skip "$SKIP" \
  --require-discovered 110 \
  --require-active 109 \
  "${CLEAN_FLAG[@]}"

python3 - "$OUT/results.json" <<'PY'
import json, sys
p=json.load(open(sys.argv[1]))
assert p['schema_version']==1
assert p['discovered_targets']==110
assert p['active_targets']==109
assert p['modeled_complete']==109
assert p['failures']==0
assert p['skipped_targets']==['no_errors_projects/openapi-client-gen']
assert p['queries']==['leak','double_free','use_after_free']
assert p['legacy_detector_comparison'] is False
assert p['historical_result_comparison'] is False
print('CQPL SCHEMA_V1 COMPATIBILITY: PASS active=109 complete=109')
PY

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS.FINAL -print0 \
    | sort -z \
    | xargs -0 sha256sum \
    | sed 's#  \./#  #' > SHA256SUMS.FINAL
)

echo "CQPL SCHEMA_V1 COMPATIBILITY SWEEP: PASS"
echo "evidence=$OUT"
