#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

from unknown_explanations import UnknownExplanationError, explain_unknown

EXPECTED_RUSTC = "rustc 1.84.0-nightly (3fee0f12e 2024-11-20)"
REQUIRED_CAPS = {
    "allocation_state_v1",
    "allocation_contracts_v1",
    "allocation_contracts_v2",
    "mir_semantic_labels_v1",
    "mir_semantics_v2",
}


def run(cmd: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None,
        stdout=None, stderr=None) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, env=env, text=True, stdout=stdout, stderr=stderr)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def load_target_config(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw or raw.lstrip().startswith("#"):
            continue
        parts = raw.split("\t", 2)
        if len(parts) != 3:
            raise ValueError(f"invalid target config row: {raw!r}")
        rel, target, _why = parts
        out[rel] = "" if target == "-" else target
    return out


def safe_name(rel: str) -> str:
    name = Path(rel).name
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", name)


def main() -> int:
    ap = argparse.ArgumentParser(description="Run CREMA v6Q-r1b MIR-v2 + all CQPL v6Q-r1c queries on one tests_and_target_repos Cargo root")
    ap.add_argument("--root", type=Path, required=True)
    ap.add_argument("--relative-path", required=True, help="path relative to tests_and_target_repos")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--toolchain", default=os.environ.get("CREMA_RUST_TOOLCHAIN", "nightly-2024-11-21"))
    ap.add_argument(
        "--explain-unk-verbose", action="store_true",
        help="print the full human-readable explanation for every UNKNOWN query (sidecars remain mandatory regardless)",
    )
    args = ap.parse_args()

    root = args.root.resolve()
    cqpl = Path(os.environ.get("CREMA_CQPL_DIR", str(Path(__file__).resolve().parents[1]))).resolve()
    crema = (root / "crema").resolve()
    tests = (root / "tests_and_target_repos").resolve()
    target = (tests / args.relative_path).resolve()

    if tests not in target.parents:
        raise SystemExit("relative path escapes tests_and_target_repos")
    if not (target / "Cargo.toml").is_file():
        raise SystemExit(f"missing Cargo.toml: {target}")
    if not (crema / "Cargo.toml").is_file():
        raise SystemExit(f"missing CREMA manifest: {crema / 'Cargo.toml'}")

    rustc = subprocess.check_output(["rustup", "run", args.toolchain, "rustc", "--version"], text=True).strip()
    if rustc != EXPECTED_RUSTC:
        raise SystemExit(f"unexpected pinned rustc: {rustc}")

    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out = (args.out or (root / "repro-results" / f"cqpl-v6q-r1c-one-{safe_name(args.relative_path)}-{stamp}")).resolve()
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    (out / "queries").mkdir()

    target_cfg = load_target_config(cqpl / "artifact" / "TARGET_ANALYSIS_CONFIG.tsv")
    entry_overrides = json.loads((cqpl / "regression" / "reference" / "entry_overrides.json").read_text())
    cargo_target = target_cfg.get(args.relative_path, "")
    entry = entry_overrides.get(args.relative_path)

    checker_manifest = cqpl / "cqpl_checker" / "Cargo.toml"
    checker = cqpl / "cqpl_checker" / "target" / "debug" / "cqpl_checker"

    # Build checker first.
    with (out / "checker-build.log").open("w", encoding="utf-8") as log:
        cp = run(["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(checker_manifest)], cwd=root, stdout=log, stderr=subprocess.STDOUT)
    if cp.returncode != 0 or not checker.is_file():
        raise SystemExit("checker build failed; see checker-build.log")

    # Build the subject exactly as the corpus runner does.
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target / "target")
    build_cmd = ["cargo", f"+{args.toolchain}", "build", "--manifest-path", str(target / "Cargo.toml")]
    if (target / "Cargo.lock").is_file():
        build_cmd.append("--locked")
    with (out / "build.log").open("w", encoding="utf-8") as log:
        cp = run(build_cmd, cwd=target, env=env, stdout=log, stderr=subprocess.STDOUT)
    if cp.returncode != 0:
        raise SystemExit("target build failed; see build.log")

    # Isolation sentinels mirror the frozen corpus protocol.
    svf_output = root / "SVF-example" / "output"
    svf_output.mkdir(parents=True, exist_ok=True)
    sentinel = svf_output / f"ZZ_CQPL_ONE_STALE_{os.getpid()}_A_FINAL_ICFG.json"
    sentinel.write_text("{invalid-stale-global-svf\n", encoding="utf-8")
    (crema / "ffi_functions.json").write_text('{"ffi_functions":["__CQPL_ONE_STALE__"]}\n', encoding="utf-8")

    annotated = out / "annotated_icfg_v2.json"
    identity = out / "allocation_identity.json"
    cmd = [
        "cargo", f"+{args.toolchain}", "run", "--manifest-path", str(crema / "Cargo.toml"), "--", str(target)
    ]
    if entry:
        cmd += ["--entry", entry]
    if cargo_target:
        cmd += ["--cargo-target", cargo_target]
    cmd += [
        "--only-icfg-annotated",
        "--cqpl-schema-version", "2",
        "--annotated-icfg-out", str(annotated),
        "--allocation-identity-out", str(identity),
        "--mir-semantics-v2",
    ]
    (out / "crema-command.txt").write_text(" ".join(json.dumps(x) for x in cmd) + "\n", encoding="utf-8")
    with (out / "crema-export.log").open("w", encoding="utf-8") as log:
        cp = run(cmd, cwd=crema, stdout=log, stderr=subprocess.STDOUT)
    sentinel.unlink(missing_ok=True)
    if cp.returncode != 0:
        raise SystemExit(f"CREMA export failed rc={cp.returncode}; see crema-export.log")
    if not annotated.is_file() or not identity.is_file():
        raise SystemExit("CREMA returned zero but required artifact is missing")

    export_text = (out / "crema-export.log").read_text(errors="replace")
    if "mir_semantics_profile=v2-extension-over-v6O" not in export_text:
        raise SystemExit("MIR-v2 profile marker missing")
    if sentinel.name in export_text:
        raise SystemExit("stale SVF sentinel was consumed")

    ffi_summary = crema / "ffi_functions.json"
    if not ffi_summary.is_file():
        raise SystemExit("ffi_functions.json missing")
    shutil.copy2(ffi_summary, out / "ffi_functions.json")
    ffi_doc = json.loads(ffi_summary.read_text())
    if "__CQPL_ONE_STALE__" in ffi_doc.get("ffi_functions", []):
        raise SystemExit("stale FFI sentinel survived extraction")

    graph = json.loads(annotated.read_text())
    if graph.get("schema_version") != 2:
        raise SystemExit(f"unexpected schema_version={graph.get('schema_version')}")
    caps = set(graph.get("capabilities", []))
    missing = REQUIRED_CAPS - caps
    if missing:
        raise SystemExit(f"artifact missing required capabilities: {sorted(missing)}")

    queries = sorted((cqpl / "queries_v2").glob("*.cqpl"))
    if len(queries) != 12:
        raise SystemExit(f"expected 12 queries_v2, found {len(queries)}")

    rows = []
    counts = {"ff": 0, "unk": 0, "tt": 0}
    unknown_explanations = []
    for query in queries:
        stdout_path = out / "queries" / f"{query.stem}.json"
        stderr_path = out / "queries" / f"{query.stem}.stderr.log"
        with stdout_path.open("w", encoding="utf-8") as so, stderr_path.open("w", encoding="utf-8") as se:
            qcp = run([str(checker), str(annotated), str(query), "--json"], cwd=cqpl / "cqpl_checker", stdout=so, stderr=se)
        if qcp.returncode != 0:
            raise SystemExit(f"query {query.name} failed rc={qcp.returncode}; see {stderr_path}")
        doc = json.loads(stdout_path.read_text())
        result = doc.get("result")
        if result not in counts:
            raise SystemExit(f"query {query.name} returned invalid result {result!r}")
        counts[result] += 1
        rows.append((query.stem, qcp.returncode, result))
        if result == "unk":
            explain_path = out / "queries" / f"{query.stem}.explain.json"
            try:
                record = explain_unknown(
                    checker=checker,
                    artifact=annotated,
                    query=query,
                    result=result,
                    explanation=explain_path,
                    cwd=cqpl / "cqpl_checker",
                    verbose=args.explain_unk_verbose,
                )
            except UnknownExplanationError as e:
                raise SystemExit(f"query {query.name} returned unk but explanation generation failed: {e}") from e
            assert record is not None
            record.update({
                "target": args.relative_path,
                "artifact": str(annotated),
                "explanation": explain_path.relative_to(out).as_posix(),
            })
            unknown_explanations.append(record)
            print(
                f"{query.stem}=unk "
                f"explanation={record['explanation']} "
                f"reasons={';'.join(record['reason_frontier'])} "
                f"supporting_findings={record['supporting_findings']}"
            )
        else:
            print(f"{query.stem}={result}")

    if len(unknown_explanations) != counts["unk"]:
        raise SystemExit(
            f"UNK explanation closure failed: unk={counts['unk']} explanations={len(unknown_explanations)}"
        )

    with (out / "query-results.tsv").open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["query", "rc", "result"])
        w.writerows(rows)

    with (out / "unknown-explanations.tsv").open("w", encoding="utf-8", newline="") as f:
        fields = [
            "target", "artifact", "query", "result", "explanation",
            "reason_frontier", "witnesses", "supporting_findings",
            "supporting_finding_kinds", "supporting_finding_strengths",
        ]
        w = csv.DictWriter(f, delimiter="\t", fieldnames=fields)
        w.writeheader()
        for record in unknown_explanations:
            row = dict(record)
            for key in ["reason_frontier", "supporting_finding_kinds", "supporting_finding_strengths"]:
                row[key] = ";".join(row[key])
            w.writerow({key: row.get(key, "") for key in fields})

    unknown_summary = {
        "schema": "cqpl_unknown_explanations_v1",
        "policy": "every_v2_unk_requires_valid_specific_explanation",
        "target": args.relative_path,
        "unknown_results": counts["unk"],
        "explanations_generated": len(unknown_explanations),
        "complete": len(unknown_explanations) == counts["unk"],
        "reports": unknown_explanations,
    }
    (out / "unknown-explanations-summary.json").write_text(
        json.dumps(unknown_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    summary = {
        "profile": "CREMA-CQPL-v6Q-r1c-one-target",
        "relative_path": args.relative_path,
        "cargo_target": cargo_target or None,
        "entry_override": entry,
        "toolchain": args.toolchain,
        "rustc": rustc,
        "schema_version": 2,
        "capabilities": sorted(caps),
        "nodes": len(graph.get("nodes", [])),
        "variables": len(graph.get("variables", [])),
        "allocations": len(graph.get("allocations", [])),
        "queries": len(rows),
        "result_counts": counts,
    }
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")

    files = sorted(p for p in out.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
    with (out / "SHA256SUMS").open("w", encoding="utf-8") as f:
        for p in files:
            f.write(f"{sha256(p)}  {p.relative_to(out).as_posix()}\n")

    print(json.dumps(summary, indent=2))
    print(f"CQPL_ONE_TARGET v6Q-r1c: PASS queries=12 out={out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
