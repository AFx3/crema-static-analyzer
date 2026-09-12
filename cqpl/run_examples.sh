#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/cqpl_checker"
cargo test
cargo run -- ../fixtures/cross_language_uaf.json ../queries/use_after_free.cqpl
