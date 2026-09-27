#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$PWD}"
FROZEN118="${2:-}"
TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$ROOT/repro-results/bodyless-ffi-baseline-$STAMP"
FIXROOT="$ROOT/tests_and_target_repos/a-code_c_ffi_bodyless_gate"
MANIFEST="$ROOT/cqpl/bodyless_ffi_fixture_manifest.json"
CHECKER="$ROOT/cqpl/cqpl_checker/target/debug/cqpl_checker"

GENERATED_TRACKED=(
  "crema/ffi_functions.json"
  "crema/global_icfg.json"
  "crema/global_icfg_nodes_edges.dot"
  "SVF-example/callgraph_initial.dot.dot"
)

restore_generated_tracked() {
  local rel
  for rel in "${GENERATED_TRACKED[@]}"; do
    if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1; then
      git -C "$ROOT" restore -- "$rel" >/dev/null 2>&1 || true
    fi
  done
}

for rel in "${GENERATED_TRACKED[@]}"; do
  if git -C "$ROOT" ls-files --error-unmatch "$rel" >/dev/null 2>&1 &&
     ! git -C "$ROOT" diff --quiet -- "$rel"; then
    echo "ERROR: generated tracked artifact is already dirty before gate: $rel" >&2
    echo "restore it before running the gate" >&2
    exit 2
  fi
done
trap restore_generated_tracked EXIT


mkdir -p "$OUT"

run_logged() {
  local label="$1"
  local log="$2"
  shift 2
  if ! "$@" >"$log" 2>&1; then
    echo "ERROR: $label failed; log follows: $log" >&2
    echo "----- BEGIN $label LOG -----" >&2
    cat "$log" >&2 || true
    echo "----- END $label LOG -----" >&2
    return 1
  fi
}

run_crema_logged() {
  local label="$1"
  local log="$2"
  shift 2
  # Historical CREMA's non-v6O pipeline launches:
  #   cargo run --package ffi_extraction
  # as a child process without overriding cwd. Therefore the parent CREMA
  # process must run from the crema Cargo workspace. --manifest-path alone is
  # insufficient because it does not change the child process cwd.
  if ! (
    cd "$ROOT/crema"
    "$@"
  ) >"$log" 2>&1; then
    echo "ERROR: $label failed; log follows: $log" >&2
    echo "----- BEGIN $label LOG -----" >&2
    cat "$log" >&2 || true
    echo "----- END $label LOG -----" >&2
    return 1
  fi
}

echo "schema=cqpl_bodyless_ffi_phase_a_gate_v1"
echo "git_head=$(git -C "$ROOT" rev-parse HEAD)"
echo "git_branch=$(git -C "$ROOT" branch --show-current)"
echo "toolchain=$TOOLCHAIN"
echo "out=$OUT"

echo "[1/6] Static checks"
git -C "$ROOT" diff --check
[[ -f "$ROOT/crema/Cargo.toml" ]] || { echo "missing crema/Cargo.toml" >&2; exit 2; }
grep -q '^members = \["ffi_extraction"\]' "$ROOT/crema/Cargo.toml" || {
  echo "ERROR: expected ffi_extraction to be a member of the crema workspace" >&2
  exit 2
}
python3 -m py_compile \
  "$ROOT/cqpl/scripts/audit_bodyless_ffi_baseline.py" \
  "$ROOT/cqpl/scripts/summarize_bodyless_ffi_fixtures.py"
python3 - "$MANIFEST" "$FIXROOT" <<'PY'
import json,sys
from pathlib import Path
m=json.load(open(sys.argv[1])); root=Path(sys.argv[2])
missing=[]
for f in m["fixtures"]:
    d=root/f["target"]
    for rel in ("Cargo.toml","Cargo.lock","src/main.rs"):
        if not (d/rel).exists(): missing.append(str(d/rel))
if missing:
    raise SystemExit("missing fixture files:\n"+"\n".join(missing))
print(f"fixture manifest: OK ({len(m['fixtures'])} targets)")
PY

echo "[2/6] Existing producer/checker tests"
cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/crema/Cargo.toml" --locked
cargo +"$TOOLCHAIN" test --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked
cargo +"$TOOLCHAIN" build --manifest-path "$ROOT/cqpl/cqpl_checker/Cargo.toml" --locked

echo "[3/6] Build and characterize explicit bodyless/represented fixtures"
python3 - "$MANIFEST" <<'PY' > "$OUT/fixture-order.txt"
import json,sys
for f in json.load(open(sys.argv[1]))["fixtures"]:
    print(f["target"])
PY

printf 'target\tproducer_status\tproducer_detail\n' > "$OUT/producer-status.tsv"

classify_crema_failure() {
  local log="$1"
  if grep -Fq "schema-v2 fail-closed: reachable modeled alloc event" "$log" &&
     grep -Fq "has no AbstractAllocId/event_identity" "$log"; then
    printf '%s' "fail_closed_alloc_identity_missing"
  elif grep -Fq "schema-v2 fail-closed:" "$log"; then
    printf '%s' "fail_closed_other"
  else
    printf '%s' "producer_error"
  fi
}

while IFS= read -r name; do
  target="$FIXROOT/$name"
  od="$OUT/$name"
  mkdir -p "$od"
  echo "=== $name ==="

  run_logged \
    "$name cargo-build" \
    "$od/cargo-build.log" \
    cargo +"$TOOLCHAIN" build --manifest-path "$target/Cargo.toml" --locked

  if (
    cd "$ROOT/crema"
    cargo +"$TOOLCHAIN" run --locked -- \
      "$target" \
      --only-icfg-annotated \
      --cqpl-schema-version 2 \
      --mir-semantics-v2 \
      --annotated-icfg-out "$od/annotated_icfg_v2.json" \
      --allocation-identity-out "$od/allocation_identity.json"
  ) >"$od/crema.log" 2>&1; then
    producer_status="pass"
    producer_detail="annotated schema-v2 artifact exported"
  else
    producer_status="$(classify_crema_failure "$od/crema.log")"
    producer_detail="$(grep -F "schema-v2 fail-closed:" "$od/crema.log" | tail -1 | tr '\t' ' ' || true)"
    [[ -n "$producer_detail" ]] || producer_detail="CREMA returned nonzero; inspect crema.log"
    echo "  producer_status=$producer_status"
    echo "  $producer_detail"
  fi

  printf '%s\t%s\t%s\n' "$name" "$producer_status" "$producer_detail" >> "$OUT/producer-status.tsv"

  if [[ "$producer_status" != "pass" ]]; then
    continue
  fi

  for spec in \
    "leak_alloc_state:$ROOT/cqpl/queries_v2/leak_alloc_state.cqpl" \
    "double_free_alloc_state:$ROOT/cqpl/queries_v2/double_free_alloc_state.cqpl" \
    "use_after_free_alloc_state:$ROOT/cqpl/queries_v2/use_after_free_alloc_state.cqpl" \
    "allocator_mismatch_ub_v2:$ROOT/cqpl/queries_v2/allocator_mismatch_ub_v2.cqpl"
  do
    q="${spec%%:*}"; qp="${spec#*:}"
    run_logged \
      "$name $q checker" \
      "$od/$q.checker.log" \
      "$CHECKER" "$od/annotated_icfg_v2.json" "$qp" \
        --explain-json "$od/$q.explain.json"
  done
done < "$OUT/fixture-order.txt"

echo "[4/6] Freeze fixture characterization"
python3 "$ROOT/cqpl/scripts/summarize_bodyless_ffi_fixtures.py" \
  --run-root "$OUT" \
  --manifest "$MANIFEST" \
  --producer-status "$OUT/producer-status.tsv" \
  --out "$OUT/BODYLESS_FFI_FIXTURE_BASELINE.json"

echo "[5/6] EFX1 evidence census"
python3 "$ROOT/cqpl/scripts/audit_bodyless_ffi_baseline.py" \
  --artifact-root "$OUT" \
  --out "$OUT/FIXTURE_EFX1_INVENTORY.json"

if [[ -n "$FROZEN118" ]]; then
  python3 "$ROOT/cqpl/scripts/audit_bodyless_ffi_baseline.py" \
    --artifact-root "$FROZEN118" \
    --out "$OUT/FROZEN118_EFX1_INVENTORY.json"
else
  echo "frozen118 audit skipped (optional second argument not supplied)"
fi

echo "[6/6] Freeze manifest"
python3 - "$ROOT" "$OUT" "$FROZEN118" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
root=Path(sys.argv[1]); out=Path(sys.argv[2]); frozen=sys.argv[3] or None
files=[]
for p in sorted(out.rglob("*")):
    if p.is_file():
        files.append({"path":str(p.relative_to(out)),"sha256":hashlib.sha256(p.read_bytes()).hexdigest(),"bytes":p.stat().st_size})
fixture_report=json.load(open(out/"BODYLESS_FFI_FIXTURE_BASELINE.json"))
fixture_inventory=json.load(open(out/"FIXTURE_EFX1_INVENTORY.json"))
report={
 "schema":"cqpl_bodyless_ffi_phase_a_gate_v2",
 "status":"PASS" if fixture_report["status"]=="PASS" and fixture_inventory["status"]=="PASS" else "FAIL",
 "git_head":subprocess.check_output(["git","-C",str(root),"rev-parse","HEAD"],text=True).strip(),
 "git_branch":subprocess.check_output(["git","-C",str(root),"branch","--show-current"],text=True).strip(),
 "toolchain":"nightly-2024-11-21",
 "fixture_count":len(json.load(open(root/"cqpl/bodyless_ffi_fixture_manifest.json"))["fixtures"]),
 "producer_status_counts":fixture_report["producer_status_counts"],
 "expected_structured_fail_closed_count":fixture_report["expected_structured_fail_closed_count"],
 "historical_frozen118":frozen,
 "semantic_change_in_this_checkpoint":False,
 "files":files,
}
(out/"BODYLESS_FFI_PHASE_A_GATE.json").write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
print(f"CQPL_BODYLESS_FFI_PHASE_A_GATE: {report['status']}")
print("fixture_count=",report["fixture_count"])
print("producer_status_counts=",report["producer_status_counts"])
print("out=",out)
if report["status"]!="PASS":
    raise SystemExit(2)
PY
