#!/usr/bin/env bash
set -euo pipefail

REPO="${1:-$HOME/Documenti/a-phd}"
TOOLCHAIN="${CQPL_RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$REPO/repro-results/ctl-semantic-laws-$STAMP"
mkdir -p "$OUT"

cd "$REPO"

{
  echo "schema=cqpl_ctl_semantic_laws_gate_v1"
  echo "git_head=$(git rev-parse HEAD)"
  echo "git_branch=$(git branch --show-current)"
  echo "rust_toolchain=$TOOLCHAIN"
} | tee "$OUT/MANIFEST.txt"

echo "[1/4] Static checks"
git diff --check
python3 - <<'PY'
from pathlib import Path
for p in [
    Path('cqpl/scripts/gate_ctl_semantic_laws.py'),
]:
    compile(p.read_text(), str(p), 'exec')
print('python syntax: OK')
PY

echo "[2/4] CQPL checker full test suite"
set +e
cargo "+$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml \
  2>&1 | tee "$OUT/CQPL_TESTS.log"
rc=${PIPESTATUS[0]}
set -e
if [[ $rc -ne 0 ]]; then
  echo "CQPL_CTL_SEMANTIC_LAWS_GATE: FAIL (cargo test)" >&2
  exit "$rc"
fi
if grep -q '^warning:' "$OUT/CQPL_TESTS.log"; then
  echo "CQPL_CTL_SEMANTIC_LAWS_GATE: FAIL (framework warning)" >&2
  grep '^warning:' "$OUT/CQPL_TESTS.log" >&2 || true
  exit 3
fi

echo "[3/4] Independent exhaustive three-state oracle"
python3 cqpl/scripts/gate_ctl_semantic_laws.py \
  --max-states 3 \
  --out "$OUT/CTL_SEMANTIC_LAWS_ORACLE.json" \
  | tee "$OUT/CTL_SEMANTIC_LAWS_ORACLE.console.log"

echo "[4/4] Gate summary"
python3 - "$OUT/CTL_SEMANTIC_LAWS_ORACLE.json" <<'PY' | tee "$OUT/SUMMARY.txt"
import json, sys
r=json.load(open(sys.argv[1]))
assert r['status']=='PASS'
assert r['max_states']==3
assert len(r['valid_law_checks'])==16
assert set(r['valid_law_checks'].values()) == {250785}
assert all(v['found'] for v in r['negative_controls'].values())
print('CQPL_CTL_SEMANTIC_LAWS_GATE: PASS')
print('oracle_binary_cases_per_law=250785')
print('positive_laws=16')
print('total_positive_law_checks=4012560')
print('rust_exhaustive_entry_configurations=2610')
print('negative_controls=', sorted(r['negative_controls']))
PY

echo "out=$OUT"
