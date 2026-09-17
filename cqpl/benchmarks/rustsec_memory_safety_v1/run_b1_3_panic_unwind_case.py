#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

EXPECTED_RUSTC = "rustc 1.84.0-nightly (3fee0f12e 2024-11-20)"
REQUIRED_CAPS = {
    "allocation_state_v1",
    "allocation_contracts_v1",
    "allocation_contracts_v2",
    "mir_semantic_labels_v1",
    "mir_semantics_v2",
    "panic_unwind_lifecycle_v1",
    "panic_lifecycle_state_v1",
    "panic_lifecycle_state_v2",
}

# The historical FINAL112 contract remains exactly the frozen 12 queries in
# queries_v2. The panic lifecycle probe is experimental and intentionally kept
# outside that directory so old-corpus regression remains byte/cardinality
# stable.
FROZEN_QUERY_STEMS = {
    "allocator_mismatch_ub",
    "allocator_mismatch_ub_v2",
    "double_free_alloc",
    "double_free_alloc_state",
    "leak_alloc",
    "leak_alloc_state",
    "mir_rvalue_presence",
    "mir_statement_presence",
    "mir_structural_allocator_example",
    "mir_terminator_presence",
    "use_after_free_alloc",
    "use_after_free_alloc_state",
}
EXPERIMENTAL_QUERY_STEMS = {"panic_repeat_drop_alloc"}
REQUIRED_QUERY_STEMS = FROZEN_QUERY_STEMS | EXPERIMENTAL_QUERY_STEMS


MEMORY_QUERIES = {
    "double_free_alloc", "double_free_alloc_state",
    "use_after_free_alloc", "use_after_free_alloc_state",
    "leak_alloc", "leak_alloc_state",
    "allocator_mismatch_ub", "allocator_mismatch_ub_v2",
    "panic_repeat_drop_alloc",
}


def run(
    cmd: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    log: Path | None = None,
) -> subprocess.CompletedProcess:
    if log is None:
        return subprocess.run(cmd, cwd=cwd, env=env, text=True)
    with log.open("w", encoding="utf-8") as f:
        return subprocess.run(
            cmd, cwd=cwd, env=env, text=True,
            stdout=f, stderr=subprocess.STDOUT,
        )


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def prepare_harness_workspace(case: Path, variant: str, variant_out: Path) -> Path:
    """Copy only the B1.2 subject/harness inputs into an analysis workspace.

    `aligned_box 0.3.1` is published with edition=2024.  The benchmark is pinned
    to nightly-2024-11-21; if that Cargo rejects the normalized manifest, the
    analysis workspace applies the already-admitted B1.2 manifest shim only to
    the copied manifest.  Canonical benchmark source bytes remain untouched.
    """
    work_case = variant_out / "analysis-workspace" / "case"
    subject = case / variant
    harness = case / "harness" / variant
    reproducer = case / "reproducer"
    for required in (subject / "Cargo.toml", harness / "Cargo.toml", reproducer / "main.rs"):
        if not required.is_file():
            raise SystemExit(f"missing B1.2 input: {required}")

    shutil.copytree(subject, work_case / variant)
    shutil.copytree(harness, work_case / "harness" / variant)
    shutil.copytree(reproducer, work_case / "reproducer")

    copied_manifest = work_case / variant / "Cargo.toml"
    manifest = copied_manifest.read_text(encoding="utf-8")
    if 'edition = "2024"' in manifest:
        copied_manifest.write_text(
            manifest.replace('edition = "2024"', 'edition = "2021"', 1),
            encoding="utf-8",
        )
        (variant_out / "manifest-shim.txt").write_text(
            "analysis-only shim: edition=2024 -> edition=2021 in copied subject manifest\n"
            f"canonical_manifest={subject / 'Cargo.toml'}\n",
            encoding="utf-8",
        )

    return work_case / "harness" / variant


def represented_dependency_nodes(doc: dict, needle: str) -> list[str]:
    ids = [str(node.get("id", "")) for node in doc.get("nodes", []) if isinstance(node, dict)]
    return sorted(node_id for node_id in ids if needle in node_id and "aligned_box" in node_id)


def panic_lifecycle_diagnostics(doc: dict) -> dict:
    coverage_counts = {"complete": 0, "unresolved": 0}
    unresolved_nodes: list[str] = []
    repeat_drop_may_nodes: list[str] = []
    repeat_drop_may_records = 0

    for raw_node in doc.get("nodes", []):
        if not isinstance(raw_node, dict):
            continue
        node_id = str(raw_node.get("id", ""))
        coverage = raw_node.get("panic_lifecycle_coverage")
        if coverage in coverage_counts:
            coverage_counts[coverage] += 1
        if coverage == "unresolved":
            unresolved_nodes.append(node_id)

        node_has_repeat_drop = False
        for record in raw_node.get("panic_lifecycle", []):
            if not isinstance(record, dict):
                continue
            if (
                record.get("certainty") == "may_abstract"
                and record.get("may_own") is True
                and record.get("may_partial_drop") is True
                and record.get("may_stale_owner") is True
            ):
                repeat_drop_may_records += 1
                node_has_repeat_drop = True
        if node_has_repeat_drop:
            repeat_drop_may_nodes.append(node_id)

    return {
        "coverage_counts": coverage_counts,
        "unresolved_nodes": sorted(unresolved_nodes),
        "repeat_drop_may_records": repeat_drop_may_records,
        "repeat_drop_may_nodes": sorted(repeat_drop_may_nodes),
    }


def analyze_variant(
    *,
    cqpl: Path,
    crema: Path,
    case: Path,
    variant: str,
    toolchain: str,
    out: Path,
    required_dependency_defpath: str,
) -> dict:
    variant_out = out / variant
    variant_out.mkdir(parents=True, exist_ok=True)
    source = prepare_harness_workspace(case, variant, variant_out)

    annotated = variant_out / "annotated_icfg_v2.json"
    identity = variant_out / "allocation_identity.json"
    coverage = variant_out / "semantic_coverage.json"
    plan = variant_out / "cargo_analysis_plan.json"

    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(variant_out / "cargo-target")

    build = [
        "cargo", f"+{toolchain}", "build",
        "--manifest-path", str(source / "Cargo.toml"),
        "--bin", "reproducer",
    ]
    if (source / "Cargo.lock").is_file():
        build.append("--locked")
    cp = run(build, cwd=source, env=env, log=variant_out / "build.log")
    if cp.returncode != 0:
        raise SystemExit(f"{variant}: cargo build failed; see {variant_out / 'build.log'}")

    # Important: analyze the *reproducer harness*, not the aligned_box library in
    # isolation.  The A3 profile must import reachable dependency MIR so the
    # caller cleanup/unwind chain and the vulnerable/fixed dependency body coexist
    # in one ICFG.
    cmd = [
        "cargo", f"+{toolchain}", "run",
        "--manifest-path", str(crema / "Cargo.toml"), "--",
        str(source),
        "--analysis-mode", "application",
        "--cargo-kind", "bin",
        "--cargo-target", "reproducer",
        "--only-icfg-annotated",
        "--cqpl-schema-version", "2",
        "--annotated-icfg-out", str(annotated),
        "--allocation-identity-out", str(identity),
        "--cargo-plan-out", str(plan),
        "--semantic-coverage-out", str(coverage),
        "--mir-semantics-v2",
        "--panic-unwind-lifecycle-v1",
    ]
    (variant_out / "crema-command.txt").write_text(
        " ".join(json.dumps(x) for x in cmd) + "\n", encoding="utf-8"
    )
    cp = run(cmd, cwd=crema, env=env, log=variant_out / "crema-export.log")
    if cp.returncode != 0:
        raise SystemExit(f"{variant}: CREMA failed rc={cp.returncode}; see {variant_out / 'crema-export.log'}")
    if not annotated.is_file() or not identity.is_file():
        raise SystemExit(f"{variant}: CREMA returned success without required artifacts")

    doc = json.loads(annotated.read_text(encoding="utf-8"))
    caps = set(doc.get("capabilities", []))
    missing = REQUIRED_CAPS - caps
    if missing:
        raise SystemExit(f"{variant}: missing capabilities {sorted(missing)}")

    imported = represented_dependency_nodes(doc, required_dependency_defpath)
    return {
        # Keep the result JSON-serializable.  These values are later passed
        # through `str(...)` to the checker, so retaining `Path` objects here
        # provides no benefit and makes summary.json fail at the very end of an
        # otherwise successful differential run.
        "annotated": str(annotated),
        "identity": str(identity),
        "annotated_sha256": sha256(annotated),
        "identity_sha256": sha256(identity),
        "entry": doc.get("entry"),
        "capabilities": sorted(caps),
        "node_count": len(doc.get("nodes", [])),
        "dependency_nodes": imported,
        "dependency_mir_imported": bool(imported),
        "panic_lifecycle": panic_lifecycle_diagnostics(doc),
    }


def discover_queries(cqpl: Path) -> list[Path]:
    """Return the frozen 12 queries plus the explicit A3 lifecycle probe.

    Keeping the probe outside queries_v2 is an experimental-isolation invariant:
    FINAL112 must continue to see exactly the historical 12-query language
    surface, while this RustSec runner opts into the new capability explicitly.
    """
    frozen_dir = cqpl / "queries_v2"
    frozen = sorted(frozen_dir.glob("*.cqpl"))
    frozen_stems = {query.stem for query in frozen}
    if frozen_stems != FROZEN_QUERY_STEMS:
        missing = sorted(FROZEN_QUERY_STEMS - frozen_stems)
        extra = sorted(frozen_stems - FROZEN_QUERY_STEMS)
        raise SystemExit(
            f"frozen queries_v2 contract mismatch: missing={missing} extra={extra}"
        )

    experimental_dir = cqpl / "queries_experimental"
    experimental = [experimental_dir / f"{stem}.cqpl" for stem in sorted(EXPERIMENTAL_QUERY_STEMS)]
    missing_experimental = [str(path) for path in experimental if not path.is_file()]
    if missing_experimental:
        raise SystemExit(f"missing experimental lifecycle queries: {missing_experimental}")

    queries = frozen + experimental
    if {query.stem for query in queries} != REQUIRED_QUERY_STEMS:
        raise SystemExit("internal RustSec query contract mismatch")
    return queries


def write_model_check_trace(path: Path, trace: dict) -> None:
    path.write_text(json.dumps(trace, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(
        description="B1.3/A3 panic-unwind differential runner over the admitted reproducer harness"
    )
    ap.add_argument("--root", type=Path, required=True, help="a-phd root containing crema/ and cqpl/")
    ap.add_argument("--case-id", default="rustsec_2026_0282_aligned_box_realloc_panic")
    ap.add_argument(
        "--dependency-defpath",
        default="realloc_with_default",
        help="substring that must occur in an imported aligned_box dependency MIR node",
    )
    ap.add_argument("--toolchain", default=os.environ.get("CREMA_RUST_TOOLCHAIN", "nightly-2024-11-21"))
    ap.add_argument("--out", type=Path)
    args = ap.parse_args()

    root = args.root.resolve()
    cqpl = (root / "cqpl").resolve()
    crema = (root / "crema").resolve()
    case = (cqpl / "benchmarks" / "rustsec_memory_safety_v1" / "cases" / args.case_id).resolve()
    if not case.is_dir():
        raise SystemExit(f"missing case: {case}")

    rustc = subprocess.check_output(
        ["rustup", "run", args.toolchain, "rustc", "--version"], text=True
    ).strip()
    if rustc != EXPECTED_RUSTC:
        raise SystemExit(f"unexpected pinned rustc: {rustc}")

    out = (args.out or (case / "evidence" / "b1_3_a3_panic_unwind_v1")).resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    checker_manifest = cqpl / "cqpl_checker" / "Cargo.toml"
    checker = cqpl / "cqpl_checker" / "target" / "debug" / "cqpl_checker"
    cp = run(
        ["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(checker_manifest)],
        cwd=cqpl,
        log=out / "checker-build.log",
    )
    if cp.returncode != 0 or not checker.is_file():
        raise SystemExit("CQPL checker build failed")

    results = {}
    for variant in ("vulnerable", "fixed"):
        results[variant] = analyze_variant(
            cqpl=cqpl,
            crema=crema,
            case=case,
            variant=variant,
            toolchain=args.toolchain,
            out=out,
            required_dependency_defpath=args.dependency_defpath,
        )

    missing_dependency = [
        variant for variant, result in results.items()
        if not result["dependency_mir_imported"]
    ]
    if missing_dependency:
        summary = {
            "case_id": args.case_id,
            "classification": "dependency_mir_not_imported",
            "missing_variants": missing_dependency,
            "required_dependency_defpath": args.dependency_defpath,
            "variants": results,
            "note": "do not attribute an ff/ff result to panic semantics until the aligned_box body is present in both ICFGs",
        }
        (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(summary, indent=2))
        return 4

    structural_differential = (
        results["vulnerable"]["annotated_sha256"]
        != results["fixed"]["annotated_sha256"]
    )
    if not structural_differential:
        summary = {
            "case_id": args.case_id,
            "classification": "analysis_input_not_differentiated",
            "structural_differential": False,
            "variants": results,
            "note": "dependency MIR is present, but vulnerable/fixed annotated ICFGs are still identical; do not claim panic-unwind causality yet",
        }
        (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(summary, indent=2))
        return 3

    queries = discover_queries(cqpl)

    # Persist an execution ledger independently from summary.json.  It is
    # updated after each checker invocation so an interrupted run still proves
    # which model-checking queries actually executed.
    trace_path = out / "model-checking-execution.json"
    model_check_trace = {
        "checker": str(checker),
        "query_sources": {
            "frozen": str(cqpl / "queries_v2"),
            "experimental": str(cqpl / "queries_experimental"),
        },
        "queries_discovered": [query.stem for query in queries],
        "required_queries": sorted(REQUIRED_QUERY_STEMS),
        "variants": {"vulnerable": [], "fixed": []},
    }
    write_model_check_trace(trace_path, model_check_trace)

    matrix: dict[str, dict[str, str]] = {}
    for variant in ("vulnerable", "fixed"):
        qdir = out / variant / "queries"
        qdir.mkdir()
        for query in queries:
            stdout = qdir / f"{query.stem}.json"
            stderr = qdir / f"{query.stem}.stderr.log"
            command_file = qdir / f"{query.stem}.command.txt"
            explain = qdir / f"{query.stem}.explain.json"

            checker_cmd = [
                str(checker),
                str(results[variant]["annotated"]),
                str(query),
                "--json",
            ]
            # The panic lifecycle atom is MAY-only.  Preserve its diagnostic
            # frontier in the same benchmark run rather than requiring a
            # separate manual checker invocation.
            if query.stem == "panic_repeat_drop_alloc":
                checker_cmd.extend([
                    "--explain-json", str(explain),
                    "--explain-unk-verbose",
                ])

            command_file.write_text(
                " ".join(json.dumps(arg) for arg in checker_cmd) + "\n",
                encoding="utf-8",
            )

            with stdout.open("w", encoding="utf-8") as so, stderr.open("w", encoding="utf-8") as se:
                cp = subprocess.run(
                    checker_cmd,
                    cwd=cqpl / "cqpl_checker",
                    text=True,
                    stdout=so,
                    stderr=se,
                )

            record = {
                "query": query.stem,
                "query_file": str(query),
                "command_file": str(command_file),
                "stdout": str(stdout),
                "stderr": str(stderr),
                "returncode": cp.returncode,
            }
            if query.stem == "panic_repeat_drop_alloc":
                record["explain_json"] = str(explain)
            model_check_trace["variants"][variant].append(record)
            write_model_check_trace(trace_path, model_check_trace)

            if cp.returncode != 0:
                raise SystemExit(f"{variant}/{query.name}: checker failed rc={cp.returncode}")
            verdict = json.loads(stdout.read_text(encoding="utf-8")).get("result")
            if verdict not in {"ff", "unk", "tt"}:
                raise SystemExit(f"{variant}/{query.name}: invalid verdict {verdict!r}")
            if query.stem == "panic_repeat_drop_alloc" and verdict == "tt":
                raise SystemExit(
                    f"{variant}/{query.name}: unsound tt for MAY-only panic lifecycle predicate"
                )
            if (
                query.stem == "panic_repeat_drop_alloc"
                and verdict == "unk"
                and not explain.is_file()
            ):
                raise SystemExit(
                    f"{variant}/{query.name}: unk verdict without explainer artifact"
                )

            record["verdict"] = verdict
            record["explain_exists"] = explain.is_file() if query.stem == "panic_repeat_drop_alloc" else False
            write_model_check_trace(trace_path, model_check_trace)
            matrix.setdefault(query.stem, {})[variant] = verdict

    different = {
        q: values for q, values in matrix.items()
        if values.get("vulnerable") != values.get("fixed")
    }
    memory_different = {q: v for q, v in different.items() if q in MEMORY_QUERIES}

    summary = {
        "case_id": args.case_id,
        "classification": (
            "symbolically_differentiated"
            if memory_different
            else "semantic_gap_after_distinct_inputs"
        ),
        "structural_differential": True,
        "dependency_mir_imported": True,
        "memory_query_differential": memory_different,
        "all_query_differential": different,
        "matrix": matrix,
        "model_checking": {
            "execution_trace": str(trace_path),
            "queries_executed": [query.stem for query in queries],
            "query_count": len(queries),
        },
        "variants": results,
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
