#!/usr/bin/env python3
from pathlib import Path
import csv
import json
import sys

root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent
errors = []

ground_path = root / "ground_truth.json"
subjects_path = root / "subjects.tsv"
caps_path = root / "expected_capabilities.tsv"
candidates_path = root / "candidate_selection.tsv"

try:
    ground = json.loads(ground_path.read_text(encoding="utf-8"))
except Exception as exc:
    print("ERROR: ground_truth.json:", exc)
    raise SystemExit(1)

expected_ground_keys = {"benchmark_id", "schema_version", "status", "cases"}
unknown_ground = set(ground) - expected_ground_keys
missing_ground = expected_ground_keys - set(ground)

if unknown_ground:
    errors.append(f"ground_truth unknown keys: {sorted(unknown_ground)}")
if missing_ground:
    errors.append(f"ground_truth missing keys: {sorted(missing_ground)}")
if ground.get("benchmark_id") != "rustsec_memory_safety_v1":
    errors.append("wrong benchmark_id")
if ground.get("schema_version") != "rustsec_ground_truth_v1":
    errors.append("wrong schema_version")
if ground.get("status") != "scaffold":
    errors.append("B1 ground_truth status must be scaffold")
if not isinstance(ground.get("cases"), list):
    errors.append("cases must be list")

def read_tsv(path, expected_header):
    try:
        with path.open(newline="", encoding="utf-8") as f:
            reader = csv.reader(f, delimiter="\t")
            rows = list(reader)
    except Exception as exc:
        errors.append(f"{path.name}: {exc}")
        return []
    if not rows:
        errors.append(f"{path.name}: empty file")
        return []
    if rows[0] != expected_header:
        errors.append(
            f"{path.name}: header mismatch: got={rows[0]!r} expected={expected_header!r}"
        )
    return rows[1:]

subject_rows = read_tsv(
    subjects_path,
    [
        "case_id", "variant", "crate", "version_or_commit",
        "relative_path", "entrypoint", "features", "status"
    ],
)
cap_rows = read_tsv(
    caps_path,
    ["case_id", "capability", "reason", "status"],
)
candidate_rows = read_tsv(
    candidates_path,
    [
        "case_id", "advisory", "crate", "bug_family",
        "candidate_reason", "selection_status"
    ],
)

cases = ground.get("cases") if isinstance(ground.get("cases"), list) else []

if cases:
    errors.append("B1 scaffold must contain zero admitted/candidate cases")
if subject_rows:
    errors.append("B1 scaffold subjects.tsv must contain header only")
if cap_rows:
    errors.append("B1 scaffold expected_capabilities.tsv must contain header only")
if candidate_rows:
    errors.append("B1 scaffold candidate_selection.tsv must contain header only")

print("ground_truth_cases  =", len(cases))
print("subject_rows        =", len(subject_rows))
print("capability_rows     =", len(cap_rows))
print("candidate_rows      =", len(candidate_rows))
print("errors              =", len(errors))
print("accuracy_ready      = NO")

for error in errors:
    print("ERROR:", error)

if errors:
    print("RUSTSEC_B1_SCAFFOLD: FAIL")
    raise SystemExit(1)

print("RUSTSEC_B1_SCAFFOLD: PASS")
