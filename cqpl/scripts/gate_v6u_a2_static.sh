#!/usr/bin/env bash
set +e
ROOT="${1:-/home/af/Documenti/a-phd}"; BASE=88cd3e5c3148fe21776a7d9f6efbd9f7a20461b0; cd "$ROOT" || exit 2; FAIL=0
echo "=== V6U-A2 STATIC ==="
[[ "$(git rev-parse HEAD)" == "$BASE" ]] || { echo "BASELINE_HEAD: FAIL"; FAIL=1; }
git diff --cached --quiet || { echo "INDEX_EMPTY: FAIL"; FAIL=1; }
[[ -z "$(git diff --name-only "$BASE" -- cqpl/queries_v2)" ]] || { echo "QUERIES_V2_FROZEN: FAIL"; FAIL=1; }
for p in crema/src/icfg.rs crema/src/structs.rs crema/src/abstract_domain.rs crema/src/identity.rs cqpl/cqpl_checker/src; do
  git diff --quiet "$BASE" -- "$p" || { echo "UNEXPECTED_DIFF $p"; FAIL=1; }
done
PROD=$(git diff --name-only "$BASE" -- crema/src | sort); EXPECTED=$'crema/src/cqpl_export.rs\ncrema/src/main.rs'
[[ "$PROD" == "$EXPECTED" ]] || { echo "TRACKED_PRODUCTION_SCOPE: FAIL"; printf '%s\n' "$PROD"; FAIL=1; }
PYTHONDONTWRITEBYTECODE=1 python3 cqpl/scripts/test_library_effects_v1.py; R1=$?; [[ $R1 -eq 0 ]] || FAIL=1
PYTHONDONTWRITEBYTECODE=1 python3 cqpl/scripts/verify_library_effects_v1.py "$ROOT" --rust-status candidate --expected-rust-summaries 5; R2=$?; [[ $R2 -eq 0 ]] || FAIL=1
TMP=$(mktemp); PYTHONDONTWRITEBYTECODE=1 python3 cqpl/scripts/generate_library_effects_v1_rust.py --root "$ROOT" --out "$TMP"; R3=$?; cmp -s "$TMP" crema/src/library_effects_v1_generated.rs; R4=$?; rm -f "$TMP"; [[ $R3 -eq 0 && $R4 -eq 0 ]] || FAIL=1
if rg -n 'rustc_(box_into_raw|box_from_raw|box_leak|mem_forget_owned_box|mem_drop_raw_pointer)_v1' crema/src/cqpl_export.rs; then echo "HARDCODED_PROJECTION: FAIL"; FAIL=1; fi
rg -n 'allocation_disposition_projection_for_evidence' crema/src/cqpl_export.rs >/dev/null || { echo "REGISTRY_PROJECTION_USE: FAIL"; FAIL=1; }
git diff --check -- crema/src/cqpl_export.rs crema/src/main.rs; R5=$?; [[ $R5 -eq 0 ]] || FAIL=1
if [[ $FAIL -eq 0 ]]; then echo "V6U_A2_STATIC_GATE: PASS"; exit 0; else echo "V6U_A2_STATIC_GATE: FAIL"; exit 1; fi
