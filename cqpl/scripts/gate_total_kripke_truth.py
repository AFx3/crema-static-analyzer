#!/usr/bin/env python3
"""Differential gate for cqpl4 total Kripke truth semantics.

Runs the current checker on the exact 118 frozen annotated ICFGs from a prior
full13 gate and compares all 12 canonical queries plus the experimental
normal-execution double-free query against the frozen pre-cqpl4 JSON outputs.

Truth deltas are *classified*, not automatically rejected: totalization is a
semantic correction and may intentionally change formulas that observe strong
next at a terminal. Execution errors, missing baseline data, or malformed
outputs are hard failures.
"""
from __future__ import annotations

import argparse
import json
import subprocess
from collections import Counter, defaultdict
from pathlib import Path


def load_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except Exception as exc:
        raise RuntimeError(f"cannot parse JSON {path}: {exc}") from exc


def assessment_tuple(doc: dict) -> tuple[str, str, str]:
    a = doc.get("assessment") or {}
    return (
        str(a.get("subresult", "")),
        str(a.get("direction", "")),
        str(a.get("strength", "")),
    )


def run_checker(checker: Path, artifact: Path, query: Path) -> tuple[dict | None, str | None]:
    cp = subprocess.run(
        [str(checker), str(artifact), str(query), "--json"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if cp.returncode != 0:
        detail = " ".join(x.strip() for x in cp.stderr.splitlines() if x.strip())[:600]
        return None, f"rc={cp.returncode}: {detail}"
    try:
        return json.loads(cp.stdout), None
    except Exception as exc:
        return None, f"invalid checker JSON: {type(exc).__name__}: {exc}"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--checker", type=Path, required=True)
    ap.add_argument("--baseline-full13", type=Path, required=True)
    ap.add_argument("--queries", type=Path, required=True)
    ap.add_argument("--experimental-query", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    if not args.checker.is_file():
        raise SystemExit(f"missing checker: {args.checker}")
    if not args.baseline_full13.is_dir():
        raise SystemExit(f"missing baseline full13 directory: {args.baseline_full13}")

    subjects_path = args.baseline_full13 / "subjects.tsv"
    if not subjects_path.is_file():
        raise SystemExit(f"missing subjects manifest: {subjects_path}")

    canonical = sorted(args.queries.glob("*.cqpl"))
    if len(canonical) != 12:
        raise SystemExit(f"expected exactly 12 canonical queries, found {len(canonical)}")
    if not args.experimental_query.is_file():
        raise SystemExit(f"missing experimental query: {args.experimental_query}")

    subjects: list[tuple[str, str, Path]] = []
    for line in subjects_path.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            raise SystemExit(f"bad subject row: {line!r}")
        subjects.append((parts[0], parts[1], Path(parts[2])))
    if len(subjects) != 118:
        raise SystemExit(f"expected 118 subjects, found {len(subjects)}")

    args.out.mkdir(parents=True, exist_ok=True)
    results_dir = args.out / "results"
    results_dir.mkdir(exist_ok=True)

    truth_delta = []
    assessment_delta = []
    errors = []
    new_truth = Counter()
    old_truth = Counter()
    truth_delta_by_query = Counter()
    assessment_delta_by_query = Counter()
    transitions = Counter()
    assessment_transitions = Counter()

    queries = [(q, "canonical", args.baseline_full13 / "matrix-canonical12") for q in canonical]
    queries.append((args.experimental_query, "experimental", args.baseline_full13 / "matrix-double-free-normal"))

    attempts = 0
    for group, subject, artifact in subjects:
        if not artifact.is_file():
            errors.append({"group": group, "subject": subject, "artifact": str(artifact), "error": "artifact_missing"})
            continue
        for query, query_set, baseline_matrix in queries:
            attempts += 1
            stem = query.stem
            baseline_json = baseline_matrix / "results" / subject / f"{stem}.json"
            if not baseline_json.is_file():
                errors.append({
                    "group": group, "subject": subject, "query": stem,
                    "error": "baseline_json_missing", "path": str(baseline_json),
                })
                continue

            old = load_json(baseline_json)
            new, err = run_checker(args.checker, artifact, query)
            if err is not None or new is None:
                errors.append({"group": group, "subject": subject, "query": stem, "error": err})
                continue

            od = results_dir / subject
            od.mkdir(exist_ok=True)
            (od / f"{stem}.json").write_text(json.dumps(new, indent=2, sort_keys=True) + "\n")

            old_result = str(old.get("result"))
            new_result = str(new.get("result"))
            old_truth[old_result] += 1
            new_truth[new_result] += 1
            transitions[(old_result, new_result)] += 1

            if old_result != new_result:
                rec = {
                    "group": group, "subject": subject, "query": stem, "query_set": query_set,
                    "artifact": str(artifact), "old_result": old_result, "new_result": new_result,
                    "old_assessment": assessment_tuple(old), "new_assessment": assessment_tuple(new),
                }
                truth_delta.append(rec)
                truth_delta_by_query[stem] += 1

            old_a = assessment_tuple(old)
            new_a = assessment_tuple(new)
            assessment_transitions[(old_a, new_a)] += 1
            if old_a != new_a:
                assessment_delta.append({
                    "group": group, "subject": subject, "query": stem, "query_set": query_set,
                    "artifact": str(artifact), "old_result": old_result, "new_result": new_result,
                    "old_assessment": old_a, "new_assessment": new_a,
                })
                assessment_delta_by_query[stem] += 1

    expected_attempts = 118 * 13
    hard_ok = attempts == expected_attempts and not errors
    status = (
        "FAIL" if not hard_ok else
        "PASS_NO_DELTA" if not truth_delta and not assessment_delta else
        "REVIEW_SEMANTIC_DELTA"
    )

    report = {
        "schema": "cqpl_total_kripke_differential_v1",
        "status": status,
        "subjects": len(subjects),
        "canonical_queries": len(canonical),
        "experimental_queries": 1,
        "attempts": attempts,
        "expected_attempts": expected_attempts,
        "errors": errors,
        "old_truth_counts": dict(sorted(old_truth.items())),
        "new_truth_counts": dict(sorted(new_truth.items())),
        "truth_delta_count": len(truth_delta),
        "truth_delta_by_query": dict(sorted(truth_delta_by_query.items())),
        "truth_transitions": {
            f"{a}->{b}": n for (a, b), n in sorted(transitions.items())
        },
        "assessment_delta_count": len(assessment_delta),
        "assessment_delta_by_query": dict(sorted(assessment_delta_by_query.items())),
        "truth_deltas": truth_delta,
        "assessment_deltas": assessment_delta,
        "criteria": {
            "attempts_1534": attempts == expected_attempts,
            "no_execution_errors": not errors,
            "truth_deltas_classified": True,
            "assessment_deltas_classified": True,
        },
    }
    (args.out / "TOTAL_KRIPKE_DIFFERENTIAL.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n"
    )

    print("TOTAL_KRIPKE_DIFFERENTIAL:", status)
    print("  attempts=", attempts)
    print("  errors=", len(errors))
    print("  truth_delta_count=", len(truth_delta))
    print("  truth_delta_by_query=", dict(sorted(truth_delta_by_query.items())))
    print("  assessment_delta_count=", len(assessment_delta))
    print("  assessment_delta_by_query=", dict(sorted(assessment_delta_by_query.items())))
    print("  report=", args.out / "TOTAL_KRIPKE_DIFFERENTIAL.json")

    return 2 if not hard_ok else 0


if __name__ == "__main__":
    raise SystemExit(main())
