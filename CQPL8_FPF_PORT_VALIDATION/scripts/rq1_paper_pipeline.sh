#!/usr/bin/env bash
set -euo pipefail

MODE=${1:-}
BOOT=${2:-}
OUT=${3:-}
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PORT=${WORKTREE:-$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)}
KIT="$PORT/CQPL8_FPF_PORT_VALIDATION"
ORACLE=${ORACLE:-$PORT/cqpl/artifact/RQ1_ORACLE_175.csv}

if [[ "$MODE" != --full && "$MODE" != --smoke-cstringcargo ]]; then
  echo "Usage: $0 {--full|--smoke-cstringcargo} NEW_BOOTSTRAP_DIR NEW_OUTPUT_DIR" >&2
  exit 2
fi
[[ -n "$BOOT" && -n "$OUT" ]] || { echo "Fresh bootstrap and output paths are required" >&2; exit 2; }
[[ ! -e "$BOOT" && ! -e "$OUT" ]] || {
  echo "Refusing historical/reused bootstrap or output directory" >&2; exit 2;
}

# Always compile the current source into a fresh dedicated bootstrap. The RQ1
# runner then checks all recorded source and binary hashes before any export.
bash "$KIT/scripts/bootstrap.sh" "$BOOT" build
bash "$KIT/scripts/bootstrap.sh" "$BOOT" ffi-smoke

args=(--worktree "$PORT" --bootstrap "$BOOT" --oracle "$ORACLE" --out "$OUT")
if [[ "$MODE" == --smoke-cstringcargo ]]; then
  args+=(--only a-code_full_rust/cstringcargo_enum_df_only_rust)
fi
python3 "$KIT/scripts/run_rq1_final.py" "${args[@]}"
