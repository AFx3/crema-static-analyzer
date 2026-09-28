#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 "$ROOT/cqpl/scripts/verify_bodyless_ffi_ele1_d4.py" --root "$ROOT" --run
