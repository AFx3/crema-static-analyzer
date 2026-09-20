#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-$(pwd)}"
ROOT="$(cd "$ROOT" && pwd)"
TOOLCHAIN="nightly-2024-11-21"
QUERY_OLD="$ROOT/cqpl/queries_v2/double_free_alloc_state.cqpl"
QUERY_NORMAL="$ROOT/cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl"
OUT_ROOT="$ROOT/repro-results/gate-double-free-normal-execution"

run_crema() {
  local rel="$1"
  local out="$2"
  local target="$ROOT/tests_and_target_repos/$rel"
  rm -rf "$out"
  mkdir -p "$out"
  cargo +"$TOOLCHAIN" build --manifest-path "$target/Cargo.toml" --locked >/dev/null
  (
    cd "$ROOT/crema"
    cargo +"$TOOLCHAIN" run -- \
      "$target" \
      --only-icfg-annotated \
      --cqpl-schema-version 2 \
      --mir-semantics-v2 \
      --annotated-icfg-out "$out/annotated_icfg_v2.json" \
      --allocation-identity-out "$out/allocation_identity.json"
  ) >"$out/crema.log" 2>&1
  test -s "$out/annotated_icfg_v2.json"
}

run_query() {
  local artifact="$1"
  local query="$2"
  local explain="$3"
  local log="$4"
  (
    cd "$ROOT/cqpl/cqpl_checker"
    cargo +"$TOOLCHAIN" run -- \
      "$artifact" "$query" --explain-json "$explain"
  ) >"$log" 2>&1
}

assert_report() {
  local report="$1"
  local expected_result="$2"
  local expected_subresult="$3"
  python3 - "$report" "$expected_result" "$expected_subresult" <<'PY'
import json, sys
p, expected_result, expected_subresult = sys.argv[1:]
r = json.load(open(p, encoding="utf-8"))
assert r["result"] == expected_result, (p, r["result"], expected_result)
assert r["assessment"]["subresult"] == expected_subresult, (p, r["assessment"]["subresult"], expected_subresult)
if r["assessment"].get("assessment_scope") is not None:
    pass
for cert in r.get("diagnostic_certificates", []):
    if cert.get("assessment_scope") == "normal_execution":
        for edge in cert.get("abstract_witness", {}).get("edges", []):
            flows = edge.get("flows", [])
            assert isinstance(flows, list) and "normal" in flows, (p, edge)
            assert edge.get("basis") == "typed_edge_flow_v1", (p, edge)
print(f"PASS {p}: {expected_result}/{expected_subresult}")
PY
}

echo "[1/3] clean_alloc_read_and_drop: all-execution vs normal-execution"
CLEAN="$OUT_ROOT/clean"
run_crema "a-code_full_rust/clean_alloc_read_and_drop" "$CLEAN"
run_query "$CLEAN/annotated_icfg_v2.json" "$QUERY_OLD" "$CLEAN/old.explain.json" "$CLEAN/old.log"
run_query "$CLEAN/annotated_icfg_v2.json" "$QUERY_NORMAL" "$CLEAN/normal.explain.json" "$CLEAN/normal.log"
assert_report "$CLEAN/old.explain.json" unk unk_true
assert_report "$CLEAN/normal.explain.json" unk unk_false
python3 - "$CLEAN/old.explain.json" "$CLEAN/normal.explain.json" <<'PY'
import json, sys
old, new = (json.load(open(p, encoding="utf-8")) for p in sys.argv[1:])
assert old["result"] == new["result"] == "unk"
assert not new.get("supporting_findings"), new.get("supporting_findings")
ref = new.get("refuting_findings", [])
assert ref, "expected refuting findings"
assert all(f["kind"] == "all_candidate_drop_suffixes_exclude_repeated_drop" for f in ref)
assert len(new.get("diagnostic_certificates", [])) == len(ref)
print("PASS clean: truth unchanged; unwind-only positive witness removed; negative certificate present")
PY

echo "[2/3] boxed_bool double-free: real normal witness remains positive"
BOOL="$OUT_ROOT/boxed_bool_df"
run_crema "a-code_full_rust/a-double_free_full_rust_literals/boxed_bool" "$BOOL"
run_query "$BOOL/annotated_icfg_v2.json" "$QUERY_NORMAL" "$BOOL/normal.explain.json" "$BOOL/normal.log"
assert_report "$BOOL/normal.explain.json" unk unk_true

echo "[3/3] boxed_char double-free: real normal witness remains positive"
CHAR="$OUT_ROOT/boxed_char_df"
run_crema "a-code_full_rust/a-double_free_full_rust_literals/boxed_char" "$CHAR"
run_query "$CHAR/annotated_icfg_v2.json" "$QUERY_NORMAL" "$CHAR/normal.explain.json" "$CHAR/normal.log"
assert_report "$CHAR/normal.explain.json" unk unk_true

echo "DOUBLE_FREE_NORMAL_EXECUTION_GATE: PASS"
echo "out=$OUT_ROOT"
