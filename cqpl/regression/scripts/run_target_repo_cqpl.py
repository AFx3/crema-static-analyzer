#!/usr/bin/env python3
"""Run CQPL over CREMA's real tests_and_target_repos corpus.

Scientific policy:
- CREMA is only a producer of an AnnotatedIcfg via --only-icfg-annotated.
- CQPL evaluates the same three official formulas for every selected target.
- Legacy CREMA results are reference observations, not ground truth for CQPL.
- A legacy positive with CQPL=ff is highlighted for review, but the runner does
  not silently rewrite either oracle.
- Only explicitly reviewed CQPL expectations are enforced when requested.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
from typing import Any

QUERY_FILES = {
    "leak": "leak.cqpl",
    "double_free": "double_free.cqpl",
    "use_after_free": "use_after_free.cqpl",
}
LEGACY_CLASS_FOR_QUERY = {
    "leak": "ML",
    "double_free": "DF",
    "use_after_free": "UAF",
}
# Phase-5 allocator/ownership-contract diagnostics are intentionally not
# translated into CQPL yet.  The current formal language has no allocation-
# family/provenance atom capable of expressing UB_FFI without inventing
# semantics outside the theory.
UNMODELED_LEGACY_CLASSES = {"UB_FFI"}
PHASE5_DIR = "a-code_c_to_rust_alloc"
FROZEN_EXCLUDED = {
    "no_errors_projects/openapi-client-gen",
    "a-code_full_rust/drop_raw_ptr_no_free",
}


def run(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    stdout_path: Path | None = None,
    timeout: float | None = None,
) -> subprocess.CompletedProcess[str]:
    if stdout_path is None:
        return subprocess.run(
            cmd, cwd=cwd, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=timeout,
        )
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    with stdout_path.open("w", encoding="utf-8") as f:
        return subprocess.run(
            cmd, cwd=cwd, text=True,
            stdout=f, stderr=subprocess.STDOUT,
            timeout=timeout,
        )


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def discover_minimal_cargo_roots(tests: Path) -> list[Path]:
    tests = tests.resolve()
    manifests = []
    for p in tests.rglob("Cargo.toml"):
        rel_parts = p.relative_to(tests).parts
        if "target" in rel_parts or any(part.startswith(".") for part in rel_parts):
            continue
        manifests.append(p)
    roots = []
    manifest_dirs = {p.parent.resolve() for p in manifests}
    for manifest in manifests:
        d = manifest.parent.resolve()
        anc = d.parent
        nested_under_cargo_root = False
        while anc != tests.resolve() and tests.resolve() in anc.parents:
            if anc in manifest_dirs:
                nested_under_cargo_root = True
                break
            anc = anc.parent
        if not nested_under_cargo_root:
            roots.append(d)
    return sorted(set(roots), key=lambda p: p.relative_to(tests).as_posix())


def target_key(rel: str) -> str:
    p = Path(rel)
    name = p.name
    parent = p.parent.name
    if parent == "a-double_free_full_rust_literals":
        return f"{name}__df"
    if parent == "a-memory_leaks_full_rust_literals":
        return f"{name}__ml"
    if parent == "a-use_after_free_full_rust_literals":
        return f"{name}__uaf"
    return name


def select_targets(roots: list[Path], tests: Path, scope: str) -> list[tuple[str, Path]]:
    tests = tests.resolve()
    out = []
    for p in roots:
        rel = p.relative_to(tests).as_posix()
        if scope == "frozen92":
            if rel in FROZEN_EXCLUDED or rel.startswith(PHASE5_DIR + "/"):
                continue
        elif scope == "phase5-focus16":
            if not rel.startswith(PHASE5_DIR + "/"):
                continue
        elif scope != "all":
            raise ValueError(scope)
        out.append((rel, p))
    return out


def validate_annotated_icfg(path: Path) -> dict[str, int]:
    d = json.loads(path.read_text(encoding="utf-8"))
    if d.get("schema_version") != 1:
        raise ValueError(f"schema_version={d.get('schema_version')!r}, expected 1")
    variables = d.get("variables", [])
    var_ids = [v.get("id") for v in variables]
    if len(var_ids) != len(set(var_ids)) or any(not x for x in var_ids):
        raise ValueError("invalid/duplicate program variable id")
    nodes = d.get("nodes", [])
    node_ids = [n.get("id") for n in nodes]
    if len(node_ids) != len(set(node_ids)) or any(not x for x in node_ids):
        raise ValueError("invalid/duplicate node id")
    node_set = set(node_ids)
    if d.get("entry") not in node_set:
        raise ValueError("entry node is absent")
    for n in nodes:
        missing = set(n.get("successors", [])) - node_set
        if missing:
            raise ValueError(f"node {n.get('id')} has missing successors: {sorted(missing)}")
        for label in n.get("labels", []):
            if label.get("variable") not in set(var_ids):
                raise ValueError(f"label references undeclared variable {label.get('variable')}")
    return {"nodes": len(nodes), "variables": len(variables)}


def load_json(path: Path, default: Any) -> Any:
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else default


def clean_shared_artifacts(crema: Path, root: Path) -> None:
    for name in ["ffi_functions.json", "global_icfg.json", "global_icfg_nodes_edges.dot", "cqpl_annotated_icfg.json"]:
        try:
            (crema / name).unlink()
        except FileNotFoundError:
            pass
    svf = root / "SVF-example"
    for name in ["ffi.ll", "ffi.pre.bc", "ffi.pre.svf.bc"]:
        try:
            (svf / name).unlink()
        except FileNotFoundError:
            pass
    output = svf / "output"
    if output.is_dir():
        for p in output.glob("*_A_FINAL_ICFG.json"):
            p.unlink(missing_ok=True)


def parse_checker_json(stdout: str) -> str:
    d = json.loads(stdout)
    result = d.get("result")
    if result not in {"ff", "unk", "tt"}:
        raise ValueError(f"invalid CQPL result {result!r}")
    return result


def relation(legacy: list[str] | None, query: str, result: str) -> str:
    if legacy is None:
        return "no-legacy-reference"
    cls = LEGACY_CLASS_FOR_QUERY[query]
    old_positive = cls in legacy
    new_nonrefuting = result in {"unk", "tt"}
    if old_positive and new_nonrefuting:
        return "legacy-positive/cqpl-nonrefuting"
    if old_positive and not new_nonrefuting:
        return "REVIEW:legacy-positive/cqpl-refuted"
    if not old_positive and new_nonrefuting:
        return "cqpl-only-nonrefuting"
    return "both-negative-or-refuting"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", type=Path, default=Path(os.environ.get("CREMA_PHD_ROOT", ".")).resolve())
    ap.add_argument("--scope", choices=["all", "frozen92", "phase5-focus16"], default="all")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--toolchain", default=os.environ.get("CREMA_TOOLCHAIN", "nightly-2024-11-21"))
    ap.add_argument(
        "--query-timeout-seconds", type=float, default=120.0,
        help="per-query CQPL wall-clock timeout; <=0 disables it (default: 120)",
    )
    ap.add_argument("--only", action="append", default=[], help="run only a target key or relative target path; repeatable")
    ap.add_argument("--enforce-reviewed-oracle", action="store_true")
    ap.add_argument("--list-targets", action="store_true", help="print selected targets and exit without building/analyzing")
    args = ap.parse_args()

    root = args.root
    tests = root / "tests_and_target_repos"
    crema = root / "crema"
    cqpl = root / "cqpl"
    checker_manifest = cqpl / "cqpl_checker" / "Cargo.toml"
    checker_bin = cqpl / "cqpl_checker" / "target" / "debug" / "cqpl_checker"
    reference = cqpl / "regression" / "reference"
    oracle_path = cqpl / "regression" / "oracles" / "reviewed_cqpl.json"
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = (args.out or root / "repro-results" / f"cqpl-{args.scope}-{stamp}").resolve()
    raw = out / "raw"
    raw.mkdir(parents=True, exist_ok=True)

    required = [tests, crema / "Cargo.toml", checker_manifest]
    missing = [str(p) for p in required if not p.exists()]
    if missing:
        print("ERROR missing: " + ", ".join(missing), file=sys.stderr)
        return 2

    roots = discover_minimal_cargo_roots(tests)
    selected = select_targets(roots, tests, args.scope)
    if args.only:
        wanted = set(args.only)
        selected = [(rel, p) for rel, p in selected if target_key(rel) in wanted or rel in wanted]

    # Protocol guards: these prevent accidental corpus drift from being silently called frozen92/focus16.
    if args.scope == "frozen92" and not args.only and len(selected) != 92:
        print(f"ERROR frozen92 discovery yielded {len(selected)} targets, expected 92", file=sys.stderr)
        return 3
    if args.scope == "phase5-focus16" and not args.only and len(selected) != 16:
        print(f"ERROR phase5-focus16 discovery yielded {len(selected)} targets, expected 16", file=sys.stderr)
        return 3

    if args.list_targets:
        for rel, _target in selected:
            print(f"{target_key(rel)}\t{rel}")
        print(f"TOTAL\t{len(selected)}")
        return 0

    # Build checker once; execute the binary directly thereafter so stdout is pure JSON.
    preflight = run(["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(checker_manifest)])
    (out / "cqpl-checker-build.log").write_text(preflight.stdout + preflight.stderr, encoding="utf-8")
    if preflight.returncode != 0 or not checker_bin.is_file():
        print("ERROR CQPL checker build failed", file=sys.stderr)
        return 2

    legacy = load_json(reference / "legacy_frozen92.json", {})
    legacy.update(load_json(reference / "legacy_phase5_focus16.json", {}))
    entries = load_json(reference / "entry_overrides.json", {})
    reviewed_doc = load_json(oracle_path, {"targets": {}})
    reviewed = reviewed_doc.get("targets", {})

    env_lines = [
        f"timestamp_utc={dt.datetime.now(dt.timezone.utc).isoformat()}",
        f"root={root}", f"scope={args.scope}", f"toolchain={args.toolchain}",
        f"query_timeout_seconds={args.query_timeout_seconds}",
        f"discovered_minimal_cargo_roots={len(roots)}", f"selected_targets={len(selected)}",
    ]
    for cmd in [["rustc", f"+{args.toolchain}", "-vV"], ["cargo", f"+{args.toolchain}", "--version"]]:
        cp = run(cmd)
        env_lines.append("$ " + " ".join(cmd))
        env_lines.extend((cp.stdout + cp.stderr).splitlines())
    (out / "environment.txt").write_text("\n".join(env_lines) + "\n", encoding="utf-8")

    results: dict[str, Any] = {}
    infra_failures = 0
    oracle_mismatches = 0
    review_flags = 0
    status_lines = ["key\trelative_path\tbuild\texport\tschema\tleak\tdouble_free\tuse_after_free\toracle\n"]

    for idx, (rel, target) in enumerate(selected, 1):
        key = target_key(rel)
        print(f"[{idx}/{len(selected)}] {key} <- {rel}")
        tdir = raw / re.sub(r"[^A-Za-z0-9_.-]+", "_", key)
        tdir.mkdir(parents=True, exist_ok=True)
        item: dict[str, Any] = {"relative_path": rel, "key": key, "entry": entries.get(rel), "cqpl": {}}

        build_log = tdir / "build.log"
        cp = run(["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(target / "Cargo.toml")], stdout_path=build_log)
        item["build_exit"] = cp.returncode
        if cp.returncode != 0:
            infra_failures += 1
            item["status"] = "build-failed"
            results[key] = item
            status_lines.append(f"{key}\t{rel}\t{cp.returncode}\t-\t-\t-\t-\t-\t-\n")
            print(f"  BUILD_FAIL rc={cp.returncode}")
            continue

        clean_shared_artifacts(crema, root)
        ksharp = tdir / "annotated_icfg.json"
        cmd = ["cargo", f"+{args.toolchain}", "run", "--manifest-path", str(crema / "Cargo.toml"), "--", str(target)]
        if item["entry"]:
            cmd += ["-f", item["entry"]]
        cmd += ["--only-icfg-annotated", "--annotated-icfg-out", str(ksharp)]
        export_log = tdir / "crema-export.log"
        cp = run(cmd, cwd=crema, stdout_path=export_log)
        item["export_exit"] = cp.returncode
        if cp.returncode != 0 or not ksharp.is_file():
            infra_failures += 1
            item["status"] = "export-failed"
            results[key] = item
            status_lines.append(f"{key}\t{rel}\t0\t{cp.returncode}\t-\t-\t-\t-\t-\n")
            print(f"  EXPORT_FAIL rc={cp.returncode} file={ksharp.is_file()}")
            continue

        try:
            census = validate_annotated_icfg(ksharp)
            item["schema"] = "pass"
            item["census"] = census
        except Exception as e:
            infra_failures += 1
            item["schema"] = "fail"
            item["schema_error"] = str(e)
            item["status"] = "schema-failed"
            results[key] = item
            status_lines.append(f"{key}\t{rel}\t0\t0\tfail\t-\t-\t-\t-\n")
            print(f"  SCHEMA_FAIL {e}")
            continue

        query_failure = False
        query_timeout = args.query_timeout_seconds if args.query_timeout_seconds > 0 else None
        for qname, qfile in QUERY_FILES.items():
            qpath = cqpl / "queries" / qfile
            started = time.perf_counter()
            try:
                cp = run(
                    [str(checker_bin), str(ksharp), str(qpath), "--json"],
                    timeout=query_timeout,
                )
            except subprocess.TimeoutExpired as e:
                elapsed = time.perf_counter() - started
                infra_failures += 1
                query_failure = True
                stderr = e.stderr if isinstance(e.stderr, str) else ""
                (tdir / f"{qname}.stderr.log").write_text(stderr or "", encoding="utf-8")
                item["cqpl"][qname] = {
                    "timeout": True,
                    "timeout_seconds": query_timeout,
                    "elapsed_seconds": round(elapsed, 6),
                    "error": f"CQPL query exceeded {query_timeout}s wall-clock timeout",
                }
                print(f"  QUERY_TIMEOUT {qname} after {elapsed:.3f}s")
                continue

            elapsed = time.perf_counter() - started
            (tdir / f"{qname}.stderr.log").write_text(cp.stderr, encoding="utf-8")
            if cp.returncode != 0:
                infra_failures += 1
                query_failure = True
                item["cqpl"][qname] = {
                    "exit": cp.returncode,
                    "elapsed_seconds": round(elapsed, 6),
                    "error": cp.stderr.strip(),
                }
                continue
            try:
                result = parse_checker_json(cp.stdout)
            except Exception as e:
                infra_failures += 1
                query_failure = True
                item["cqpl"][qname] = {
                    "exit": cp.returncode,
                    "elapsed_seconds": round(elapsed, 6),
                    "error": str(e),
                    "stdout": cp.stdout,
                }
                continue
            (tdir / f"{qname}.json").write_text(cp.stdout, encoding="utf-8")
            item["cqpl"][qname] = {
                "exit": 0,
                "result": result,
                "elapsed_seconds": round(elapsed, 6),
            }

        item["legacy_reference"] = legacy.get(key)
        item["legacy_unmodeled_classes"] = sorted(
            set(item["legacy_reference"] or []) & UNMODELED_LEGACY_CLASSES
        )
        item["relations"] = {}
        for qname in QUERY_FILES:
            qr = item["cqpl"].get(qname, {})
            if "result" in qr:
                r = relation(item["legacy_reference"], qname, qr["result"])
                item["relations"][qname] = r
                if r.startswith("REVIEW:"):
                    review_flags += 1

        expected = reviewed.get(key, {})
        target_mismatches = []
        for qname in QUERY_FILES:
            if qname not in expected:
                continue
            observed = item["cqpl"].get(qname, {}).get("result")
            if observed != expected[qname]:
                target_mismatches.append({"query": qname, "expected": expected[qname], "observed": observed})
        item["reviewed_oracle"] = "pass" if not target_mismatches else "mismatch"
        if target_mismatches:
            oracle_mismatches += len(target_mismatches)
            item["oracle_mismatches"] = target_mismatches

        vals = [item["cqpl"].get(q, {}).get("result", "ERR") for q in QUERY_FILES]
        if query_failure:
            item["modeled_query_status"] = "incomplete"
        elif all(v == "ff" for v in vals):
            # This deliberately does NOT mean "program memory-safe".
            # It means only that every currently modeled CQPL error formula
            # (Leak/DF/UAF) is refuted on this abstraction.
            item["modeled_query_status"] = "all-modeled-queries-refuted"
        else:
            item["modeled_query_status"] = "at-least-one-modeled-query-nonrefuting"

        item["status"] = "query-failed" if query_failure else "complete"
        results[key] = item
        status_lines.append(f"{key}\t{rel}\t0\t0\tpass\t{vals[0]}\t{vals[1]}\t{vals[2]}\t{item['reviewed_oracle']}\n")
        unmodeled = item["legacy_unmodeled_classes"]
        suffix = f" unmodeled={unmodeled}" if unmodeled else ""
        print(
            f"  CQPL leak={vals[0]} df={vals[1]} uaf={vals[2]} "
            f"legacy={item['legacy_reference']} modeled={item['modeled_query_status']}{suffix}"
        )

    output = {
        "schema_version": 1,
        "scope": args.scope,
        "selected_targets": len(selected),
        "semantics": {"ff": "refuted", "unk": "not refuted / potential match", "tt": "established by three-valued composition"},
        "modeled_queries": list(QUERY_FILES),
        "unmodeled_legacy_classes": sorted(UNMODELED_LEGACY_CLASSES),
        "warning": (
            "Legacy CREMA classes are differential reference observations, not a proof oracle for CQPL. "
            "'all-modeled-queries-refuted' is not a proof of whole-program memory safety."
        ),
        "results": dict(sorted(results.items())),
    }
    (out / "results.cqpl.json").write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out / "status.tsv").write_text("".join(status_lines), encoding="utf-8")

    candidate = {
        "schema_version": 1,
        "status": "OBSERVED CANDIDATE ONLY - requires target-by-target review before promotion",
        "scope": args.scope,
        "targets": {
            key: {q: data.get("result") for q, data in item.get("cqpl", {}).items() if data.get("result") in {"ff", "unk", "tt"}}
            for key, item in sorted(results.items()) if item.get("status") == "complete"
        },
    }
    (out / "candidate_oracle.UNREVIEWED.json").write_text(json.dumps(candidate, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    # Hash all evidence after writing aggregate files, excluding SHA256SUMS itself.
    files = sorted(p for p in out.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
    (out / "SHA256SUMS").write_text("".join(f"{sha256(p)}  {p.relative_to(out).as_posix()}\n" for p in files), encoding="utf-8")

    print()
    print(f"CQPL target-repo run complete: selected={len(selected)} infra_failures={infra_failures} review_flags={review_flags} reviewed_oracle_mismatches={oracle_mismatches}")
    print(out)
    if infra_failures:
        return 4
    if args.enforce_reviewed_oracle and oracle_mismatches:
        return 5
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
