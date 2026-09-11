#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${CREMA_REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
CREMA="$ROOT/crema"
TESTS="$ROOT/tests_and_target_repos"
SVF_EXAMPLE="$ROOT/SVF-example"
TOOLCHAIN="${CREMA_TOOLCHAIN:-nightly-2024-11-21}"
OUT="${1:-$ROOT/repro-results/isolated-smoke92-$(date -u +%Y%m%dT%H%M%SZ)}"

die(){ echo "ERROR: $*" >&2; exit 2; }
[[ -f "$CREMA/Cargo.toml" ]] || die "missing $CREMA/Cargo.toml"
[[ -x "$SVF_EXAMPLE/src/svf-example" ]] || die "missing SVF driver: $SVF_EXAMPLE/src/svf-example"

mkdir -p "$OUT/raw"
printf "key\tgroup\ttarget\texit_code\n" > "$OUT/status.tsv"
printf "openapi-client-gen\n" > "$OUT/excluded-targets.txt"

{
  echo "timestamp_utc=$(date -u +%FT%TZ)"
  echo "repo_root=$ROOT"
  echo "git_commit=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo UNKNOWN)"
  echo "requested_toolchain=$TOOLCHAIN"
  echo "--- root/default ---"
  rustc -vV 2>&1 || true
  echo "--- target build toolchain ---"
  rustc +"$TOOLCHAIN" -vV
  echo "--- crema directory toolchain ---"
  (cd "$CREMA" && rustc -vV)
  echo "--- cargo target build ---"
  cargo +"$TOOLCHAIN" --version
  echo "--- cargo crema dir ---"
  (cd "$CREMA" && cargo --version)
  echo "--- clang ---"
  clang --version | head -1
  echo "--- hashes ---"
  sha256sum "$CREMA/src/abstract_domain.rs" "$SVF_EXAMPLE/src/svf-example"
} > "$OUT/environment.txt"

clean_analysis_artifacts(){
  rm -f "$CREMA/ffi_functions.json" "$CREMA/global_icfg.json" "$CREMA/global_icfg_nodes_edges.dot" || true
  rm -f "$SVF_EXAMPLE/ffi.ll" || true
  [[ -d "$SVF_EXAMPLE/output" ]] && find "$SVF_EXAMPLE/output" -maxdepth 1 -type f -name '*_A_FINAL_ICFG.json' -delete 2>/dev/null || true
}

targets=(
"a-code_full_rust/a-double_free_full_rust_literals/boxed_i32|df|boxed_i32__df|"
"a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_i32|ml|boxed_i32__ml|"
"a-code_full_rust/a-use_after_free_full_rust_literals/boxed_i32|uaf|boxed_i32__uaf|"
"a-code_full_rust/clean_cstring_no_errors_only_rust|mixed|clean_cstring_no_errors_only_rust|"
"a-code_full_rust/cstringcargo_enum_df_only_rust|mixed|cstringcargo_enum_df_only_rust|"
"a-code_full_rust/uaf_read_ptr|mixed|uaf_read_ptr|"
"a-code_c_ffi/clean_mul_fn_ffi_no_errors|ffi|clean_mul_fn_ffi_no_errors|"
"a-code_c_ffi/cstr_cargo_df_ffi|ffi|cstr_cargo_df_ffi|"
"a-code_c_ffi/uaf_mem_leak_ffi|ffi|uaf_mem_leak_ffi|"
"found vulns/noGenerator|github|noGenerator|"
)

for spec in "${targets[@]}"; do
  IFS='|' read -r rel group key entry <<<"$spec"
  echo "BUILD $rel"
  cargo +"$TOOLCHAIN" build --manifest-path "$TESTS/$rel/Cargo.toml" >/dev/null 2>&1
  rc=$?
  if [[ $rc -ne 0 ]]; then
    echo "  build failed rc=$rc"
    printf "%s\t%s\t%s\t125\n" "$key" "$group" "$rel" >> "$OUT/status.tsv"
    continue
  fi

  clean_analysis_artifacts
  mkdir -p "$OUT/raw/$group"
  raw="$OUT/raw/$group/$key.log"
  echo "RUN $key"

  if [[ -n "$entry" ]]; then
    (cd "$CREMA" && cargo +"$TOOLCHAIN" run -- "$TESTS/$rel" -f "$entry") >"$raw" 2>&1
  else
    (cd "$CREMA" && cargo +"$TOOLCHAIN" run -- "$TESTS/$rel") >"$raw" 2>&1
  fi
  rc=$?
  printf "%s\t%s\t%s\t%s\n" "$key" "$group" "$rel" "$rc" >> "$OUT/status.tsv"

  if [[ -f "$CREMA/global_icfg.json" ]]; then
    python3 "$SCRIPT_DIR/mir_census.py" one "$CREMA/global_icfg.json" "$key" \
      -o "$OUT/raw/$group/$key.mir-census.json" || true
  fi

  tmp="$OUT/raw/$group/$key.wrapped.log"
  { echo "=== $key ==="; cat "$raw"; } > "$tmp"
  python3 "$SCRIPT_DIR/normalize_results.py" "$tmp" > "$OUT/raw/$group/$key.normalized.json"
  rm -f "$tmp"

  classes="$(python3 - <<PY
import json
d=json.load(open("$OUT/raw/$group/$key.normalized.json"))
print(d.get("$key"))
PY
)"
  echo "  rc=$rc classes=$classes"
done

mapfile -d '' census_files < <(find "$OUT/raw" -type f -name '*.mir-census.json' -print0 | sort -z)
if (( ${#census_files[@]} > 0 )); then
  python3 "$SCRIPT_DIR/mir_census.py" aggregate "${census_files[@]}" \
    -o "$OUT/mir-census.aggregate.json"
fi

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum
) > "$OUT/SHA256SUMS"

echo
echo "Isolated CREMA smoke completed:"
echo "$OUT"
