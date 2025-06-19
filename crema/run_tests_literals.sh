#!/usr/bin/env bash
set -euo pipefail

LOG_DF="z-DF_errors.log"
: > "$LOG_DF"   # truncate

BASE="../tests_and_target_repos/a-code_full_rust/a-double_free_full_rust_literals"
TYPES=( bool char f32 f64 i128 i16 i32 i64 i8 isize u128 u16 u32 u64 u8 usize )

for t in "${TYPES[@]}"; do
  DIR="$BASE/boxed_$t"
  if [[ -d "$DIR" ]]; then
    {
      echo "=== boxed_$t ==="
      # run and filter both stdout and stderr
      cargo run "$DIR" 2>&1 \
        | grep -E '🤖💬 Potential memory issues detected|Double Free Issues|Free detected at source line'
      echo
    } | tee -a "$LOG_DF"
  else
    echo "Warning: directory '$DIR' not found, skipping." >&2
  fi
done

echo "Done. DOUBLE FREES issue summaries are in $LOG_DF."

LOG_ML="z-ML_errors.log"
: > "$LOG_ML"   # truncate
BASE="../tests_and_target_repos/a-code_full_rust/a-memory_leaks_full_rust_literals"
TYPES=( bool char f32 f64 i128 i16 i32 i64 i8 isize u128 u16 u32 u64 u8 usize )

for t in "${TYPES[@]}"; do
  DIR="$BASE/boxed_$t"
  if [[ -d "$DIR" ]]; then
    {
      echo "=== boxed_$t ==="
      # run and filter both stdout and stderr
      cargo run "$DIR" 2>&1 \
        | grep -E '🤖💬 Potential memory issues detected|Never Free Issues'
      echo
    } | tee -a "$LOG_ML"
  else
    echo "Warning: directory '$DIR' not found, skipping." >&2
  fi
done

echo "Done. MEMORY LEAKS issue summaries are in $LOG_ML."


LOG_UAF="z-UAF_errors.log"
: > "$LOG_UAF"   # truncate
BASE="../tests_and_target_repos/a-code_full_rust/a-use_after_free_full_rust_literals"
TYPES=( bool char f32 f64 i128 i16 i32 i64 i8 isize u128 u16 u32 u64 u8 usize )

for t in "${TYPES[@]}"; do
  DIR="$BASE/boxed_$t"
  if [[ -d "$DIR" ]]; then
    {
      echo "=== boxed_$t ==="
      # run and filter both stdout and stderr
      cargo run "$DIR" 2>&1 \
        | grep -E '🤖💬 Potential memory issues detected|Use-After-Free Issues / Undefined behaviour|Use detected at source line'
      echo
    } | tee -a "$LOG_UAF"
  else
    echo "Warning: directory '$DIR' not found, skipping." >&2
  fi
done

echo "Done. UAF issue summaries are in $LOG_UAF."