#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CQPL=$(cd "$SCRIPT_DIR/.." && pwd)
ROOT=${CREMA_PHD_ROOT:-$(cd "$CQPL/.." && pwd)}
NIGHTLY=${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}
BASE=${V6S_BASE:-$ROOT/repro-results/cqpl-v6q-r1c-final112}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=${V6S_OUT:-$ROOT/repro-results/cqpl-v6s-r1-validation-$STAMP}
FRESH="$OUT/fresh112"
EXPECTED_RUSTC='rustc 1.84.0-nightly (3fee0f12e 2024-11-20)'

fail() { echo "V6S_R1_VALIDATION: FAIL: $*" >&2; exit 1; }

[[ -f "$ROOT/crema/Cargo.toml" ]] || fail "v6S changes CREMA; validator must run from an installed repository tree"
[[ -f "$BASE/query-matrix/query-results-wide.tsv" ]] || fail "missing frozen baseline matrix: $BASE/query-matrix/query-results-wide.tsv"
[[ -f "$CQPL/run_all.sh" ]] || fail "missing $CQPL/run_all.sh"
mkdir -p "$OUT"

cat <<INFO
=== CREMA/CQPL v6S-r1 allocation disposition validation ===
root=$ROOT
cqpl=$CQPL
baseline=$BASE
out=$OUT
toolchain=$NIGHTLY
semantic_goal=old-12-query truth identity + new MAY disposition provenance
INFO

echo '=== static candidate boundary ==='
python3 "$CQPL/scripts/verify_v6s_r1_static.py" "$CQPL"

echo '=== pinned toolchain ==='
RUSTC_VERSION=$(rustup run "$NIGHTLY" rustc --version)
echo "$RUSTC_VERSION"
[[ "$RUSTC_VERSION" == "$EXPECTED_RUSTC" ]] || fail "unexpected rustc: $RUSTC_VERSION"
cargo +"$NIGHTLY" --version

echo '=== CREMA unit/regression tests ==='
CARGO_TARGET_DIR="$OUT/crema-test-target" \
  cargo +"$NIGHTLY" test --manifest-path "$ROOT/crema/Cargo.toml"

echo '=== fresh 112-subject producer + checker run ==='
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
CQPL_RUN_ALL_OUT="$FRESH" \
CQPL_CRATES_IO_OFFLINE="${CQPL_CRATES_IO_OFFLINE:-0}" \
  "$CQPL/run_all.sh"

[[ -f "$FRESH/query-matrix/query-results-wide.tsv" ]] || fail 'fresh matrix missing'
[[ -f "$FRESH/subjects.tsv" ]] || fail 'fresh subjects.tsv missing'

echo '=== allocation_disposition_v1 artifact boundary ==='
python3 - "$FRESH/subjects.tsv" <<'PY'
import json,sys
from pathlib import Path
rows=[]
for raw in Path(sys.argv[1]).read_text().splitlines():
    if not raw.strip() or raw.startswith('#'): continue
    group,target,path=raw.split('\t')
    rows.append((group,target,Path(path)))
assert len(rows)==112, len(rows)
record_total=0
for group,target,p in rows:
    d=json.loads(p.read_text())
    assert d.get('schema_version')==2, p
    assert 'allocation_disposition_v1' in set(d.get('capabilities',[])), p
    for n in d.get('nodes',[]):
        assert 'allocation_disposition' in n, (p,n.get('id'))
        assert isinstance(n['allocation_disposition'],list), (p,n.get('id'))
        record_total += len(n['allocation_disposition'])
print(f'V6S_R1_ARTIFACT_BOUNDARY: PASS subjects={len(rows)} disposition_records={record_total}')
PY

echo '=== frozen 112 x 12 result identity ==='
python3 - "$BASE/query-matrix/query-results-wide.tsv" "$FRESH/query-matrix/query-results-wide.tsv" "$OUT/result-identity.json" <<'PY'
import csv,json,sys
from pathlib import Path
base_path,fresh_path,out_path=map(Path,sys.argv[1:])
def load(p):
    with p.open(newline='') as f:
        rows=list(csv.DictReader(f,delimiter='\t'))
    d={(r['group'],r['target']):r for r in rows}
    assert len(rows)==112 and len(d)==112,(p,len(rows),len(d))
    return rows,d
brows,b=load(base_path); frows,f=load(fresh_path)
queries=[c for c in brows[0].keys() if c not in {'group','target'}]
assert len(queries)==12,queries
m=[]
for key in sorted(set(b)|set(f)):
    if key not in b or key not in f:
        m.append({'subject':key,'query':'<subject-set>','baseline':key in b,'fresh':key in f})
        continue
    for q in queries:
        if b[key][q]!=f[key][q]:
            m.append({'group':key[0],'target':key[1],'query':q,'baseline':b[key][q],'fresh':f[key][q]})
out={'schema':'cqpl_v6s_r1_result_identity_v1','subjects':112,'queries':12,'attempts':1344,'mismatches':m}
out_path.write_text(json.dumps(out,indent=2,sort_keys=True)+'\n')
if m:
    print('V6S_R1_RESULT_IDENTITY: FAIL mismatches=',len(m))
    for x in m[:30]: print(x)
    raise SystemExit(2)
print('V6S_R1_RESULT_IDENTITY: PASS subjects=112 queries=12 attempts=1344 mismatches=0')
PY

echo '=== disposition census + fixture invariants ==='
python3 "$CQPL/scripts/analyze_allocation_disposition.py" \
  --subjects "$FRESH/subjects.tsv" \
  --baseline-wide "$BASE/query-matrix/query-results-wide.tsv" \
  --out "$OUT/allocation-disposition-summary.json"

python3 - "$OUT/allocation-disposition-summary.json" <<'PY'
import json,sys
x=json.load(open(sys.argv[1]))
err=[]
if x.get('subjects')!=112: err.append('subjects != 112')
if x.get('leak_alloc_unknown_baseline')!=105: err.append('baseline leak unknown != 105')
fixtures=x.get('fixtures',{})
for target in ['boxed_bool__ml','clean_into_from_raw','drop_raw_ptr_no_free']:
    if target not in fixtures:
        err.append(f'missing fixture {target}')
        continue
    for name,value in fixtures[target].get('checks',{}).items():
        if not value: err.append(f'{target}: {name}=false')
if err:
    print('V6S_R1_FIXTURE_GATES: FAIL')
    for e in err: print(' -',e)
    raise SystemExit(3)
print('V6S_R1_FIXTURE_GATES: PASS')
print('boxed_bool__ml=box_into_raw observed; no box_from_raw')
print('clean_into_from_raw=box_into_raw + box_from_raw observed')
print('drop_raw_ptr_no_free=raw_pointer_drop_noop; no drop_l/may_deallocate/FREED at raw-drop node')
PY

echo '=== v6R explainability compatibility on fresh graph results ==='
# The ordinary truth matrix is the semantic compatibility gate.  v6S does not
# require old v6R explanation JSON to be byte-identical because node artifacts
# now carry an additional validated field that explain.rs intentionally ignores.

cat <<INFO
V6S_R1_VALIDATION: PASS
subjects=112 queries=12 attempts=1344 result_mismatches=0
allocation_disposition_v1=present-on-112/112
leak_unknown_baseline=105
raw_pointer_mem_drop=pointee-noop-validated
old_query_semantics=unchanged
summary=$OUT/allocation-disposition-summary.json
result_identity=$OUT/result-identity.json
fresh_run=$FRESH
INFO
