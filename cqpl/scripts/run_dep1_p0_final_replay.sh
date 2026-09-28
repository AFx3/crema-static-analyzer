#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FIXTURE="$ROOT/tests_and_target_repos/a-code_dependency_body_ingestion_gate"
PRIOR="$ROOT/repro-results/dep1-p0b-final-20260928T134934Z"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/dep1-p0-final-replay-$STAMP"
HOST_TRIPLE="x86_64-unknown-linux-gnu"
PROBE="$ROOT/cqpl/dep1_p0b_probe/target/debug/dep1_p0b_probe"
mkdir -p "$OUT/probe-source/src" "$OUT/probe-source/fixture/app/src" "$OUT/probe-source/fixture/dep/src" "$OUT/probe-source/fixture/dep2/src" "$OUT/verifier-source" "$OUT/host-control-previous" "$OUT/raw"

cp "$ROOT/cqpl/dep1_p0b_probe/Cargo.toml" "$ROOT/cqpl/dep1_p0b_probe/Cargo.lock" "$OUT/probe-source/"
cp "$ROOT/cqpl/dep1_p0b_probe/src/main.rs" "$OUT/probe-source/src/"
cp "$FIXTURE/Cargo.toml" "$FIXTURE/Cargo.lock" "$OUT/probe-source/fixture/"
cp "$FIXTURE/app/Cargo.toml" "$FIXTURE/app/build.rs" "$OUT/probe-source/fixture/app/"
cp "$FIXTURE/app/src/main.rs" "$OUT/probe-source/fixture/app/src/"
cp "$FIXTURE/dep/Cargo.toml" "$OUT/probe-source/fixture/dep/"
cp "$FIXTURE/dep/src/lib.rs" "$OUT/probe-source/fixture/dep/src/"
cp "$FIXTURE/dep2/Cargo.toml" "$OUT/probe-source/fixture/dep2/"
cp "$FIXTURE/dep2/src/lib.rs" "$OUT/probe-source/fixture/dep2/src/"
cp "$ROOT/cqpl/scripts/run_dep1_p0_final_replay.sh" "$OUT/probe-source/run.sh"
cp "$ROOT/cqpl/scripts/verify_dep1_p0_final_replay.py" "$OUT/verifier-source/verify.py"

python3 - "$PRIOR" "$OUT/host-control-previous" <<'PY'
import hashlib,json,pathlib,shutil,sys
src,dst=map(pathlib.Path,sys.argv[1:])
checks={x.split('  ',1)[1]:x.split('  ',1)[0] for x in (src/'SHA256SUMS').read_text().splitlines()}
names=['host-unit-control.json','host-triple.txt','cargo-host-no-target.jsonl','cargo-host-no-target.stderr','cargo-host-explicit-target.jsonl','cargo-host-explicit-target.stderr','cargo-fingerprint-evidence.json','logs/rustc-host-no-target.jsonl','logs/rustc-host-explicit-target.jsonl']
prov={'source_run':str(src),'files':{}}
for name in names:
    original=src/name
    expected=checks[name]
    actual=hashlib.sha256(original.read_bytes()).hexdigest()
    if actual!=expected: raise SystemExit(f'prior evidence hash mismatch: {name}')
    target=dst/name
    target.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(original,target)
    target.chmod(0o444)
    prov['files'][name]={'original_sha256':expected,'copied_sha256':hashlib.sha256(target.read_bytes()).hexdigest()}
(dst/'provenance.json').write_text(json.dumps(prov,indent=2)+'\n')
PY

{
  git -C "$ROOT" rev-parse HEAD
  git -C "$ROOT" branch --show-current
  git -C "$ROOT" diff --check
  rustc +nightly-2024-11-21 --version --verbose
  cargo +nightly-2024-11-21 --version
  python3 --version
} > "$OUT/environment.txt" 2>&1
cargo metadata --manifest-path "$FIXTURE/Cargo.toml" --format-version 1 > "$OUT/cargo-metadata.json"
RUSTC_BOOTSTRAP=1 cargo +nightly-2024-11-21 build --offline --manifest-path "$ROOT/cqpl/dep1_p0b_probe/Cargo.toml" > "$OUT/probe-build.stdout" 2> "$OUT/probe-build.stderr"

cat > "$OUT/commands.txt" <<EOF
STANDARD_EXPLICIT_TARGET
env -u RUSTFLAGS CARGO_TARGET_DIR="$OUT/target-standard-explicit-target" RUSTC_WRAPPER="$PROBE" DEP1_P0B_OUTPUT="$OUT/raw/standard.json" DEP1_P0B_RUSTC_LOG="$OUT/rustc-standard-explicit-target.jsonl" cargo +nightly-2024-11-21 build --target $HOST_TRIPLE --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics

FORCED_EXPLICIT_TARGET
RUSTFLAGS=-Zalways-encode-mir=yes CARGO_TARGET_DIR="$OUT/target-forced-explicit-target" RUSTC_WRAPPER="$PROBE" DEP1_P0B_OUTPUT="$OUT/raw/forced.json" DEP1_P0B_RUSTC_LOG="$OUT/rustc-forced-explicit-target.jsonl" cargo +nightly-2024-11-21 build --target $HOST_TRIPLE --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics
EOF

unset RUSTFLAGS || true
export RUSTC_WRAPPER="$PROBE"
export CARGO_TARGET_DIR="$OUT/target-standard-explicit-target"
export DEP1_P0B_OUTPUT="$OUT/raw/standard.json" DEP1_P0B_RUSTC_LOG="$OUT/rustc-standard-explicit-target.jsonl"
cargo +nightly-2024-11-21 build --target "$HOST_TRIPLE" --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics > "$OUT/cargo-standard-explicit-target.jsonl" 2> "$OUT/cargo-standard-explicit-target.stderr"
test -s "$OUT/raw/standard.json"
sha256sum "$OUT/raw/standard.json" > "$OUT/standard-immediate.sha256"
cp "$OUT/raw/standard.json" "$OUT/foreign-mir-standard-explicit-target.json"
chmod 0444 "$OUT/foreign-mir-standard-explicit-target.json"
cmp "$OUT/raw/standard.json" "$OUT/foreign-mir-standard-explicit-target.json"
unset DEP1_P0B_OUTPUT

export RUSTFLAGS='-Zalways-encode-mir=yes'
export CARGO_TARGET_DIR="$OUT/target-forced-explicit-target"
export DEP1_P0B_OUTPUT="$OUT/raw/forced.json" DEP1_P0B_RUSTC_LOG="$OUT/rustc-forced-explicit-target.jsonl"
cargo +nightly-2024-11-21 build --target "$HOST_TRIPLE" --manifest-path "$FIXTURE/app/Cargo.toml" --bin dep1_app --message-format=json-render-diagnostics > "$OUT/cargo-forced-explicit-target.jsonl" 2> "$OUT/cargo-forced-explicit-target.stderr"
test -s "$OUT/raw/forced.json"
sha256sum "$OUT/raw/forced.json" > "$OUT/forced-immediate.sha256"
cp "$OUT/raw/forced.json" "$OUT/foreign-mir-forced-explicit-target.json"
chmod 0444 "$OUT/foreign-mir-forced-explicit-target.json"
cmp "$OUT/raw/forced.json" "$OUT/foreign-mir-forced-explicit-target.json"
unset DEP1_P0B_OUTPUT

if [[ -n "${DEP1_P0B_OUTPUT:-}" ]]; then echo 'DEP1_P0B_OUTPUT unexpectedly set before packaging' >&2; exit 1; fi
unset RUSTFLAGS RUSTC_WRAPPER DEP1_P0B_RUSTC_LOG CARGO_TARGET_DIR
git -C "$ROOT" diff --check > "$OUT/git-diff-check.txt" 2>&1
git -C "$ROOT" status --short > "$OUT/git-status-short.txt"
python3 "$OUT/verifier-source/verify.py" "$OUT" "$ROOT"
printf '%s\n' "$OUT"
