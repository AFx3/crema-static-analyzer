#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
KIT="$ROOT/CQPL8_FPF_PORT_VALIDATION"
DEFAULT_ORACLE="$SCRIPT_DIR/RQ1_ORACLE_175.csv"
ORACLE="$DEFAULT_ORACLE"
OUT=""
TOOLCHAIN=nightly-2024-11-21
CANONICAL_ORACLE_SHA=6a191f753d74fa49ed48418e5af81f70bfaf89408782473f1a25f7147af2854a
CANONICAL_FIXTURE_SHA=583745a43c7d5c90e33413e0cd7814e53476bb43cfd56cad94b1032f4d118386

usage() {
  cat <<'EOF'
Usage: bash cqpl/artifact/run_rq1.sh [--out NEW_DIRECTORY] [--oracle CSV]

Runs the complete 175-target RQ1 artifact: fresh bootstrap, FFI smoke, five
queries per crate, strict 700/700 primary validation, and CSV/LaTeX reporting.
The output directory must not exist. --oracle is an explicit noncanonical
override and is recorded in provenance.
EOF
}

while (($#)); do
  case "$1" in
    --out) (($# >= 2)) || { echo "--out requires a path" >&2; exit 2; }; OUT=$2; shift 2 ;;
    --oracle) (($# >= 2)) || { echo "--oracle requires a path" >&2; exit 2; }; ORACLE=$2; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

for command in bash python3 cargo rustup rustc git sha256sum awk grep; do
  command -v "$command" >/dev/null || { echo "Missing dependency: $command" >&2; exit 2; }
done
for file in \
  "$KIT/scripts/bootstrap.sh" "$KIT/scripts/export_one_target.py" \
  "$KIT/scripts/run_rq1_final.py" "$KIT/scripts/report_rq1_tables.py" \
  "$KIT/A3_TARGET_CAPABILITIES.tsv" "$ORACLE" \
  "$ROOT/cqpl/queries_v3/leak_normal_completion.cqpl" \
  "$ROOT/cqpl/queries_v3/double_free_full.cqpl" \
  "$ROOT/cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl" \
  "$ROOT/cqpl/queries_v3/use_after_free_full.cqpl" \
  "$ROOT/cqpl/queries_v3/allocator_mismatch_full.cqpl"; do
  test -f "$file" || { echo "Missing artifact input: $file" >&2; exit 2; }
done
git -C "$ROOT" diff --quiet && git -C "$ROOT" diff --cached --quiet || {
  echo "Refusing a checkout with tracked source changes; commit or stash them first" >&2; exit 2;
}
for relative in \
  CQPL8_FPF_PORT_VALIDATION/A3_TARGET_CAPABILITIES.tsv \
  CQPL8_FPF_PORT_VALIDATION/scripts/bootstrap.sh \
  CQPL8_FPF_PORT_VALIDATION/scripts/export_one_target.py \
  CQPL8_FPF_PORT_VALIDATION/scripts/run_rq1_final.py \
  CQPL8_FPF_PORT_VALIDATION/scripts/report_rq1_tables.py \
  cqpl/artifact/run_rq1.sh \
  cqpl/artifact/RQ1_ORACLE_175.csv \
  cqpl/analysis_profiles/full_memory_safety_v1.json \
  cqpl/evaluation_manifests/full_memory_safety_v1/dataset_targets_196.txt \
  cqpl/queries_v3/leak_normal_completion.cqpl \
  cqpl/queries_v3/double_free_full.cqpl \
  cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl \
  cqpl/queries_v3/use_after_free_full.cqpl \
  cqpl/queries_v3/allocator_mismatch_full.cqpl \
  tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs; do
  git -C "$ROOT" ls-files --error-unmatch "$relative" >/dev/null 2>&1 || {
    echo "Artifact dependency is not versioned: $relative" >&2; exit 2;
  }
done

rustup toolchain list | awk '{print $1}' | grep -Fx "$TOOLCHAIN-x86_64-unknown-linux-gnu" >/dev/null || {
  echo "Missing pinned toolchain: $TOOLCHAIN" >&2; exit 2;
}
FIXTURE="$ROOT/tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs"
test "$(sha256sum "$FIXTURE" | awk '{print $1}')" = "$CANONICAL_FIXTURE_SHA" || {
  echo "Canonical CStringCargo fixture hash mismatch" >&2; exit 2;
}
if [[ "$ORACLE" == "$DEFAULT_ORACLE" ]]; then
  test "$(sha256sum "$ORACLE" | awk '{print $1}')" = "$CANONICAL_ORACLE_SHA" || {
    echo "Canonical RQ1 oracle hash mismatch" >&2; exit 2;
  }
else
  echo "NOTICE: using explicit noncanonical oracle override: $ORACLE" >&2
fi

if [[ -z "$OUT" ]]; then
  BASE=/mnt/wsx-storage/af
  test -d "$BASE" && test -w "$BASE" || BASE="${TMPDIR:-/tmp}"
  OUT="$BASE/cqpl8-rq1-artifact-$(date -u +%Y%m%dT%H%M%SZ)-$$"
fi
test ! -e "$OUT" || { echo "Refusing existing output: $OUT" >&2; exit 2; }
mkdir -p "$OUT"
BOOT="$OUT/bootstrap"; CAMPAIGN="$OUT/campaign"; TABLES="$OUT/tables"
HEAD=$(git -C "$ROOT" rev-parse HEAD)
python3 - "$OUT/ARTIFACT_INVOCATION.json" "$ROOT" "$HEAD" "$ORACLE" "$DEFAULT_ORACLE" <<'PY'
import hashlib,json,sys
from pathlib import Path
out,root,head,oracle,default=Path(sys.argv[1]),Path(sys.argv[2]),sys.argv[3],Path(sys.argv[4]).resolve(),Path(sys.argv[5]).resolve()
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for block in iter(lambda:f.read(1<<20),b''):h.update(block)
 return h.hexdigest()
out.write_text(json.dumps({'schema':'rq1_artifact_invocation_v1','repository_root':str(root.resolve()),'git_head':head,'oracle':str(oracle),'oracle_sha256':sha(oracle),'canonical_oracle':oracle==default},indent=2,sort_keys=True)+'\n')
PY

WORKTREE="$ROOT" EXPECTED_HEAD="$HEAD" bash "$KIT/scripts/bootstrap.sh" "$BOOT" build
WORKTREE="$ROOT" EXPECTED_HEAD="$HEAD" bash "$KIT/scripts/bootstrap.sh" "$BOOT" ffi-smoke
python3 "$KIT/scripts/run_rq1_final.py" \
  --worktree "$ROOT" --bootstrap "$BOOT" --oracle "$ORACLE" --out "$CAMPAIGN"

python3 - "$CAMPAIGN" <<'PY'
import csv,json,sys
from pathlib import Path
c=Path(sys.argv[1]);s=json.loads((c/'EXECUTION_SUMMARY.json').read_text())
assert s['five_query_rows']==875 and s['successful_five_query_rows']==875
assert s['primary_rows']==700 and s['successful_primary_rows']==700
assert s['df_primary_query_ids']==['DF_NORMAL'] and s['status']=='PASS'
with (c/'PRIMARY_RESULTS.tsv').open(newline='') as f: rows=list(csv.DictReader(f,delimiter='\t'))
assert len(rows)==700 and all(r['execution_status']=='OK' for r in rows)
keys={(r['target_id'],r['query_id']) for r in rows};assert len(keys)==700
expected={'ML','DF_NORMAL','UAF','ALLOC_MISMATCH'}
by_target={}
for r in rows:by_target.setdefault(r['target_id'],set()).add(r['query_id'])
assert len(by_target)==175 and all(q==expected for q in by_target.values())
with (c/'ALL_FIVE_QUERY_RESULTS.tsv').open(newline='') as f: all_rows=list(csv.DictReader(f,delimiter='\t'))
assert len(all_rows)==875 and all(r['execution_status']=='OK' for r in all_rows)
assert len({(r['target_id'],r['query_id']) for r in all_rows})==875
PY

python3 "$KIT/scripts/report_rq1_tables.py" \
  --campaign "$CAMPAIGN" --oracle "$ORACLE" \
  --capabilities "$KIT/A3_TARGET_CAPABILITIES.tsv" \
  --fixture-source "$FIXTURE" --out "$TABLES"

for file in EXECUTION_SUMMARY.json PRIMARY_RESULTS.tsv ALL_FIVE_QUERY_RESULTS.tsv; do
  test -s "$CAMPAIGN/$file" || { echo "Missing campaign output: $file" >&2; exit 3; }
done
for file in RQ1_CLASSIFICATION.tsv RQ1_PER_QUERY.csv RQ1_PER_QUERY.tex \
            RQ1_GLOBAL_MICRO.csv RQ1_GLOBAL_MICRO.tex RQ1_REPORT_PROVENANCE.json; do
  test -s "$TABLES/$file" || { echo "Missing report output: $file" >&2; exit 3; }
done

echo "RQ1 artifact completed"
echo "root:      $ROOT"
echo "bootstrap: $BOOT"
echo "campaign:  $CAMPAIGN"
echo "tables:    $TABLES"
echo "micro metrics:"
cat "$TABLES/RQ1_GLOBAL_MICRO.csv"
