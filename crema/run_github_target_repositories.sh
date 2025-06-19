#!/usr/bin/env bash
set -euo pipefail

LOG="Z-github_target_repos.log"
: > "$LOG"

# 1st folder "found vulns"
BASE1="../tests_and_target_repos/found vulns"
PROJECTS1=( noGenerator wasm-demo skip-list-test rusant napkin-math-test shared-register whisper-rs-example lock-free )
FLAGS1=( "" "rust::demo_new_state::bb0" "" "" "" "" "" "rust::LockFreeStack::::push::bb0" )

# 2nd folder: "no_errors_projects"
BASE2="../tests_and_target_repos/no_errors_projects"
PROJECTS2=( rust_memory unsized_struct unsafely-created-owned-type lock_free_non_blocking_linked_list c-callback-rust-closure concurrent-verification stackswap-coroutines rc-playground square rust_hw3 openapi-client-gen rust-boks )
FLAGS2=()
for ((i=0; i<${#PROJECTS2[@]}; i++)); do
  FLAGS2+=( "" )
done

PATTERN="🤖💬 Potential memory issues detected|☢ Double Free Issues|☢ Never Free Issues|☢ Use-After-Free Issues|Free detected at source line|Use detected at source line|Possible UNDEFINED BEHAVIOUR!|🪲  WARNING:|🤖💬 NO Issues detected:"

process_set() {
  local BASE="$1"; shift
  local -n PROJECTS=$1; shift
  local -n FLAGS=$1; shift

  for idx in "${!PROJECTS[@]}"; do
    local proj="${PROJECTS[idx]}"
    local flag="${FLAGS[idx]}"
    local DIR="$BASE/$proj"

    if [[ ! -d "$DIR" ]]; then
      echo "Warning: directory '$DIR' not found, skipping." >&2
      continue
    fi

    {
      echo "=== $proj ==="
      output=$(cargo run "$DIR" ${flag:+-f "$flag"} 2>&1) || true

      # swallow grep failures so never exit here
      echo "$output" \
        | grep -E -A1 --no-group-separator "$PATTERN" \
        || true

      echo
    } | tee -a "$LOG"
  done
}

process_set "$BASE1" PROJECTS1 FLAGS1
process_set "$BASE2" PROJECTS2 FLAGS2

echo "Done. Filtered output saved in '$LOG'."
