#!/usr/bin/env bash
# cqpl/benchmarks/rustsec_memory_safety_v1/gate_b1_1_candidate_set.sh
#
# B1.1 RustSec candidate set gate.
#
# Verifica che il repo sia ancorato al freeze di B1.1 (tag cqpl-v6v-b1-1-r1)
# e che il candidate set B1.1 sia internamente consistente.
#
# Modalità B1.2:
#   Se esiste almeno un cases/<id>/case.json con
#   materialization_status=="admitted", il gate rileva automaticamente
#   la B1.2 mode ed esenta dai GENERATED_SYNC i due file che B1.2
#   modifica legittimamente:
#     - ground_truth.json     (case.status candidate -> admitted + hashes)
#     - selection_summary.json (admitted_cases/subject_rows da 0 -> N/2N)
#   candidate_selection.tsv e expected_capabilities.tsv restano verificati
#   in GENERATED_SYNC anche in B1.2 mode, perché B1.2 non li tocca.
#
# Override esplicito:
#   B1_2_ALLOW=1   forza la B1.2 mode
#   B1_2_ALLOW=0   forza la B1.1 mode (utile per verificare retro-compat)

set +e

ROOT="${1:-/home/af/Documenti/a-phd}"

TAG="${B1_1_BASE_TAG:-cqpl-v6v-b1-1-r1}"
EXPECTED_VERSION="${B1_1_EXPECTED_VERSION:-CREMA-CQPL-v6V-B1.1-r1}"

cd "$ROOT" || exit 2

FAIL=0

echo "============================================================"
echo " B1.1 RUSTSEC CANDIDATE SET GATE"
echo "============================================================"
echo " base_tag         = $TAG"
echo " expected_version = $EXPECTED_VERSION"

# --- 0. B1.2 mode detection -------------------------------------------
BENCH_REL="cqpl/benchmarks/rustsec_memory_safety_v1"

B1_2_DETECTED=0
if [[ -d "$BENCH_REL/cases" ]]; then
    for cj in "$BENCH_REL"/cases/*/case.json; do
        [[ -f "$cj" ]] || continue
        if grep -q '"materialization_status"[[:space:]]*:[[:space:]]*"admitted"' "$cj"; then
            B1_2_DETECTED=1
            break
        fi
    done
fi

case "${B1_2_ALLOW:-}" in
    1) B1_2_MODE=1 ;;
    0) B1_2_MODE=0 ;;
    *) B1_2_MODE=$B1_2_DETECTED ;;
esac

echo " b1_2_mode        = $B1_2_MODE (detected=$B1_2_DETECTED, B1_2_ALLOW=${B1_2_ALLOW:-auto})"
echo "------------------------------------------------------------"

# --- 1. tag exists ----------------------------------------------------
if ! git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "BASE_TAG_EXISTS: FAIL tag=$TAG"
    exit 3
fi

HEAD_SHA=$(git rev-parse HEAD)
TAG_SHA=$(git rev-list -n 1 "$TAG")

echo "head=$HEAD_SHA"
echo "tag_commit=$TAG_SHA"

# --- 2. HEAD descends from B1.1 freeze --------------------------------
if git merge-base --is-ancestor "$TAG" HEAD; then
    echo "BASE_TAG_BINDING: PASS (HEAD contains $TAG)"
else
    echo "BASE_TAG_BINDING: FAIL (HEAD does not descend from $TAG)"
    FAIL=1
fi

# --- 3. VERSION --------------------------------------------------------
VERSION=$(cat cqpl/VERSION 2>/dev/null)

if [[ "$VERSION" == "$EXPECTED_VERSION" ]]; then
    echo "BASE_VERSION: PASS"
else
    echo "BASE_VERSION: FAIL value=$VERSION expected=$EXPECTED_VERSION"
    FAIL=1
fi

# --- 4. index empty ----------------------------------------------------
git diff --cached --quiet
RC_INDEX=$?

if [[ $RC_INDEX -eq 0 ]]; then
    echo "INDEX_EMPTY: PASS"
else
    echo "INDEX_EMPTY: FAIL"
    FAIL=1
fi

# --- 5. frozen subtrees ------------------------------------------------
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

# --- 6. verify benchmark (B1.1 semantic gate) -------------------------
BENCH="$BENCH_REL"

PYTHONDONTWRITEBYTECODE=1 \
python3 "$BENCH/verify_benchmark_v1.py" "$BENCH"
RC_VERIFY=$?
[[ $RC_VERIFY -eq 0 ]] || FAIL=1

# --- 7. deterministic regeneration + sync check -----------------------
TMP=$(mktemp -d)

PYTHONDONTWRITEBYTECODE=1 \
python3 \
    "$BENCH/generate_b1_1_candidate_set.py" \
    --benchmark-dir "$BENCH" \
    --out-dir "$TMP"

RC_GENERATE=$?

# Files whose regenerated form is expected to differ under B1.2
B1_2_MUTABLE=(
    "ground_truth.json"
    "selection_summary.json"
)

ALL_SYNC_FILES=(
    "ground_truth.json"
    "candidate_selection.tsv"
    "expected_capabilities.tsv"
    "selection_summary.json"
)

is_b1_2_mutable() {
    local f="$1"
    for m in "${B1_2_MUTABLE[@]}"; do
        [[ "$f" == "$m" ]] && return 0
    done
    return 1
}

for p in "${ALL_SYNC_FILES[@]}"; do
    if [[ $B1_2_MODE -eq 1 ]] && is_b1_2_mutable "$p"; then
        echo "GENERATED_SYNC $p: SKIP (B1.2 mode, file legitimately divergent)"
        continue
    fi

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
echo "b1_2_mode=$B1_2_MODE"

if [[ $FAIL -eq 0 ]]; then
    echo "V6V_B1_1_GATE: PASS"
    exit 0
else
    echo "V6V_B1_1_GATE: FAIL"
    exit 1
fi
