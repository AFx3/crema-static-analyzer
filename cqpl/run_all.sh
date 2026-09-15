#!/usr/bin/env bash
set -euo pipefail

CQPL_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
export CREMA_CQPL_DIR="$CQPL_DIR"
ROOT="${CREMA_PHD_ROOT:-$(cd "$CQPL_DIR/.." && pwd)}"
NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${CQPL_RUN_ALL_OUT:-$ROOT/repro-results/cqpl-v6q-r1c-final112-$STAMP}"
SKIP="no_errors_projects/openapi-client-gen"

CORPUS="$OUT/corpus"
REGISTRY="$OUT/registry"
MATRIX="$OUT/query-matrix"
SUBJECTS="$OUT/subjects.tsv"
RUNNER="$CQPL_DIR/scripts/run_corpus_allocator_contracts.py"
REGISTRY_RUNNER="$CQPL_DIR/scripts/run_registry_crates_v6q_r1c.sh"
MATRIX_RUNNER="$CQPL_DIR/scripts/run_queries_v2_matrix.py"
EXPECTED="$CQPL_DIR/artifact/EXPECTED_TARGETS_V6L.txt"
TARGET_CONFIG="$CQPL_DIR/artifact/TARGET_ANALYSIS_CONFIG.tsv"
ENTRY_OVERRIDES="$CQPL_DIR/regression/reference/entry_overrides.json"
CHECKER_MANIFEST="$CQPL_DIR/cqpl_checker/Cargo.toml"
CHECKER="$CQPL_DIR/cqpl_checker/target/debug/cqpl_checker"
EXPECTED_RUSTC='rustc 1.84.0-nightly (3fee0f12e 2024-11-20)'

CLEAN_FLAG=()
if [[ "${CQPL_CLEAN_TARGETS:-0}" == "1" ]]; then
  CLEAN_FLAG=(--clean-targets)
fi

fail() { echo "CQPL RUN_ALL v6Q-r1c FAIL: $*" >&2; exit 1; }

command -v rustup >/dev/null || fail 'rustup not found'
command -v cargo >/dev/null || fail 'cargo not found'
command -v python3 >/dev/null || fail 'python3 not found'
[[ -d "$ROOT/tests_and_target_repos" ]] || fail "missing $ROOT/tests_and_target_repos"
for f in "$RUNNER" "$REGISTRY_RUNNER" "$MATRIX_RUNNER" "$EXPECTED" "$TARGET_CONFIG" "$ENTRY_OVERRIDES" "$CHECKER_MANIFEST"; do
  [[ -f "$f" ]] || fail "missing $f"
done

QUERY_COUNT="$(find "$CQPL_DIR/queries_v2" -maxdepth 1 -type f -name '*.cqpl' | wc -l)"
[[ "$QUERY_COUNT" -eq 12 ]] || fail "expected exactly 12 queries_v2, found $QUERY_COUNT"

RUSTC_VERSION="$(rustup run "$NIGHTLY" rustc --version)"
[[ "$RUSTC_VERSION" == "$EXPECTED_RUSTC" ]] || fail "unexpected pinned rustc: $RUSTC_VERSION"

rm -rf "$OUT"
mkdir -p "$OUT"
rustup run "$NIGHTLY" rustc --version --verbose > "$OUT/rustc-version.txt"
cargo +"$NIGHTLY" --version > "$OUT/cargo-version.txt"

cat <<INFO
=== CQPL run_all: CREMA/CQPL v6Q-r1c final112 ===
mode=schema-v2 + mir_semantics_v2 + allocation_state_v1 + allocation_contracts_v2
root=$ROOT
out=$OUT
toolchain=$NIGHTLY
corpus=110 discovered / 109 active / 1 skipped
registry=unicode-ident 1.0.18; ryu 1.0.20; memchr 2.7.4
queries_v2=12
subject_count=112
expected_attempts=1344
silent_lib_bin_fallback=DISABLED (target kind is explicit; ambiguity is a hard error)
INFO

# Compile and test the exact checker before any expensive analysis.  This also
# exercises the regression that the parser accepts every terminator category
# emitted by CREMA's mir_semantic_labels_v1 producer.
cargo +"$NIGHTLY" test --manifest-path "$CHECKER_MANIFEST"
cargo +"$NIGHTLY" build --manifest-path "$CHECKER_MANIFEST"
[[ -x "$CHECKER" ]] || fail "checker binary missing after build: $CHECKER"

# 1. Frozen 109-target corpus, using the canonical target configuration and the
#    actual v6Q MIR-v2 producer path.  The runner still computes the canonical
#    four memory properties, which are cross-checked against the 12-query matrix.
python3 "$RUNNER" \
  --root "$ROOT" \
  --out "$CORPUS" \
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

python3 - "$CORPUS" <<'PY'
import json,sys
from pathlib import Path
out=Path(sys.argv[1]); r=json.loads((out/'results.json').read_text())
assert r['schema_version']==2
assert r['discovered_targets']==110 and r['active_targets']==109
assert r['modeled_complete']==109 and r['failures']==0
assert r['skipped_targets']==['no_errors_projects/openapi-client-gen']
arts=sorted(out.glob('raw/*/annotated_icfg_v2.json'))
assert len(arts)==109, len(arts)
required={'allocation_state_v1','allocation_contracts_v1','allocation_contracts_v2','mir_semantic_labels_v1','mir_semantics_v2'}
for p in arts:
    d=json.loads(p.read_text()); caps=set(d.get('capabilities',[]))
    assert d.get('schema_version')==2, p
    assert required <= caps, (p, sorted(required-caps))
logs=sorted(out.glob('raw/*/crema-export.log'))
assert len(logs)==109, len(logs)
for p in logs:
    assert '--mir-semantics-v2' in p.read_text(errors='replace'), p
print('CQPL_V6Q_R1C_CORPUS: PASS active=109 complete=109 mir_v2_exports=109')
PY

python3 "$CQPL_DIR/scripts/render_ae_table.py" \
  "$CORPUS/results.json" \
  --full "$CORPUS/artifact-evaluation-table.tex" \
  --summary "$CORPUS/artifact-evaluation-summary.tex"

# 2. Three pinned crates.io subjects.  They are explicit library analyses with
#    exact versions/features/API roots; no silent fallback to a different bin or
#    lib target is permitted.
CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
CQPL_REGISTRY_OUT="$REGISTRY" \
CQPL_CRATES_IO_OFFLINE="${CQPL_CRATES_IO_OFFLINE:-0}" \
  "$REGISTRY_RUNNER"

# 3. Freeze the exact 112 graph subjects used by the query matrix.
: > "$SUBJECTS"
find "$CORPUS/raw" -mindepth 2 -maxdepth 2 -type f -name annotated_icfg_v2.json -print0 \
  | sort -z \
  | while IFS= read -r -d '' artifact; do
      name="$(basename "$(dirname "$artifact")")"
      printf 'corpus\t%s\t%s\n' "$name" "$artifact"
    done >> "$SUBJECTS"
for name in unicode-ident-1.0.18 ryu-1.0.20 memchr-2.7.4; do
  artifact="$REGISTRY/$name/analysis/annotated_icfg_v2.json"
  [[ -s "$artifact" ]] || fail "missing registry graph $artifact"
  printf 'registry\t%s\t%s\n' "$name" "$artifact" >> "$SUBJECTS"
done
[[ "$(wc -l < "$SUBJECTS")" -eq 112 ]] || fail "subject census is not 112"

# 4. Execute every queries_v2 formula on every final graph.
python3 "$MATRIX_RUNNER" \
  --checker "$CHECKER" \
  --queries "$CQPL_DIR/queries_v2" \
  --subject-tsv "$SUBJECTS" \
  --out "$MATRIX"

# 5. Scientific regression gates: cardinality/domain, exact canonical-four
#    cross-check, and invariants observed in the audited final112 freeze.
python3 - "$CORPUS/results.json" "$MATRIX/query-results-long.tsv" "$MATRIX/summary.json" "$OUT/final112-summary.json" <<'PY'
import csv,json,sys
from collections import Counter,defaultdict
from pathlib import Path
corpus=json.load(open(sys.argv[1])); longp=Path(sys.argv[2]); matrix_summary=json.load(open(sys.argv[3])); out=Path(sys.argv[4])
rows=list(csv.DictReader(longp.open(),delimiter='\t'))
assert matrix_summary['subjects']==112 and matrix_summary['queries']==12 and matrix_summary['attempts']==1344
assert matrix_summary['missing_artifacts']==[]
assert len(rows)==1344
assert all(r['rc']=='0' for r in rows)
assert set(r['result'] for r in rows) <= {'ff','unk','tt'}
cell={(r['group'],r['target'],r['query']):r['result'] for r in rows}
assert len(cell)==1344
mapping={
 'leak':'leak_alloc_state',
 'double_free':'double_free_alloc_state',
 'use_after_free':'use_after_free_alloc_state',
 'allocator_mismatch':'allocator_mismatch_ub_v2',
}
for target,item in corpus['results'].items():
    for legacy_name,qname in mapping.items():
        assert item['queries'][legacy_name]['result']==cell[('corpus',target,qname)], (target,legacy_name,qname)
for group,target,_ in sorted({(r['group'],r['target'],r['artifact']) for r in rows}):
    for a,b in [
      ('double_free_alloc','double_free_alloc_state'),
      ('leak_alloc','leak_alloc_state'),
      ('use_after_free_alloc','use_after_free_alloc_state'),
      ('allocator_mismatch_ub','allocator_mismatch_ub_v2'),
    ]:
        assert cell[(group,target,a)]==cell[(group,target,b)], (group,target,a,b)
    assert cell[(group,target,'mir_terminator_presence')]=='tt', (group,target)
for r in rows:
    if r['query'] in {
      'allocator_mismatch_ub','allocator_mismatch_ub_v2',
      'double_free_alloc','double_free_alloc_state',
      'leak_alloc','leak_alloc_state',
      'mir_structural_allocator_example',
      'use_after_free_alloc','use_after_free_alloc_state',
    }:
        assert r['result']!='tt', r
counts=Counter(r['result'] for r in rows)
summary={
 'profile':'CREMA-CQPL-v6Q-r1c-final112',
 'subjects':112,'corpus_subjects':109,'registry_subjects':3,'queries':12,'attempts':1344,
 'result_counts':dict(sorted(counts.items())),
 'canonical_four_crosscheck':'PASS',
 'event_state_pair_regression':'PASS',
 'allocator_v1_v2_regression':'PASS',
 'memory_queries_no_tt':'PASS',
 'term_return_all_subjects':'PASS',
 'precision_note':'leak may remain conservative/unknown on many subjects; PASS is not an accuracy claim',
}
out.write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
PY

# 6. Freeze every output byte.
(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS.FINAL -print0 \
    | sort -z \
    | xargs -0 sha256sum > SHA256SUMS.FINAL
  sha256sum -c SHA256SUMS.FINAL
)

echo 'CQPL RUN_ALL v6Q-r1c: PASS subjects=112 queries=12 attempts=1344'
echo "summary=$OUT/final112-summary.json"
echo "matrix=$MATRIX/query-results-wide.tsv"
echo "evidence=$OUT"
