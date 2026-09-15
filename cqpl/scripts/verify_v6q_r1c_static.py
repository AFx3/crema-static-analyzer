#!/usr/bin/env python3
from __future__ import annotations

import csv
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]).resolve()
errors: list[str] = []


def check(cond: bool, msg: str) -> None:
    if not cond:
        errors.append(msg)


def text(rel: str) -> str:
    p = root / rel
    check(p.is_file(), f"missing {rel}")
    return p.read_text(errors="replace") if p.is_file() else ""


expected_q = {
    "allocator_mismatch_ub.cqpl",
    "allocator_mismatch_ub_v2.cqpl",
    "double_free_alloc.cqpl",
    "double_free_alloc_state.cqpl",
    "leak_alloc.cqpl",
    "leak_alloc_state.cqpl",
    "mir_rvalue_presence.cqpl",
    "mir_statement_presence.cqpl",
    "mir_structural_allocator_example.cqpl",
    "mir_terminator_presence.cqpl",
    "use_after_free_alloc.cqpl",
    "use_after_free_alloc_state.cqpl",
}
queries = sorted((root / "queries_v2").glob("*.cqpl"))
check(len(queries) == 12, f"queries_v2 count={len(queries)} expected=12")
check({p.name for p in queries} == expected_q, "queries_v2 exact set mismatch")

parser = text("cqpl_checker/src/parser.rs")
terms = [
    "goto", "switch_int", "unwind_resume", "unwind_terminate", "return",
    "unreachable", "drop", "call", "tail_call", "assert", "yield",
    "coroutine_drop", "false_edge", "false_unwind", "inline_asm", "unhandled",
]
for t in terms:
    check(f'"{t}"' in parser, f"parser missing terminator {t}")
check(
    "parses_full_v6q_terminator_vocabulary_emitted_by_crema" in parser,
    "missing full terminator vocabulary regression test",
)

runner = text("scripts/run_corpus_allocator_contracts.py")
check("CREMA_CQPL_DIR" in runner, "corpus runner is not self-contained via CREMA_CQPL_DIR")
check(runner.count('"--mir-semantics-v2"') == 1, "corpus runner must add exactly one MIR-v2 export flag")
pos = runner.find('"--annotated-icfg-out", str(annotated)')
window = runner[pos:pos + 900] if pos >= 0 else ""
check(
    pos >= 0
    and "if args.schema_version == 2:" in window
    and '"--allocation-identity-out"' in window
    and '"--mir-semantics-v2"' in window,
    "schema-v2 CREMA export block does not couple identity output + MIR-v2",
)

runall = text("run_all.sh")
for token in [
    "subjects=112", "queries=12", "attempts=1344",
    "run_registry_crates_v6q_r1c.sh", "run_queries_v2_matrix.py",
    "CREMA_CQPL_DIR", "--schema-version 2",
]:
    check(token in runall, f"run_all missing gate/token: {token}")
check('cargo +"$NIGHTLY" test' in runall, "run_all does not test checker before corpus evaluation")
check("CQPL RUN_ALL v6Q-r1c: PASS subjects=112 queries=12 attempts=1344" in runall, "run_all final marker missing")

single = text("scripts/run_one_target_v6q_r1c.py")
for token in [
    "--relative-path", "--mir-semantics-v2", "annotated_icfg_v2.json",
    "allocation_identity.json", "TARGET_ANALYSIS_CONFIG.tsv", "entry_overrides.json",
    "queries_v2", "CQPL_ONE_TARGET v6Q-r1c: PASS",
]:
    check(token in single, f"single-target runner missing {token}")

registry = text("scripts/run_registry_crates_v6q_r1c.sh")
for token in [
    "--analysis-mode", "--cargo-kind", "--api-root", "--mir-semantics-v2",
    "UNRESOLVED_HIGHER_ORDER:", "OptionMap callee=", "rvalues.unmodeled",
    "statements.unmodeled", "terminators.unmodeled",
]:
    check(token in registry, f"registry runner missing {token}")

targets_path = root / "artifact" / "CRATES_IO_TARGETS.json"
try:
    targets = json.loads(targets_path.read_text())
    got = [(c["name"], c["version"], c["analysis_mode"], c["cargo_kind"], c["api_root"]) for c in targets["crates"]]
    want = [
        ("unicode-ident", "1.0.18", "library", "lib", "unicode_ident::is_xid_start"),
        ("ryu", "1.0.20", "library", "lib", "ryu::pretty::format32"),
        ("memchr", "2.7.4", "library", "lib", "memchr::memchr::memchr"),
    ]
    check(got == want, f"CRATES_IO_TARGETS mismatch: {got!r}")
    check("no silent lib/bin fallback" in targets.get("selection_policy", ""), "registry selection policy must forbid silent target substitution")
except Exception as e:
    errors.append(f"CRATES_IO_TARGETS parse: {e}")

version = text("VERSION")
check("CREMA-CQPL-v6Q-r1c" in version, "VERSION is not v6Q-r1c")
check("status=final112-runtime-validated-frozen" in version, "VERSION is not final runtime-validated")
check("semantic_baseline=CREMA-CQPL-v6Q-r1b" in version, "VERSION semantic baseline mismatch")
check("bd77b63d67523346b4e3c3cd9554f9787d7685e0f46265912137d55a98a8e902" in version, "VERSION runtime evidence SHA missing")

runtime_path = root / "artifact" / "FINAL112_RUNTIME_EVIDENCE.json"
try:
    r = json.loads(runtime_path.read_text())
    check(r.get("status") == "PASS", "runtime evidence is not PASS")
    check(r.get("subjects") == 112 and r.get("queries") == 12 and r.get("attempts") == 1344, "runtime evidence cardinality mismatch")
    check(r.get("query_nonzero_rc") == 0, "runtime evidence has nonzero query rc")
    check(r.get("r1c_vs_r1b_matrix_mismatches") == 0, "r1c/r1b matrix mismatch")
    check(r.get("sha256sums_final_failures") == 0, "runtime evidence checksum failures")
except Exception as e:
    errors.append(f"FINAL112_RUNTIME_EVIDENCE parse: {e}")

summary_path = root / "artifact" / "FINAL112_AUDIT_SUMMARY.json"
try:
    a = json.loads(summary_path.read_text())
    check(a["census"]["subjects"] == 112, "audit subjects !=112")
    check(a["query_matrix"]["attempts"] == 1344, "audit attempts !=1344")
    check(a["query_matrix"]["graph_validation_failures"] == 0, "audit graph validation failures nonzero")
    check(a["query_matrix"]["query_result_mismatches"] == 0, "audit query mismatches nonzero")
    check(a["query_matrix"]["result_counts"] == {"ff": 650, "unk": 468, "tt": 226}, "audit truth counts differ")
    check(a["identity_sidecars"]["records_checked"] == 6894 and a["identity_sidecars"]["mismatches"] == 0, "identity sidecar audit mismatch")
    check(a["source_oracle_audit"]["unresolved_inconsistencies"] == 0, "source/oracle unresolved inconsistencies nonzero")
    check(a.get("candidate_status") == "RUNTIME_VALIDATED_FINAL_FREEZE", "audit summary is not final freeze")
except Exception as e:
    errors.append(f"FINAL112_AUDIT_SUMMARY parse: {e}")

# The 12 query formulas remain byte-identical to the audited run according to the frozen delta record.
delta_path = root / "artifact" / "V6Q_R1C_DELTA_SUMMARY.json"
try:
    delta = json.loads(delta_path.read_text())
    check(delta.get("all_12_queries_byte_identical_to_final_run") is True, "12 frozen queries are not byte-identical according to delta summary")
    changed = [x["file"] for x in delta["checker_source_delta"] if x["changed"]]
    check(changed == ["parser.rs"], f"unexpected checker source delta: {changed}")
except Exception as e:
    errors.append(f"V6Q_R1C_DELTA_SUMMARY parse: {e}")

# Per-subject audits.
gqa = root / "artifact" / "FINAL112_GRAPH_QUERY_AUDIT.tsv"
try:
    rows = list(csv.DictReader(gqa.open(), delimiter="\t"))
    check(len(rows) == 112, f"graph/query audit rows={len(rows)} expected=112")
    check(len({(r["group"], r["target"]) for r in rows}) == 112, "graph/query audit subject duplicates")
    check(all(r["graph_validation"] == "PASS" for r in rows), "graph/query audit has graph validation failure")
except Exception as e:
    errors.append(f"graph/query audit parse: {e}")

soa = root / "artifact" / "FINAL112_SOURCE_ORACLE_AUDIT.tsv"
try:
    rows = list(csv.DictReader(soa.open(), delimiter="\t"))
    check(len(rows) == 112, f"source/oracle audit rows={len(rows)} expected=112")
    allowed = {"RESULT_SOURCE_CONSISTENT", "RESULT_SOURCE_CONSISTENT_EXPLAINED_LEGACY_DEVIATION", "RESULT_CONSISTENT_NO_REFERENCE_ORACLE"}
    check(all(r["status"] in allowed for r in rows), "source/oracle audit has unresolved status")
except Exception as e:
    errors.append(f"source/oracle audit parse: {e}")

for rel in [
    "README.md", "LANGUAGE.md", "QUERY_CATALOG.md", "ANALYSIS_GUIDE.md",
    "ANNOTATED_ICFG.md", "FINAL_VALIDATION.md", "FINALIZATION.md", "INTEGRATION_NEXT.md",
    "capabilities/mir_semantic_labels_v1.md",
]:
    s = text(rel)
    check("v6Q-r1c" in s or rel == "capabilities/mir_semantic_labels_v1.md", f"{rel} does not describe current r1c state")

for rel in ["README.md", "LANGUAGE.md", "ANNOTATED_ICFG.md", "FINAL_VALIDATION.md"]:
    s = text(rel)
    check("v6N-r1a" not in s, f"{rel} retains stale v6N-r1a current-state text")

# JSON validity.
for p in root.rglob("*.json"):
    if "target" in p.parts:
        continue
    try:
        json.loads(p.read_text())
    except Exception as e:
        errors.append(f"invalid JSON {p.relative_to(root)}: {e}")

# Exact package manifest.
manifest_path = root / "MANIFEST_SHA256.json"
try:
    m = json.loads(manifest_path.read_text())
    shipped = []
    for p in root.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(root).as_posix()
        if rel == "MANIFEST_SHA256.json" or "__pycache__" in p.parts or p.suffix == ".pyc":
            continue
        if "target" in p.parts:
            continue
        shipped.append(rel)
    shipped = sorted(shipped)
    check(sorted(m) == shipped, f"manifest file-set mismatch missing={sorted(set(shipped)-set(m))[:10]} extra={sorted(set(m)-set(shipped))[:10]}")
    for rel in sorted(set(m) & set(shipped)):
        got = hashlib.sha256((root / rel).read_bytes()).hexdigest()
        if got != m[rel]:
            errors.append(f"manifest hash mismatch {rel}")
except Exception as e:
    errors.append(f"manifest validation: {e}")

if errors:
    print("V6Q_R1C_STATIC_VERIFY: FAIL")
    for e in errors:
        print(" -", e)
    sys.exit(1)

print("V6Q_R1C_STATIC_VERIFY: PASS")
print("status=final112-runtime-validated-frozen")
print("queries=12 subjects=112 attempts=1344 graph_mismatches=0 query_mismatches=0 identity_records=6894")
print("runtime_matrix_r1c_vs_r1b_mismatches=0")
print("terminator_parser_vocabulary=16 aligned-with-CREMA-v6Q-producer")
print("run_all=self-contained-via-CREMA_CQPL_DIR")
