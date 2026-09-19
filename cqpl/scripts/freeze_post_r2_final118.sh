#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$PWD}"
ROOT="$(cd "$ROOT" && pwd)"
cd "$ROOT"

TOOLCHAIN="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
FREEZE_DIR="$ROOT/cqpl/artifact/POST_R2_FINAL118"
EXPECTED_MAIN="$FREEZE_DIR/EXPECTED_MAIN_TARGETS.txt"
EXPECTED_EXP="$FREEZE_DIR/EXPECTED_EXPERIMENTAL_TARGETS.txt"
QUERY_MANIFEST="$FREEZE_DIR/QUERY_SHA256SUMS"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/post-r2-final118-$STAMP"
MAIN="$OUT/main"
EXP="$OUT/experimental"
REGISTRY="$OUT/registry"
MATRIX="$OUT/query-matrix"
SUBJECTS="$OUT/subjects.tsv"
RUNNER="$ROOT/cqpl/scripts/run_corpus_allocator_contracts.py"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"

fail() { echo "POST_R2_FINAL118: FAIL: $*" >&2; exit 2; }

[[ -f "$RUNNER" ]] || fail "missing runner $RUNNER"
python3 "$RUNNER" --help | grep -q -- '--tests-root' \
  || fail "runner lacks --tests-root; install corrected runner first"
[[ -d "$ROOT/tests_and_target_repos" ]] || fail "missing tests_and_target_repos"
[[ -d "$ROOT/test_and_target_repos_experimental" ]] || fail "missing test_and_target_repos_experimental"
[[ -f "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" ]] || fail "missing TARGET_ANALYSIS_CONFIG.tsv"
[[ -f "$ROOT/cqpl/regression/reference/entry_overrides.json" ]] || fail "missing entry_overrides.json"

rm -rf "$FREEZE_DIR"
mkdir -p "$FREEZE_DIR" "$OUT"

# -----------------------------------------------------------------------------
# Gate 1: freeze exact target snapshots and unchanged query set.
# -----------------------------------------------------------------------------
python3 - "$ROOT/tests_and_target_repos" "$EXPECTED_MAIN" <<'PY'
from pathlib import Path
import sys

def discover(tests: Path):
    tests = tests.resolve()
    manifests=[]
    for p in tests.rglob('Cargo.toml'):
        rel=p.relative_to(tests).parts
        if 'target' in rel or '.git' in rel or any(x.startswith('.') for x in rel):
            continue
        manifests.append(p)
    manifest_dirs={p.parent.resolve() for p in manifests}
    roots=[]
    for manifest in manifests:
        d=manifest.parent.resolve(); anc=d.parent; nested=False
        while anc != tests and tests in anc.parents:
            if anc in manifest_dirs:
                nested=True; break
            anc=anc.parent
        if not nested:
            roots.append(d)
    return sorted({p.relative_to(tests).as_posix() for p in roots})

root=Path(sys.argv[1]); out=Path(sys.argv[2])
rows=discover(root)
assert len(rows)==112, len(rows)
assert 'a-code_full_rust/clean_alloc_read_and_drop' in rows
assert 'a-code_c_ffi/uaf-df-mail-lillo' in rows
out.write_text('\n'.join(rows)+'\n', encoding='utf-8')
print(f'MAIN_SNAPSHOT: PASS discovered={len(rows)}')
PY

python3 - "$ROOT/test_and_target_repos_experimental" "$EXPECTED_EXP" <<'PY'
from pathlib import Path
import sys

def discover(tests: Path):
    tests = tests.resolve()
    manifests=[]
    for p in tests.rglob('Cargo.toml'):
        rel=p.relative_to(tests).parts
        if 'target' in rel or '.git' in rel or any(x.startswith('.') for x in rel):
            continue
        manifests.append(p)
    manifest_dirs={p.parent.resolve() for p in manifests}
    roots=[]
    for manifest in manifests:
        d=manifest.parent.resolve(); anc=d.parent; nested=False
        while anc != tests and tests in anc.parents:
            if anc in manifest_dirs:
                nested=True; break
            anc=anc.parent
        if not nested:
            roots.append(d)
    return sorted({p.relative_to(tests).as_posix() for p in roots})

root=Path(sys.argv[1]); out=Path(sys.argv[2])
rows=discover(root)
expected={
    'bmulti/bmulti_clean_two_args_ffi',
    'bmulti/bmulti_df_second_ffi',
    'bmulti/bmulti_leak_second_ffi',
    'bmulti/bmulti_uaf_second_ffi',
}
assert len(rows)==4, rows
assert set(rows)==expected, (rows, expected)
out.write_text('\n'.join(rows)+'\n', encoding='utf-8')
print('EXPERIMENTAL_SNAPSHOT: PASS discovered=4')
PY

python3 "$RUNNER" \
  --root "$ROOT" \
  --tests-root "$ROOT/tests_and_target_repos" \
  --out /tmp/post-r2-final118-main-census \
  --schema-version 2 \
  --contract-capability v2 \
  --expected-targets "$EXPECTED_MAIN" \
  --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" \
  --toolchain "$TOOLCHAIN" \
  --skip no_errors_projects/openapi-client-gen \
  --require-discovered 112 \
  --require-active 111 \
  --list-targets \
  > "$FREEZE_DIR/MAIN_CENSUS.txt"

grep -Fx $'TOTAL_DISCOVERED\t112' "$FREEZE_DIR/MAIN_CENSUS.txt" >/dev/null
grep -Fx $'TOTAL_ACTIVE\t111' "$FREEZE_DIR/MAIN_CENSUS.txt" >/dev/null
grep -Fx $'TOTAL_SKIPPED\t1' "$FREEZE_DIR/MAIN_CENSUS.txt" >/dev/null

python3 "$RUNNER" \
  --root "$ROOT" \
  --tests-root "$ROOT/test_and_target_repos_experimental" \
  --out /tmp/post-r2-final118-exp-census \
  --schema-version 2 \
  --contract-capability v2 \
  --expected-targets "$EXPECTED_EXP" \
  --target-config "$ROOT/cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv" \
  --entry-overrides "$ROOT/cqpl/regression/reference/entry_overrides.json" \
  --toolchain "$TOOLCHAIN" \
  --require-discovered 4 \
  --require-active 4 \
  --list-targets \
  > "$FREEZE_DIR/EXPERIMENTAL_CENSUS.txt"

grep -Fx $'TOTAL_DISCOVERED\t4' "$FREEZE_DIR/EXPERIMENTAL_CENSUS.txt" >/dev/null
grep -Fx $'TOTAL_ACTIVE\t4' "$FREEZE_DIR/EXPERIMENTAL_CENSUS.txt" >/dev/null
grep -Fx $'TOTAL_SKIPPED\t0' "$FREEZE_DIR/EXPERIMENTAL_CENSUS.txt" >/dev/null

find cqpl/queries_v2 -maxdepth 1 -type f -name '*.cqpl' -print0 \
  | sort -z | xargs -0 sha256sum > "$QUERY_MANIFEST"
[[ "$(wc -l < "$QUERY_MANIFEST")" -eq 12 ]] || fail "expected 12 frozen queries"
QUERY_MANIFEST_SHA="$(sha256sum "$QUERY_MANIFEST" | awk '{print $1}')"
[[ "$QUERY_MANIFEST_SHA" == '23f0758d8459c8482b75c6c6c85b1f41f708a0ce99da505b90e4c6f782086a95' ]] \
  || fail "query manifest changed: $QUERY_MANIFEST_SHA"

echo "GATE 1: PASS main=112/111 experimental=4/4 queries=12"

# -----------------------------------------------------------------------------
# Gate 2: fresh analyses for main + experimental + registry, then 118x12 matrix.
# -----------------------------------------------------------------------------
cargo +"$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml
cargo +"$TOOLCHAIN" build --manifest-path cqpl/cqpl_checker/Cargo.toml
[[ -x "$CHECKER" ]] || fail "checker missing after build"

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

python3 - "$MAIN/results.json" "$EXP/results.json" <<'PY'
import json,sys
main=json.load(open(sys.argv[1])); exp=json.load(open(sys.argv[2]))
assert main['discovered_targets']==112
assert main['active_targets']==111
assert main['modeled_complete']==111
assert main['failures']==0
assert main['skipped_targets']==['no_errors_projects/openapi-client-gen']
assert exp['discovered_targets']==4
assert exp['active_targets']==4
assert exp['modeled_complete']==4
assert exp['failures']==0
assert exp['skipped_targets']==[]
print('CORPORA: PASS main=111 experimental=4')
PY

CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$TOOLCHAIN" \
CQPL_REGISTRY_OUT="$REGISTRY" \
  bash cqpl/scripts/run_registry_crates_v6q_r1c.sh

: > "$SUBJECTS"
find "$MAIN/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 \
  | sort -z \
  | while IFS= read -r -d '' artifact; do
      name="$(basename "$(dirname "$artifact")")"
      printf 'corpus\t%s\t%s\n' "$name" "$artifact"
    done >> "$SUBJECTS"

find "$EXP/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 \
  | sort -z \
  | while IFS= read -r -d '' artifact; do
      name="$(basename "$(dirname "$artifact")")"
      printf 'experimental\t%s\t%s\n' "$name" "$artifact"
    done >> "$SUBJECTS"

for name in unicode-ident-1.0.18 ryu-1.0.20 memchr-2.7.4; do
  artifact="$REGISTRY/$name/analysis/annotated_icfg_v2.json"
  [[ -s "$artifact" ]] || fail "missing registry artifact $artifact"
  printf 'registry\t%s\t%s\n' "$name" "$artifact" >> "$SUBJECTS"
done

python3 - "$SUBJECTS" <<'PY'
import sys
rows=[]
for raw in open(sys.argv[1], encoding='utf-8'):
    raw=raw.rstrip('\n')
    if not raw: continue
    p=raw.split('\t')
    assert len(p)==3, raw
    rows.append(tuple(p))
assert len(rows)==118, len(rows)
from collections import Counter
c=Counter(r[0] for r in rows)
assert c=={'corpus':111,'experimental':4,'registry':3}, c
names=[r[1] for r in rows]
assert len(names)==len(set(names)), 'subject names collide; matrix result dirs are name-keyed'
print('SUBJECTS: PASS total=118 corpus=111 experimental=4 registry=3')
PY

python3 cqpl/scripts/run_queries_v2_matrix.py \
  --checker "$CHECKER" \
  --queries "$ROOT/cqpl/queries_v2" \
  --subject-tsv "$SUBJECTS" \
  --out "$MATRIX"

python3 - "$MATRIX/summary.json" <<'PY'
import json,sys
x=json.load(open(sys.argv[1]))
assert x['subjects']==118, x
assert x['queries']==12, x
assert x['attempts']==1416, x
assert x['missing_artifacts']==[], x['missing_artifacts']
assert set(x['result_counts']) <= {'ff','unk','tt'}, x['result_counts']
assert sum(x['result_counts'].values())==1416, x['result_counts']
print('MATRIX: PASS subjects=118 queries=12 attempts=1416 counts='+str(x['result_counts']))
PY

echo "GATE 2: PASS fresh FINAL118 matrix complete"

R2_NEUTRALITY="$OUT/r2-final118-neutrality"
R2_COMPARATOR="$ROOT/cqpl/scripts/validate_typed_edge_flow_final118_neutrality.py"
[[ -f "$R2_COMPARATOR" ]] || fail "missing R2 comparator $R2_COMPARATOR"
python3 "$R2_COMPARATOR" \
  --subjects "$SUBJECTS" \
  --checker "$CHECKER" \
  --queries-dir "$ROOT/cqpl/queries_v2" \
  --out "$R2_NEUTRALITY"
python3 - "$R2_NEUTRALITY/FINAL118_R2_NEUTRALITY.json" <<'PY'
import json,sys
x=json.load(open(sys.argv[1]))
assert x['subjects']==118 and x['queries']==12 and x['attempts']==1416, x
assert x['truth_or_assessment_delta_count']==0, x
assert x['status']=='PASS', x
assert x['typed_edge_counts']['normal']>0, x
assert x['typed_edge_counts']['unwind']>0, x
print('R2_NEUTRALITY: PASS 0/1416')
PY


# -----------------------------------------------------------------------------
# Gate 3: freeze compact evidence and audit against historical FINAL112.
# -----------------------------------------------------------------------------
cp "$SUBJECTS" "$FREEZE_DIR/SUBJECTS.tsv"
cp "$MAIN/corpus-source-SHA256SUMS" "$FREEZE_DIR/MAIN_SOURCE_SHA256SUMS"
cp "$EXP/corpus-source-SHA256SUMS" "$FREEZE_DIR/EXPERIMENTAL_SOURCE_SHA256SUMS"
cp "$MATRIX/query-results-long.tsv" "$FREEZE_DIR/QUERY_RESULTS_LONG.tsv"
cp "$MATRIX/query-results-wide.tsv" "$FREEZE_DIR/QUERY_RESULTS_WIDE.tsv"
cp "$MATRIX/unknown-explanations.tsv" "$FREEZE_DIR/UNKNOWN_EXPLANATIONS.tsv"
cp "$MATRIX/summary.json" "$FREEZE_DIR/MATRIX_SUMMARY.json"
cp "$MATRIX/unknown-explanations-summary.json" "$FREEZE_DIR/UNKNOWN_SUMMARY.json"
cp "$R2_NEUTRALITY/FINAL118_R2_NEUTRALITY.json" "$FREEZE_DIR/R2_NEUTRALITY.json"
cp "$R2_NEUTRALITY/FINAL118_R2_NEUTRALITY.tsv" "$FREEZE_DIR/R2_NEUTRALITY.tsv"

OLD_MATRIX="$(
  find "$ROOT/repro-results" -type f \
    -path '*/efx1-r2-provenance-r1_2-final112-*/query-matrix/query-results-wide.tsv' \
    | sort | tail -n 1
)"
[[ -n "$OLD_MATRIX" && -f "$OLD_MATRIX" ]] || fail "historical FINAL112 matrix not found"
echo "$OLD_MATRIX" > "$FREEZE_DIR/HISTORICAL_FINAL112_MATRIX_PATH.txt"

python3 - "$FREEZE_DIR" "$OLD_MATRIX" "$OUT" <<'PY'
from pathlib import Path
from collections import Counter
import csv, hashlib, json, sys

freeze=Path(sys.argv[1]); old_path=Path(sys.argv[2]); run=Path(sys.argv[3])

def rows(path):
    with path.open(newline='', encoding='utf-8') as f:
        return list(csv.DictReader(f, delimiter='\t'))

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

summary=json.loads((freeze/'MATRIX_SUMMARY.json').read_text())
r2=json.loads((freeze/'R2_NEUTRALITY.json').read_text())
assert r2['truth_or_assessment_delta_count']==0, r2
assert r2['status']=='PASS', r2
assert summary['subjects']==118
assert summary['queries']==12
assert summary['attempts']==1416
assert summary['missing_artifacts']==[]
assert set(summary['result_counts']) <= {'ff','unk','tt'}
assert sum(summary['result_counts'].values())==1416

long=rows(freeze/'QUERY_RESULTS_LONG.tsv')
assert len(long)==1416, len(long)
assert all(r['rc']=='0' for r in long)
assert all(r['result'] in {'ff','unk','tt'} for r in long)

unknown=rows(freeze/'UNKNOWN_EXPLANATIONS.tsv')
assert len(unknown)==summary['result_counts'].get('unk',0)
sub=Counter(r['subresult'] for r in unknown)
dirn=Counter(r['direction'] for r in unknown)
strength=Counter(r['strength'] for r in unknown)
assert set(sub) <= {'unk_true','unk_unoriented','unk_false','unk_mixed'}

us=json.loads((freeze/'UNKNOWN_SUMMARY.json').read_text())
assert us['complete'] is True, us.get('failures')
assert us['unknown_results']==len(unknown)
assert us['explanations_generated']==len(unknown)

new_rows=rows(freeze/'QUERY_RESULTS_WIDE.tsv')
old_rows=rows(old_path)
assert len(old_rows)==112, len(old_rows)
assert len(new_rows)==118, len(new_rows)

def idx(rs):
    return {(r['group'],r['target']):r for r in rs}
old=idx(old_rows); new=idx(new_rows)
assert len(old)==112 and len(new)==118
assert set(old) <= set(new)
expected_new={
    ('corpus','clean_alloc_read_and_drop'),
    ('corpus','uaf-df-mail-lillo'),
    ('experimental','bmulti_clean_two_args_ffi'),
    ('experimental','bmulti_df_second_ffi'),
    ('experimental','bmulti_leak_second_ffi'),
    ('experimental','bmulti_uaf_second_ffi'),
}
actual_new=set(new)-set(old)
assert actual_new==expected_new, (actual_new, expected_new)

query_cols=[c for c in old_rows[0] if c not in {'group','target'}]
assert len(query_cols)==12, query_cols
deltas=[]
for subject, before in old.items():
    after=new[subject]
    for q in query_cols:
        if before[q] != after[q]:
            deltas.append((subject,q,before[q],after[q]))
assert not deltas, deltas[:20]

# Event/state parity remains an invariant of the frozen query suite.
cell={(r['group'],r['target'],r['query']):r['result'] for r in long}
pairs=[
    ('double_free_alloc','double_free_alloc_state'),
    ('leak_alloc','leak_alloc_state'),
    ('use_after_free_alloc','use_after_free_alloc_state'),
    ('allocator_mismatch_ub','allocator_mismatch_ub_v2'),
]
subjects={(r['group'],r['target']) for r in long}
for g,t in subjects:
    for a,b in pairs:
        assert cell[g,t,a]==cell[g,t,b], (g,t,a,b,cell[g,t,a],cell[g,t,b])

# Current schema-v2 memory predicates remain MAY-only before MUST work.
mem={
    'allocator_mismatch_ub','allocator_mismatch_ub_v2',
    'double_free_alloc','double_free_alloc_state',
    'leak_alloc','leak_alloc_state',
    'use_after_free_alloc','use_after_free_alloc_state',
}
for r in long:
    if r['query'] in mem:
        assert r['result'] != 'tt', r

# Explicit sanity checks for the two newly-added main controls already measured.
def val(group,target,query):
    return cell[group,target,query]
assert val('corpus','clean_alloc_read_and_drop','double_free_alloc')=='unk'
assert val('corpus','clean_alloc_read_and_drop','double_free_alloc_state')=='unk'
assert val('corpus','clean_alloc_read_and_drop','use_after_free_alloc')=='ff'
assert val('corpus','clean_alloc_read_and_drop','allocator_mismatch_ub')=='ff'
assert val('corpus','uaf-df-mail-lillo','double_free_alloc')=='unk'
assert val('corpus','uaf-df-mail-lillo','use_after_free_alloc')=='unk'
assert val('corpus','uaf-df-mail-lillo','leak_alloc')=='unk'
assert val('corpus','uaf-df-mail-lillo','allocator_mismatch_ub')=='unk'

result={
    'schema':'cqpl_post_r2_final118_freeze_v1',
    'status':'PASS',
    'run':str(run),
    'subjects':118,
    'main_active':111,
    'experimental_active':4,
    'registry_subjects':3,
    'queries':12,
    'attempts':1416,
    'new_subjects':[
        {'group':g,'target':t} for g,t in sorted(expected_new)
    ],
    'historical_final112_truth_delta_count':0,
    'profile':'POST_R2_FINAL118',
    'semantic_profile':'schema-v2 + restored live SVF evidence + typed_edge_flow_v1',
    'r2_neutrality':r2,
    'truth_counts':summary['result_counts'],
    'subresult_counts':dict(sorted(sub.items())),
    'direction_counts':dict(sorted(dirn.items())),
    'strength_counts':dict(sorted(strength.items())),
    'query_manifest_sha256':sha(freeze/'QUERY_SHA256SUMS'),
    'artifact_sha256':{
        'expected_main_targets':sha(freeze/'EXPECTED_MAIN_TARGETS.txt'),
        'expected_experimental_targets':sha(freeze/'EXPECTED_EXPERIMENTAL_TARGETS.txt'),
        'subjects':sha(freeze/'SUBJECTS.tsv'),
        'main_source_manifest':sha(freeze/'MAIN_SOURCE_SHA256SUMS'),
        'experimental_source_manifest':sha(freeze/'EXPERIMENTAL_SOURCE_SHA256SUMS'),
        'query_long':sha(freeze/'QUERY_RESULTS_LONG.tsv'),
        'query_wide':sha(freeze/'QUERY_RESULTS_WIDE.tsv'),
        'unknown_explanations':sha(freeze/'UNKNOWN_EXPLANATIONS.tsv'),
        'r2_neutrality':sha(freeze/'R2_NEUTRALITY.json'),
        'historical_final112_matrix':sha(old_path),
    },
}
(freeze/'FREEZE.json').write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
print(json.dumps(result,indent=2,sort_keys=True))
print('POST_R2_FINAL118_AUDIT: PASS')
PY

(
  cd "$FREEZE_DIR"
  find . -type f ! -name SHA256SUMS -print0 \
    | sort -z | xargs -0 sha256sum > SHA256SUMS
  sha256sum -c SHA256SUMS
)

echo "GATE 3: PASS POST_R2_FINAL118 frozen"
echo "RUN=$OUT"
echo "FREEZE=$FREEZE_DIR"
echo "No tag was created."
