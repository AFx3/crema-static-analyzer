#!/usr/bin/env python3
"""
RUSTSEC B1.1/B1.2 candidate-set gate.

Modalità:
  - B1.1 (nessun case admitted): invarianti strette, subjects.tsv header-only,
    ground.cases[].status=="candidate", summary.admitted==0, summary.subject_rows==0.
  - B1.2 (>=1 case admitted in cases/<id>/case.json):
      * subjects.tsv può contenere 2 righe (8 colonne) per admitted;
      * ground.cases[].status può essere candidate|admitted|excluded;
      * per status=="admitted" richiede commit-key + source hash;
      * summary.admitted == len(admitted), summary.subject_rows == 2*len(admitted);
      * accuracy_ready resta obbligatoriamente False in entrambe le modalità;
      * delega a verify_b1_2.py --root <root> --quiet.

candidate_selection.tsv resta a 6 colonne in entrambe le modalità.
"""

from pathlib import Path
import csv
import json
import subprocess
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
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        errors.append(f"{name}: {exc}")
        return None


evidence = load_json("selection_evidence.json")
ground = load_json("ground_truth.json")
summary = load_json("selection_summary.json")


def read_tsv(name, expected_header):
    path = root / name
    try:
        with path.open(newline="", encoding="utf-8") as handle:
            rows = list(csv.reader(handle, delimiter="\t"))
    except Exception as exc:
        errors.append(f"{name}: {exc}")
        return []

    if not rows:
        errors.append(f"{name}: empty file")
        return []

    if rows[0] != expected_header:
        errors.append(
            f"{name}: header mismatch: "
            f"got={rows[0]!r} expected={expected_header!r}"
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


# ---------------------------------------------------------------------------
# B1.2 mode detection
# ---------------------------------------------------------------------------

def _scan_b1_2_admitted(base_root):
    cases_dir = base_root / "cases"
    if not cases_dir.is_dir():
        return set()
    admitted = set()
    for cj in cases_dir.glob("*/case.json"):
        try:
            rec = json.loads(cj.read_text(encoding="utf-8"))
        except Exception:
            continue
        if rec.get("materialization_status") == "admitted":
            cid = rec.get("case_id")
            if cid:
                admitted.add(cid)
    return admitted


B1_2_ADMITTED = _scan_b1_2_admitted(root)
B1_2_MODE = bool(B1_2_ADMITTED)


# ---------------------------------------------------------------------------
# selection_evidence.json
# ---------------------------------------------------------------------------

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
        errors.append("selection_evidence.json: top-level key mismatch")

    if evidence.get("schema_version") != "rustsec_selection_evidence_v1":
        errors.append("wrong evidence schema version")

    if evidence.get("gate") != "B1.1":
        errors.append("wrong evidence gate")

    if evidence.get("accuracy_ready") is not False:
        errors.append("B1.1 accuracy_ready must be false")

    evidence_cases = evidence.get("cases")
    if not isinstance(evidence_cases, list):
        errors.append("evidence cases must be list")
        evidence_cases = []
else:
    evidence_cases = []


# ---------------------------------------------------------------------------
# ground_truth.json (mode-aware)
# ---------------------------------------------------------------------------

if ground is not None:
    if ground.get("benchmark_id") != "rustsec_memory_safety_v1":
        errors.append("wrong ground benchmark_id")

    if ground.get("schema_version") != "rustsec_ground_truth_v1":
        errors.append("wrong ground schema_version")

    allowed_ground_status = {"candidate_set"}
    if B1_2_MODE:
        allowed_ground_status.add("materialized_set")

    if ground.get("status") not in allowed_ground_status:
        errors.append(
            f"ground status={ground.get('status')!r} "
            f"not in {sorted(allowed_ground_status)}"
        )

    ground_cases = ground.get("cases")
    if not isinstance(ground_cases, list):
        errors.append("ground cases must be list")
        ground_cases = []
else:
    ground_cases = []


# ---------------------------------------------------------------------------
# Conti globali e unicità
# ---------------------------------------------------------------------------

if len(evidence_cases) != 14:
    errors.append(
        f"evidence case count={len(evidence_cases)}, expected 14"
    )

if len(ground_cases) != 14:
    errors.append(
        f"ground case count={len(ground_cases)}, expected 14"
    )

case_ids = [case.get("case_id") for case in ground_cases]
advisories = [case.get("advisory") for case in ground_cases]

if len(set(case_ids)) != len(case_ids):
    errors.append("duplicate ground case_id")

if len(set(advisories)) != len(advisories):
    errors.append("duplicate advisory")


# ---------------------------------------------------------------------------
# Check per-caso (mode-aware)
# ---------------------------------------------------------------------------

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

B1_2_REQUIRED_HASH_FIELDS = (
    "source_archive_sha256",
    "source_tree_sha256",
)

if B1_2_MODE:
    allowed_case_status = {"candidate", "admitted", "excluded"}
else:
    allowed_case_status = {"candidate"}

for case in ground_cases:
    cid = case.get("case_id")

    if case.get("status") not in allowed_case_status:
        errors.append(
            f"{cid}: status={case.get('status')!r} "
            f"not in {sorted(allowed_case_status)}"
        )

    if case.get("bug_family") not in allowed_families:
        errors.append(f"{cid}: invalid bug_family")

    source = case.get("ground_truth_source", "")
    if not source.startswith("https://rustsec.org/advisories/"):
        errors.append(f"{cid}: ground_truth_source must be RustSec")

    for side in ("vulnerable", "fixed"):
        obj = case.get(side)

        if (
            not isinstance(obj, dict)
            or not isinstance(obj.get("version_or_commit"), str)
            or not obj["version_or_commit"]
        ):
            errors.append(f"{cid}: invalid {side}")
            continue

        if case.get("status") == "admitted":
            if "commit" not in obj:
                errors.append(
                    f"{cid}.{side}.commit key missing "
                    f"(value may be null)"
                )
            for f in B1_2_REQUIRED_HASH_FIELDS:
                if not obj.get(f):
                    errors.append(
                        f"{cid}.{side}.{f} missing "
                        f"(required for admitted)"
                    )


# ---------------------------------------------------------------------------
# subjects.tsv (mode-aware)
# ---------------------------------------------------------------------------

if not B1_2_MODE:
    if subject_rows:
        errors.append("B1.1 subjects.tsv must remain header-only")
else:
    subj_by_case = {}
    for row in subject_rows:
        if len(row) != 8:
            errors.append(
                f"subjects.tsv: expected 8 columns, "
                f"got {len(row)}: {row!r}"
            )
            continue
        cid, variant = row[0], row[1]
        if variant not in {"vulnerable", "fixed"}:
            errors.append(
                f"subjects.tsv: bad variant {variant!r} for {cid}"
            )
            continue
        subj_by_case.setdefault(cid, []).append(variant)

    for cid in sorted(B1_2_ADMITTED):
        variants = sorted(subj_by_case.get(cid, []))
        if variants != ["fixed", "vulnerable"]:
            errors.append(
                f"subjects.tsv: {cid} variants={variants}, "
                f"expected ['fixed','vulnerable']"
            )

    for cid in subj_by_case:
        if cid not in B1_2_ADMITTED:
            errors.append(
                f"subjects.tsv: unexpected case_id {cid} "
                f"(not admitted)"
            )


# ---------------------------------------------------------------------------
# candidate_selection.tsv (frozen a 6 colonne in B1.1 e B1.2)
# ---------------------------------------------------------------------------

if len(candidate_rows) != 14:
    errors.append(
        f"candidate rows={len(candidate_rows)}, expected 14"
    )

candidate_ids = {row[0] for row in candidate_rows if row}

if candidate_ids != set(case_ids):
    errors.append("candidate_selection case set mismatch")

for row in candidate_rows:
    if len(row) != 6 or row[5] != "selected_candidate":
        errors.append(f"candidate_selection status mismatch: {row!r}")


# ---------------------------------------------------------------------------
# expected_capabilities.tsv
# ---------------------------------------------------------------------------

capability_case_ids = {row[0] for row in capability_rows if row}

if capability_case_ids != set(case_ids):
    errors.append("expected_capabilities case set mismatch")

allowed_cap_status = {
    "present",
    "present_but_insufficient",
    "research_gap_hypothesis",
    "unknown",
}

for row in capability_rows:
    if len(row) != 4 or row[3] not in allowed_cap_status:
        errors.append(f"bad capability row: {row!r}")


# ---------------------------------------------------------------------------
# selection_summary.json (mode-aware)
# ---------------------------------------------------------------------------

if summary is not None:
    if summary.get("candidate_cases") != 14:
        errors.append("selection_summary candidate count")

    if not B1_2_MODE:
        if summary.get("admitted_cases") != 0:
            errors.append("selection_summary admitted != 0")
        if summary.get("subject_rows") != 0:
            errors.append("selection_summary subjects != 0")
    else:
        expected_admitted = len(B1_2_ADMITTED)
        expected_subjects = 2 * expected_admitted

        if summary.get("admitted_cases") != expected_admitted:
            errors.append(
                f"selection_summary admitted_cases="
                f"{summary.get('admitted_cases')} "
                f"expected {expected_admitted}"
            )
        if summary.get("subject_rows") != expected_subjects:
            errors.append(
                f"selection_summary subject_rows="
                f"{summary.get('subject_rows')} "
                f"expected {expected_subjects}"
            )

    if summary.get("accuracy_ready") is not False:
        errors.append("selection_summary accuracy_ready must be false")


# ---------------------------------------------------------------------------
# Riepilogo
# ---------------------------------------------------------------------------

panic_count = sum(
    bool(case.get("panic_dependent")) for case in ground_cases
)
ffi_count = sum(
    bool(case.get("ffi_dependent")) for case in ground_cases
)

print("candidate_cases      =", len(ground_cases))
print(
    "unique_crates        =",
    len({case.get("crate") for case in ground_cases}),
)
print("panic_dependent      =", panic_count)
print("ffi_dependent        =", ffi_count)
print("candidate_rows       =", len(candidate_rows))
print("capability_rows      =", len(capability_rows))
print("subject_rows         =", len(subject_rows))
print("admitted_cases       =", len(B1_2_ADMITTED))
print(
    "accuracy_ready       =",
    "NO" if not (summary or {}).get("accuracy_ready") else "YES",
)
print("b1_2_mode            =", "YES" if B1_2_MODE else "NO")
print("errors               =", len(errors))


# ---------------------------------------------------------------------------
# Delega inline a verify_b1_2.py (solo in B1.2 mode)
# ---------------------------------------------------------------------------

if B1_2_MODE:
    b12 = root / "verify_b1_2.py"
    if not b12.exists():
        errors.append(
            "B1.2 mode active but verify_b1_2.py missing in "
            + str(root)
        )
    else:
        rc_b12 = subprocess.call(
            [
                sys.executable,
                "-B",
                str(b12),
                "--root",
                str(root),
                "--quiet",
            ],
            cwd=str(root),
        )
        if rc_b12 != 0:
            errors.append(f"verify_b1_2.py returned rc={rc_b12}")
        else:
            print("B1.2 consistency hook: PASS")
else:
    print("B1.2: no admitted cases yet, skipping consistency check")


# ---------------------------------------------------------------------------
# Verdetto
# ---------------------------------------------------------------------------

for error in errors:
    print("ERROR:", error)

if errors:
    print("RUSTSEC_B1_1_CANDIDATE_SET: FAIL")
    raise SystemExit(1)

print("RUSTSEC_B1_1_CANDIDATE_SET: PASS")
