#!/usr/bin/env python3
from pathlib import Path
import csv
import json
import sys

root = (
    Path(sys.argv[1]).resolve()
    if len(sys.argv) > 1
    else Path(__file__).resolve().parent
)

errors = []

def load_json(name):
    path = root / name
    try:
        return json.loads(
            path.read_text(
                encoding="utf-8"
            )
        )
    except Exception as exc:
        errors.append(
            f"{name}: {exc}"
        )
        return None

evidence = load_json(
    "selection_evidence.json"
)
ground = load_json(
    "ground_truth.json"
)
summary = load_json(
    "selection_summary.json"
)

def read_tsv(name, expected_header):
    path = root / name

    try:
        with path.open(
            newline="",
            encoding="utf-8",
        ) as handle:
            rows = list(
                csv.reader(
                    handle,
                    delimiter="\t",
                )
            )
    except Exception as exc:
        errors.append(
            f"{name}: {exc}"
        )
        return []

    if not rows:
        errors.append(
            f"{name}: empty file"
        )
        return []

    if rows[0] != expected_header:
        errors.append(
            f"{name}: header mismatch: "
            f"got={rows[0]!r} "
            f"expected={expected_header!r}"
        )

    return rows[1:]

subject_rows = read_tsv(
    "subjects.tsv",
    [
        "case_id",
        "variant",
        "crate",
        "version_or_commit",
        "relative_path",
        "entrypoint",
        "features",
        "status",
    ],
)

candidate_rows = read_tsv(
    "candidate_selection.tsv",
    [
        "case_id",
        "advisory",
        "crate",
        "bug_family",
        "candidate_reason",
        "selection_status",
    ],
)

capability_rows = read_tsv(
    "expected_capabilities.tsv",
    [
        "case_id",
        "capability",
        "reason",
        "status",
    ],
)

if evidence is not None:
    expected_keys = {
        "schema_version",
        "benchmark_id",
        "gate",
        "retrieved_at_utc",
        "source_policy",
        "scientific_status",
        "accuracy_ready",
        "cases",
    }

    if set(evidence) != expected_keys:
        errors.append(
            "selection_evidence.json: "
            "top-level key mismatch"
        )

    if (
        evidence.get("schema_version")
        != "rustsec_selection_evidence_v1"
    ):
        errors.append(
            "wrong evidence schema version"
        )

    if evidence.get("gate") != "B1.1":
        errors.append(
            "wrong evidence gate"
        )

    if evidence.get("accuracy_ready") is not False:
        errors.append(
            "B1.1 accuracy_ready must be false"
        )

    evidence_cases = evidence.get("cases")

    if not isinstance(
        evidence_cases,
        list,
    ):
        errors.append(
            "evidence cases must be list"
        )
        evidence_cases = []
else:
    evidence_cases = []

if ground is not None:
    if (
        ground.get("benchmark_id")
        != "rustsec_memory_safety_v1"
    ):
        errors.append(
            "wrong ground benchmark_id"
        )

    if (
        ground.get("schema_version")
        != "rustsec_ground_truth_v1"
    ):
        errors.append(
            "wrong ground schema_version"
        )

    if (
        ground.get("status")
        != "candidate_set"
    ):
        errors.append(
            "ground status must be candidate_set"
        )

    ground_cases = ground.get("cases")

    if not isinstance(
        ground_cases,
        list,
    ):
        errors.append(
            "ground cases must be list"
        )
        ground_cases = []
else:
    ground_cases = []

if len(evidence_cases) != 14:
    errors.append(
        f"evidence case count="
        f"{len(evidence_cases)}, expected 14"
    )

if len(ground_cases) != 14:
    errors.append(
        f"ground case count="
        f"{len(ground_cases)}, expected 14"
    )

case_ids = [
    case.get("case_id")
    for case in ground_cases
]

advisories = [
    case.get("advisory")
    for case in ground_cases
]

if len(set(case_ids)) != len(case_ids):
    errors.append(
        "duplicate ground case_id"
    )

if len(set(advisories)) != len(advisories):
    errors.append(
        "duplicate advisory"
    )

allowed_families = {
    "use_after_free",
    "double_free",
    "invalid_deallocation",
    "allocator_mismatch",
    "memory_leak",
    "raw_ownership_lifecycle",
    "panic_unwind_memory_safety",
    "other_memory_safety",
}

for case in ground_cases:
    if case.get("status") != "candidate":
        errors.append(
            f"{case.get('case_id')}: "
            "status must be candidate"
        )

    if (
        case.get("bug_family")
        not in allowed_families
    ):
        errors.append(
            f"{case.get('case_id')}: "
            "invalid bug_family"
        )

    source = case.get(
        "ground_truth_source",
        "",
    )

    if not source.startswith(
        "https://rustsec.org/advisories/"
    ):
        errors.append(
            f"{case.get('case_id')}: "
            "ground_truth_source must be RustSec"
        )

    for side in (
        "vulnerable",
        "fixed",
    ):
        obj = case.get(side)

        if (
            not isinstance(obj, dict)
            or not isinstance(
                obj.get("version_or_commit"),
                str,
            )
            or not obj["version_or_commit"]
        ):
            errors.append(
                f"{case.get('case_id')}: "
                f"invalid {side}"
            )

if subject_rows:
    errors.append(
        "B1.1 subjects.tsv must remain "
        "header-only"
    )

if len(candidate_rows) != 14:
    errors.append(
        f"candidate rows="
        f"{len(candidate_rows)}, expected 14"
    )

candidate_ids = {
    row[0]
    for row in candidate_rows
    if row
}

if candidate_ids != set(case_ids):
    errors.append(
        "candidate_selection case set mismatch"
    )

for row in candidate_rows:
    if (
        len(row) != 6
        or row[5] != "selected_candidate"
    ):
        errors.append(
            "candidate_selection status mismatch"
        )

capability_case_ids = {
    row[0]
    for row in capability_rows
    if row
}

if capability_case_ids != set(case_ids):
    errors.append(
        "expected_capabilities case set mismatch"
    )

allowed_cap_status = {
    "present",
    "present_but_insufficient",
    "research_gap_hypothesis",
    "unknown",
}

for row in capability_rows:
    if (
        len(row) != 4
        or row[3] not in allowed_cap_status
    ):
        errors.append(
            f"bad capability row: {row!r}"
        )

if summary is not None:
    if summary.get(
        "candidate_cases"
    ) != 14:
        errors.append(
            "selection_summary candidate count"
        )

    if summary.get(
        "admitted_cases"
    ) != 0:
        errors.append(
            "selection_summary admitted != 0"
        )

    if summary.get(
        "subject_rows"
    ) != 0:
        errors.append(
            "selection_summary subjects != 0"
        )

    if summary.get(
        "accuracy_ready"
    ) is not False:
        errors.append(
            "selection_summary accuracy_ready"
        )

panic_count = sum(
    bool(case.get("panic_dependent"))
    for case in ground_cases
)

ffi_count = sum(
    bool(case.get("ffi_dependent"))
    for case in ground_cases
)

print(
    "candidate_cases      =",
    len(ground_cases),
)
print(
    "unique_crates        =",
    len({
        case.get("crate")
        for case in ground_cases
    }),
)
print(
    "panic_dependent      =",
    panic_count,
)
print(
    "ffi_dependent        =",
    ffi_count,
)
print(
    "candidate_rows       =",
    len(candidate_rows),
)
print(
    "capability_rows      =",
    len(capability_rows),
)
print(
    "subject_rows         =",
    len(subject_rows),
)
print(
    "admitted_cases       = 0"
)
print(
    "accuracy_ready       = NO"
)
print(
    "errors               =",
    len(errors),
)

for error in errors:
    print(
        "ERROR:",
        error,
    )

if errors:
    print(
        "RUSTSEC_B1_1_CANDIDATE_SET: FAIL"
    )
    raise SystemExit(1)

print(
    "RUSTSEC_B1_1_CANDIDATE_SET: PASS"
)
