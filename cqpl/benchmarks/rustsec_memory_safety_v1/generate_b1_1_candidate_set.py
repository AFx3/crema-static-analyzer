#!/usr/bin/env python3
from pathlib import Path
import argparse
import csv
import io
import json

parser = argparse.ArgumentParser()
parser.add_argument("--benchmark-dir", required=True)
parser.add_argument("--out-dir", required=True)
args = parser.parse_args()

benchmark_dir = Path(args.benchmark_dir).resolve()
out_dir = Path(args.out_dir).resolve()
out_dir.mkdir(parents=True, exist_ok=True)

evidence = json.loads(
    (benchmark_dir / "selection_evidence.json").read_text(
        encoding="utf-8"
    )
)

cases = evidence["cases"]

ground_cases = []
candidate_rows = []
capability_rows = []

for item in cases:
    ground_cases.append({
        "case_id": item["case_id"],
        "advisory": item["advisory"],
        "crate": item["crate"],
        "vulnerable": {
            "version_or_commit": item["vulnerable_range"],
        },
        "fixed": {
            "version_or_commit": item["patched_range"],
        },
        "bug_family": item["bug_family"],
        "affected_function": item["affected_function"],
        "ground_truth_source": item["ground_truth_source"],
        "panic_dependent": item["panic_dependent"],
        "ffi_dependent": item["ffi_dependent"],
        "status": "candidate",
    })

    candidate_rows.append([
        item["case_id"],
        item["advisory"],
        item["crate"],
        item["bug_family"],
        item["candidate_reason"],
        "selected_candidate",
    ])

    for capability, reason, status in item["expected_capabilities"]:
        capability_rows.append([
            item["case_id"],
            capability,
            reason,
            status,
        ])

ground = {
    "benchmark_id": "rustsec_memory_safety_v1",
    "schema_version": "rustsec_ground_truth_v1",
    "status": "candidate_set",
    "cases": sorted(
        ground_cases,
        key=lambda x: x["case_id"],
    ),
}

(out_dir / "ground_truth.json").write_text(
    json.dumps(
        ground,
        indent=2,
        sort_keys=True,
    ) + "\n",
    encoding="utf-8",
)

def write_tsv(path, header, rows):
    buf = io.StringIO()
    writer = csv.writer(
        buf,
        delimiter="\t",
        lineterminator="\n",
    )
    writer.writerow(header)
    for row in sorted(rows, key=lambda r: tuple(r)):
        writer.writerow(row)
    path.write_text(
        buf.getvalue(),
        encoding="utf-8",
    )

write_tsv(
    out_dir / "candidate_selection.tsv",
    [
        "case_id",
        "advisory",
        "crate",
        "bug_family",
        "candidate_reason",
        "selection_status",
    ],
    candidate_rows,
)

write_tsv(
    out_dir / "expected_capabilities.tsv",
    [
        "case_id",
        "capability",
        "reason",
        "status",
    ],
    capability_rows,
)

summary = {
    "gate": "B1.1",
    "candidate_cases": len(cases),
    "unique_crates": len({
        item["crate"]
        for item in cases
    }),
    "panic_dependent_cases": sum(
        bool(item["panic_dependent"])
        for item in cases
    ),
    "ffi_dependent_cases": sum(
        bool(item["ffi_dependent"])
        for item in cases
    ),
    "admitted_cases": 0,
    "subject_rows": 0,
    "accuracy_ready": False,
    "bug_family_counts": {},
}

for item in cases:
    family = item["bug_family"]
    summary["bug_family_counts"][family] = (
        summary["bug_family_counts"].get(
            family,
            0,
        ) + 1
    )

(out_dir / "selection_summary.json").write_text(
    json.dumps(
        summary,
        indent=2,
        sort_keys=True,
    ) + "\n",
    encoding="utf-8",
)

print("candidate_cases =", len(cases))
print("unique_crates =", summary["unique_crates"])
print("panic_dependent_cases =", summary["panic_dependent_cases"])
print("ffi_dependent_cases =", summary["ffi_dependent_cases"])
print("accuracy_ready = NO")
print("RUSTSEC_B1_1_GENERATE: PASS")
