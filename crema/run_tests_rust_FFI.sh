#!/usr/bin/env bash
set -euo pipefail

LOG="Z-rust_FFI_mix.log"
: > "$LOG"

BASE="../tests_and_target_repos/a-code_c_ffi"
PROJECTS=(
  branch_df_mem_leak_ffi
  clean_mul_fn_ffi_no_errors
  cstr_cargo_df_ffi
  cstr_expect_uaf_and_ub_ffi
  cstringcargo_df_ffi
  df_rand_cargo_c_ffi
  for_df_ffi
  for_memory_leak_ffi
  uaf_mem_leak_ffi
  vuln_only_mem_leak_but_df_branch_overapprox_FFI
  warning_ub_bool
  warning_ub_int
  warning_ub_mult
  warning_ub_string
)

# match banners AND detail lines; -A1 grabs the line after each match
PATTERN="🤖💬 Potential memory issues detected|☢ Double Free Issues|☢ Never Free Issues|☢ Use-After-Free Issues|Free detected at source line|Use detected at source line|Possible UNDEFINED BEHAVIOUR!|🪲  WARNING:"

for proj in "${PROJECTS[@]}"; do
  DIR="$BASE/$proj"
  if [[ ! -d "$DIR" ]]; then
    echo "Warning: directory '$DIR' not found, skipping." >&2
    continue
  fi

  {
    echo "=== $proj ==="
    # merge stderr, grep with -A1, and if no matches print fallback
    cargo run "$DIR" 2>&1 \
      | grep -E -A1 --no-group-separator "$PATTERN" \
      || echo "(no memory‑issues detected)"
    echo
  } | tee -a "$LOG"
done

echo "Done. Filtered results are in '$LOG'."
