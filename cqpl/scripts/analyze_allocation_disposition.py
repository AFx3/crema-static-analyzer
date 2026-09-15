#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Summarize v6S-r1 allocation_disposition_v1 MAY observations."
    )
    p.add_argument("--subjects", required=True, type=Path,
                   help="TSV: group, target, annotated_icfg_v2.json")
    p.add_argument("--baseline-wide", required=True, type=Path,
                   help="Frozen 112x12 query-results-wide.tsv used only to select leak_alloc=unk subjects")
    p.add_argument("--out", required=True, type=Path)
    return p.parse_args()


def load_baseline(path: Path) -> dict[tuple[str, str], dict[str, str]]:
    with path.open(newline="") as f:
        rows = list(csv.DictReader(f, delimiter="\t"))
    out: dict[tuple[str, str], dict[str, str]] = {}
    for row in rows:
        key = (row["group"], row["target"])
        if key in out:
            raise SystemExit(f"duplicate baseline row: {key}")
        out[key] = row
    if len(out) != 112:
        raise SystemExit(f"expected 112 baseline rows, found {len(out)}")
    return out


def load_subjects(path: Path) -> list[tuple[str, str, Path]]:
    rows: list[tuple[str, str, Path]] = []
    for lineno, raw in enumerate(path.read_text().splitlines(), 1):
        if not raw.strip() or raw.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) != 3:
            raise SystemExit(f"bad subjects row {lineno}: {raw!r}")
        group, target, graph = parts
        rows.append((group, target, Path(graph)))
    if len(rows) != 112:
        raise SystemExit(f"expected 112 subjects, found {len(rows)}")
    return rows


def subject_summary(graph: Path) -> dict:
    data = json.loads(graph.read_text())
    caps = set(data.get("capabilities", []))
    if data.get("schema_version") != 2:
        raise SystemExit(f"not schema v2: {graph}")
    if "allocation_disposition_v1" not in caps:
        raise SystemExit(f"missing allocation_disposition_v1: {graph}")

    records = []
    for node in data.get("nodes", []):
        if "allocation_disposition" not in node:
            raise SystemExit(f"capability present but node lacks allocation_disposition: {graph}: {node.get('id')}")
        for rec in node.get("allocation_disposition", []):
            x = dict(rec)
            x["node"] = node.get("id")
            records.append(x)

    kinds = sorted({r["kind"] for r in records})
    return {
        "records": records,
        "kinds": kinds,
        "signature": ";".join(kinds) if kinds else "<none>",
    }


def fixture_invariants(target: str, graph: Path, summary: dict) -> dict:
    records = summary["records"]
    kinds = Counter(r["kind"] for r in records)
    result = {
        "target": target,
        "graph": str(graph),
        "kind_counts": dict(sorted(kinds.items())),
        "checks": {},
    }

    if target == "boxed_bool__ml":
        result["checks"] = {
            "has_box_into_raw": kinds["box_into_raw"] > 0,
            "has_no_box_from_raw": kinds["box_from_raw"] == 0,
        }
    elif target == "clean_into_from_raw":
        result["checks"] = {
            "has_box_into_raw": kinds["box_into_raw"] > 0,
            "has_box_from_raw": kinds["box_from_raw"] > 0,
        }
    elif target == "drop_raw_ptr_no_free":
        noops = [r for r in records if r["kind"] == "raw_pointer_drop_noop"]
        nodes_by_id = {n["id"]: n for n in json.loads(graph.read_text()).get("nodes", [])}
        bad_drop_labels = []
        bad_freed = []
        bad_may_dealloc_same_node = []
        for r in noops:
            node = nodes_by_id[r["node"]]
            alloc = r["allocation"]
            if any(l.get("predicate") == "drop" and l.get("allocation") == alloc
                   for l in node.get("allocation_labels", [])):
                bad_drop_labels.append(r["node"])
            if any(d.get("kind") == "may_deallocate" and d.get("allocation") == alloc
                   for d in node.get("allocation_disposition", [])):
                bad_may_dealloc_same_node.append(r["node"])
            for cell in (node.get("allocation_post") or {}).get("cells", []):
                if cell.get("allocation") == alloc and cell.get("value") == "FREED":
                    bad_freed.append(r["node"])
        result["checks"] = {
            "has_raw_pointer_drop_noop": bool(noops),
            "raw_drop_nodes_without_drop_l": not bad_drop_labels,
            "raw_drop_nodes_without_may_deallocate": not bad_may_dealloc_same_node,
            "raw_drop_nodes_pointee_not_freed": not bad_freed,
        }
        result["raw_drop_nodes"] = sorted({r["node"] for r in noops})
        result["bad_drop_label_nodes"] = sorted(set(bad_drop_labels))
        result["bad_freed_nodes"] = sorted(set(bad_freed))
        result["bad_may_deallocate_nodes"] = sorted(set(bad_may_dealloc_same_node))
    return result


def main() -> int:
    args = parse_args()
    baseline = load_baseline(args.baseline_wide)
    subjects = load_subjects(args.subjects)

    all_kind_presence = Counter()
    all_signatures = Counter()
    record_counts = Counter()
    leak_kind_presence = Counter()
    leak_signatures = Counter()
    fixtures = {}
    subject_rows = []

    for group, target, graph in subjects:
        if not graph.is_file():
            raise SystemExit(f"missing graph: {graph}")
        key = (group, target)
        if key not in baseline:
            raise SystemExit(f"subject missing from frozen baseline: {key}")
        ss = subject_summary(graph)
        for kind in ss["kinds"]:
            all_kind_presence[kind] += 1
        for rec in ss["records"]:
            record_counts[rec["kind"]] += 1
        all_signatures[ss["signature"]] += 1

        leak_unknown = baseline[key]["leak_alloc"] == "unk"
        if leak_unknown:
            for kind in ss["kinds"]:
                leak_kind_presence[kind] += 1
            leak_signatures[ss["signature"]] += 1

        if target in {"boxed_bool__ml", "clean_into_from_raw", "drop_raw_ptr_no_free"}:
            fixtures[target] = fixture_invariants(target, graph, ss)

        subject_rows.append({
            "group": group,
            "target": target,
            "leak_alloc_baseline": baseline[key]["leak_alloc"],
            "record_count": len(ss["records"]),
            "signature": ss["signature"],
        })

    leak_unknown_count = sum(1 for r in baseline.values() if r["leak_alloc"] == "unk")
    if leak_unknown_count != 105:
        raise SystemExit(f"frozen leak_alloc unknown count changed: {leak_unknown_count}")

    required_fixtures = {"boxed_bool__ml", "clean_into_from_raw", "drop_raw_ptr_no_free"}
    missing = sorted(required_fixtures - set(fixtures))
    if missing:
        raise SystemExit(f"missing required fixture subjects: {missing}")

    out = {
        "schema": "cqpl_v6s_r1_allocation_disposition_summary_v1",
        "scientific_note": (
            "All v6S-r1 disposition records are MAY observations. Presence counts overlap; "
            "signature_counts partition subjects by the set of observed kinds. Missing records "
            "are not interpreted as MUST-negative evidence."
        ),
        "subjects": 112,
        "leak_alloc_unknown_baseline": leak_unknown_count,
        "record_counts": dict(sorted(record_counts.items())),
        "subject_kind_presence": dict(sorted(all_kind_presence.items())),
        "subject_signature_counts": dict(sorted(all_signatures.items())),
        "leak_unknown_kind_presence": dict(sorted(leak_kind_presence.items())),
        "leak_unknown_signature_counts": dict(sorted(leak_signatures.items())),
        "fixtures": fixtures,
        "subjects_detail": subject_rows,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print("V6S_R1_DISPOSITION_ANALYSIS: PASS")
    print(f"subjects=112 leak_alloc_unknown={leak_unknown_count}")
    print("fixture_checks=" + ",".join(
        f"{name}:{'PASS' if all(x['checks'].values()) else 'FAIL'}"
        for name, x in sorted(fixtures.items())
    ))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
