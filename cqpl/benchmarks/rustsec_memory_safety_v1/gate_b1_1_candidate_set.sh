#!/usr/bin/env bash
set +e

ROOT="${1:-/home/af/Documenti/a-phd}"
TAG="${B1_1_BASE_TAG:-cqpl-v6u-a2-r1}"

cd "$ROOT" || exit 2

FAIL=0

echo "============================================================"
echo " B1.1 RUSTSEC CANDIDATE SET GATE"
echo "============================================================"

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "BASE_TAG_EXISTS: FAIL"
    exit 3
fi

HEAD_SHA=$(git rev-parse HEAD)
TAG_SHA=$(git rev-list -n 1 "$TAG")

echo "head=$HEAD_SHA"
echo "tag_commit=$TAG_SHA"

if [[ "$HEAD_SHA" == "$TAG_SHA" ]]; then
    echo "BASE_TAG_BINDING: PASS"
else
    echo "BASE_TAG_BINDING: FAIL"
    FAIL=1
fi

VERSION=$(cat cqpl/VERSION 2>/dev/null)

if [[ "$VERSION" == "CREMA-CQPL-v6U-A2-r1" ]]; then
    echo "BASE_VERSION: PASS"
else
    echo "BASE_VERSION: FAIL value=$VERSION"
    FAIL=1
fi

git diff --cached --quiet
RC_INDEX=$?

if [[ $RC_INDEX -eq 0 ]]; then
    echo "INDEX_EMPTY: PASS"
else
    echo "INDEX_EMPTY: FAIL"
    FAIL=1
fi

git diff --quiet "$TAG" -- cqpl/queries_v2
RC_QUERIES=$?

if [[ $RC_QUERIES -eq 0 ]]; then
    echo "QUERIES_V2_FROZEN: PASS"
else
    echo "QUERIES_V2_FROZEN: FAIL"
    FAIL=1
fi

git diff --quiet "$TAG" -- crema/src cqpl/cqpl_checker/src
RC_PROD=$?

if [[ $RC_PROD -eq 0 ]]; then
    echo "ACTIVE_SEMANTICS_UNCHANGED: PASS"
else
    echo "ACTIVE_SEMANTICS_UNCHANGED: FAIL"
    git diff --name-status "$TAG" -- crema/src cqpl/cqpl_checker/src
    FAIL=1
fi

BENCH="cqpl/benchmarks/rustsec_memory_safety_v1"

PYTHONDONTWRITEBYTECODE=1 \
python3 "$BENCH/verify_benchmark_v1.py" "$BENCH"

RC_VERIFY=$?
[[ $RC_VERIFY -eq 0 ]] || FAIL=1

TMP=$(mktemp -d)

PYTHONDONTWRITEBYTECODE=1 \
python3 \
    "$BENCH/generate_b1_1_candidate_set.py" \
    --benchmark-dir "$BENCH" \
    --out-dir "$TMP"

RC_GENERATE=$?

for p in \
    ground_truth.json \
    candidate_selection.tsv \
    expected_capabilities.tsv \
    selection_summary.json
do
    cmp -s "$TMP/$p" "$BENCH/$p"
    rc=$?

    if [[ $rc -eq 0 ]]; then
        echo "GENERATED_SYNC $p: PASS"
    else
        echo "GENERATED_SYNC $p: FAIL"
        FAIL=1
    fi
done

rm -rf "$TMP"

[[ $RC_GENERATE -eq 0 ]] || FAIL=1

echo
echo "============================================================"
echo " B1.1 SUMMARY"
echo "============================================================"
echo "verify_rc=$RC_VERIFY"
echo "generate_rc=$RC_GENERATE"

if [[ $FAIL -eq 0 ]]; then
    echo "V6V_B1_1_GATE: PASS"
    exit 0
else
    echo "V6V_B1_1_GATE: FAIL"
    exit 1
fi
