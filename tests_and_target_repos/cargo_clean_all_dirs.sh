#!/usr/bin/env bash
set -euo pipefail

# Cerca tutti i file Cargo.toml a partire dalla cartella corrente
find . -type f -name 'Cargo.toml' | while IFS= read -r cargofile; do
  # Ottiene la directory che contiene Cargo.toml
  dir=$(dirname "$cargofile")
  # Rimuove il prefisso "./" per rendere l'output più pulito
  pretty_dir=${dir#./}

  echo "Cleaning cargo project at: $pretty_dir"
  (
    cd "$dir"
    # Esegue cargo clean, ma ignora eventuali errori
    cargo clean 2>/dev/null || true
  )
done

echo "All cleaned."
