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
OUT="$ROOT/repro-results/gate-w1-diagnostic-witness-$STAMP"
MAIN="$OUT/main"
EXP="$OUT/experimental"
REGISTRY="$OUT/registry"
MATRIX="$OUT/query-matrix"
SUBJECTS="$OUT/subjects.tsv"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"

fail(){ echo "GATE_W1_DIAGNOSTIC_WITNESS: FAIL: $*" >&2; exit 2; }

[[ -f "$BASE/QUERY_RESULTS_LONG.tsv" ]] || fail "missing frozen truth baseline: $BASE/QUERY_RESULTS_LONG.tsv"
[[ -f "$ASSESSMENT_BASE/UNKNOWN_EXPLANATIONS.tsv" ]] || fail "missing frozen post-L1 assessment baseline: $ASSESSMENT_BASE/UNKNOWN_EXPLANATIONS.tsv"
[[ -f "$ASSESSMENT_BASE/GATE_L1_NORMAL_EXECUTION.json" ]] || fail "missing frozen post-L1 gate summary"
python3 - "$ASSESSMENT_BASE/GATE_L1_NORMAL_EXECUTION.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
assert d.get("status") == "PASS", d
assert d.get("truth_delta_count") == 0, d
assert d.get("non_target_assessment_delta_count") == 0, d
PY
[[ -f "$ORACLE" ]] || fail "missing source oracle: $ORACLE"
[[ -d "$ROOT/tests_and_target_repos" ]] || fail "missing $ROOT/tests_and_target_repos"
[[ -d "$ROOT/test_and_target_repos_experimental" ]] || fail "missing $ROOT/test_and_target_repos_experimental"
[[ -f "$ROOT/cqpl/scripts/gate_w1_diagnostic_witness.py" ]] || fail "missing installed W1 gate"

mkdir -p "$OUT"
printf '%s\n' \
  "root=$ROOT" \
  "toolchain=$TOOLCHAIN" \
  "truth_baseline=$BASE" \
  "assessment_baseline=$ASSESSMENT_BASE" \
  "git_head=$(git rev-parse HEAD)" \
  "git_branch=$(git branch --show-current)" \
  > "$OUT/environment.txt"
git status --short > "$OUT/git-status-before.txt"

# Important: do NOT use the historical grep -Fx preflight here. Inline # comments
# are parser-neutral in CQPL. The two focused checker runs below are the semantic
# preflight for typed_edge_flow_v1 + normal_execution.

echo "[1/9] Unit tests + checker build"
cargo +"$TOOLCHAIN" test --manifest-path crema/Cargo.toml
cargo +"$TOOLCHAIN" test --manifest-path cqpl/cqpl_checker/Cargo.toml
cargo +"$TOOLCHAIN" build --manifest-path cqpl/cqpl_checker/Cargo.toml

echo "[2/9] Focused source-to-query controls"
for spec in \
  'clean:a-code_full_rust/clean_alloc_read_and_drop' \
  'boxed:a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool'
do
  tag="${spec%%:*}"
  target="${spec#*:}"
  dir="$OUT/focus-$tag"
  python3 cqpl/scripts/run_one_target_v6q_r1c.py \
    --root "$ROOT" --relative-path "$target" --out "$dir" --toolchain "$TOOLCHAIN"
  "$CHECKER" "$dir/annotated_icfg_v2.json" cqpl/queries_v2/leak_alloc_state.cqpl --json \
    > "$OUT/focus-$tag.json"
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

echo "[3/9] Fresh main corpus"
EXPECTED_MAIN="$BASE/EXPECTED_MAIN_TARGETS.txt"
EXPECTED_EXP="$BASE/EXPECTED_EXPERIMENTAL_TARGETS.txt"
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

echo "[4/9] Fresh experimental corpus"
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

echo "[5/9] Fresh registry controls"
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$TOOLCHAIN" \
CQPL_REGISTRY_OUT="$REGISTRY" \
  bash cqpl/scripts/run_registry_crates_v6q_r1c.sh

echo "[6/9] Build exact 118-subject manifest"
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

# Fail closed before running the matrix: every fresh W1 subject must advertise
# source_provenance_v1 and every node must carry the atomic payload.
python3 - "$SUBJECTS" "$OUT/source-provenance-census.json" <<'PY'
import json,sys
from pathlib import Path
subjects=Path(sys.argv[1]); out=Path(sys.argv[2])
rows=[]; bad=[]; nodes=0; allocation_events=0; anchors=0
for line in subjects.read_text().splitlines():
    group,name,artifact=line.split('\t')
    p=Path(artifact); data=json.loads(p.read_text())
    caps=set(data.get('capabilities',[]))
    rec={'group':group,'target':name,'artifact':artifact,'nodes':len(data.get('nodes',[]))}
    if 'source_provenance_v1' not in caps:
        bad.append({**rec,'error':'missing source_provenance_v1 capability'})
        continue
    missing=[n.get('id') for n in data.get('nodes',[]) if 'source_provenance' not in n]
    if missing:
        bad.append({**rec,'error':f'{len(missing)} nodes missing source_provenance','examples':missing[:5]})
        continue
    nanchors=0; nevents=0
    for n in data.get('nodes',[]):
        sp=n['source_provenance']
        nanchors += len(sp.get('anchors',[]))
        nevents += len(sp.get('allocation_events',[]))
        nanchors += sum(len(e.get('anchors',[])) for e in sp.get('allocation_events',[]))
    nodes += len(data.get('nodes',[])); allocation_events += nevents; anchors += nanchors
    rows.append({**rec,'allocation_events':nevents,'source_anchors':nanchors})
report={'schema':'source_provenance_v1_census','subjects':len(rows),'nodes':nodes,'allocation_events':allocation_events,'source_anchors':anchors,'failures':bad,'status':'PASS' if not bad and len(rows)==118 else 'FAIL'}
out.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
print(json.dumps(report,indent=2,sort_keys=True))
if report['status']!='PASS': raise SystemExit(2)
PY

echo "[7/9] Execute 118 x 12 = 1416 fresh query matrix"
python3 cqpl/scripts/run_queries_v2_matrix.py \
  --checker "$CHECKER" \
  --queries "$ROOT/cqpl/queries_v2" \
  --subject-tsv "$SUBJECTS" \
  --out "$MATRIX"

echo "[8/9] Historical L1 regression gate"
python3 cqpl/scripts/gate_l1_normal_execution.py \
  --baseline "$BASE" \
  --matrix "$MATRIX" \
  --oracle "$ORACLE" \
  --out "$OUT/gate-l1"

echo "[9/9] W1 semantic-neutrality + certificate gate"
python3 cqpl/scripts/gate_w1_diagnostic_witness.py \
  --truth-baseline "$BASE" \
  --assessment-baseline "$ASSESSMENT_BASE" \
  --matrix "$MATRIX" \
  --out "$OUT/gate-w1" \
  --expected-attempts 1416

python3 - "$OUT/gate-l1/GATE_L1_NORMAL_EXECUTION.json" "$OUT/gate-w1/GATE_W1_DIAGNOSTIC_WITNESS.json" <<'PY'
import json,sys
l1=json.load(open(sys.argv[1])); w1=json.load(open(sys.argv[2]))
assert l1['status']=='PASS', l1
assert w1['status']=='PASS', w1
assert w1['attempts']==1416, w1
assert w1['truth_delta_count']==0, w1
assert w1['assessment_delta_count']==0, w1
assert not w1['certificate_validation_failures'], w1
print('GATE_W1_DIAGNOSTIC_WITNESS: PASS')
print(f"  attempts={w1['attempts']}")
print(f"  truth_delta_count={w1['truth_delta_count']}")
print(f"  assessment_delta_count={w1['assessment_delta_count']}")
print(f"  certifiable_findings={w1['certifiable_findings']}")
print(f"  directional_findings={w1['directional_findings']}")
print(f"  non_directional_findings={w1['non_directional_findings']}")
print(f"  diagnostic_certificates={w1['diagnostic_certificates']}")
print(f"  source_capability_unknown_reports={w1['source_capability_unknown_reports']}")
print(f"  source_status_counts={w1['source_status_counts']}")
PY

git status --short > "$OUT/git-status-after.txt"

# Portable, relocation-safe key manifest.  Paths are relative to the gate root.
(
  cd "$OUT"
  sha256sum \
    gate-l1/GATE_L1_NORMAL_EXECUTION.json \
    gate-w1/GATE_W1_DIAGNOSTIC_WITNESS.json \
    source-provenance-census.json \
    query-matrix/summary.json \
    query-matrix/query-results-long.tsv \
    query-matrix/unknown-explanations-summary.json \
    subjects.tsv \
    environment.txt \
    git-status-before.txt \
    git-status-after.txt \
    > GATE_SHA256SUMS
)

# Full self-contained artifact manifest, also relocation-safe.
(
  cd "$OUT"
  find . -type f ! -name ARTIFACT_SHA256SUMS -print0 \
    | LC_ALL=C sort -z \
    | xargs -0 sha256sum \
    > ARTIFACT_SHA256SUMS
)

echo "GATE_W1_DIAGNOSTIC_WITNESS: PASS out=$OUT"
