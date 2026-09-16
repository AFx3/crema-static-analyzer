#!/usr/bin/env python3
from pathlib import Path
import argparse
import json

parser = argparse.ArgumentParser()
parser.add_argument(
    "mode",
    choices=["leak", "roundtrip", "raw_drop"],
)
parser.add_argument("artifact")
args = parser.parse_args()

document = json.loads(
    Path(args.artifact).read_text(encoding="utf-8")
)

rows = [
    (node["id"], record)
    for node in document.get("nodes", [])
    for record in node.get("allocation_disposition", [])
]

errors = []

def records(kind):
    return [
        (node_id, record)
        for node_id, record in rows
        if record.get("kind") == kind
    ]

expected = {
    "box_into_raw": (
        "preserve_manual_obligation",
        "rustc_box_into_raw_v1",
    ),
    "box_from_raw": (
        "restore_raii_obligation",
        "rustc_box_from_raw_v1",
    ),
    "raw_pointer_drop_noop": (
        "no_pointee_lifecycle_effect",
        "rustc_mem_drop_raw_pointer_v1",
    ),
}

for kind, (effect, basis) in expected.items():
    for node_id, record in records(kind):
        if record.get("certainty") != "may_abstract":
            errors.append(
                f"{kind}@{node_id}: "
                f"certainty={record.get('certainty')!r}"
            )

        if record.get("obligation_effect") != effect:
            errors.append(
                f"{kind}@{node_id}: "
                f"obligation_effect={record.get('obligation_effect')!r}, "
                f"expected={effect!r}"
            )

        if record.get("basis") != basis:
            errors.append(
                f"{kind}@{node_id}: "
                f"basis={record.get('basis')!r}, "
                f"expected={basis!r}"
            )

if args.mode == "leak":
    into = records("box_into_raw")
    back = records("box_from_raw")

    if len(into) != 1:
        errors.append(
            f"expected exactly one box_into_raw, got {len(into)}"
        )

    if back:
        errors.append(
            f"expected zero box_from_raw, got {len(back)}"
        )

elif args.mode == "roundtrip":
    into = records("box_into_raw")
    back = records("box_from_raw")

    if len(into) != 1:
        errors.append(
            f"expected exactly one box_into_raw, got {len(into)}"
        )

    if not back:
        errors.append("box_from_raw missing")

    if into and back:
        into_allocations = {
            record["allocation"]
            for _, record in into
        }
        back_allocations = {
            record["allocation"]
            for _, record in back
        }

        if into_allocations != back_allocations:
            errors.append(
                "Box::from_raw does not restore the same AbstractAllocId: "
                f"into={sorted(into_allocations)!r} "
                f"from={sorted(back_allocations)!r}"
            )

else:
    noops = records("raw_pointer_drop_noop")
    node_map = {
        node["id"]: node
        for node in document.get("nodes", [])
    }

    if not noops:
        errors.append("raw_pointer_drop_noop missing")

    for node_id, record in noops:
        allocation = record["allocation"]
        node = node_map.get(node_id)

        if node is None:
            errors.append(
                f"node missing for raw_pointer_drop_noop: {node_id}"
            )
            continue

        if any(
            label.get("predicate") == "drop"
            and label.get("allocation") == allocation
            for label in node.get("allocation_labels", [])
        ):
            errors.append(
                f"{node_id}: raw-pointer noop also exported "
                f"drop for allocation {allocation}"
            )

        if any(
            item.get("kind") == "may_deallocate"
            and item.get("allocation") == allocation
            for item in node.get("allocation_disposition", [])
        ):
            errors.append(
                f"{node_id}: raw-pointer noop also exported "
                f"may_deallocate for allocation {allocation}"
            )

print("mode                =", args.mode)
print("disposition_records =", len(rows))
print("errors              =", len(errors))

for error in errors:
    print("ERROR:", error)

if errors:
    print("V6U_A2_FOCUSED_ARTIFACT: FAIL")
    raise SystemExit(1)

print("V6U_A2_FOCUSED_ARTIFACT: PASS")
