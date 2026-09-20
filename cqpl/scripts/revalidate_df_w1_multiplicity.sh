#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:?usage: $0 REPO EXISTING_FULL13_RESULTS}"
SRC="${2:?usage: $0 REPO EXISTING_FULL13_RESULTS}"
ROOT="$(cd "$ROOT" && pwd)"
SRC="$(cd "$SRC" && pwd)"
TOOLCHAIN="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
BASE="$ROOT/cqpl/artifact/POST_R2_FINAL118"
ASSESSMENT_BASE="$ROOT/cqpl/artifact/GATE_L1_NORMAL_EXECUTION_FINAL118"
ORACLE="$ROOT/cqpl/artifact/FINAL112_SOURCE_ORACLE_AUDIT.tsv"
QUERY="$ROOT/cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl"
SUBJECTS="$SRC/subjects.tsv"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/revalidate-df-w1-multiplicity-$STAMP"
CANON="$OUT/matrix-canonical12"
EXP="$OUT/matrix-double-free-normal"
mkdir -p "$OUT"

[[ -s "$SUBJECTS" ]] || { echo "missing subjects.tsv: $SUBJECTS" >&2; exit 2; }
[[ "$(wc -l < "$SUBJECTS")" -eq 118 ]] || { echo "subjects.tsv is not 118 rows" >&2; exit 2; }

cd "$ROOT"
echo '[1/5] CQPL tests + build'
cargo +"$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml
cargo +"$TOOLCHAIN" build --manifest-path cqpl/cqpl_checker/Cargo.toml

echo '[2/5] Re-evaluate canonical 118 x 12 on existing fresh ICFGs'
python3 cqpl/scripts/run_queries_v2_matrix.py \
  --checker "$CHECKER" \
  --queries "$ROOT/cqpl/queries_v2" \
  --subject-tsv "$SUBJECTS" \
  --out "$CANON"

echo '[3/5] L1 + W1 exact freeze checks'
python3 cqpl/scripts/gate_l1_normal_execution.py \
  --baseline "$BASE" --matrix "$CANON" --oracle "$ORACLE" --out "$OUT/gate-l1"
python3 cqpl/scripts/gate_w1_diagnostic_witness.py \
  --truth-baseline "$BASE" --assessment-baseline "$ASSESSMENT_BASE" \
  --matrix "$CANON" --out "$OUT/gate-w1" --expected-attempts 1416

echo '[4/5] Re-evaluate experimental query + full13 audit'
python3 cqpl/scripts/run_single_query_matrix.py \
  --checker "$CHECKER" --query "$QUERY" --subject-tsv "$SUBJECTS" --out "$EXP"
python3 cqpl/scripts/validate_full13_double_free.py \
  --canonical-matrix "$CANON" --experimental-matrix "$EXP" --out "$OUT/full13-validation"

echo '[5/5] Assert frozen W1 diagnostic multiplicity'
python3 - "$OUT/gate-w1/GATE_W1_DIAGNOSTIC_WITNESS.json" "$CANON" "$OUT/full13-validation/FULL13_DOUBLE_FREE_REPORT.json" <<'PY'
import json,sys
from pathlib import Path
w1=json.load(open(sys.argv[1]))
canon=Path(sys.argv[2])
f13=json.load(open(sys.argv[3]))
assert w1['status']=='PASS', w1
expected={
 'certifiable_findings':469,
 'diagnostic_certificates':469,
 'directional_findings':456,
 'directional_certificates':456,
 'non_directional_findings':13,
 'non_directional_certificates':13,
}
for k,v in expected.items(): assert w1.get(k)==v,(k,w1.get(k),v)
assert w1['source_status_counts']=={'grounded':1360,'source_unavailable':2},w1['source_status_counts']
repeated=0
for p in canon.glob('results/*/*.explain.json'):
    d=json.load(open(p))
    repeated += sum(f.get('kind')=='repeated_drop_without_reallocation' for key in ('supporting_findings','refuting_findings') for f in d.get(key,[]))
assert repeated==190, repeated
assert f13['status']=='PASS',f13
assert f13['truth_equivalence']['mismatch_count']==0,f13['truth_equivalence']
assert f13['uaf_unwind_audit']['positive_certificates_requiring_non_normal_edge']==0,f13['uaf_unwind_audit']
print(json.dumps({
 'status':'PASS',
 'w1_certificates':w1['diagnostic_certificates'],
 'repeated_drop_findings':repeated,
 'source_status_counts':w1['source_status_counts'],
 'experimental_unknown_assessment_counts':f13['experimental_unknown_assessment_counts'],
 'uaf_non_normal_positive_certificates':f13['uaf_unwind_audit']['positive_certificates_requiring_non_normal_edge'],
},indent=2))
PY

echo "DF_W1_MULTIPLICITY_REVALIDATION: PASS out=$OUT"
