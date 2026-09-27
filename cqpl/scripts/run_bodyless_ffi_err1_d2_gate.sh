#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "${1:-$PWD}" && pwd)"
TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2024-11-21}"
BASELINE="94a9834b7f36c1ebb63369130f6c669c50846a51"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${D2_GATE_OUT:-$ROOT/repro-results/bodyless-ffi-err1-d2-$STAMP}"
MANIFEST="$ROOT/cqpl/bodyless_ffi_err1_d2_fixture_manifest.json"
STATUS_TSV="$OUT/runner-status.tsv"
FIXTURE_ROOT_REL="a-code_c_ffi_bodyless_gate"
BASELINE_TMP=""
BASELINE_ROOT=""
export PYTHONDONTWRITEBYTECODE=1
GENERATED_FIXTURE_TARGETS=(
  b44_memmove_freed_src_uaf_read
  b45_memmove_freed_dst_uaf_write
  b46_memchr_freed_buffer_uaf_read
  b47_strchr_freed_string_uaf_read
  b48_memmove_zero_extent_no_memory_event
  b49_memchr_zero_extent_no_memory_event
  b50_body_present_memmove_control
  b51_memmove_dynamic_extent_may_effect
  b52_memcpy_return_exact_alias_uaf
  b53_memmove_return_exact_alias_uaf
  b54_memset_return_exact_alias_uaf
  b55_memchr_return_derived_alias_uaf
  b56_strchr_return_derived_alias_uaf
  b57_getenv_borrowed_no_allocation
  b58_memmove_zero_extent_return_alias
  b59_memchr_zero_extent_no_derived_alias
  b60_body_present_memmove_return_control
  b61_unknown_pointer_return_fail_closed
)

cleanup_fixture_targets() {
  local target artifact
  local cleanup_rc=0
  for target in "${GENERATED_FIXTURE_TARGETS[@]}"; do
    artifact="$ROOT/tests_and_target_repos/a-code_c_ffi_bodyless_gate/$target/target"
    if [[ -e "$artifact" || -L "$artifact" ]]; then
      # Only Cargo's generated directory in each explicitly named D1 fixture.
      # Refuse tracked content rather than deleting committed fixture files.
      if [[ -n "$(git -C "$ROOT" ls-files -- "tests_and_target_repos/a-code_c_ffi_bodyless_gate/$target/target")" ]]; then
        echo "ERROR: refusing to remove tracked fixture target content: $artifact" >&2
        cleanup_rc=1
      else
        rm -rf -- "$artifact" || cleanup_rc=1
      fi
    fi
  done
  return "$cleanup_rc"
}

GENERATED_TRACKED=(
  "crema/ffi_functions.json"
  "crema/global_icfg.json"
  "crema/global_icfg_nodes_edges.dot"
  "SVF-example/callgraph_initial.dot.dot"
)

restore_generated() {
  local rel
  for rel in "${GENERATED_TRACKED[@]}"; do
    if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1; then
      git -C "$ROOT" restore -- "$rel" >/dev/null 2>&1 || true
    fi
  done
}

cleanup() {
  local exit_rc=$?
  cleanup_fixture_targets || exit_rc=2
  restore_generated
  if [[ -n "$BASELINE_TMP" && "$BASELINE_TMP" == /tmp/cqpl-err1-d2-baseline.* && -d "$BASELINE_TMP" ]]; then
    rm -rf -- "$BASELINE_TMP"
  fi
  trap - EXIT
  exit "$exit_rc"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$OUT/logs" "$OUT/baseline" "$OUT/candidate"
printf 'side\ttarget\trc\n' > "$STATUS_TSV"

echo "schema=cqpl_bodyless_ffi_err1_d2_gate_v1"
echo "baseline_commit=$BASELINE"
echo "git_head=$(git -C "$ROOT" rev-parse HEAD)"
echo "git_branch=$(git -C "$ROOT" branch --show-current)"
echo "toolchain=$TOOLCHAIN"
echo "out=$OUT"

for rel in "${GENERATED_TRACKED[@]}"; do
  if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1 &&
     ! git -C "$ROOT" diff --quiet -- "$rel"; then
    echo "ERROR: generated tracked artifact is dirty before D2 gate: $rel" >&2
    exit 2
  fi
done

echo "[1/8] Baseline, environment, preimage, and source hygiene"
{
  git -C "$ROOT" status --short --branch
  git -C "$ROOT" rev-parse HEAD
  git -C "$ROOT" branch --show-current
  rustup run "$TOOLCHAIN" rustc --version
  cargo +"$TOOLCHAIN" --version
  python3 --version
} > "$OUT/environment.txt" 2>&1

[[ "$(git -C "$ROOT" branch --show-current)" == cqpl6-bodyless-ffi-effect-gate ]] || exit 2
git -C "$ROOT" merge-base --is-ancestor "$BASELINE" HEAD || {
  echo "ERROR: candidate does not descend from baseline $BASELINE" >&2
  exit 2
}
git -C "$ROOT" diff --check > "$OUT/git-diff-check.initial.log" 2>&1 || {
  cat "$OUT/git-diff-check.initial.log" >&2
  exit 2
}
python3 - "$ROOT" "$MANIFEST" <<'PY'
import json,sys
from pathlib import Path
root=Path(sys.argv[1]); manifest=Path(sys.argv[2])
doc=json.loads(manifest.read_text())
assert doc["schema"] == "cqpl_bodyless_ffi_err1_d2_fixture_manifest_v1"
assert len(doc["fixtures"]) == 10
for fixture in doc["fixtures"]:
    target=root/"tests_and_target_repos/a-code_c_ffi_bodyless_gate"/fixture["target"]
    for rel in ("Cargo.toml", "Cargo.lock", "src/main.rs"):
        assert (target/rel).is_file(), target/rel
print("D2 manifest/source inventory: OK")
PY

python3 - "$ROOT" "$MANIFEST" "$OUT/source-postimages-at-start.json" <<'PY' || exit 2
import hashlib,json,sys
from pathlib import Path
root=Path(sys.argv[1]); manifest=json.loads(Path(sys.argv[2]).read_text())
pre=json.loads((root/"cqpl/bodyless_ffi_err1_d2_preimage_sha256.json").read_text())
aux=json.loads((root/"cqpl/bodyless_ffi_err1_d2_test_constructor_preimages.json").read_text())
paths=set(pre["source_preimages"]) | set(pre["frozen_capability_hashes"]) | set(aux)
paths.update([
    "cqpl/scripts/run_bodyless_ffi_err1_d2_gate.sh", "cqpl/scripts/verify_bodyless_ffi_err1_d2.py",
    "cqpl/bodyless_ffi_err1_d2_fixture_manifest.json", "cqpl/bodyless_ffi_err1_d2_preimage_sha256.json",
    "cqpl/bodyless_ffi_err1_d2_test_constructor_preimages.json",
    "cqpl/capabilities/external_return_relations_v1.md", "cqpl/D2_EXTERNAL_RETURN_RELATIONS_V1_GATE.md",
])
for fixture in manifest["fixtures"]:
    directory=root/"tests_and_target_repos/a-code_c_ffi_bodyless_gate"/fixture["target"]
    paths.update(str(p.relative_to(root)) for p in directory.rglob("*") if p.is_file() and "target" not in p.relative_to(directory).parts)
Path(sys.argv[3]).write_text(json.dumps({p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in sorted(paths)},indent=2)+"\n")
PY

echo "[2/8] Clean full CQPL and CREMA test suites"
cargo +"$TOOLCHAIN" clean --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" \
  > "$OUT/logs/cqpl-clean.log" 2>&1
cqpl_clean_rc=$?
cargo +"$TOOLCHAIN" clean --manifest-path "$ROOT/crema/Cargo.toml" \
  > "$OUT/logs/crema-clean.log" 2>&1
crema_clean_rc=$?

if [[ "$cqpl_clean_rc" -eq 0 ]]; then
  cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked \
    > "$OUT/logs/cqpl-tests.log" 2>&1
  cqpl_test_rc=$?
else
  cqpl_test_rc=$cqpl_clean_rc
fi
if [[ "$crema_clean_rc" -eq 0 ]]; then
  cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/crema/Cargo.toml" --locked \
    > "$OUT/logs/crema-tests.log" 2>&1
  crema_test_rc=$?
else
  crema_test_rc=$crema_clean_rc
fi
echo "cqpl_test_rc=$cqpl_test_rc"
echo "crema_test_rc=$crema_test_rc"

cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/crema/Cargo.toml" --locked err1 \
  > "$OUT/logs/err1-producer-tests.log" 2>&1
producer_test_rc=$?
cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked err1 \
  > "$OUT/logs/err1-checker-tests.log" 2>&1
consumer_test_rc=$?

cargo +"$TOOLCHAIN" build --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked \
  > "$OUT/logs/cqpl-build.log" 2>&1
checker_build_rc=$?
if [[ "$cqpl_test_rc" -ne 0 || "$crema_test_rc" -ne 0 || "$checker_build_rc" -ne 0 || "$producer_test_rc" -ne 0 || "$consumer_test_rc" -ne 0 ]]; then
  echo "ERROR: full software tests/build failed; inspect $OUT/logs" >&2
  exit 3
fi

echo "[3/8] Isolated exact-baseline checkout"
BASELINE_TMP="$(mktemp -d /tmp/cqpl-err1-d2-baseline.XXXXXX)"
BASELINE_ROOT="$BASELINE_TMP/repo"
git clone --shared --no-checkout "$ROOT" "$BASELINE_ROOT" \
  > "$OUT/logs/baseline-clone.log" 2>&1 || {
    cat "$OUT/logs/baseline-clone.log" >&2
    exit 4
  }
git -C "$BASELINE_ROOT" checkout --detach "$BASELINE" \
  > "$OUT/logs/baseline-checkout.log" 2>&1 || {
    cat "$OUT/logs/baseline-checkout.log" >&2
    exit 4
  }
resolved_baseline="$(git -C "$BASELINE_ROOT" rev-parse HEAD)"
if [[ "$resolved_baseline" != "$BASELINE" ]]; then
  echo "ERROR: isolated baseline resolved to $resolved_baseline" >&2
  exit 4
fi

python3 - "$ROOT" "$BASELINE" "$OUT/existing-targets.txt" <<'PY'
import subprocess,sys
from pathlib import Path
root=Path(sys.argv[1]); commit=sys.argv[2]; out=Path(sys.argv[3])
prefix="tests_and_target_repos/a-code_c_ffi_bodyless_gate/"
raw=subprocess.check_output(
    ["git","-C",str(root),"ls-tree","-d","-r","--name-only",commit,prefix],
    text=True,
)
targets=[]
for line in raw.splitlines():
    if line.startswith(prefix):
        rest=line[len(prefix):]
        if rest and "/" not in rest:
            targets.append(rest)
out.write_text("\n".join(sorted(targets))+"\n")
assert len(targets) == 65, len(targets)
print(f"baseline targets={len(targets)}")
PY
python3 - "$MANIFEST" "$OUT/new-targets.txt" <<'PY'
import json,sys
from pathlib import Path
doc=json.loads(Path(sys.argv[1]).read_text())
Path(sys.argv[2]).write_text("\n".join(f["target"] for f in doc["fixtures"])+"\n")
PY

run_one() {
  local side="$1"
  local tree="$2"
  local target="$3"
  local runner="$tree/cqpl/scripts/run_one_target_v6q_r1c.py"
  local target_out="$OUT/$side/$target"
  local log="$OUT/logs/$side-$target.log"
  python3 "$runner" \
    --root "$tree" \
    --relative-path "$FIXTURE_ROOT_REL/$target" \
    --out "$target_out" \
    --toolchain "$TOOLCHAIN" \
    > "$log" 2>&1
  local rc=$?
  printf '%s\t%s\t%s\n' "$side" "$target" "$rc" >> "$STATUS_TSV"
  if [[ "$rc" -ne 0 ]]; then
    echo "  FAIL $side/$target rc=$rc (see $log)"
  else
    echo "  PASS $side/$target"
  fi
}

echo "[4/8] Exact 94a9834 baseline replay for every pre-D2 fixture"
while IFS= read -r target; do
  [[ -n "$target" ]] || continue
  run_one baseline "$BASELINE_ROOT" "$target"
done < "$OUT/existing-targets.txt"

echo "[5/8] Candidate replay for every pre-D2 fixture"
while IFS= read -r target; do
  [[ -n "$target" ]] || continue
  run_one candidate "$ROOT" "$target"
done < "$OUT/existing-targets.txt"

echo "[6/8] Candidate D2 fixture matrix"
while IFS= read -r target; do
  [[ -n "$target" ]] || continue
  run_one candidate "$ROOT" "$target"
done < "$OUT/new-targets.txt"

echo "[7/8] Baseline EFM2 candidate-checker acceptance"
legacy_artifact="$OUT/baseline/b14a_bodyless_strlen/annotated_icfg_v2.json"
legacy_query="$ROOT/cqpl/queries_v2/use_after_free_alloc_state.cqpl"
legacy_checker="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
if [[ -f "$legacy_artifact" && -x "$legacy_checker" ]]; then
  "$legacy_checker" "$legacy_artifact" "$legacy_query" --json \
    > "$OUT/baseline-efm2-candidate-checker.json" \
    2> "$OUT/logs/baseline-efm2-candidate-checker.log"
  legacy_checker_rc=$?
else
  legacy_checker_rc=127
fi
echo "legacy_checker_rc=$legacy_checker_rc"

cleanup_fixture_targets || {
  echo "ERROR: D1 fixture target cleanup failed" >&2
  exit 2
}
restore_generated
git -C "$ROOT" diff --check > "$OUT/git-diff-check.final.log" 2>&1

echo "[8/8] Artifact/schema/proof/differential verification"
python3 "$ROOT/cqpl/scripts/verify_bodyless_ffi_err1_d2.py" \
  --root "$ROOT" \
  --run-root "$OUT" \
  --manifest "$MANIFEST" \
  --status-tsv "$STATUS_TSV" \
  --cqpl-test-rc "$cqpl_test_rc" \
  --crema-test-rc "$crema_test_rc" \
  --legacy-checker-rc "$legacy_checker_rc" \
  --out "$OUT/gate.json"
verify_rc=$?

echo "D2_GATE_JSON=$OUT/gate.json"
exit "$verify_rc"
