#!/usr/bin/env python3
from pathlib import Path
import argparse
import csv
import json

parser = argparse.ArgumentParser()
parser.add_argument("--baseline", required=True)
parser.add_argument("--candidate", required=True)
parser.add_argument("--out", required=True)
args = parser.parse_args()

baseline = Path(args.baseline).resolve()
candidate = Path(args.candidate).resolve()
errors = []


def load_matrix(root):
    path = root / "query-matrix" / "query-results-long.tsv"

    with path.open(
        newline="",
        encoding="utf-8",
    ) as handle:
        rows = list(
            csv.DictReader(
                handle,
                delimiter="\t",
            )
        )

    mapping = {
        (
            row["group"],
            row["target"],
            row["query"],
        ): (
            row["result"],
            int(row["rc"]),
        )
        for row in rows
    }

    return rows, mapping


base_rows, base_matrix = load_matrix(baseline)
cand_rows, cand_matrix = load_matrix(candidate)

base_matrix_keys = set(base_matrix)
cand_matrix_keys = set(cand_matrix)

missing_matrix_keys = sorted(
    base_matrix_keys - cand_matrix_keys
)
extra_matrix_keys = sorted(
    cand_matrix_keys - base_matrix_keys
)

truth_rc_mismatches = [
    key
    for key in sorted(
        base_matrix_keys & cand_matrix_keys
    )
    if base_matrix[key] != cand_matrix[key]
]

if missing_matrix_keys:
    errors.append(
        f"matrix missing keys={len(missing_matrix_keys)}"
    )

if extra_matrix_keys:
    errors.append(
        f"matrix extra keys={len(extra_matrix_keys)}"
    )

if truth_rc_mismatches:
    errors.append(
        f"truth/rc mismatches={len(truth_rc_mismatches)}"
    )


def load_subjects(root):
    result = {}

    with (root / "subjects.tsv").open(
        encoding="utf-8"
    ) as handle:
        for raw in handle:
            raw = raw.rstrip("\n")

            if not raw:
                continue

            group, target, artifact = raw.split(
                "\t",
                2,
            )

            result[(group, target)] = Path(
                artifact
            )

    return result


base_subjects = load_subjects(baseline)
cand_subjects = load_subjects(candidate)

base_subject_keys = set(base_subjects)
cand_subject_keys = set(cand_subjects)

missing_subjects = sorted(
    base_subject_keys - cand_subject_keys
)
extra_subjects = sorted(
    cand_subject_keys - base_subject_keys
)

if missing_subjects:
    errors.append(
        f"subject missing keys={len(missing_subjects)}"
    )

if extra_subjects:
    errors.append(
        f"subject extra keys={len(extra_subjects)}"
    )


def normalized_dispositions(path):
    document = json.loads(
        path.read_text(encoding="utf-8")
    )

    rows = []

    for node in document.get("nodes", []):
        for record in node.get(
            "allocation_disposition",
            [],
        ):
            rows.append((
                node["id"],
                json.dumps(
                    record,
                    sort_keys=True,
                    separators=(",", ":"),
                ),
            ))

    return sorted(rows)


disposition_subject_mismatches = []
base_disposition_records = 0
cand_disposition_records = 0

for key in sorted(
    base_subject_keys & cand_subject_keys
):
    base_value = normalized_dispositions(
        base_subjects[key]
    )
    cand_value = normalized_dispositions(
        cand_subjects[key]
    )

    base_disposition_records += len(
        base_value
    )
    cand_disposition_records += len(
        cand_value
    )

    if base_value != cand_value:
        disposition_subject_mismatches.append(
            key
        )

if disposition_subject_mismatches:
    errors.append(
        "allocation_disposition subject mismatches="
        f"{len(disposition_subject_mismatches)}"
    )


def load_unknown_summary(root):
    path = (
        root
        / "query-matrix"
        / "unknown-explanations-summary.json"
    )

    return json.loads(
        path.read_text(encoding="utf-8")
    )


base_unknown = load_unknown_summary(
    baseline
)
cand_unknown = load_unknown_summary(
    candidate
)

for name, summary in (
    ("baseline", base_unknown),
    ("candidate", cand_unknown),
):
    unknown_results = summary.get(
        "unknown_results"
    )
    explanations_generated = summary.get(
        "explanations_generated"
    )
    complete = summary.get("complete")

    if unknown_results != 468:
        errors.append(
            f"{name} unknown_results="
            f"{unknown_results!r}, expected 468"
        )

    if explanations_generated != unknown_results:
        errors.append(
            f"{name} explanation closure mismatch: "
            f"unknown_results={unknown_results!r} "
            f"explanations_generated="
            f"{explanations_generated!r}"
        )

    if complete is not True:
        errors.append(
            f"{name} unknown closure "
            f"complete={complete!r}"
        )


def explanation_sidecars(root):
    query_matrix_root = (
        root
        / "query-matrix"
        / "results"
    )
    corpus_root = (
        root
        / "corpus"
        / "raw"
    )

    matrix = {
        path.relative_to(root).as_posix():
            path
        for path in query_matrix_root.rglob(
            "*.explain.json"
        )
    }

    corpus = {
        path.relative_to(root).as_posix():
            path
        for path in corpus_root.rglob(
            "*.explain.json"
        )
    }

    total = dict(matrix)
    total.update(corpus)

    return matrix, corpus, total


(
    base_matrix_explain,
    base_corpus_explain,
    base_total_explain,
) = explanation_sidecars(baseline)

(
    cand_matrix_explain,
    cand_corpus_explain,
    cand_total_explain,
) = explanation_sidecars(candidate)


def compare_json_map(
    label,
    baseline_map,
    candidate_map,
):
    baseline_keys = set(baseline_map)
    candidate_keys = set(candidate_map)

    missing = sorted(
        baseline_keys - candidate_keys
    )
    extra = sorted(
        candidate_keys - baseline_keys
    )

    mismatches = []

    for key in sorted(
        baseline_keys & candidate_keys
    ):
        base_value = json.loads(
            baseline_map[key].read_text(
                encoding="utf-8"
            )
        )
        cand_value = json.loads(
            candidate_map[key].read_text(
                encoding="utf-8"
            )
        )

        if base_value != cand_value:
            mismatches.append(key)

    if missing:
        errors.append(
            f"{label} missing={len(missing)}"
        )

    if extra:
        errors.append(
            f"{label} extra={len(extra)}"
        )

    if mismatches:
        errors.append(
            f"{label} mismatches="
            f"{len(mismatches)}"
        )

    return missing, extra, mismatches


(
    matrix_explain_missing,
    matrix_explain_extra,
    matrix_explain_mismatches,
) = compare_json_map(
    "query-matrix explanation sidecars",
    base_matrix_explain,
    cand_matrix_explain,
)

(
    corpus_explain_missing,
    corpus_explain_extra,
    corpus_explain_mismatches,
) = compare_json_map(
    "corpus explanation sidecars",
    base_corpus_explain,
    cand_corpus_explain,
)

(
    total_explain_missing,
    total_explain_extra,
    total_explain_mismatches,
) = compare_json_map(
    "total explanation sidecars",
    base_total_explain,
    cand_total_explain,
)


base_unknown_results = base_unknown.get(
    "unknown_results"
)
cand_unknown_results = cand_unknown.get(
    "unknown_results"
)

if len(base_matrix_explain) != base_unknown_results:
    errors.append(
        "baseline query-matrix sidecar count "
        f"{len(base_matrix_explain)} != "
        f"unknown_results {base_unknown_results}"
    )

if len(cand_matrix_explain) != cand_unknown_results:
    errors.append(
        "candidate query-matrix sidecar count "
        f"{len(cand_matrix_explain)} != "
        f"unknown_results {cand_unknown_results}"
    )


summary = {
    "baseline_subjects":
        len(base_subjects),
    "candidate_subjects":
        len(cand_subjects),

    "baseline_attempts":
        len(base_rows),
    "candidate_attempts":
        len(cand_rows),

    "missing_matrix_keys":
        len(missing_matrix_keys),
    "extra_matrix_keys":
        len(extra_matrix_keys),
    "truth_rc_mismatches":
        len(truth_rc_mismatches),

    "missing_subjects":
        len(missing_subjects),
    "extra_subjects":
        len(extra_subjects),

    "baseline_disposition_records":
        base_disposition_records,
    "candidate_disposition_records":
        cand_disposition_records,
    "disposition_subject_mismatches":
        len(
            disposition_subject_mismatches
        ),

    "baseline_unknown_results":
        base_unknown_results,
    "candidate_unknown_results":
        cand_unknown_results,
    "baseline_explanations_generated":
        base_unknown.get(
            "explanations_generated"
        ),
    "candidate_explanations_generated":
        cand_unknown.get(
            "explanations_generated"
        ),
    "baseline_unknown_complete":
        base_unknown.get("complete"),
    "candidate_unknown_complete":
        cand_unknown.get("complete"),

    "baseline_matrix_unknown_sidecars":
        len(base_matrix_explain),
    "candidate_matrix_unknown_sidecars":
        len(cand_matrix_explain),
    "matrix_unknown_sidecar_mismatches":
        len(matrix_explain_mismatches),

    "baseline_corpus_explain_sidecars":
        len(base_corpus_explain),
    "candidate_corpus_explain_sidecars":
        len(cand_corpus_explain),
    "corpus_explain_sidecar_mismatches":
        len(corpus_explain_mismatches),

    "baseline_total_explain_sidecars":
        len(base_total_explain),
    "candidate_total_explain_sidecars":
        len(cand_total_explain),
    "total_explain_sidecar_mismatches":
        len(total_explain_mismatches),

    "errors": errors,
}

Path(args.out).write_text(
    json.dumps(
        summary,
        indent=2,
        sort_keys=True,
    ) + "\n",
    encoding="utf-8",
)

print(json.dumps(
    summary,
    indent=2,
    sort_keys=True,
))

for key in truth_rc_mismatches[:20]:
    print(
        "TRUTH_MISMATCH",
        key,
        base_matrix[key],
        cand_matrix[key],
    )

for key in (
    disposition_subject_mismatches[
        :20
    ]
):
    print(
        "DISPOSITION_MISMATCH",
        key,
    )

for key in (
    matrix_explain_mismatches[:20]
):
    print(
        "MATRIX_UNKNOWN_SIDECAR_MISMATCH",
        key,
    )

for key in (
    corpus_explain_mismatches[:20]
):
    print(
        "CORPUS_EXPLAIN_SIDECAR_MISMATCH",
        key,
    )


ok = (
    len(base_subjects) == 112
    and len(cand_subjects) == 112

    and len(base_rows) == 1344
    and len(cand_rows) == 1344

    and base_disposition_records == 392
    and cand_disposition_records == 392

    and base_unknown_results == 468
    and cand_unknown_results == 468

    and len(base_matrix_explain) == 468
    and len(cand_matrix_explain) == 468

    and base_unknown.get(
        "explanations_generated"
    ) == 468
    and cand_unknown.get(
        "explanations_generated"
    ) == 468

    and base_unknown.get(
        "complete"
    ) is True
    and cand_unknown.get(
        "complete"
    ) is True

    and len(base_total_explain)
        == len(cand_total_explain)

    and not errors
)

print(
    "V6U_A2_EQUIVALENCE:",
    "PASS" if ok else "FAIL",
)

raise SystemExit(
    0 if ok else 1
)
