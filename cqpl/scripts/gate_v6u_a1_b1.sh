#!/usr/bin/env bash
set +e

ROOT="${1:-/home/af/Documenti/a-phd}"
BASE_COMMIT="88cd3e5c3148fe21776a7d9f6efbd9f7a20461b0"
BASE_TAG="cqpl-v6t-r1"

cd "$ROOT" || {
  echo "ERROR: cannot cd to $ROOT"
  exit 2
}

echo "============================================================"
echo " CREMA/CQPL v6U A1 + B1 GATE"
echo "============================================================"

RC_BASE=0
RC_PYCOMPILE=0
RC_UNIT=0
RC_MODELS=0
RC_BENCH=0
RC_QUERIES=0
RC_PROD=0

HEAD=$(git rev-parse HEAD 2>/dev/null)
TAG_COMMIT=$(git rev-list -n 1 "$BASE_TAG" 2>/dev/null)

echo "head=$HEAD"
echo "baseline=$BASE_COMMIT"
echo "tag_commit=$TAG_COMMIT"

if [[ "$HEAD" == "$BASE_COMMIT" && "$TAG_COMMIT" == "$BASE_COMMIT" ]]; then
  echo "BASELINE_IDENTITY: PASS"
else
  echo "BASELINE_IDENTITY: FAIL"
  RC_BASE=1
fi

echo
echo "=== Python syntax ==="
python3 -m py_compile   cqpl/scripts/library_effects_v1.py   cqpl/scripts/test_library_effects_v1.py   cqpl/scripts/verify_library_effects_v1.py   cqpl/benchmarks/rustsec_memory_safety_v1/verify_benchmark_v1.py
RC_PYCOMPILE=$?
echo "python_compile_rc=$RC_PYCOMPILE"

echo
echo "=== library_effects_v1 unit tests ==="
python3 cqpl/scripts/test_library_effects_v1.py
RC_UNIT=$?

echo
echo "=== library model registries ==="
python3 cqpl/scripts/verify_library_effects_v1.py "$ROOT"
RC_MODELS=$?

echo
echo "=== RustSec benchmark scaffold ==="
python3   cqpl/benchmarks/rustsec_memory_safety_v1/verify_benchmark_v1.py   "$ROOT/cqpl/benchmarks/rustsec_memory_safety_v1"
RC_BENCH=$?

echo
echo "=== frozen queries ==="
QUERY_DELTA=$(git diff --name-only "$BASE_COMMIT" -- cqpl/queries_v2)
if [[ -z "$QUERY_DELTA" ]]; then
  echo "QUERIES_V2_FROZEN: PASS"
else
  echo "QUERIES_V2_FROZEN: FAIL"
  printf '%s\n' "$QUERY_DELTA"
  RC_QUERIES=1
fi

echo
echo "=== active production source ==="
PROD_DELTA=$(git diff --name-only "$BASE_COMMIT" --   crema/src   cqpl/cqpl_checker/src   cqpl/queries_v2)

if [[ -z "$PROD_DELTA" ]]; then
  echo "ACTIVE_PRODUCTION_UNCHANGED: PASS"
else
  echo "ACTIVE_PRODUCTION_UNCHANGED: FAIL"
  printf '%s\n' "$PROD_DELTA"
  RC_PROD=1
fi

echo
echo "=== installed candidate files ==="
git status --short --   cqpl/capabilities/library_effects_v1.md   cqpl/library_models   cqpl/scripts/library_effects_v1.py   cqpl/scripts/test_library_effects_v1.py   cqpl/scripts/verify_library_effects_v1.py   cqpl/scripts/gate_v6u_a1_b1.sh   cqpl/benchmarks/rustsec_memory_safety_v1

echo
echo "============================================================"
echo " A1+B1 SUMMARY"
echo "============================================================"
echo "baseline_rc=$RC_BASE"
echo "python_compile_rc=$RC_PYCOMPILE"
echo "unit_rc=$RC_UNIT"
echo "models_rc=$RC_MODELS"
echo "benchmark_rc=$RC_BENCH"
echo "queries_rc=$RC_QUERIES"
echo "production_rc=$RC_PROD"

FAIL=0
for rc in   "$RC_BASE"   "$RC_PYCOMPILE"   "$RC_UNIT"   "$RC_MODELS"   "$RC_BENCH"   "$RC_QUERIES"   "$RC_PROD"
do
  if [[ "$rc" -ne 0 ]]; then
    FAIL=1
  fi
done

if [[ "$FAIL" -eq 0 ]]; then
  echo "V6U_A1_B1_GATE: PASS"
  exit 0
else
  echo "V6U_A1_B1_GATE: FAIL"
  exit 1
fi
