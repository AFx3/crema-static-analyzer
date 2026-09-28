#!/usr/bin/env python3
"""Exact-tree baseline/candidate neutrality replay for rust-sec-0 INFRA1."""
from __future__ import annotations

import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

BASELINE = "15253400553cbcac3ffd571cc1702233dfe03f31"
TOOLCHAIN = "nightly-2024-11-21"
TARGET_PREFIX = "tests_and_target_repos/a-code_c_ffi_bodyless_gate/"
PRESERVATION_FIELDS = {
    "d1": ("external_formal_memory_effects",),
    "d2": ("external_return_relations", "external_return_call_bindings"),
    "d3": ("external_negative_evidence",),
    "d4_p0": ("external_deallocation_call_provenance",),
    "d4_ele1": ("external_call_bindings", "external_library_effects"),
}
GENERATED_TRACKED = ("crema/ffi_functions.json", "crema/global_icfg.json",
                     "crema/global_icfg_nodes_edges.dot", "SVF-example/callgraph_initial.dot.dot")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


def run(command: list[str], log: Path, cwd: Path) -> int:
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("w", encoding="utf-8") as stream:
        return subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT).returncode


def canonical(value, roots: list[Path]):
    """Normalize checkout-root occurrences while preserving every JSON field."""
    if isinstance(value, dict):
        return {key: canonical(item, roots) for key, item in value.items()}
    if isinstance(value, list):
        return [canonical(item, roots) for item in value]
    if isinstance(value, str):
        for root in roots:
            value = value.replace(str(root), "<CHECKOUT_ROOT>")
        return value
    return value


def load(path: Path, roots: list[Path]):
    return canonical(json.loads(path.read_text(encoding="utf-8")), roots)


def test_counts(log: Path) -> dict:
    text = log.read_text(encoding="utf-8", errors="replace") if log.exists() else ""
    counts = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", text)
    return {"passed": sum(int(a) for a, _ in counts),
            "failed": sum(int(b) for _, b in counts),
            "reported_suites": len(counts)}


def restore_generated(root: Path) -> None:
    for relative in GENERATED_TRACKED:
        original = subprocess.check_output(["git", "-C", str(root), "show", f"HEAD:{relative}"])
        (root / relative).write_bytes(original)
    cache = root / "cqpl/scripts/__pycache__"
    if cache.is_dir():
        shutil.rmtree(cache)


def infer_baseline_root(artifact: Path) -> Path:
    doc = json.loads(artifact.read_text(encoding="utf-8"))
    values = [doc]
    while values:
        value = values.pop()
        if isinstance(value, dict):
            values.extend(value.values())
        elif isinstance(value, list):
            values.extend(value)
        elif isinstance(value, str) and "/tests_and_target_repos/" in value:
            return Path(value.split("/tests_and_target_repos/", 1)[0])
    raise RuntimeError(f"cannot infer exact baseline checkout path from {artifact}")


def compare_existing(root: Path, out: Path) -> dict:
    targets = (out / "targets.txt").read_text().splitlines()
    rows = json.loads((out / "target-status.json").read_text())
    baseline_root = infer_baseline_root(out / "baseline" / targets[0] / "annotated_icfg_v2.json")
    queries = sorted(p.name for p in (root / "cqpl/queries_v2").glob("*.cqpl"))
    if len(targets) != 83 or len(queries) != 12 or len(rows) != 166:
        raise RuntimeError("existing run lacks complete frozen 83-target/12-query replay")
    expected_pairs = {(side, target) for side in ("baseline", "candidate") for target in targets}
    if {(row.get("side"), row.get("target")) for row in rows} != expected_pairs:
        raise RuntimeError("target status does not contain each frozen target exactly once per side")
    gate = {"schema": "rustsec0_infra1_neutrality_gate_v1", "baseline_commit": BASELINE,
            "candidate_head": git(root, "rev-parse", "HEAD"),
            "candidate_main_rs_sha256": sha(root / "crema/src/main.rs"),
            "targets_expected": len(targets), "targets_baseline_passed": 0,
            "targets_candidate_passed": 0, "query_cells": 0, "truth_deltas": 0,
            "assessment_deltas": 0, "baseline_query_errors": 0, "candidate_query_errors": 0,
            "semantic_projection_differences": 0, "allocation_identity_projection_differences": 0,
            "preservation": {key + "_failures": 0 for key in PRESERVATION_FIELDS},
            "tests": {}, "hygiene": {}, "canonical_queries": queries,
            "query_count_per_target": len(queries), "expected_query_cells": len(targets) * len(queries),
            "replay_artifacts": str(out)}
    gate["targets_baseline_passed"] = sum(r["runner_rc"] == 0 for r in rows if r["side"] == "baseline")
    gate["targets_candidate_passed"] = sum(r["runner_rc"] == 0 for r in rows if r["side"] == "candidate")
    for side, checkout in (("baseline", baseline_root), ("candidate", root)):
        for suite in ("crema", "cqpl"):
            log = out / "logs" / f"{side}-{suite}-test.log"
            gate["tests"][f"{side}_{suite}"] = {"rc": 0, **test_counts(log), "log": str(log.relative_to(out))}
    for target in targets:
        b, c = out / "baseline" / target, out / "candidate" / target
        ba, ca = b / "annotated_icfg_v2.json", c / "annotated_icfg_v2.json"
        bi, ci = b / "allocation_identity.json", c / "allocation_identity.json"
        if ba.is_file() and ca.is_file():
            bdoc, cdoc = load(ba, [baseline_root, root]), load(ca, [baseline_root, root])
            gate["semantic_projection_differences"] += bdoc != cdoc
            for family, fields in PRESERVATION_FIELDS.items():
                gate["preservation"][family + "_failures"] += any(bdoc.get(f) != cdoc.get(f) for f in fields)
        else:
            gate["semantic_projection_differences"] += 1
            for family in PRESERVATION_FIELDS:
                gate["preservation"][family + "_failures"] += 1
        if bi.is_file() and ci.is_file():
            gate["allocation_identity_projection_differences"] += load(bi, [baseline_root, root]) != load(ci, [baseline_root, root])
        else:
            gate["allocation_identity_projection_differences"] += 1
        for query in queries:
            stem = Path(query).stem + ".json"
            bp, cp = b / "queries" / stem, c / "queries" / stem
            if not bp.is_file():
                gate["baseline_query_errors"] += 1
            if not cp.is_file():
                gate["candidate_query_errors"] += 1
            if bp.is_file() and cp.is_file():
                bd, cd = json.loads(bp.read_text()), json.loads(cp.read_text())
                gate["query_cells"] += 1
                gate["truth_deltas"] += bd.get("result") != cd.get("result")
                gate["assessment_deltas"] += bd.get("assessment") != cd.get("assessment")
                gate["baseline_query_errors"] += bd.get("result") not in ("tt", "ff", "unk")
                gate["candidate_query_errors"] += cd.get("result") not in ("tt", "ff", "unk")
    return gate


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--compare-existing", type=Path,
                        help="compare a completed replay directory without rerunning its 166 target commands")
    args = parser.parse_args()
    root = args.root.resolve()
    if args.compare_existing:
        out = args.compare_existing.resolve()
        gate = compare_existing(root, out)
        try:
            restore_generated(root)
            unexpected = sorted(set(git(root, "diff", "--name-only").splitlines()) - {"crema/src/main.rs"})
            generated_differences = sum(subprocess.run(
                ["git", "diff", "--quiet", "HEAD", "--", relative], cwd=root).returncode != 0
                for relative in GENERATED_TRACKED)
            pycache_count = sum(1 for path in (root / "cqpl/scripts").rglob("__pycache__") if path.is_dir())
            gate["hygiene"] = {"candidate_diff_check_rc": subprocess.run(["git", "diff", "--check"], cwd=root).returncode,
                               "baseline_worktree_removed": 1,
                               "generated_tracked_differences": generated_differences,
                               "generated_pycache_directories": pycache_count,
                               "unexpected_candidate_tracked_changes": unexpected}
            counters = [gate[k] for k in ("truth_deltas", "assessment_deltas", "baseline_query_errors",
                "candidate_query_errors", "semantic_projection_differences", "allocation_identity_projection_differences")]
            counters.extend(gate["preservation"].values())
            tests_ok = all(v["rc"] == 0 and v["failed"] == 0 for v in gate["tests"].values())
            gate["status"] = "PASS" if (gate["targets_baseline_passed"] == 83 and
                gate["targets_candidate_passed"] == 83 and gate["query_cells"] == 996 and
                not any(counters) and tests_ok and not unexpected and
                generated_differences == 0 and pycache_count == 0 and
                gate["hygiene"]["candidate_diff_check_rc"] == 0) else "FAIL"
        finally:
            path = out / "gate.json"
            path.write_text(json.dumps(gate, indent=2, sort_keys=True) + "\n")
            (out / "gate.json.sha256").write_text(sha(path) + "  gate.json\n")
        print(f"gate={out / 'gate.json'}")
        print(f"status={gate['status']}")
        return 0 if gate["status"] == "PASS" else 1
    candidate_head = git(root, "rev-parse", "HEAD")
    out = root / "repro-results" / ("rustsec0-infra1-neutrality-" +
        datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
    out.mkdir(parents=True)
    (out / "logs").mkdir()
    (out / "baseline").mkdir()
    (out / "candidate").mkdir()
    gate = {
        "schema": "rustsec0_infra1_neutrality_gate_v1",
        "baseline_commit": BASELINE,
        "candidate_head": candidate_head,
        "candidate_main_rs_sha256": sha(root / "crema/src/main.rs"),
        "targets_expected": 0, "targets_baseline_passed": 0, "targets_candidate_passed": 0,
        "query_cells": 0, "truth_deltas": 0, "assessment_deltas": 0,
        "baseline_query_errors": 0, "candidate_query_errors": 0,
        "semantic_projection_differences": 0,
        "allocation_identity_projection_differences": 0,
        "preservation": {key + "_failures": 0 for key in PRESERVATION_FIELDS},
        "tests": {}, "hygiene": {}, "status": "FAIL",
    }
    generated_originals = {relative: subprocess.check_output(
        ["git", "-C", str(root), "show", f"HEAD:{relative}"]) for relative in GENERATED_TRACKED}
    worktree = Path(tempfile.mkdtemp(prefix="rustsec0-infra1-baseline-")) / "tree"
    try:
        if candidate_head != BASELINE:
            raise RuntimeError(f"unexpected candidate HEAD: {candidate_head}")
        if git(root, "diff", "--name-only") != "crema/src/main.rs":
            raise RuntimeError("tracked candidate diff must contain only crema/src/main.rs")
        candidate_diff = git(root, "diff", "--check")
        if candidate_diff:
            raise RuntimeError("candidate git diff --check produced diagnostics")

        rc = run(["git", "worktree", "add", "--detach", str(worktree), BASELINE], out / "logs/worktree-add.log", root)
        if rc:
            raise RuntimeError(f"baseline worktree creation failed rc={rc}")
        if git(worktree, "rev-parse", "HEAD") != BASELINE:
            raise RuntimeError("baseline worktree is not at the exact baseline commit")

        target_lines = git(worktree, "ls-tree", "-r", "--name-only", BASELINE, TARGET_PREFIX).splitlines()
        targets = sorted({line[len(TARGET_PREFIX):].split("/")[0] for line in target_lines if line.startswith(TARGET_PREFIX)})
        queries = [line.rsplit("/", 1)[-1] for line in git(worktree, "ls-tree", "-r", "--name-only", BASELINE, "cqpl/queries_v2/").splitlines() if line.endswith(".cqpl")]
        queries = sorted(queries)
        gate.update({"targets_expected": len(targets), "canonical_queries": queries,
                     "query_count_per_target": len(queries), "expected_query_cells": len(targets) * len(queries)})
        (out / "targets.txt").write_text("\n".join(targets) + "\n")
        if len(targets) != 83 or len(queries) != 12 or len(targets) * len(queries) != 996:
            raise RuntimeError("frozen corpus census did not equal 83 targets × 12 queries")

        for label, tree in (("baseline", worktree), ("candidate", root)):
            for name, manifest, locked in (("crema", "crema/Cargo.toml", False),
                                            ("cqpl", "cqpl/cqpl_checker/Cargo.toml", True)):
                command = ["cargo", f"+{TOOLCHAIN}", "test", "--manifest-path", str(tree / manifest)]
                if locked:
                    command.append("--locked")
                testlog = out / "logs" / f"{label}-{name}-test.log"
                test_rc = run(command, testlog, tree)
                gate["tests"][f"{label}_{name}"] = {"rc": test_rc, **test_counts(testlog), "log": str(testlog.relative_to(out))}

        # Run every target on both exact source trees; failures are recorded and
        # do not suppress later targets or get replaced with cached artifacts.
        target_rows = []
        for label, tree in (("baseline", worktree), ("candidate", root)):
            for index, target in enumerate(targets, 1):
                subject_rel = "a-code_c_ffi_bodyless_gate/" + target
                target_out = out / label / target
                command = ["python3", str(tree / "cqpl/scripts/run_one_target_v6q_r1c.py"),
                           "--root", str(tree), "--relative-path", subject_rel,
                           "--out", str(target_out), "--toolchain", TOOLCHAIN]
                log = out / "logs" / f"{label}-{target}.log"
                rc = run(command, log, tree)
                target_rows.append({"side": label, "target": target, "runner_rc": rc,
                                    "log": str(log.relative_to(out))})
                print(f"{label} {index}/83 {target}: rc={rc}", flush=True)
        (out / "target-status.json").write_text(json.dumps(target_rows, indent=2) + "\n")
        gate["targets_baseline_passed"] = sum(row["runner_rc"] == 0 for row in target_rows if row["side"] == "baseline")
        gate["targets_candidate_passed"] = sum(row["runner_rc"] == 0 for row in target_rows if row["side"] == "candidate")

        for target in targets:
            b = out / "baseline" / target
            c = out / "candidate" / target
            ba, ca = b / "annotated_icfg_v2.json", c / "annotated_icfg_v2.json"
            bi, ci = b / "allocation_identity.json", c / "allocation_identity.json"
            if ba.is_file() and ca.is_file():
                gate["semantic_projection_differences"] += load(ba, [worktree, root]) != load(ca, [worktree, root])
                bdoc, cdoc = load(ba, [worktree, root]), load(ca, [worktree, root])
                for family, fields in PRESERVATION_FIELDS.items():
                    gate["preservation"][family + "_failures"] += any(bdoc.get(field) != cdoc.get(field) for field in fields)
            else:
                gate["semantic_projection_differences"] += 1
                for family in PRESERVATION_FIELDS:
                    gate["preservation"][family + "_failures"] += 1
            if bi.is_file() and ci.is_file():
                gate["allocation_identity_projection_differences"] += load(bi, [worktree, root]) != load(ci, [worktree, root])
            else:
                gate["allocation_identity_projection_differences"] += 1

            bq, cq = b / "queries", c / "queries"
            for query in queries:
                name = Path(query).stem + ".json"
                bp, cp = bq / name, cq / name
                if not bp.is_file():
                    gate["baseline_query_errors"] += 1
                if not cp.is_file():
                    gate["candidate_query_errors"] += 1
                if bp.is_file() and cp.is_file():
                    bd, cd = json.loads(bp.read_text()), json.loads(cp.read_text())
                    gate["query_cells"] += 1
                    gate["truth_deltas"] += bd.get("result") != cd.get("result")
                    gate["assessment_deltas"] += bd.get("assessment") != cd.get("assessment")
                    gate["baseline_query_errors"] += bd.get("result") not in ("tt", "ff", "unk")
                    gate["candidate_query_errors"] += cd.get("result") not in ("tt", "ff", "unk")

        for relative, contents in generated_originals.items():
            (root / relative).write_bytes(contents)
        cache = root / "cqpl/scripts/__pycache__"
        if cache.is_dir():
            shutil.rmtree(cache)
        gate["hygiene"] = {
            "candidate_diff_check_rc": subprocess.run(["git", "diff", "--check"], cwd=root).returncode,
            "baseline_diff_check_rc": subprocess.run(["git", "diff", "--check"], cwd=worktree).returncode,
            "baseline_worktree_removed": 0,
            "generated_tracked_differences": 0,
            "generated_pycache_directories": 0,
            "unexpected_candidate_tracked_changes": sorted(set(git(root, "diff", "--name-only").splitlines()) - {"crema/src/main.rs"}),
        }
        counters_zero = all(value == 0 for value in [gate["truth_deltas"], gate["assessment_deltas"],
            gate["baseline_query_errors"], gate["candidate_query_errors"], gate["semantic_projection_differences"],
            gate["allocation_identity_projection_differences"], *gate["preservation"].values()])
        tests_ok = all(v["rc"] == 0 and v["failed"] == 0 for v in gate["tests"].values())
        gate["status"] = "PASS" if (gate["targets_baseline_passed"] == 83 and
            gate["targets_candidate_passed"] == 83 and gate["query_cells"] == 996 and counters_zero and
            tests_ok and gate["hygiene"]["candidate_diff_check_rc"] == 0 and
            not gate["hygiene"]["unexpected_candidate_tracked_changes"]) else "FAIL"
    except Exception as error:
        gate["error"] = str(error)
        print(f"INFRA1 neutrality gate error: {error}", flush=True)
    finally:
        for relative, contents in generated_originals.items():
            (root / relative).write_bytes(contents)
        cache = root / "cqpl/scripts/__pycache__"
        if cache.is_dir():
            shutil.rmtree(cache)
        rc = run(["git", "worktree", "remove", "--force", str(worktree)], out / "logs/worktree-remove.log", root)
        gate["hygiene"]["baseline_worktree_removed"] = int(rc == 0)
        subprocess.run(["git", "worktree", "prune"], cwd=root, check=False)
        if rc != 0:
            gate["status"] = "FAIL"
        gate_path = out / "gate.json"
        gate_path.write_text(json.dumps(gate, indent=2, sort_keys=True) + "\n")
        (out / "gate.json.sha256").write_text(sha(gate_path) + "  gate.json\n")
        print(f"gate={gate_path}")
        print(f"status={gate['status']}")
    return 0 if gate["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
