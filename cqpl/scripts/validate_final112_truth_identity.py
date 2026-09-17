#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path

FROZEN_QUERIES = [
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
]
EXPECTED_SUBJECTS = 112
EXPECTED_ATTEMPTS = EXPECTED_SUBJECTS * len(FROZEN_QUERIES)
VALID_TRUTHS = {"ff", "unk", "tt"}


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def truth_counts(rows: list[dict[str, str]]) -> dict[str, int]:
    counts = Counter(row[q] for row in rows for q in FROZEN_QUERIES)
    return {k: counts.get(k, 0) for k in sorted(VALID_TRUTHS)}


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    ap = argparse.ArgumentParser(
        description=(
            "Fail-closed FINAL112 gate: preserve the historical audit and "
            "admit only versioned, reviewed oracle-consistent precision deltas"
        )
    )
    ap.add_argument("--fresh-wide", type=Path, required=True)
    ap.add_argument(
        "--audit",
        type=Path,
        default=root / "artifact" / "FINAL112_GRAPH_QUERY_AUDIT.tsv",
    )
    ap.add_argument(
        "--approved-deltas",
        type=Path,
        action="append",
        default=None,
        help=(
            "Reviewed precision-delta TSV; may be repeated. If omitted, the "
            "versioned A3 R4 and B1.1-r1 delta files are used."
        ),
    )
    ap.add_argument(
        "--source-oracle-audit",
        type=Path,
        default=root / "artifact" / "FINAL112_SOURCE_ORACLE_AUDIT.tsv",
    )
    ap.add_argument("--out", type=Path)
    args = ap.parse_args()

    audit_rows = read_tsv(args.audit)
    fresh_rows = read_tsv(args.fresh_wide)
    delta_paths = args.approved_deltas or [
        root / "artifact" / "A3_R4_FINAL112_PRECISION_DELTAS.tsv",
        root / "artifact" / "B1_1_R1_FINAL112_PRECISION_DELTAS.tsv",
        root / "artifact" / "BCONTRACT_DROP1_R1_FINAL112_PRECISION_DELTAS.tsv",
    ]
    delta_rows: list[dict[str, str]] = []
    delta_source_for_row: list[str] = []
    for delta_path in delta_paths:
        rows = read_tsv(delta_path)
        delta_rows.extend(rows)
        delta_source_for_row.extend([delta_path.name] * len(rows))
    source_rows = read_tsv(args.source_oracle_audit)

    if len(audit_rows) != EXPECTED_SUBJECTS:
        raise SystemExit(f"audit subject count {len(audit_rows)} != {EXPECTED_SUBJECTS}")
    if len(fresh_rows) != EXPECTED_SUBJECTS:
        raise SystemExit(f"fresh subject count {len(fresh_rows)} != {EXPECTED_SUBJECTS}")

    fresh_header = set(fresh_rows[0]) if fresh_rows else set()
    expected_header = {"group", "target", *FROZEN_QUERIES}
    if fresh_header != expected_header:
        raise SystemExit(
            "fresh matrix is not the frozen 12-query surface: "
            f"missing={sorted(expected_header - fresh_header)} "
            f"extra={sorted(fresh_header - expected_header)}"
        )

    audit = {(r["group"], r["target"]): r for r in audit_rows}
    fresh = {(r["group"], r["target"]): r for r in fresh_rows}
    source = {(r["group"], r["target"]): r for r in source_rows}

    expected_delta: dict[tuple[str, str, str], tuple[str, str]] = {}
    delta_metadata: dict[tuple[str, str, str], dict[str, str]] = {}
    allowlist_errors: list[str] = []
    for row_index, row in enumerate(delta_rows):
        key = (row["group"], row["target"], row["query"])
        if key in expected_delta:
            allowlist_errors.append(f"duplicate approved delta {key}")
            continue
        old = row["baseline_result"]
        new = row["a3_result"]
        if row["query"] not in FROZEN_QUERIES:
            allowlist_errors.append(f"approved delta uses non-frozen query {key}")
        if old not in VALID_TRUTHS or new not in VALID_TRUTHS or old == new:
            allowlist_errors.append(f"invalid truth transition {key}: {old}->{new}")
        if (old, new) != ("unk", "ff"):
            allowlist_errors.append(
                f"reviewed FINAL112 delta files only admit unk->ff refinements, got {key}: {old}->{new}"
            )
        subject_key = (row["group"], row["target"])
        if subject_key not in audit:
            allowlist_errors.append(f"approved delta subject absent from historical audit: {subject_key}")
        elif audit[subject_key].get(row["query"]) != old:
            allowlist_errors.append(
                f"approved delta baseline drift {key}: allowlist={old} audit={audit[subject_key].get(row['query'])}"
            )
        oracle = source.get(subject_key)
        expected_classes = row["source_reference_classes"]
        actual_classes = oracle.get("reference_classes", "") if oracle else None
        normalized_expected = "" if expected_classes == "<none>" else expected_classes
        if actual_classes != normalized_expected:
            allowlist_errors.append(
                f"source oracle class drift {subject_key}: allowlist={expected_classes!r} audit={actual_classes!r}"
            )
        expected_delta[key] = (old, new)
        delta_metadata[key] = dict(row, approved_delta_source=delta_source_for_row[row_index])

    observed: dict[tuple[str, str, str], tuple[str, str]] = {}
    subject_set_mismatches: list[dict[str, str]] = []
    for subject_key in sorted(set(audit) | set(fresh)):
        if subject_key not in audit or subject_key not in fresh:
            subject_set_mismatches.append({
                "group": subject_key[0],
                "target": subject_key[1],
                "query": "<subject-set>",
                "audit": str(subject_key in audit),
                "fresh": str(subject_key in fresh),
            })
            continue
        for query in FROZEN_QUERIES:
            old = audit[subject_key][query]
            new = fresh[subject_key][query]
            if old != new:
                observed[(subject_key[0], subject_key[1], query)] = (old, new)

    missing_approved = sorted(set(expected_delta) - set(observed))
    extra_unapproved = sorted(set(observed) - set(expected_delta))
    wrong_approved = sorted(
        key for key in set(expected_delta) & set(observed)
        if expected_delta[key] != observed[key]
    )

    historical_counts = truth_counts(audit_rows)
    expected_counts = dict(historical_counts)
    for old, new in expected_delta.values():
        expected_counts[old] -= 1
        expected_counts[new] += 1
    fresh_counts = truth_counts(fresh_rows)

    approved_observed = []
    for key in sorted(set(expected_delta) & set(observed)):
        row = delta_metadata[key]
        approved_observed.append({
            "group": key[0],
            "target": key[1],
            "query": key[2],
            "audit": observed[key][0],
            "fresh": observed[key][1],
            "source_reference_classes": row["source_reference_classes"],
            "approved_delta_source": row["approved_delta_source"],
            "rationale": row["rationale"],
        })

    unapproved_details = [
        {"group": k[0], "target": k[1], "query": k[2], "audit": observed[k][0], "fresh": observed[k][1]}
        for k in extra_unapproved
    ]
    wrong_details = [
        {
            "group": k[0], "target": k[1], "query": k[2],
            "expected_audit": expected_delta[k][0], "expected_fresh": expected_delta[k][1],
            "observed_audit": observed[k][0], "observed_fresh": observed[k][1],
        }
        for k in wrong_approved
    ]

    result = {
        "schema": "cqpl_final112_precision_delta_audit_v2",
        "historical_baseline": "FINAL112_GRAPH_QUERY_AUDIT.tsv",
        "historical_baseline_preserved": True,
        "subjects": len(fresh_rows),
        "queries": len(FROZEN_QUERIES),
        "attempts": len(fresh_rows) * len(FROZEN_QUERIES),
        "historical_result_counts": historical_counts,
        "expected_result_counts_after_approved_deltas": expected_counts,
        "result_counts": fresh_counts,
        "approved_precision_delta_count": len(expected_delta),
        "approved_precision_delta_sources": [p.name for p in delta_paths],
        "approved_precision_deltas_observed": approved_observed,
        "missing_approved_delta_count": len(missing_approved),
        "missing_approved_deltas": [
            {"group": k[0], "target": k[1], "query": k[2], "expected": f"{expected_delta[k][0]}->{expected_delta[k][1]}"}
            for k in missing_approved
        ],
        "unapproved_mismatch_count": len(extra_unapproved) + len(subject_set_mismatches),
        "unapproved_mismatches": subject_set_mismatches + unapproved_details,
        "wrong_approved_transition_count": len(wrong_approved),
        "wrong_approved_transitions": wrong_details,
        "allowlist_error_count": len(allowlist_errors),
        "allowlist_errors": allowlist_errors,
    }

    passed = (
        result["attempts"] == EXPECTED_ATTEMPTS
        and not allowlist_errors
        and not missing_approved
        and not extra_unapproved
        and not wrong_approved
        and not subject_set_mismatches
        and fresh_counts == expected_counts
    )
    result["status"] = "PASS" if passed else "FAIL"

    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2, sort_keys=True))

    if not passed:
        return 3
    print(
        "FINAL112_PRECISION_AUDIT: PASS "
        f"subjects={EXPECTED_SUBJECTS} queries={len(FROZEN_QUERIES)} "
        f"attempts={EXPECTED_ATTEMPTS} approved_deltas={len(expected_delta)} unapproved=0"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
