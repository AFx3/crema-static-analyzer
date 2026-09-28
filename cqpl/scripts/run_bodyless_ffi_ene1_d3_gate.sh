#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "${1:-$PWD}" && pwd)"
export PYTHONDONTWRITEBYTECODE=1
exec python3 "$ROOT/cqpl/scripts/verify_bodyless_ffi_ene1_d3.py" --root "$ROOT" --run
