#!/usr/bin/env bash
set -euo pipefail
ROOT="${1:-$PWD}"
TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/bodyless-ffi-phase-b-minimal-$STAMP"
FIXROOT="$ROOT/tests_and_target_repos/a-code_c_ffi_bodyless_gate"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"
mkdir -p "$OUT"

GENERATED_TRACKED=(
  "crema/ffi_functions.json"
  "crema/global_icfg.json"
  "crema/global_icfg_nodes_edges.dot"
  "SVF-example/callgraph_initial.dot.dot"
)
cleanup() {
  local rel
  for rel in "${GENERATED_TRACKED[@]}"; do
    if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1; then
      git -C "$ROOT" restore -- "$rel" >/dev/null 2>&1 || true
    fi
  done
}
trap cleanup EXIT

for rel in "${GENERATED_TRACKED[@]}"; do
  if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1 &&
     ! git -C "$ROOT" diff --quiet -- "$rel"; then
    echo "ERROR: generated tracked artifact dirty before gate: $rel" >&2
    exit 2
  fi
done

echo "schema=cqpl_bodyless_ffi_phase_b_minimal_v1"
echo "git_head=$(git -C "$ROOT" rev-parse HEAD)"
echo "git_branch=$(git -C "$ROOT" branch --show-current)"
echo "toolchain=$TOOLCHAIN"
echo "out=$OUT"

echo "[1/5] Unit tests + checker build"
git -C "$ROOT" diff --check
python3 -m py_compile "$ROOT/cqpl/scripts/verify_bodyless_ffi_phase_b_minimal.py"
cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/crema/Cargo.toml" --locked
cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked
cargo +"$TOOLCHAIN" build --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked

echo "[2/5] Focused Phase-B fixtures"
TARGETS=(
  b01_malloc_leak
  b08b_c_malloc_then_rust_dealloc_mismatch
  b16_two_bodyless_malloc_callsites
  b17_loop_bodyless_malloc_site_reuse
  b18_getenv_pointer_return_not_alloc
  b19_calloc_leak
)
for name in "${TARGETS[@]}"; do
  echo "=== $name ==="
  target="$FIXROOT/$name"
  od="$OUT/$name"
  mkdir -p "$od"

  cargo +"$TOOLCHAIN" build --manifest-path "$target/Cargo.toml" --locked \
    >"$od/cargo-build.log" 2>&1 || {
      cat "$od/cargo-build.log" >&2
      exit 3
    }

  (
    cd "$ROOT/crema"
    cargo +"$TOOLCHAIN" run --locked -- \
      "$target" \
      --only-icfg-annotated \
      --cqpl-schema-version 2 \
      --mir-semantics-v2 \
      --annotated-icfg-out "$od/annotated_icfg_v2.json" \
      --allocation-identity-out "$od/allocation_identity.json"
  ) >"$od/crema.log" 2>&1 || {
    echo "FAIL: CREMA export failed for $name" >&2
    cat "$od/crema.log" >&2
    exit 4
  }

  for spec in \
    "leak_alloc_state:$ROOT/cqpl/queries_v2/leak_alloc_state.cqpl" \
    "double_free_alloc_state:$ROOT/cqpl/queries_v2/double_free_alloc_state.cqpl" \
    "use_after_free_alloc_state:$ROOT/cqpl/queries_v2/use_after_free_alloc_state.cqpl" \
    "allocator_mismatch_ub_v2:$ROOT/cqpl/queries_v2/allocator_mismatch_ub_v2.cqpl"
  do
    q="${spec%%:*}"
    qp="${spec#*:}"
    "$CHECKER" "$od/annotated_icfg_v2.json" "$qp" \
      --explain-json "$od/$q.explain.json" \
      >"$od/$q.checker.log" 2>&1 || {
        cat "$od/$q.checker.log" >&2
        exit 5
      }
  done
done

echo "[3/5] Semantic verification"
python3 "$ROOT/cqpl/scripts/verify_bodyless_ffi_phase_b_minimal.py" \
  --run-root "$OUT" \
  --out "$OUT/BODYLESS_FFI_PHASE_B_MINIMAL.json"

echo "[4/5] Previous Phase-A pass fixtures remain exportable"
for name in \
  b02_malloc_free_clean \
  b03_malloc_double_free \
  b04_free_then_strlen_uaf_read \
  b05_free_then_memset_uaf_write \
  b06_free_src_then_memcpy_uaf_read \
  b07_free_dst_then_memcpy_uaf_write \
  b08a_rust_box_then_c_free_mismatch \
  b09_malloc_free_family_match \
  b10a_realloc_branch_clean \
  b10b_realloc_then_old_use \
  b10c_realloc_then_old_free \
  b11_memcpy_returned_alias \
  b12_strlen_nofree_control \
  b13_memcmp_two_pointer_argmem \
  b14a_bodyless_strlen \
  b14b_body_present_reader \
  b15_external_write_pointer_unknown
do
  target="$FIXROOT/$name"
  od="$OUT/regression-$name"
  mkdir -p "$od"
  (
    cd "$ROOT/crema"
    cargo +"$TOOLCHAIN" run --locked -- \
      "$target" \
      --only-icfg-annotated \
      --cqpl-schema-version 2 \
      --mir-semantics-v2 \
      --annotated-icfg-out "$od/annotated_icfg_v2.json" \
      --allocation-identity-out "$od/allocation_identity.json"
  ) >"$od/crema.log" 2>&1 || {
    echo "FAIL: prior Phase-A pass fixture regressed: $name" >&2
    cat "$od/crema.log" >&2
    exit 6
  }
done

echo "[5/5] Freeze"
python3 - "$ROOT" "$OUT" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
root=Path(sys.argv[1]); out=Path(sys.argv[2])
semantic=json.load(open(out/"BODYLESS_FFI_PHASE_B_MINIMAL.json"))
files=[]
for p in sorted(out.rglob("*")):
    if p.is_file():
        files.append({
            "path":str(p.relative_to(out)),
            "sha256":hashlib.sha256(p.read_bytes()).hexdigest(),
            "bytes":p.stat().st_size,
        })
freeze={
    "schema":"cqpl_bodyless_ffi_phase_b_minimal_freeze_v1",
    "status":semantic["status"],
    "strategy":"new AbstractAllocId + existing TOP",
    "git_head":subprocess.check_output(
        ["git","-C",str(root),"rev-parse","HEAD"],text=True
    ).strip(),
    "toolchain":"nightly-2024-11-21",
    "criteria":semantic["criteria"],
    "files":files,
}
(out/"PHASE_B_MINIMAL_FREEZE.json").write_text(
    json.dumps(freeze,indent=2,sort_keys=True)+"\n"
)
print("CQPL_BODYLESS_FFI_PHASE_B_MINIMAL_GATE:",freeze["status"])
print("out=",out)
if freeze["status"]!="PASS":
    raise SystemExit(2)
PY
