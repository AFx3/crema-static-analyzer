#!/usr/bin/env bash
set +e
ROOT="${1:-/home/af/Documenti/a-phd}"; NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"; STAMP=$(date -u +%Y%m%dT%H%M%SZ); OUT="${V6U_A2_FOCUSED_OUT:-$ROOT/repro-results/cqpl-v6u-a2-focused-$STAMP}"; mkdir -p "$OUT"; FAIL=0
run_one(){ local name="$1" rel="$2" mode="$3" dest="$OUT/$1"; python3 "$ROOT/cqpl/scripts/run_one_target_v6q_r1c.py" --root "$ROOT" --relative-path "$rel" --out "$dest" --toolchain "$NIGHTLY" --explain-unk-verbose; local rr=$?; [[ $rr -eq 0 ]] || return $rr; local art; art=$(find "$dest" -type f -name annotated_icfg_v2.json -print | sort | head -n1); [[ -n "$art" ]] || return 90; PYTHONDONTWRITEBYTECODE=1 python3 "$ROOT/cqpl/scripts/check_v6u_a2_focused_artifact.py" "$mode" "$art"; }
run_one boxed_bool_leak 'a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool' leak; R1=$?; [[ $R1 -eq 0 ]] || FAIL=1
run_one clean_into_from_raw 'a-code_full_rust/clean_into_from_raw' roundtrip; R2=$?; [[ $R2 -eq 0 ]] || FAIL=1
mapfile -t D < <(find "$ROOT/tests_and_target_repos" -type d -name drop_raw_ptr_no_free -print | sort)
if [[ ${#D[@]} -eq 1 ]]; then REL="${D[0]#$ROOT/tests_and_target_repos/}"; run_one drop_raw_ptr_no_free "$REL" raw_drop; R3=$?; [[ $R3 -eq 0 ]] || FAIL=1; else echo "RAW_DROP_TARGET_DISCOVERY: FAIL count=${#D[@]}"; R3=91; FAIL=1; fi
echo "leak_rc=$R1"; echo "roundtrip_rc=$R2"; echo "raw_drop_rc=$R3"; echo "focused_out=$OUT"
if [[ $FAIL -eq 0 ]]; then echo "V6U_A2_FOCUSED_GATE: PASS"; exit 0; else echo "V6U_A2_FOCUSED_GATE: FAIL"; exit 1; fi
