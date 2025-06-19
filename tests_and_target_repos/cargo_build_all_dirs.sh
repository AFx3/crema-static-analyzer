#!/usr/bin/env bash
set -euo pipefail

# from the current dir, search all Cargo.toml files
find . -type f -name 'Cargo.toml' | while IFS= read -r cargofile; do
  # get the dir containing the cargo.toml file
  dir=$(dirname "$cargofile")
  # remove the prefix "./" 
  pretty_dir=${dir#./}

  echo "Building cargo project at: $pretty_dir"
  (
    cd "$dir"
    # -x the command and ignore errors
    cargo build 2>/dev/null || true
  )
done

echo "All built."
