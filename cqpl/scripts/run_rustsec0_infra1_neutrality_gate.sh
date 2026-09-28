#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 "$ROOT/cqpl/scripts/verify_rustsec0_infra1_neutrality.py" --root "$ROOT"
