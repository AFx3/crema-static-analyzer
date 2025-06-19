#!/usr/bin/env bash
set -euo pipefail

LOG="Z-only_rust_mix.log"
: > "$LOG"   # truncate any existing file

# base path
BASE="../tests_and_target_repos/a-code_full_rust"

# list of only_rust_mix project folder names
PROJECTS=(
  clean_cstring_no_errors_only_rust
  cstringcargo_enum_df_only_rust
  struct_point_df_only_rust
  clean_into_from_raw
  cstring_df_only_rust
  struct_point_mem_leak
  clean_struct_point_mem_leak_into_raw_no_errors
  explicit_drop_df_only_rust
  uaf_read_ptr
  closure_df
  std_mem_forget_mem_leak_rust
)

for proj in "${PROJECTS[@]}"; do
  DIR="$BASE/$proj"
  if [[ -d "$DIR" ]]; then
    {
      echo "=== $proj ==="
      cargo run "$DIR" 2>&1 \
        | grep -E '🤖💬 Potential memory issues detected|Double Free Issues|Never Free Issues|Free detected at source line|NO Issues detected:|🪲  WARNING:|Use-After-Free Issues / Undefined behaviour|Use detected at source line:'
      echo
    } | tee -a "$LOG"
  else
    echo "Warning: directory '$DIR' not found, skipping." >&2
  fi
done

echo "All done. Collected memory‐issue summaries in '$LOG'."
