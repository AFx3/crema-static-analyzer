#!/usr/bin/env python3
# cqpl/benchmarks/rustsec_memory_safety_v1/verify_b1_2.py
"""
B1.2 verification gate.

Verifica che:
  (A) ogni caso admitted soddisfi il contratto B1.2 (case.json);
  (B) gli aggregati (subjects.tsv, ground_truth.json,
      selection_summary.json) siano coerenti coi casi admitted;
  (C) nessun artefatto di build/cache sia presente sotto cases/.

Non modifica candidate_selection.tsv: quel file resta frozen a 6 colonne
in B1.1 e in B1.2.

Uso:
    python3 verify_b1_2.py [--root <benchmark-dir>] [--quiet]
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CASES_DIR = HERE / "cases"
SUBJECTS_TSV = HERE / "subjects.tsv"
GROUND_TRUTH_JSON = HERE / "ground_truth.json"
SELECTION_TSV = HERE / "candidate_selection.tsv"
EXPECTED_CAPABILITIES_TSV = HERE / "expected_capabilities.tsv"
SELECTION_SUMMARY_JSON = HERE / "selection_summary.json"

FORBIDDEN_DIR_NAMES = {"target", "__pycache__"}
FORBIDDEN_SUFFIXES = {".pyc", ".rlib", ".rmeta", ".d"}

REQUIRED_VARIANT_FIELDS_NONNULL = (
    "version",
    "source_archive_sha256",
    "source_tree_sha256",
    "build_status",
)

ALLOWED_EXCLUSION_REASONS = {
    "source_unavailable",
    "source_integrity_mismatch",
    "build_incompatible",
    "reproducer_not_reproducible",
    "fixed_pair_not_equivalent",
    "native_dependency_unavailable",
    "advisory_ambiguous",
}

ALLOWED_MATERIALIZATION_STATUS = {
    "candidate",
    "materialized",
    "admitted",
    "excluded",
}


class Reporter:
    def __init__(self) -> None:
        self.errors: list[str] = []
        self.warnings: list[str] = []

    def err(self, msg: str) -> None:
        self.errors.append(msg)

    def warn(self, msg: str) -> None:
        self.warnings.append(msg)

    def ok(self) -> bool:
        return not self.errors


def load_json(path: Path) -> dict | list:
    return json.loads(path.read_text(encoding="utf-8"))


def load_tsv(path: Path) -> list[dict]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8") as f:
        rdr = csv.DictReader(f, delimiter="\t")
        return list(rdr)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def iter_case_dirs() -> list[Path]:
    if not CASES_DIR.exists():
        return []
    return sorted(p for p in CASES_DIR.iterdir() if p.is_dir())


def load_cases(rep: Reporter) -> list[dict]:
    out: list[dict] = []
    for d in iter_case_dirs():
        cj = d / "case.json"
        if not cj.exists():
            rep.err(f"{d.name}: missing case.json")
            continue
        try:
            rec = load_json(cj)
        except Exception as e:  # noqa: BLE001
            rep.err(f"{d.name}: invalid case.json ({e})")
            continue
        if not isinstance(rec, dict):
            rep.err(f"{d.name}: case.json is not an object")
            continue
        rec["_dir"] = d
        out.append(rec)
    return out


def check_case_contract(rec: dict, rep: Reporter) -> None:
    name = rec["_dir"].name

    if rec.get("schema") != "b1.2-case-v1":
        rep.err(
            f"{name}: schema={rec.get('schema')!r}, expected 'b1.2-case-v1'"
        )

    if rec.get("case_id") != name:
        rep.err(f"{name}: case_id={rec.get('case_id')!r} != dirname")

    status = rec.get("materialization_status")
    if status not in ALLOWED_MATERIALIZATION_STATUS:
        rep.err(
            f"{name}: materialization_status={status!r} "
            f"not in {sorted(ALLOWED_MATERIALIZATION_STATUS)}"
        )
        return

    if status != "admitted":
        return

    for var in ("vulnerable", "fixed"):
        section = rec.get(var)
        if not isinstance(section, dict):
            rep.err(f"{name}: missing section {var!r}")
            continue

        if "commit" not in section:
            rep.err(
                f"{name}: {var}.commit key missing "
                f"(value may be null)"
            )
        else:
            c = section.get("commit")
            if c is not None and (not isinstance(c, str) or not c):
                rep.err(
                    f"{name}: {var}.commit must be null or non-empty string"
                )

        for field in REQUIRED_VARIANT_FIELDS_NONNULL:
            v = section.get(field)
            if not v:
                rep.err(f"{name}: {var}.{field} missing/empty")

        if section.get("build_status") != "pass":
            rep.err(
                f"{name}: {var}.build_status != 'pass' "
                f"(got {section.get('build_status')!r})"
            )

    bc = rec.get("build_contract") or {}
    if not isinstance(bc, dict):
        rep.err(f"{name}: build_contract not an object")
    else:
        if not bc.get("rust_toolchain"):
            rep.err(f"{name}: build_contract.rust_toolchain missing")
        if not bc.get("target"):
            rep.err(f"{name}: build_contract.target missing")
        if not isinstance(bc.get("cargo_features"), list):
            rep.err(f"{name}: build_contract.cargo_features must be a list")
        if not isinstance(bc.get("native_dependencies"), list):
            rep.err(
                f"{name}: build_contract.native_dependencies must be a list"
            )

    repro = rec.get("reproducer") or {}
    if not isinstance(repro, dict):
        rep.err(f"{name}: reproducer not an object")
    else:
        rel = repro.get("file")
        if not rel:
            rep.err(f"{name}: reproducer.file missing")
        else:
            p = rec["_dir"] / rel
            if not p.is_file():
                rep.err(f"{name}: reproducer file not found: {rel}")
            else:
                expected = repro.get("sha256")
                if expected:
                    actual = sha256_file(p)
                    if actual != expected:
                        rep.err(
                            f"{name}: reproducer sha256 mismatch "
                            f"(case.json={expected[:12]}… "
                            f"disk={actual[:12]}…)"
                        )
                else:
                    rep.err(f"{name}: reproducer.sha256 missing")

        if not repro.get("provenance"):
            rep.err(f"{name}: reproducer.provenance missing")
        if not repro.get("expected_vulnerable_outcome"):
            rep.err(
                f"{name}: reproducer.expected_vulnerable_outcome missing"
            )
        if not repro.get("expected_fixed_outcome"):
            rep.err(
                f"{name}: reproducer.expected_fixed_outcome missing"
            )

    val = rec.get("validation") or {}
    if not isinstance(val, dict):
        rep.err(f"{name}: validation not an object")
    else:
        if val.get("vulnerable_reproduces") is not True:
            rep.err(f"{name}: validation.vulnerable_reproduces != true")
        if val.get("fixed_reproduces") is not False:
            rep.err(f"{name}: validation.fixed_reproduces != false")

    reason = rec.get("reason_if_not_admitted")
    if reason is not None and reason not in ALLOWED_EXCLUSION_REASONS:
        rep.err(
            f"{name}: reason_if_not_admitted={reason!r} "
            f"not in closed enum"
        )


def check_no_build_artifacts(rep: Reporter) -> None:
    if not CASES_DIR.exists():
        return
    for p in CASES_DIR.rglob("*"):
        if p == CASES_DIR:
            continue
        if p.is_dir() and p.name in FORBIDDEN_DIR_NAMES:
            rep.err(f"forbidden build/cache dir: {p}")
        elif p.is_file() and p.suffix in FORBIDDEN_SUFFIXES:
            rep.err(f"forbidden build artifact file: {p}")


def check_subjects(admitted: set[str], rep: Reporter) -> None:
    rows = load_tsv(SUBJECTS_TSV)
    by_case: dict[str, list[str]] = {}

    for r in rows:
        cid = r.get("case_id")
        var = r.get("variant")
        if not cid or var not in {"vulnerable", "fixed"}:
            rep.err(f"subjects.tsv: malformed row: {r}")
            continue
        by_case.setdefault(cid, []).append(var)

    if not admitted:
        if by_case:
            rep.err(
                "subjects.tsv: must be header-only while no admitted cases"
            )
        return

    for cid in sorted(admitted):
        variants = sorted(by_case.get(cid, []))
        if variants != ["fixed", "vulnerable"]:
            rep.err(
                f"subjects.tsv: {cid} variants={variants}, "
                f"expected ['fixed','vulnerable']"
            )
    for cid in by_case:
        if cid not in admitted:
            rep.err(f"subjects.tsv: {cid} present but not admitted")


def check_ground_truth(admitted: set[str], rep: Reporter) -> None:
    if not GROUND_TRUTH_JSON.exists():
        rep.err("ground_truth.json missing")
        return

    gt = load_json(GROUND_TRUTH_JSON)
    if not isinstance(gt, dict):
        rep.err("ground_truth.json: expected object")
        return

    if gt.get("benchmark_id") != "rustsec_memory_safety_v1":
        rep.err("ground_truth.json: wrong benchmark_id")
    if gt.get("schema_version") != "rustsec_ground_truth_v1":
        rep.err("ground_truth.json: wrong schema_version")

    cases = gt.get("cases")
    if not isinstance(cases, list):
        rep.err("ground_truth.json: missing 'cases' list")
        return

    by_id: dict[str, dict] = {}
    for c in cases:
        if not isinstance(c, dict):
            continue
        cid = c.get("case_id")
        if cid:
            by_id[cid] = c

    for cid in sorted(admitted):
        entry = by_id.get(cid)
        if entry is None:
            rep.err(f"ground_truth.json: no case entry for {cid}")
            continue

        if entry.get("status") != "admitted":
            rep.err(
                f"ground_truth.json[{cid}].status="
                f"{entry.get('status')!r}, expected 'admitted'"
            )
            continue

        for var in ("vulnerable", "fixed"):
            sec = entry.get(var)
            if not isinstance(sec, dict):
                rep.err(
                    f"ground_truth.json[{cid}].{var} not an object"
                )
                continue

            if not sec.get("version_or_commit"):
                rep.err(
                    f"ground_truth.json[{cid}].{var}.version_or_commit "
                    f"missing"
                )

            if "commit" not in sec:
                rep.err(
                    f"ground_truth.json[{cid}].{var}.commit key missing"
                )

            for f in ("source_archive_sha256", "source_tree_sha256"):
                if not sec.get(f):
                    rep.err(
                        f"ground_truth.json[{cid}].{var}.{f} missing"
                    )


def check_selection(admitted: set[str], rep: Reporter) -> None:
    rows = load_tsv(SELECTION_TSV)
    selected_ids = {
        r.get("case_id")
        for r in rows
        if (r.get("selection_status") or "").strip() == "selected_candidate"
    }
    for cid in sorted(admitted):
        if cid not in selected_ids:
            rep.err(
                f"candidate_selection.tsv: {cid} not marked "
                f"selected_candidate"
            )


def check_summary(admitted: set[str], rep: Reporter) -> None:
    if not SELECTION_SUMMARY_JSON.exists():
        rep.err("selection_summary.json missing")
        return
    s = load_json(SELECTION_SUMMARY_JSON)
    if not isinstance(s, dict):
        rep.err("selection_summary.json: expected object")
        return

    expected_admitted = len(admitted)
    expected_subjects = 2 * expected_admitted

    if s.get("admitted_cases") != expected_admitted:
        rep.err(
            f"selection_summary.admitted_cases="
            f"{s.get('admitted_cases')} expected {expected_admitted}"
        )
    if s.get("subject_rows") != expected_subjects:
        rep.err(
            f"selection_summary.subject_rows="
            f"{s.get('subject_rows')} expected {expected_subjects}"
        )
    if s.get("accuracy_ready") is not False:
        rep.err("selection_summary.accuracy_ready must be false")


def check_expected_capabilities(admitted: set[str], rep: Reporter) -> None:
    rows = load_tsv(EXPECTED_CAPABILITIES_TSV)
    seen = {r.get("case_id") for r in rows if r.get("case_id")}
    for cid in sorted(admitted):
        if cid not in seen:
            rep.warn(f"expected_capabilities.tsv: no entry for {cid}")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--root",
        default=None,
        help="override benchmark root (default: dir of this script)",
    )
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args(argv)

    global HERE, CASES_DIR, SUBJECTS_TSV, GROUND_TRUTH_JSON
    global SELECTION_TSV, EXPECTED_CAPABILITIES_TSV, SELECTION_SUMMARY_JSON

    if args.root:
        HERE = Path(args.root).resolve()
        CASES_DIR = HERE / "cases"
        SUBJECTS_TSV = HERE / "subjects.tsv"
        GROUND_TRUTH_JSON = HERE / "ground_truth.json"
        SELECTION_TSV = HERE / "candidate_selection.tsv"
        EXPECTED_CAPABILITIES_TSV = HERE / "expected_capabilities.tsv"
        SELECTION_SUMMARY_JSON = HERE / "selection_summary.json"

    rep = Reporter()

    cases = load_cases(rep)
    admitted_set: set[str] = set()
    for rec in cases:
        check_case_contract(rec, rep)
        if rec.get("materialization_status") == "admitted":
            admitted_set.add(rec["case_id"])

    check_no_build_artifacts(rep)
    check_subjects(admitted_set, rep)
    check_ground_truth(admitted_set, rep)
    check_selection(admitted_set, rep)
    check_summary(admitted_set, rep)
    check_expected_capabilities(admitted_set, rep)

    if not args.quiet:
        print(f"cases_total     = {len(cases)}")
        print(f"cases_admitted  = {len(admitted_set)}")
        for cid in sorted(admitted_set):
            print(f"  ADMITTED {cid}")
        for w in rep.warnings:
            print(f"WARN: {w}")

    if rep.ok():
        print("VERIFY_B1_2: PASS")
        return 0

    for e in rep.errors:
        print(f"FAIL: {e}", file=sys.stderr)
    print(
        f"VERIFY_B1_2: FAIL ({len(rep.errors)} errors)",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
