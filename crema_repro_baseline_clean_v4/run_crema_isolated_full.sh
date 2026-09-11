#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CREMA_REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
CREMA="$ROOT/crema"
TESTS="$ROOT/tests_and_target_repos"
SVF_EXAMPLE="$ROOT/SVF-example"
TOOLCHAIN="${CREMA_TOOLCHAIN:-nightly-2024-11-21}"
OUT="${1:-$ROOT/repro-results/isolated-full92-$(date -u +%Y%m%dT%H%M%SZ)}"

die(){ echo "ERROR: $*" >&2; exit 2; }
[[ -f "$CREMA/Cargo.toml" ]] || die "missing $CREMA/Cargo.toml"
[[ -x "$SVF_EXAMPLE/src/svf-example" ]] || die "missing SVF driver: $SVF_EXAMPLE/src/svf-example"
mkdir -p "$OUT/raw"
printf "key\tgroup\ttarget\tentry_point\texit_code\n" > "$OUT/status.tsv"
printf "openapi-client-gen\n" > "$OUT/excluded-targets.txt"

{
  echo "timestamp_utc=$(date -u +%FT%TZ)"
  echo "repo_root=$ROOT"
  echo "git_commit=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo UNKNOWN)"
  echo "requested_toolchain=$TOOLCHAIN"
  echo "--- root/default ---"; rustc -vV 2>&1 || true
  echo "--- target build toolchain ---"; rustc +"$TOOLCHAIN" -vV
  echo "--- crema directory toolchain ---"; (cd "$CREMA" && rustc -vV)
  echo "--- cargo target build ---"; cargo +"$TOOLCHAIN" --version
  echo "--- cargo crema dir ---"; (cd "$CREMA" && cargo --version)
  echo "--- clang ---"; clang --version | head -1
  echo "--- hashes ---"
  sha256sum "$CREMA/src/abstract_domain.rs" "$SVF_EXAMPLE/src/svf-example"
} > "$OUT/environment.txt"

clean_analysis_artifacts(){
  rm -f "$CREMA/ffi_functions.json" "$CREMA/global_icfg.json" "$CREMA/global_icfg_nodes_edges.dot" || true
  rm -f "$SVF_EXAMPLE/ffi.ll" || true
  [[ -d "$SVF_EXAMPLE/output" ]] && find "$SVF_EXAMPLE/output" -maxdepth 1 -type f -name '*_A_FINAL_ICFG.json' -delete 2>/dev/null || true
}

run_one(){
  local rel="$1" group="$2" key="$3" entry="${4:-}"
  echo "BUILD [$group] $rel"
  cargo +"$TOOLCHAIN" build --manifest-path "$TESTS/$rel/Cargo.toml" >/dev/null 2>&1
  local brc=$?
  if [[ $brc -ne 0 ]]; then
    printf "%s\t%s\t%s\t%s\t125\n" "$key" "$group" "$rel" "$entry" >> "$OUT/status.tsv"
    echo "  BUILD_FAIL rc=$brc"
    return
  fi

  clean_analysis_artifacts
  mkdir -p "$OUT/raw/$group"
  local raw="$OUT/raw/$group/$key.log"
  echo "RUN   [$group] $key"
  if [[ -n "$entry" ]]; then
    (cd "$CREMA" && cargo +"$TOOLCHAIN" run -- "$TESTS/$rel" -f "$entry") >"$raw" 2>&1
  else
    (cd "$CREMA" && cargo +"$TOOLCHAIN" run -- "$TESTS/$rel") >"$raw" 2>&1
  fi
  local rc=$?
  printf "%s\t%s\t%s\t%s\t%s\n" "$key" "$group" "$rel" "$entry" "$rc" >> "$OUT/status.tsv"

  # Capture the exact MIR/ICFG surface before the next target cleans artifacts.
  if [[ -f "$CREMA/global_icfg.json" ]]; then
    python3 "$SCRIPT_DIR/mir_census.py" one "$CREMA/global_icfg.json" "$key" \
      -o "$OUT/raw/$group/$key.mir-census.json" || true
  fi

  local tmp="$OUT/raw/$group/$key.wrapped.log"
  { echo "=== $key ==="; cat "$raw"; } > "$tmp"
  python3 "$SCRIPT_DIR/normalize_results.py" "$tmp" > "$OUT/raw/$group/$key.normalized.json"
  rm -f "$tmp"

  local classes
  classes="$(python3 - <<PY
import json
d=json.load(open("$OUT/raw/$group/$key.normalized.json"))
print(d.get("$key"))
PY
)"
  echo "  rc=$rc classes=$classes"
}

types=(bool char f32 f64 i128 i16 i32 i64 i8 isize u128 u16 u32 u64 u8 usize)
for t in "${types[@]}"; do run_one "a-code_full_rust/a-double_free_full_rust_literals/boxed_$t" literals_df "boxed_${t}__df"; done
for t in "${types[@]}"; do run_one "a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_$t" literals_ml "boxed_${t}__ml"; done
for t in "${types[@]}"; do run_one "a-code_full_rust/a-use_after_free_full_rust_literals/boxed_$t" literals_uaf "boxed_${t}__uaf"; done

mixed=(
"clean_cstring_no_errors_only_rust"
"cstringcargo_enum_df_only_rust"
"struct_point_df_only_rust"
"clean_into_from_raw"
"cstring_df_only_rust"
"struct_point_mem_leak"
"clean_struct_point_mem_leak_into_raw_no_errors"
"explicit_drop_df_only_rust"
"uaf_read_ptr"
"closure_df"
"std_mem_forget_mem_leak_rust"
)
for k in "${mixed[@]}"; do run_one "a-code_full_rust/$k" mixed_rust "$k"; done

ffi=(
"branch_df_mem_leak_ffi"
"clean_mul_fn_ffi_no_errors"
"cstr_cargo_df_ffi"
"cstr_expect_uaf_and_ub_ffi"
"cstringcargo_df_ffi"
"df_rand_cargo_c_ffi"
"for_df_ffi"
"for_memory_leak_ffi"
"uaf_mem_leak_ffi"
"vuln_only_mem_leak_but_df_branch_overapprox_FFI"
"warning_ub_bool"
"warning_ub_int"
"warning_ub_mult"
"warning_ub_string"
)
for k in "${ffi[@]}"; do run_one "a-code_c_ffi/$k" ffi "$k"; done

run_one "found vulns/noGenerator" github noGenerator
run_one "found vulns/wasm-demo" github wasm-demo "rust::demo_new_state::bb0"
run_one "found vulns/skip-list-test" github skip-list-test
run_one "found vulns/rusant" github rusant
run_one "found vulns/napkin-math-test" github napkin-math-test
run_one "found vulns/shared-register" github shared-register
run_one "found vulns/whisper-rs-example" github whisper-rs-example
run_one "found vulns/lock-free" github lock-free "rust::LockFreeStack::::push::bb0"

noerr=(
"rust_memory"
"unsized_struct"
"unsafely-created-owned-type"
"lock_free_non_blocking_linked_list"
"c-callback-rust-closure"
"concurrent-verification"
"stackswap-coroutines"
"rc-playground"
"square"
"rust_hw3"
"rust-boks"
)
for k in "${noerr[@]}"; do run_one "no_errors_projects/$k" github "$k"; done

python3 - "$OUT" <<'PY'
from pathlib import Path
import json, sys
out=Path(sys.argv[1])
agg={}
for f in sorted(out.rglob("*.normalized.json")):
    agg.update(json.loads(f.read_text()))
(out/"results.normalized.json").write_text(json.dumps(agg,indent=2,sort_keys=True)+"\n")
PY

mapfile -d '' census_files < <(find "$OUT/raw" -type f -name '*.mir-census.json' -print0 | sort -z)
if (( ${#census_files[@]} > 0 )); then
  python3 "$SCRIPT_DIR/mir_census.py" aggregate "${census_files[@]}" \
    -o "$OUT/mir-census.aggregate.json"
fi

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum
) > "$OUT/SHA256SUMS"

total=$(tail -n +2 "$OUT/status.tsv" | wc -l)
failed=$(awk -F'\t' 'NR>1 && $5 != 0 {n++} END {print n+0}' "$OUT/status.tsv")
if [[ "$total" -ne 92 ]]; then
  echo "ERROR: protocol expected exactly 92 targets, got $total" >&2
  exit 3
fi
echo
echo "Isolated CREMA full92 completed: total=$total failed=$failed"
echo "Excluded by protocol: openapi-client-gen"
echo "$OUT"
