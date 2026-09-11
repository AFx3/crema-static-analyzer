#!/usr/bin/env python3
import json
import pathlib
import sys

if len(sys.argv) != 3:
    raise SystemExit(
        "usage: validate_phase4_4_full.py "
        "<phase3_1_normalized.json> <phase4_4_result_dir>"
    )

baseline_path = pathlib.Path(sys.argv[1])
result_dir = pathlib.Path(sys.argv[2])
candidate_path = result_dir / "results.normalized.json"
status_path = result_dir / "status.tsv"
excluded_path = result_dir / "excluded-targets.txt"
census_path = result_dir / "mir-census.aggregate.json"

for p in [baseline_path, candidate_path, status_path, excluded_path, census_path]:
    if not p.exists():
        raise SystemExit(f"missing required artifact: {p}")

baseline = json.loads(baseline_path.read_text())
candidate = json.loads(candidate_path.read_text())

baseline.pop("openapi-client-gen", None)
candidate.pop("openapi-client-gen", None)

if len(baseline) != 92:
    raise SystemExit(f"baseline cardinality={len(baseline)}; expected 92")
if len(candidate) != 92:
    raise SystemExit(f"candidate cardinality={len(candidate)}; expected 92")

rows = [
    line for line in status_path.read_text().splitlines()[1:]
    if line.strip()
]
if len(rows) != 92:
    raise SystemExit(f"status.tsv targets={len(rows)}; expected 92")

bad_rc = []
for row in rows:
    cols = row.split("\t")
    if len(cols) < 3:
        bad_rc.append((row, "malformed"))
        continue
    try:
        rc = int(cols[-1])
    except ValueError:
        bad_rc.append((row, "bad rc"))
        continue
    if rc != 0:
        bad_rc.append((row, rc))

if bad_rc:
    raise SystemExit(f"nonzero/malformed target rows: {bad_rc[:10]}")

excluded = {
    x.strip() for x in excluded_path.read_text().splitlines()
    if x.strip()
}
if "openapi-client-gen" not in excluded:
    raise SystemExit("openapi-client-gen not recorded in excluded-targets.txt")

def shared_register_ok(classes):
    s = set(classes or [])
    return s in ({"ML"}, {"DF", "ML"})

mismatches = []
for key in sorted(set(baseline) | set(candidate)):
    b = baseline.get(key)
    c = candidate.get(key)
    if key == "shared-register" and shared_register_ok(b) and shared_register_ok(c):
        continue
    if b != c:
        mismatches.append((key, b, c))

census = json.loads(census_path.read_text())
count = census.get("targets_with_census")
if count is not None and count != 92:
    raise SystemExit(f"census targets={count}; expected 92")

print("Phase 4.4 full validation")
print(f"baseline targets : {len(baseline)}")
print(f"candidate targets: {len(candidate)}")
print(f"status targets   : {len(rows)}")
print(f"mismatches       : {len(mismatches)}")

if mismatches:
    for key, b, c in mismatches:
        print(f"\n{key}\n  baseline : {b}\n  candidate: {c}")
    raise SystemExit(1)

print("RESULT: PASS")
