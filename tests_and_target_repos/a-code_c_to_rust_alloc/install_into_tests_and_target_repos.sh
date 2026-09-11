#!/usr/bin/env bash
set -euo pipefail

ROOT="${CREMA_PHD_ROOT:-/home/af/Documenti/a-phd}"
HERE="$(cd "$(dirname "$0")" && pwd)"
DEST="$ROOT/tests_and_target_repos/a-code_c_to_rust_alloc"

rm -rf "$DEST"
mkdir -p "$DEST"

for item in "$HERE"/*; do
  base="$(basename "$item")"
  case "$base" in
    README.md|ORACLE.json|run_phase5_focus.sh|install_into_tests_and_target_repos.sh)
      ;;
    *)
      cp -R "$item" "$DEST/"
      ;;
  esac
done

cp "$HERE/README.md" "$DEST/README.md"
cp "$HERE/ORACLE.json" "$DEST/ORACLE.json"
cp "$HERE/run_phase5_focus.sh" "$DEST/run_phase5_focus.sh"
chmod +x "$DEST/run_phase5_focus.sh"

echo "Installed Phase-5 C-origin targets to: $DEST"
