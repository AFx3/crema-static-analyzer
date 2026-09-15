#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=${CREMA_PHD_ROOT:-/home/af/Documenti/a-phd}
NIGHTLY=${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}
BASE=${V6R_BASE:-$ROOT/repro-results/cqpl-v6q-r1c-final112}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${V6R_OUT:-$ROOT/repro-results/cqpl-v6r-r1-validation-$STAMP}

# The same validator works both from the extracted candidate root and, after
# installation, from cqpl/scripts/ inside the repository.
if [[ -d "$SCRIPT_DIR/cqpl" && -f "$SCRIPT_DIR/CANDIDATE_SHA256SUMS" ]]; then
    MODE=candidate-package
    PACKAGE="$SCRIPT_DIR"
    CQPL="$SCRIPT_DIR/cqpl"
elif [[ -f "$SCRIPT_DIR/../VERSION" && -d "$SCRIPT_DIR/../cqpl_checker" ]]; then
    MODE=installed-cqpl
    CQPL=$(cd "$SCRIPT_DIR/.." && pwd)
    PACKAGE=$(cd "$CQPL/.." && pwd)
else
    echo "cannot locate CQPL tree from validator path: $SCRIPT_DIR" >&2
    exit 10
fi

mkdir -p "$OUT"

echo "=== CREMA/CQPL v6R-r1 validation ==="
echo "mode=$MODE"
echo "cqpl=$CQPL"
echo "root=$ROOT"
echo "baseline=$BASE"
echo "out=$OUT"
echo "toolchain=$NIGHTLY"

if [[ "$MODE" == candidate-package ]]; then
    (cd "$PACKAGE" && sha256sum -c CANDIDATE_SHA256SUMS)
fi

echo "=== static observational boundary ==="
python3 "$CQPL/scripts/verify_v6r_static.py" "$CQPL"

echo "=== pinned toolchain ==="
rustup run "$NIGHTLY" rustc --version
cargo +"$NIGHTLY" --version

if [[ ! -f "$BASE/subjects.tsv" ]]; then
    echo "missing frozen subjects.tsv: $BASE/subjects.tsv" >&2
    exit 20
fi
if [[ ! -f "$BASE/query-matrix/query-results-wide.tsv" ]]; then
    echo "missing frozen query matrix: $BASE/query-matrix/query-results-wide.tsv" >&2
    exit 21
fi

# Rebase subject artifact paths when a frozen run was moved/extracted elsewhere.
REBASED="$OUT/subjects.rebased.tsv"
python3 - "$BASE/subjects.tsv" "$BASE" "$REBASED" <<'PY'
from pathlib import Path
import sys
src=Path(sys.argv[1]); base=Path(sys.argv[2]).resolve(); out=Path(sys.argv[3])
rows=[]; missing=[]
marker='/repro-results/cqpl-v6q-r1c-final112/'
for lineno,raw in enumerate(src.read_text().splitlines(),1):
    if not raw.strip() or raw.startswith('#'):
        continue
    parts=raw.split('\t')
    if len(parts)!=3:
        raise SystemExit(f'bad subjects row {lineno}: {raw!r}')
    group,name,path=parts
    p=Path(path)
    if not p.is_file():
        text=str(p)
        if marker in text:
            p=base/text.split(marker,1)[1]
    if not p.is_file():
        missing.append((group,name,str(p)))
    rows.append((group,name,str(p.resolve())))
if len(rows)!=112:
    raise SystemExit(f'expected 112 subjects, got {len(rows)}')
if missing:
    for row in missing[:20]: print('MISSING_SUBJECT',*row,sep='\t')
    raise SystemExit(f'{len(missing)} subject artifacts are missing')
out.write_text(''.join('\t'.join(r)+'\n' for r in rows))
print(f'V6R_SUBJECT_REBASE: PASS subjects={len(rows)}')
PY

export CARGO_TARGET_DIR="$OUT/cargo-target"

echo "=== checker tests ==="
cargo +"$NIGHTLY" test --manifest-path "$CQPL/cqpl_checker/Cargo.toml"

echo "=== checker build ==="
cargo +"$NIGHTLY" build --manifest-path "$CQPL/cqpl_checker/Cargo.toml"
CHECKER="$CARGO_TARGET_DIR/debug/cqpl_checker"
test -x "$CHECKER"

EXPLAIN_OUT="$OUT/explainability"
echo "=== 112 x 12 observational explanation matrix ==="
python3 "$CQPL/scripts/run_explainability_matrix.py" \
  --checker "$CHECKER" \
  --queries "$CQPL/queries_v2" \
  --subject-tsv "$REBASED" \
  --baseline-wide "$BASE/query-matrix/query-results-wide.tsv" \
  --out "$EXPLAIN_OUT"

echo "=== leak frontier audit ==="
python3 "$CQPL/scripts/analyze_leak_unknowns.py" \
  --long "$EXPLAIN_OUT/explainability-long.tsv" \
  --out "$EXPLAIN_OUT/leak-unknown-summary.json"

echo "=== reviewed-oracle precision metrics ==="
python3 "$CQPL/scripts/compute_precision_metrics.py" \
  --oracle-audit "$CQPL/artifact/FINAL112_SOURCE_ORACLE_AUDIT.tsv" \
  --matrix "$BASE/query-matrix/query-results-wide.tsv" \
  --out "$EXPLAIN_OUT/precision-metrics.json"

python3 - "$EXPLAIN_OUT/explainability-summary.json" "$EXPLAIN_OUT/leak-unknown-summary.json" "$EXPLAIN_OUT/precision-metrics.json" <<'PY'
import json,sys
summary=json.load(open(sys.argv[1]))
leak=json.load(open(sys.argv[2]))
metrics=json.load(open(sys.argv[3]))
errors=[]
if summary.get('attempts_completed') != 1344: errors.append('attempts_completed != 1344')
for key in ['failures','baseline_result_mismatches','unknown_without_reason_frontier','unknown_without_specific_origin','true_without_witness','true_without_atomic_witness']:
    if summary.get(key): errors.append(f'{key} non-empty')
for q in ['leak_alloc','leak_alloc_state']:
    x=leak['queries'][q]
    if x.get('subjects') != 112: errors.append(f'{q}: subjects != 112')
    if x.get('unknown') != 105: errors.append(f'{q}: unknown != 105')
    if not x.get('all_unknown_have_may_allocation'): errors.append(f'{q}: MAY_ALLOCATION frontier != 105/105')
expected={'ML':33,'DF':29,'UAF':22,'UB_FFI':18}
for family,count in expected.items():
    x=metrics['families'][family]
    if x.get('reviewed_positive') != count: errors.append(f'{family}: reviewed_positive != {count}')
    if x.get('unexpected_ff_on_reviewed_positive') != 0: errors.append(f'{family}: unexpected_ff != 0')
    if x.get('vulnerable_non_refutation_rate') != 1.0: errors.append(f'{family}: non_refutation != 1')
if errors:
    print('V6R_R1_VALIDATION: FAIL')
    for e in errors: print(' -',e)
    raise SystemExit(30)
print('V6R_R1_VALIDATION: PASS')
print('attempts=1344 result_mismatches=0')
print('leak_unknown=105/112 per query; MAY_ALLOCATION_frontier=105/105')
print('reviewed_positive_nonrefutation=ML33/33 DF29/29 UAF22/22 UB_FFI18/18')
PY

echo "evidence=$OUT"
