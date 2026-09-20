#!/usr/bin/env python3
"""W1 source-grounded diagnostic witness acceptance gate.

The gate is deliberately split in two dimensions:
  1. semantic neutrality against a frozen pre-W1 matrix;
  2. local certificate soundness against the exact annotated ICFG that was checked.

It never treats a diagnostic certificate as a proof of a concrete execution.
"""
from __future__ import annotations

import argparse
import csv
import json
from collections import Counter
from pathlib import Path
from typing import Any


def tsv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def index(rows: list[dict[str, str]], fields: tuple[str, ...]) -> dict[tuple[str, ...], dict[str, str]]:
    out: dict[tuple[str, ...], dict[str, str]] = {}
    for row in rows:
        key = tuple(row[field] for field in fields)
        if key in out:
            raise SystemExit(f"duplicate key {key}")
        out[key] = row
    return out


def status_for(anchors: list[dict[str, Any]], *, synthetic: bool = False) -> str:
    if synthetic:
        return "synthetic_no_source_anchor"
    if not anchors:
        return "source_unavailable"
    if len(anchors) == 1:
        return "grounded"
    return "multiple_candidate_anchors"


def freeze(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def artifact_index(artifact: dict[str, Any]) -> dict[str, Any]:
    nodes = {node["id"]: node for node in artifact["nodes"]}
    allocations = {allocation["id"]: allocation for allocation in artifact.get("allocations", [])}
    typed: dict[tuple[str, str], list[str]] = {}
    for edge in artifact.get("typed_edges", []):
        typed.setdefault((edge["source"], edge["destination"]), []).append(edge["flow"])
    for flows in typed.values():
        flows[:] = sorted(set(flows))
    return {"nodes": nodes, "allocations": allocations, "typed": typed}


def node_anchors(node: dict[str, Any]) -> list[dict[str, Any]]:
    return list(node.get("source_provenance", {}).get("anchors", []))


def event_anchors(node: dict[str, Any], allocation: str, predicate: str) -> list[dict[str, Any]]:
    matches = [
        record for record in node.get("source_provenance", {}).get("allocation_events", [])
        if record.get("allocation") == allocation and record.get("predicate") == predicate
    ]
    if len(matches) > 1:
        raise AssertionError(f"duplicate source provenance for {predicate}({allocation}) at {node.get('id')}")
    return [] if not matches else matches[0].get("anchors", [])


def assert_anchor_subset(anchors: list[dict[str, Any]], universe: list[dict[str, Any]], where: str) -> None:
    allowed = {freeze(anchor) for anchor in universe}
    for anchor in anchors:
        assert freeze(anchor) in allowed, f"{where}: certificate anchor is not producer provenance"


def validate_event(event: dict[str, Any], allocation: str, nodes: dict[str, Any], where: str) -> None:
    node_id = event["node"]
    assert node_id in nodes, f"{where}: event node {node_id!r} not in projected artifact"
    node = nodes[node_id]
    anchors = event.get("source_anchors", [])
    observed = event.get("observed_predicates", [])
    if observed:
        expected: list[dict[str, Any]] = []
        for predicate in observed:
            expected.extend(event_anchors(node, allocation, predicate))
        expected_map = {freeze(anchor): anchor for anchor in expected}
        expected = [expected_map[key] for key in sorted(expected_map)]
        assert {freeze(x) for x in anchors} == {freeze(x) for x in expected}, (
            f"{where}: event anchors do not equal producer allocation-event provenance"
        )
    else:
        assert_anchor_subset(anchors, node_anchors(node), where)
    assert event["source_status"] == status_for(anchors), f"{where}: inconsistent source_status"


def validate_certificate(
    cert: dict[str, Any],
    report: dict[str, Any],
    artifact: dict[str, Any],
    where: str,
) -> None:
    assert cert.get("schema") == "query_witness_certificate_v1", f"{where}: wrong certificate schema"
    idx = artifact_index(artifact)
    nodes: dict[str, Any] = idx["nodes"]
    allocations: dict[str, Any] = idx["allocations"]
    typed: dict[tuple[str, str], list[str]] = idx["typed"]
    capabilities = set(artifact.get("capabilities", []))

    allocation = cert["allocation"]
    alloc_id = allocation["abstract_alloc_id"]
    assert alloc_id in allocations, f"{where}: unknown allocation {alloc_id!r}"
    producer_allocation = allocations[alloc_id]
    assert allocation.get("site") == producer_allocation.get("site"), f"{where}: allocation site drift"
    assert allocation.get("context", []) == producer_allocation.get("context", []), f"{where}: allocation context drift"

    site = producer_allocation.get("site") or {}
    site_kind = site.get("kind")
    canonical_node = site.get("node_id") if site_kind in {"rust_call", "c_call"} else None
    assert allocation.get("node") == canonical_node, (
        f"{where}: certificate allocation.node is not AbstractAllocId.site.node_id"
    )
    alloc_anchors = allocation.get("source_anchors", [])
    if site_kind == "synthetic":
        assert not alloc_anchors, f"{where}: synthetic allocation must not fabricate source anchors"
        assert allocation["source_status"] == "synthetic_no_source_anchor"
    else:
        expected_alloc_anchors: list[dict[str, Any]] = []
        if canonical_node in nodes:
            expected_alloc_anchors = event_anchors(nodes[canonical_node], alloc_id, "alloc")
        assert {freeze(x) for x in alloc_anchors} == {freeze(x) for x in expected_alloc_anchors}, (
            f"{where}: allocation source anchors drift from alloc-event provenance"
        )
        assert allocation["source_status"] == status_for(alloc_anchors), f"{where}: bad allocation source_status"

    if cert.get("witness_entry") is not None:
        validate_event(cert["witness_entry"], alloc_id, nodes, where + ":witness_entry")
    for i, event in enumerate(cert.get("events", [])):
        validate_event(event, alloc_id, nodes, f"{where}:events[{i}]")

    witness = cert["abstract_witness"]
    assert witness.get("model") == "annotated_abstract_icfg", f"{where}: wrong witness model"
    assert witness.get("concrete_execution") is False, f"{where}: abstract witness mislabeled concrete"
    path = witness.get("nodes", [])
    edges = witness.get("edges", [])
    assert len(edges) == max(0, len(path) - 1), f"{where}: path/edge cardinality mismatch"
    for i, (src, dst) in enumerate(zip(path, path[1:])):
        assert src in nodes and dst in nodes, f"{where}: witness node missing"
        assert dst in nodes[src].get("successors", []), f"{where}: non-ICFG witness edge {src}->{dst}"
        edge = edges[i]
        assert edge.get("source") == src and edge.get("destination") == dst, f"{where}: edge/path order drift"
        if "typed_edge_flow_v1" in capabilities:
            assert edge.get("basis") == "typed_edge_flow_v1", f"{where}: missing typed edge basis"
            assert sorted(set(edge.get("flows", []))) == typed.get((src, dst), []), f"{where}: typed flow drift"
            if cert.get("assessment_scope") == "normal_execution":
                assert "normal" in edge.get("flows", []), f"{where}: normal_execution uses non-normal edge"
        else:
            assert edge.get("basis") == "legacy_successor_relation", f"{where}: unexpected typed basis"

    report_reasons = report.get("reason_frontier", [])
    cert_reasons = [entry["reason"] for entry in cert.get("uncertainty_frontier", [])]
    assert cert_reasons == report_reasons, f"{where}: uncertainty frontier drift"


def finding_signature(finding: dict[str, Any]) -> tuple[Any, ...]:
    return (
        finding.get("kind"), finding.get("allocation"), finding.get("query_result"),
        tuple(finding.get("witness_path", [])),
        finding.get("origin_node") or (finding.get("witness_path") or [None])[0],
        finding.get("handoff_node"), finding.get("return_node"), finding.get("first_drop_node"),
        finding.get("second_drop_node"), finding.get("use_node"), finding.get("mismatch_node"),
    )


def certificate_signature(cert: dict[str, Any]) -> tuple[Any, ...]:
    role_to_field = {
        "ownership_handoff": "handoff_node", "normal_return": "return_node",
        "first_deallocation": "first_drop_node", "second_deallocation": "second_drop_node",
        "use_after_deallocation": "use_node", "mismatch_deallocation": "mismatch_node",
    }
    fields: dict[str, Any] = {name: None for name in role_to_field.values()}
    for event in cert.get("events", []):
        field = role_to_field.get(event.get("role"))
        if field and fields[field] is None:
            fields[field] = event.get("node")
    return (
        cert.get("finding_kind"), cert.get("allocation", {}).get("abstract_alloc_id"), cert.get("query_result"),
        tuple(cert.get("abstract_witness", {}).get("nodes", [])),
        cert.get("witness_entry", {}).get("node") if cert.get("witness_entry") else None,
        fields["handoff_node"], fields["return_node"], fields["first_drop_node"],
        fields["second_drop_node"], fields["use_node"], fields["mismatch_node"],
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--truth-baseline", type=Path, required=True, help="frozen pre-W1 truth-matrix artifact directory")
    ap.add_argument("--assessment-baseline", type=Path, required=True, help="frozen pre-W1 assessment artifact directory")
    ap.add_argument("--matrix", type=Path, required=True, help="fresh W1 query matrix directory")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--expected-attempts", type=int, default=1416)
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    base_long = index(tsv(args.truth_baseline / "QUERY_RESULTS_LONG.tsv"), ("group", "target", "query"))
    new_long = index(tsv(args.matrix / "query-results-long.tsv"), ("group", "target", "query"))
    if set(base_long) != set(new_long):
        raise SystemExit("W1 matrix key set differs from frozen baseline")
    truth_deltas = [
        (*key, base_long[key]["result"], new_long[key]["result"])
        for key in sorted(base_long)
        if base_long[key]["result"] != new_long[key]["result"]
    ]

    base_unk = index(tsv(args.assessment_baseline / "UNKNOWN_EXPLANATIONS.tsv"), ("group", "target", "query"))
    new_unk = index(tsv(args.matrix / "unknown-explanations.tsv"), ("group", "target", "query"))
    assessment_deltas: list[tuple[Any, ...]] = []
    if set(base_unk) != set(new_unk):
        assessment_deltas.append(("unknown_key_set", len(base_unk), len(new_unk)))
    for key in sorted(set(base_unk) & set(new_unk)):
        before = tuple(base_unk[key][field] for field in ("subresult", "direction", "strength"))
        after = tuple(new_unk[key][field] for field in ("subresult", "direction", "strength"))
        if before != after:
            assessment_deltas.append((*key, *before, *after))

    failures: list[dict[str, str]] = []
    certificate_count = 0
    finding_count = 0
    directional_certificate_count = 0
    non_directional_certificate_count = 0
    source_capability_reports = 0
    status_counts: Counter[str] = Counter()

    for key, row in sorted(new_unk.items()):
        explain_path = args.matrix / row["explanation"]
        artifact_path = Path(row["artifact"])
        try:
            report = json.loads(explain_path.read_text(encoding="utf-8"))
            artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
            has_source = "source_provenance_v1" in artifact.get("capabilities", [])
            diag_flag = report.get("diagnostics", {}).get("source_provenance_capability_present", False)
            assert diag_flag == has_source, "report capability diagnostic disagrees with artifact"
            findings = report.get("supporting_findings", []) + report.get("refuting_findings", [])
            certs = report.get("diagnostic_certificates", [])
            finding_count += len(findings)
            certificate_count += len(certs)
            if has_source:
                source_capability_reports += 1
                assert len(certs) == len(findings), "one certificate per directional finding is required"
                finding_sigs = Counter(finding_signature(f) for f in findings)
                cert_sigs = Counter(certificate_signature(c) for c in certs)
                # The historical origin_node is a witness-entry notion; this is
                # intentionally compared to certificate.witness_entry, not allocation.node.
                assert finding_sigs == cert_sigs, "certificate/findings projection mismatch"
                for i, cert in enumerate(certs):
                    validate_certificate(cert, report, artifact, f"{key}:certificate[{i}]")
                    if cert.get("direction") == "none":
                        non_directional_certificate_count += 1
                    else:
                        directional_certificate_count += 1
                    status_counts[cert["allocation"]["source_status"]] += 1
                    for event in cert.get("events", []):
                        status_counts[event["source_status"]] += 1
            else:
                assert not certs, "historical artifact without source capability emitted W1 certificate"
        except Exception as exc:  # gate must preserve exact failing subject
            failures.append({"group": key[0], "target": key[1], "query": key[2], "error": str(exc)})

    summary = {
        "schema": "cqpl_gate_w1_diagnostic_witness_v1",
        "attempts": len(new_long),
        "truth_delta_count": len(truth_deltas),
        "assessment_delta_count": len(assessment_deltas),
        "source_capability_unknown_reports": source_capability_reports,
        # Every supporting/refuting finding is certifiable, but unresolved-contract
        # candidates deliberately carry direction=none and must not be counted as
        # directional evidence.  Keep the partition explicit for paper-grade metrics.
        "certifiable_findings": finding_count,
        "directional_findings": directional_certificate_count,
        "non_directional_findings": non_directional_certificate_count,
        "diagnostic_certificates": certificate_count,
        "directional_certificates": directional_certificate_count,
        "non_directional_certificates": non_directional_certificate_count,
        "source_status_counts": dict(sorted(status_counts.items())),
        "certificate_validation_failures": failures,
        "criteria": {
            "truth_delta_0": not truth_deltas and len(new_long) == args.expected_attempts,
            "assessment_delta_0": not assessment_deltas and len(new_long) == args.expected_attempts,
            "certificate_structure_valid": not failures,
            "one_certificate_per_finding": not failures and certificate_count == finding_count,
            "certificate_direction_partition_consistent": (
                not failures
                and directional_certificate_count + non_directional_certificate_count == certificate_count
            ),
        },
    }
    summary["status"] = "PASS" if all(summary["criteria"].values()) else "FAIL"
    (args.out / "GATE_W1_DIAGNOSTIC_WITNESS.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    with (args.out / "TRUTH_DELTAS.tsv").open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["group", "target", "query", "before", "after"])
        w.writerows(truth_deltas)
    with (args.out / "ASSESSMENT_DELTAS.tsv").open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["record"])
        for row in assessment_deltas:
            w.writerow(["\t".join(map(str, row))])
    (args.out / "CERTIFICATE_FAILURES.json").write_text(
        json.dumps(failures, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["status"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())
