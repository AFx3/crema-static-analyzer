#!/usr/bin/env bash
set -euo pipefail

CQPL_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CREMA_PHD_ROOT:-$(cd "$CQPL_DIR/.." && pwd)}"
NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${CQPL_RUN_ALL_OUT:-$ROOT/repro-results/cqpl-run-all-v6n-r1a-$STAMP}"
SKIP="no_errors_projects/openapi-client-gen"

RUNNER="$CQPL_DIR/scripts/run_corpus_allocator_contracts.py"
EXPECTED="$CQPL_DIR/artifact/EXPECTED_TARGETS_V6L.txt"
TARGET_CONFIG="$CQPL_DIR/artifact/TARGET_ANALYSIS_CONFIG.tsv"
ENTRY_OVERRIDES="$CQPL_DIR/regression/reference/entry_overrides.json"
V2_MISMATCH_QUERY="$CQPL_DIR/queries_v2/allocator_mismatch_ub_v2.cqpl"

CLEAN_FLAG=()
if [[ "${CQPL_CLEAN_TARGETS:-0}" == "1" ]]; then
  CLEAN_FLAG=(--clean-targets)
fi

fail() {
  echo "CQPL RUN_ALL v6N-r1a FAIL: $*" >&2
  exit 1
}

command -v cargo >/dev/null || fail "cargo not found"
command -v rustc >/dev/null || fail "rustc not found"
command -v python3 >/dev/null || fail "python3 not found"

[[ -d "$ROOT/tests_and_target_repos" ]] || fail "missing $ROOT/tests_and_target_repos"
[[ -f "$RUNNER" ]] || fail "missing v6N corpus runner: $RUNNER"
[[ -f "$EXPECTED" ]] || fail "missing frozen corpus snapshot: $EXPECTED"
[[ -f "$TARGET_CONFIG" ]] || fail "missing target config: $TARGET_CONFIG"
[[ -f "$ENTRY_OVERRIDES" ]] || fail "missing entry_overrides.json"
[[ -f "$V2_MISMATCH_QUERY" ]] || fail "missing allocator_contracts_v2 query: $V2_MISMATCH_QUERY"

# Fail closed if the current official allocator-mismatch query was accidentally
# reverted to v1.
grep -Eq '^[[:space:]]*requires[[:space:]]+allocation_contracts_v2[[:space:]]*;' \
  "$V2_MISMATCH_QUERY" \
  || fail "allocator_mismatch_ub_v2.cqpl does not require allocation_contracts_v2"

RUSTC_VERSION="$(rustc +"$NIGHTLY" --version)"
echo "$RUSTC_VERSION" | grep -F 'rustc 1.84.0-nightly (3fee0f12e 2024-11-20)' >/dev/null \
  || fail "unexpected pinned rustc: $RUSTC_VERSION"

mkdir -p "$OUT"
rustc +"$NIGHTLY" --version --verbose > "$OUT/rustc-version.txt"

cat <<INFO
=== CQPL run_all: CREMA/CQPL v6N-r1a ===
mode=schema-v2 allocation_state_v1 + allocation_contracts_v2
root=$ROOT
out=$OUT
toolchain=$NIGHTLY
skip=$SKIP
corpus_snapshot=EXPECTED_TARGETS_V6L.txt (intentionally unchanged 110/109 census)
allocator_mismatch_query=queries_v2/allocator_mismatch_ub_v2.cqpl
legacy comparison=DISABLED
historical result comparison=DISABLED
INFO

python3 "$RUNNER" \
  --root "$ROOT" \
  --out "$OUT" \
  --schema-version 2 \
  --contract-capability v2 \
  --expected-targets "$EXPECTED" \
  --target-config "$TARGET_CONFIG" \
  --entry-overrides "$ENTRY_OVERRIDES" \
  --toolchain "$NIGHTLY" \
  --skip "$SKIP" \
  --require-discovered 110 \
  --require-active 109 \
  "${CLEAN_FLAG[@]}"

# Every successfully exported schema-v2 artifact in this current run must
# advertise both the inherited v1 capability and the v2 refinement.
python3 - "$OUT" <<'PY'
import json
import sys
from pathlib import Path

out = Path(sys.argv[1])
arts = sorted(out.glob("raw/*/annotated_icfg_v2.json"))
if len(arts) != 109:
    raise SystemExit(f"expected 109 annotated schema-v2 artifacts, found {len(arts)}")

errors = []
for p in arts:
    d = json.loads(p.read_text())
    caps = set(d.get("capabilities", []))
    missing = {"allocation_state_v1", "allocation_contracts_v1", "allocation_contracts_v2"} - caps
    if missing:
        errors.append(f"{p.parent.name}: missing capabilities {sorted(missing)}")

if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)

print("V6N_R1A_ARTIFACT_CAPABILITIES: PASS artifacts=109")
PY

python3 "$CQPL_DIR/scripts/render_ae_table.py" \
  "$OUT/results.json" \
  --full "$OUT/artifact-evaluation-table.tex" \
  --summary "$OUT/artifact-evaluation-summary.tex"

python3 - "$OUT/results.json" <<'PY'
import json
import sys

p = json.load(open(sys.argv[1]))

assert p["schema_version"] == 2
assert p["discovered_targets"] == 110
assert p["active_targets"] == 109
assert p["modeled_complete"] == 109
assert p["failures"] == 0
assert p["skipped_targets"] == ["no_errors_projects/openapi-client-gen"]
assert p["legacy_detector_comparison"] is False
assert p["historical_result_comparison"] is False
assert p["queries"] == [
    "leak",
    "double_free",
    "use_after_free",
    "allocator_mismatch",
]

print("CQPL RUN_ALL v6N-r1a SUMMARY: PASS active=109 complete=109 skipped=1 queries=4")
PY

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS.FINAL -print0 \
    | sort -z \
    | xargs -0 sha256sum \
    | sed 's#  \./#  #' > SHA256SUMS.FINAL
  sha256sum -c SHA256SUMS.FINAL
)

echo "CQPL RUN_ALL v6N-r1a: PASS"
echo "results=$OUT/results.json"
echo "status=$OUT/status.tsv"
echo "latex_full=$OUT/artifact-evaluation-table.tex"
echo "latex_summary=$OUT/artifact-evaluation-summary.tex"
echo "evidence=$OUT"
