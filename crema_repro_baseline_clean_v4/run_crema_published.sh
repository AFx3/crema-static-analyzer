#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
echo "NOTE: legacy sequential published runner is disabled in v3."
echo "Reason: openapi-client-gen is excluded by protocol due to its ~10 minute runtime."
echo "Running the isolated 92-target protocol instead."
exec "$SCRIPT_DIR/run_crema_isolated_full.sh" "$@"
