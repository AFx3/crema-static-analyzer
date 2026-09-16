#!/usr/bin/env bash
set +e
ROOT="${1:-/home/af/Documenti/a-phd}"; NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"; BASELINE="${V6U_A2_BASELINE:-$ROOT/repro-results/cqpl-v6t-r1-validation-20260915T181642Z/fresh112}"; STAMP=$(date -u +%Y%m%dT%H%M%SZ); OUT="${V6U_A2_OUT:-$ROOT/repro-results/cqpl-v6u-a2-validation-$STAMP}"; FRESH="$OUT/fresh112"; mkdir -p "$OUT"; FAIL=0
[[ -s "$BASELINE/query-matrix/query-results-long.tsv" && -s "$BASELINE/subjects.tsv" ]] || { echo "V6T_BASELINE: FAIL"; exit 2; }
cargo +"$NIGHTLY" test --manifest-path "$ROOT/crema/Cargo.toml" 2>&1 | tee "$OUT/crema-tests.log"; R1=${PIPESTATUS[0]}; [[ $R1 -eq 0 ]] || FAIL=1
CQPL_RUN_ALL_OUT="$FRESH" CREMA_PHD_ROOT="$ROOT" CREMA_RUST_TOOLCHAIN="$NIGHTLY" bash "$ROOT/cqpl/run_all.sh" 2>&1 | tee "$OUT/run_all.console.log"; R2=${PIPESTATUS[0]}; [[ $R2 -eq 0 ]] || FAIL=1
if [[ $R2 -eq 0 ]]; then PYTHONDONTWRITEBYTECODE=1 python3 "$ROOT/cqpl/scripts/compare_v6t_v6u_a2_equivalence.py" --baseline "$BASELINE" --candidate "$FRESH" --out "$OUT/equivalence.json"; R3=$?; [[ $R3 -eq 0 ]] || FAIL=1; else R3=99; fi
echo "crema_tests_rc=$R1"; echo "run_all_rc=$R2"; echo "equivalence_rc=$R3"; echo "validation_out=$OUT"
if [[ $FAIL -eq 0 ]]; then echo "V6U_A2_FULL_EQUIVALENCE_GATE: PASS"; exit 0; else echo "V6U_A2_FULL_EQUIVALENCE_GATE: FAIL"; exit 1; fi
