#!/usr/bin/env python3
"""CREMA/CQPL v6Q-r1c MIR-v2 allocator-contract corpus runner.

Runs the frozen 109-target schema-v2 corpus in either v1-baseline or v2-candidate
contract mode and records both CQPL truth values and per-(node,allocation) drop
contracts.  This permits a real contract differential instead of inferring
precision from query truth values alone.
"""
from __future__ import annotations

import argparse
import csv
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

from unknown_explanations import UnknownExplanationError, explain_unknown

QUERY_FILES = {
    1: {
        "leak": "leak.cqpl",
        "double_free": "double_free.cqpl",
        "use_after_free": "use_after_free.cqpl",
    },
    2: {
        "leak": "leak_alloc_state.cqpl",
        "double_free": "double_free_alloc_state.cqpl",
        "use_after_free": "use_after_free_alloc_state.cqpl",
        "allocator_mismatch": "allocator_mismatch_ub.cqpl",
    },
}
SOURCE_NAMES = {"Cargo.toml", "Cargo.lock", "build.rs"}
SOURCE_SUFFIXES = {".rs", ".c", ".h", ".cc", ".cpp", ".cxx", ".hpp"}


def run(cmd: list[str], *, cwd: Path | None = None, timeout: float | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)


def timeout_run(
    cmd: list[str],
    *,
    cwd: Path | None,
    log: Path,
    timeout: float,
    env: dict[str, str] | None = None,
) -> tuple[int, bool]:
    log.parent.mkdir(parents=True, exist_ok=True)
    try:
        with log.open("w", encoding="utf-8") as f:
            cp = subprocess.run(
                cmd,
                cwd=cwd,
                text=True,
                stdout=f,
                stderr=subprocess.STDOUT,
                timeout=None if timeout <= 0 else timeout,
                env=env,
            )
        return cp.returncode, False
    except subprocess.TimeoutExpired:
        with log.open("a", encoding="utf-8") as f:
            f.write(f"\nTIMEOUT after {timeout} seconds\n")
        return 124, True


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path: Path, default: Any) -> Any:
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else default


def load_target_config(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw or raw.lstrip().startswith("#"):
            continue
        parts = raw.split("\t", 2)
        if len(parts) != 3:
            raise ValueError(f"invalid target config row: {raw!r}")
        rel, cargo_target, _rationale = parts
        if rel in out:
            raise ValueError(f"duplicate target config row: {rel}")
        out[rel] = "" if cargo_target == "-" else cargo_target
    return out


def discover_minimal_cargo_roots(tests: Path) -> list[Path]:
    tests = tests.resolve()
    manifests: list[Path] = []
    for p in tests.rglob("Cargo.toml"):
        rel = p.relative_to(tests).parts
        if "target" in rel or ".git" in rel or any(x.startswith(".") for x in rel):
            continue
        manifests.append(p)
    manifest_dirs = {p.parent.resolve() for p in manifests}
    roots: list[Path] = []
    for manifest in manifests:
        d = manifest.parent.resolve()
        anc = d.parent
        nested = False
        while anc != tests and tests in anc.parents:
            if anc in manifest_dirs:
                nested = True
                break
            anc = anc.parent
        if not nested:
            roots.append(d)
    return sorted(set(roots), key=lambda p: p.relative_to(tests).as_posix())


def target_key(rel: str) -> str:
    p = Path(rel)
    if p.parent.name == "a-double_free_full_rust_literals":
        return p.name + "__df"
    if p.parent.name == "a-memory_leaks_full_rust_literals":
        return p.name + "__ml"
    if p.parent.name == "a-use_after_free_full_rust_literals":
        return p.name + "__uaf"
    return p.name


def corpus_source_files(target: Path) -> list[Path]:
    out: list[Path] = []
    for p in target.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(target).parts
        if "target" in rel or ".git" in rel or any(x.startswith(".") for x in rel):
            continue
        if p.name in SOURCE_NAMES or p.suffix.lower() in SOURCE_SUFFIXES:
            out.append(p)
    return sorted(out, key=lambda p: p.relative_to(target).as_posix())


def write_corpus_fingerprint(path: Path, selected: list[tuple[str, Path]]) -> None:
    rows: list[str] = []
    for rel, target in selected:
        for p in corpus_source_files(target):
            rows.append(f"{sha256(p)}  {rel}/{p.relative_to(target).as_posix()}\n")
    path.write_text("".join(rows), encoding="utf-8")


def parse_checker_json(stdout: str) -> str:
    obj = json.loads(stdout)
    result = obj.get("result")
    if result not in {"ff", "unk", "tt"}:
        raise ValueError(f"invalid CQPL result: {result!r}")
    return result


def classify_export_failure(text: str, rc: int) -> tuple[str, str]:
    if rc == 0:
        return "export-artifact-missing", "CREMA returned zero but required artifact is missing"
    for marker in ("UNRESOLVED_HIGHER_ORDER:", "UNRESOLVED_LOCAL_CALL:", "UNRESOLVED_GENERIC_ENTRY:"):
        if marker in text:
            line = next((x.strip() for x in text.splitlines() if marker in x), marker)
            return "semantic-refusal", line
    if "schema-v2 fail-closed:" in text:
        line = next((x.strip() for x in text.splitlines() if "schema-v2 fail-closed:" in x), "schema-v2 fail-closed")
        return "semantic-refusal", line
    return "export-failed", f"CREMA export exited {rc}"


def write_sha_manifest(out: Path) -> None:
    files = sorted(p for p in out.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
    (out / "SHA256SUMS").write_text(
        "".join(f"{sha256(p)}  {p.relative_to(out).as_posix()}\n" for p in files),
        encoding="utf-8",
    )



def extract_drop_contracts(path: Path) -> list[dict[str, Any]]:
    doc = json.loads(path.read_text(encoding="utf-8"))
    out: list[dict[str, Any]] = []
    for node in doc.get("nodes", []):
        node_id = node.get("id")
        for label in node.get("allocation_labels", []):
            if label.get("predicate") != "drop":
                continue
            contract = label.get("deallocator_contract") or {}
            out.append({
                "node": node_id,
                "allocation": label.get("allocation"),
                "certainty": label.get("certainty"),
                "family": contract.get("family"),
                "operation": contract.get("operation"),
                "language": contract.get("language"),
                "basis": contract.get("basis"),
                "owner_def_path": contract.get("owner_def_path"),
                "allocator_def_path": contract.get("allocator_def_path"),
                "callee_def_path": contract.get("callee_def_path"),
            })
    return sorted(out, key=lambda r: (str(r["node"]), str(r["allocation"])))

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--schema-version", type=int, choices=[1, 2], required=True)
    ap.add_argument("--contract-capability", choices=["v1", "v2"], required=True)
    ap.add_argument("--expected-targets", type=Path, required=True)
    ap.add_argument("--target-config", type=Path, required=True)
    ap.add_argument("--entry-overrides", type=Path, required=True)
    ap.add_argument("--toolchain", default=os.environ.get("CREMA_RUST_TOOLCHAIN", "nightly-2024-11-21"))
    ap.add_argument("--skip", action="append", default=[])
    ap.add_argument("--clean-targets", action="store_true")
    ap.add_argument("--build-timeout", type=float, default=600.0)
    ap.add_argument("--export-timeout", type=float, default=900.0)
    ap.add_argument("--query-timeout", type=float, default=180.0)
    ap.add_argument("--explain-unk-verbose", action="store_true", help="print the full human-readable explanation for every schema-v2 UNKNOWN query")
    ap.add_argument("--list-targets", action="store_true")
    ap.add_argument("--require-discovered", type=int, default=110)
    ap.add_argument("--require-active", type=int, default=109)
    args = ap.parse_args()

    root = args.root.resolve()
    out = args.out.resolve()
    tests = (root / "tests_and_target_repos").resolve()
    crema = (root / "crema").resolve()
    cqpl = Path(os.environ.get("CREMA_CQPL_DIR", str(root / "cqpl"))).resolve()
    checker_manifest = cqpl / "cqpl_checker" / "Cargo.toml"
    checker_bin = cqpl / "cqpl_checker" / "target" / "debug" / "cqpl_checker"
    query_dir = cqpl / ("queries" if args.schema_version == 1 else "queries_v2")
    query_files = dict(QUERY_FILES[args.schema_version])
    if args.schema_version == 2:
        query_files["allocator_mismatch"] = (
            "allocator_mismatch_ub_v2.cqpl" if args.contract_capability == "v2"
            else "allocator_mismatch_ub.cqpl"
        )

    required = [tests, crema / "Cargo.toml", checker_manifest, query_dir, args.expected_targets, args.target_config, args.entry_overrides]
    required += [query_dir / q for q in query_files.values()]
    missing = [str(p) for p in required if not p.exists()]
    if missing:
        print("ERROR missing required paths:\n  " + "\n  ".join(missing), file=sys.stderr)
        return 2

    expected = [x.strip() for x in args.expected_targets.read_text(encoding="utf-8").splitlines() if x.strip()]
    target_cfg = load_target_config(args.target_config)
    entry_overrides = load_json(args.entry_overrides, {})

    roots = discover_minimal_cargo_roots(tests)
    discovered = [(p.relative_to(tests).as_posix(), p) for p in roots]
    observed = [rel for rel, _ in discovered]
    if observed != expected:
        missing_expected = sorted(set(expected) - set(observed))
        extra = sorted(set(observed) - set(expected))
        print("ERROR corpus differs from frozen target snapshot", file=sys.stderr)
        for x in missing_expected:
            print(f"  MISSING {x}", file=sys.stderr)
        for x in extra:
            print(f"  EXTRA {x}", file=sys.stderr)
        return 3

    keys = [target_key(rel) for rel, _ in discovered]
    if len(keys) != len(set(keys)):
        dup = sorted(k for k in set(keys) if keys.count(k) > 1)
        print(f"ERROR duplicate target keys: {dup}", file=sys.stderr)
        return 3

    skip = set(args.skip)
    skipped = [(rel, p) for rel, p in discovered if rel in skip or target_key(rel) in skip]
    selected = [(rel, p) for rel, p in discovered if not (rel in skip or target_key(rel) in skip)]
    found_skip_tokens = {rel for rel, _ in skipped} | {target_key(rel) for rel, _ in skipped}
    unresolved_skip = sorted(skip - found_skip_tokens)
    if unresolved_skip:
        print(f"ERROR skip target(s) not found: {unresolved_skip}", file=sys.stderr)
        return 3

    if len(discovered) != args.require_discovered or len(selected) != args.require_active:
        print(
            f"ERROR census mismatch: discovered={len(discovered)} active={len(selected)} "
            f"required={args.require_discovered}/{args.require_active}",
            file=sys.stderr,
        )
        return 3

    if args.list_targets:
        skipped_rel = {rel for rel, _ in skipped}
        for rel, _ in discovered:
            print(f"{'SKIPPED' if rel in skipped_rel else 'ACTIVE'}\t{target_key(rel)}\t{rel}")
        print(f"TOTAL_DISCOVERED\t{len(discovered)}")
        print(f"TOTAL_ACTIVE\t{len(selected)}")
        print(f"TOTAL_SKIPPED\t{len(skipped)}")
        return 0

    out.mkdir(parents=True, exist_ok=True)
    raw = out / "raw"
    raw.mkdir(parents=True, exist_ok=True)

    preflight = run(["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(checker_manifest)])
    (out / "cqpl-checker-build.log").write_text(preflight.stdout + preflight.stderr, encoding="utf-8")
    if preflight.returncode != 0 or not checker_bin.is_file():
        print("ERROR CQPL checker build failed", file=sys.stderr)
        return 2

    environment: list[str] = [
        f"timestamp_utc={dt.datetime.now(dt.timezone.utc).isoformat()}",
        f"root={root}",
        f"schema_version={args.schema_version}",
        f"contract_capability={args.contract_capability}",
        f"toolchain={args.toolchain}",
        f"discovered_targets={len(discovered)}",
        f"active_targets={len(selected)}",
        f"skipped_targets={len(skipped)}",
        "legacy_detector_comparison=disabled",
        "historical_result_comparison=disabled",
    ]
    for cmd in (["rustc", f"+{args.toolchain}", "-vV"], ["cargo", f"+{args.toolchain}", "--version"], ["clang", "--version"]):
        cp = run(list(cmd))
        environment.append("$ " + " ".join(cmd))
        environment.extend((cp.stdout + cp.stderr).splitlines())
    (out / "environment.txt").write_text("\n".join(environment) + "\n", encoding="utf-8")
    (out / "targets.discovered.txt").write_text("\n".join(observed) + "\n", encoding="utf-8")
    (out / "targets.active.txt").write_text("\n".join(rel for rel, _ in selected) + "\n", encoding="utf-8")
    (out / "targets.skipped.txt").write_text("\n".join(rel for rel, _ in skipped) + "\n", encoding="utf-8")
    write_corpus_fingerprint(out / "corpus-source-SHA256SUMS", selected)

    results: dict[str, Any] = {}
    failures = 0
    unknown_explanations: list[dict[str, Any]] = []
    explanation_failures: list[dict[str, str]] = []
    status_cols = ["key", "relative_path", "cargo_target", "build", "export", "schema"] + list(query_files)
    status_rows = ["\t".join(status_cols) + "\n"]

    svf_output = root / "SVF-example" / "output"
    svf_output.mkdir(parents=True, exist_ok=True)

    for idx, (rel, target) in enumerate(selected, 1):
        key = target_key(rel)
        print(f"[{idx}/{len(selected)}] {key} <- {rel}", flush=True)
        tdir = raw / re.sub(r"[^A-Za-z0-9_.-]+", "_", key)
        tdir.mkdir(parents=True, exist_ok=True)
        cargo_target = target_cfg.get(rel, "")
        entry = entry_overrides.get(rel)
        item: dict[str, Any] = {
            "key": key,
            "relative_path": rel,
            "cargo_target": cargo_target or None,
            "entry": entry,
            "queries": {},
        }

        target_env = os.environ.copy()
        target_env["CARGO_TARGET_DIR"] = str(target / "target")

        if args.clean_targets:
            rc, timed = timeout_run(
                ["cargo", f"+{args.toolchain}", "clean", "--manifest-path", str(target / "Cargo.toml")],
                cwd=target,
                log=tdir / "cargo-clean.log",
                timeout=args.build_timeout,
                env=target_env,
            )
            if rc != 0:
                failures += 1
                item.update(status="clean-failed", clean_exit=rc, clean_timeout=timed)
                results[key] = item
                status_rows.append("\t".join([key, rel, cargo_target or "-", f"clean:{rc}", "-", "-"] + ["-"] * len(query_files)) + "\n")
                continue

        build_cmd = ["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(target / "Cargo.toml")]
        if (target / "Cargo.lock").is_file():
            build_cmd.append("--locked")
        rc, timed = timeout_run(
            build_cmd,
            cwd=target,
            log=tdir / "build.log",
            timeout=args.build_timeout,
            env=target_env,
        )
        item["build_exit"] = rc
        item["build_timeout"] = timed
        if rc != 0:
            failures += 1
            item["status"] = "build-failed"
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", str(rc), "-", "-"] + ["-"] * len(query_files)) + "\n")
            print(f"  BUILD_FAIL rc={rc}", flush=True)
            continue

        sentinel = svf_output / f"ZZ_CQPL_RUN_ALL_STALE_{os.getpid()}_{idx}_A_FINAL_ICFG.json"
        sentinel.write_text("{invalid-stale-global-svf\n", encoding="utf-8")
        (crema / "ffi_functions.json").write_text('{"ffi_functions":["__CQPL_RUN_ALL_STALE__"]}\n', encoding="utf-8")

        annotated = tdir / f"annotated_icfg_v{args.schema_version}.json"
        identity = tdir / "allocation_identity.json"
        cmd = ["cargo", f"+{args.toolchain}", "run", "--manifest-path", str(crema / "Cargo.toml"), "--", str(target)]
        if entry:
            cmd += ["--entry", entry]
        if cargo_target:
            cmd += ["--cargo-target", cargo_target]
        cmd += [
            "--only-icfg-annotated",
            "--cqpl-schema-version", str(args.schema_version),
            "--annotated-icfg-out", str(annotated),
        ]
        if args.schema_version == 2:
            cmd += [
                "--allocation-identity-out", str(identity),
                "--mir-semantics-v2",
            ]

        rc, timed = timeout_run(cmd, cwd=crema, log=tdir / "crema-export.log", timeout=args.export_timeout)
        sentinel.unlink(missing_ok=True)
        item["export_exit"] = rc
        item["export_timeout"] = timed
        export_text = (tdir / "crema-export.log").read_text(encoding="utf-8", errors="replace")
        artifact_ok = annotated.is_file() and (args.schema_version == 1 or identity.is_file())
        if rc != 0 or not artifact_ok:
            failures += 1
            klass, reason = classify_export_failure(export_text, rc)
            item["status"] = klass
            item["error"] = reason
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", "0", str(rc), klass] + ["-"] * len(query_files)) + "\n")
            print(f"  EXPORT_FAIL class={klass} rc={rc}: {reason}", flush=True)
            continue

        if sentinel.name in export_text:
            failures += 1
            item["status"] = "isolation-failed"
            item["error"] = "stale SVF sentinel was consumed"
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", "0", "0", "isolation-failed"] + ["-"] * len(query_files)) + "\n")
            continue

        ffi_summary = crema / "ffi_functions.json"
        if not ffi_summary.is_file():
            failures += 1
            item["status"] = "ffi-summary-missing"
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", "0", "0", "ffi-summary-missing"] + ["-"] * len(query_files)) + "\n")
            continue
        shutil.copy2(ffi_summary, tdir / "ffi_functions.json")
        try:
            ffi_doc = json.loads(ffi_summary.read_text(encoding="utf-8"))
            ffi_names = ffi_doc.get("ffi_functions", [])
            if "__CQPL_RUN_ALL_STALE__" in ffi_names:
                raise ValueError("stale FFI sentinel survived extraction")
            item["ffi_functions"] = ffi_names
        except Exception as e:
            failures += 1
            item["status"] = "ffi-summary-invalid"
            item["error"] = str(e)
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", "0", "0", "ffi-summary-invalid"] + ["-"] * len(query_files)) + "\n")
            continue

        try:
            item["drop_contracts"] = extract_drop_contracts(annotated)
        except Exception as e:
            failures += 1
            item["status"] = "contract-extraction-failed"
            item["error"] = str(e)
            results[key] = item
            status_rows.append("\t".join([key, rel, cargo_target or "-", "0", "0", "contract-extraction-failed"] + ["-"] * len(query_files)) + "\n")
            continue

        query_failed = False
        vals: list[str] = []
        for qname, qfile in query_files.items():
            qpath = query_dir / qfile
            started = time.perf_counter()
            try:
                cp = run(
                    [str(checker_bin), str(annotated), str(qpath), "--json"],
                    timeout=None if args.query_timeout <= 0 else args.query_timeout,
                )
            except subprocess.TimeoutExpired:
                elapsed = time.perf_counter() - started
                query_failed = True
                item["queries"][qname] = {"timeout": True, "elapsed_seconds": round(elapsed, 6)}
                (tdir / f"{qname}.stderr.log").write_text(f"TIMEOUT after {args.query_timeout}s\n", encoding="utf-8")
                vals.append("ERR")
                continue
            elapsed = time.perf_counter() - started
            (tdir / f"{qname}.stderr.log").write_text(cp.stderr, encoding="utf-8")
            if cp.returncode != 0:
                query_failed = True
                item["queries"][qname] = {"exit": cp.returncode, "elapsed_seconds": round(elapsed, 6), "error": cp.stderr.strip()}
                vals.append("ERR")
                continue
            try:
                result = parse_checker_json(cp.stdout)
            except Exception as e:
                query_failed = True
                item["queries"][qname] = {"exit": 0, "elapsed_seconds": round(elapsed, 6), "error": str(e), "stdout": cp.stdout}
                vals.append("ERR")
                continue
            (tdir / f"{qname}.json").write_text(cp.stdout, encoding="utf-8")
            query_record = {"exit": 0, "result": result, "elapsed_seconds": round(elapsed, 6)}
            if args.schema_version == 2 and result == "unk":
                explain_path = tdir / f"{qpath.stem}.explain.json"
                try:
                    explanation = explain_unknown(
                        checker=checker_bin,
                        artifact=annotated,
                        query=qpath,
                        result=result,
                        explanation=explain_path,
                        timeout=None if args.query_timeout <= 0 else args.query_timeout,
                        verbose=args.explain_unk_verbose,
                    )
                except UnknownExplanationError as e:
                    query_failed = True
                    query_record["explanation_error"] = str(e)
                    explanation_failures.append({
                        "target": key, "relative_path": rel, "query": qpath.stem, "error": str(e)
                    })
                    print(f"  {qname}=unk EXPLANATION_FAIL: {e}", flush=True)
                else:
                    assert explanation is not None
                    query_record["explanation"] = explain_path.name
                    query_record["reason_frontier"] = explanation["reason_frontier"]
                    query_record["supporting_findings"] = explanation["supporting_findings"]
                    query_record["supporting_finding_kinds"] = explanation["supporting_finding_kinds"]
                    query_record["supporting_finding_strengths"] = explanation["supporting_finding_strengths"]
                    explanation.update({
                        "target": key,
                        "relative_path": rel,
                        "artifact": str(annotated),
                        "query_slot": qname,
                        "explanation": explain_path.relative_to(out).as_posix(),
                    })
                    unknown_explanations.append(explanation)
                    print(
                        f"  {qpath.stem}=unk explanation={explain_path.name} "
                        f"reasons={';'.join(explanation['reason_frontier'])} "
                        f"supporting_findings={explanation['supporting_findings']}",
                        flush=True,
                    )
            item["queries"][qname] = query_record
            vals.append(result)

        if query_failed:
            failures += 1
            item["status"] = "query-failed"
        else:
            item["status"] = "complete"
        results[key] = item
        status_rows.append("\t".join([key, rel, cargo_target or "-", "0", "0", "pass"] + vals) + "\n")
        print("  " + " ".join(f"{q}={v}" for q, v in zip(query_files, vals)), flush=True)

    unknown_expected = 0
    if args.schema_version == 2:
        unknown_expected = sum(
            1
            for item in results.values()
            for query in item.get("queries", {}).values()
            if query.get("result") == "unk"
        )
    unknown_complete = (
        len(unknown_explanations) == unknown_expected and not explanation_failures
    )
    with (out / "unknown-explanations.tsv").open("w", encoding="utf-8", newline="") as f:
        fields = [
            "target", "relative_path", "artifact", "query_slot", "query", "result",
            "explanation", "reason_frontier", "witnesses", "supporting_findings",
            "supporting_finding_kinds", "supporting_finding_strengths",
        ]
        w = csv.DictWriter(f, delimiter="\t", fieldnames=fields)
        w.writeheader()
        for record in unknown_explanations:
            row = dict(record)
            for key_ in ["reason_frontier", "supporting_finding_kinds", "supporting_finding_strengths"]:
                row[key_] = ";".join(row[key_])
            w.writerow({key_: row.get(key_, "") for key_ in fields})
    unknown_summary = {
        "schema": "cqpl_unknown_explanations_v1",
        "policy": "every_v2_unk_requires_valid_specific_explanation",
        "schema_version": args.schema_version,
        "unknown_results": unknown_expected,
        "explanations_generated": len(unknown_explanations),
        "complete": unknown_complete,
        "failures": explanation_failures,
        "reports": unknown_explanations,
    }
    (out / "unknown-explanations-summary.json").write_text(
        json.dumps(unknown_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    complete = sum(1 for x in results.values() if x.get("status") == "complete")
    aggregate = {
        "protocol": f"CREMA-CQPL-v6Q-r1c-mirv2-contract-corpus-schema-v{args.schema_version}-{args.contract_capability}",
        "schema_version": args.schema_version,
        "contract_capability": args.contract_capability,
        "discovered_targets": len(discovered),
        "active_targets": len(selected),
        "skipped_targets": [rel for rel, _ in skipped],
        "modeled_complete": complete,
        "failures": failures,
        "queries": list(query_files),
        "semantics": {"ff": "refuted", "unk": "not-refuted/potential", "tt": "established in abstract model"},
        "legacy_detector_comparison": False,
        "historical_result_comparison": False,
        "results": dict(sorted(results.items())),
    }
    (out / "results.json").write_text(json.dumps(aggregate, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out / "status.tsv").write_text("".join(status_rows), encoding="utf-8")
    write_sha_manifest(out)

    print()
    print(
        f"CORPUS SUMMARY schema=v{args.schema_version} discovered={len(discovered)} "
        f"active={len(selected)} complete={complete} failures={failures} skipped={len(skipped)}"
    )
    print(out)
    if not unknown_complete:
        return 5
    if failures or complete != len(selected):
        return 4
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
