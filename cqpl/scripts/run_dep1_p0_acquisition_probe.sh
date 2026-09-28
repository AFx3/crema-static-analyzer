#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FIXTURE="$ROOT/tests_and_target_repos/a-code_dependency_body_ingestion_gate"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/dep1-p0-$STAMP"
mkdir -p "$OUT"
printf '%s\n' "$OUT" > "$ROOT/repro-results/dep1-p0-latest.txt"
{
  git -C "$ROOT" rev-parse HEAD
  git -C "$ROOT" branch --show-current
  git -C "$ROOT" status --short --branch
  git -C "$ROOT" diff --check
  rustc --version --verbose
  cargo --version
  python3 --version
  rustc -Z help | grep -Ei 'always.*encode.*mir|encode.*mir'
} > "$OUT/environment.txt" 2>&1
cargo metadata --manifest-path "$FIXTURE/Cargo.toml" --format-version 1 > "$OUT/cargo-metadata.json"
CARGO_TARGET_DIR="$OUT/target-standard" cargo build --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics > "$OUT/cargo-standard.jsonl" 2> "$OUT/cargo-standard.stderr"
RUSTFLAGS="-Zalways-encode-mir=yes" CARGO_TARGET_DIR="$OUT/target-forced" cargo build --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics > "$OUT/cargo-forced.jsonl" 2> "$OUT/cargo-forced.stderr"
python3 "$ROOT/cqpl/scripts/verify_dep1_p0_acquisition.py" "$OUT" "$ROOT"
