#!/usr/bin/env python3
"""Static closure gate for B1.1 CString disposition protocol.

This verifier deliberately keeps allocation_disposition_v1 frozen and checks
that the CString handoff/reclaim extension is versioned as
allocation_disposition_v2 across producer, consumer, schema, registry and
explainability surfaces.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

V1_KINDS = {
    "box_into_raw",
    "box_from_raw",
    "box_leak",
    "mem_forget_owned_box",
    "raw_pointer_drop_noop",
    "return_escape",
    "may_deallocate",
}
V1_BASES = {
    "rustc_box_into_raw_v1",
    "rustc_box_from_raw_v1",
    "rustc_box_leak_v1",
    "rustc_mem_forget_owned_box_v1",
    "rustc_mem_drop_raw_pointer_v1",
    "rust_return_identity_v1",
    "allocation_drop_label_v1",
}
V2_KINDS = {"cstring_into_raw", "cstring_from_raw"}
V2_BASES = {"rustc_cstring_into_raw_v1", "rustc_cstring_from_raw_v1"}
RUST_DOC = "https://doc.rust-lang.org/std/ffi/struct.CString.html"


def fail(msg: str) -> None:
    raise SystemExit(f"B1_1_CSTRING_PROTOCOL: FAIL: {msg}")


def contains_all(path: Path, needles: list[str]) -> None:
    text = path.read_text()
    missing = [needle for needle in needles if needle not in text]
    if missing:
        fail(f"{path}: missing {missing}")


def main() -> None:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    cqpl = root / "cqpl"
    crema = root / "crema"

    schema = json.loads((cqpl / "schemas/annotated_icfg_v2.schema.json").read_text())
    v1 = schema["$defs"]["allocationDispositionRecord"]["properties"]
    if set(v1["kind"]["enum"]) != V1_KINDS:
        fail("allocationDispositionRecord v1 kind vocabulary drifted")
    if set(v1["basis"]["enum"]) != V1_BASES:
        fail("allocationDispositionRecord v1 basis vocabulary drifted")

    ext = schema["$defs"].get("allocationDispositionRecordV2Extension", {}).get("properties", {})
    if set(ext.get("kind", {}).get("enum", [])) != V2_KINDS:
        fail("v2 CString kind extension is not exact")
    if set(ext.get("basis", {}).get("enum", [])) != V2_BASES:
        fail("v2 CString basis extension is not exact")

    node_items = schema["$defs"]["node"]["properties"]["allocation_disposition"]["items"]
    refs = {entry.get("$ref") for entry in node_items.get("oneOf", [])}
    expected_refs = {
        "#/$defs/allocationDispositionRecord",
        "#/$defs/allocationDispositionRecordV2Extension",
    }
    if refs != expected_refs:
        fail(f"node disposition union refs mismatch: {refs}")

    schema_text = (cqpl / "schemas/annotated_icfg_v2.schema.json").read_text()
    for token in ["allocation_disposition_v2", "allocation_disposition_v1"]:
        if token not in schema_text:
            fail(f"schema missing capability token {token}")

    contains_all(
        crema / "src/cqpl_export.rs",
        ["\"allocation_disposition_v1\"", "\"allocation_disposition_v2\""],
    )
    contains_all(
        cqpl / "cqpl_checker/src/main.rs",
        [
            "has_allocation_disposition_v2",
            "requires artifact capability allocation_disposition_v2",
            "rustc_cstring_into_raw_v1",
            "rustc_cstring_from_raw_v1",
        ],
    )
    contains_all(
        cqpl / "cqpl_checker/src/kripke.rs",
        [
            "has_allocation_disposition_v2",
            "requires artifact capability allocation_disposition_v2",
            '#[serde(rename = "cstring_into_raw")]',
            '#[serde(rename = "cstring_from_raw")]',
            "b1_1_disposition_v2_wire_names_are_exact_and_closed",
        ],
    )
    contains_all(
        crema / "src/structs.rs",
        [
            '#[serde(rename = "cstring_into_raw")]',
            '#[serde(rename = "cstring_from_raw")]',
        ],
    )
    contains_all(
        crema / "src/library_effects_v1.rs",
        ["b1_1_cstring_producer_evidence_wire_names_match_protocol"],
    )
    contains_all(
        cqpl / "cqpl_checker/src/explain.rs",
        ["ProducerCertifiedCStringIntoRaw", "producer_certified_cstring_into_raw"],
    )

    registry = json.loads((cqpl / "library_models/rust_std_v1.json").read_text())
    projections = {
        s["legacy_projection"]["allocation_disposition_kind"]: s["legacy_projection"]["basis"]
        for s in registry.get("summaries", [])
        if s.get("legacy_projection")
        and s["legacy_projection"].get("allocation_disposition_kind") in V2_KINDS
    }
    if projections != {
        "cstring_into_raw": "rustc_cstring_into_raw_v1",
        "cstring_from_raw": "rustc_cstring_from_raw_v1",
    }:
        fail(f"CString registry projection mismatch: {projections}")

    v1_doc = (cqpl / "capabilities/allocation_disposition_v1.md").read_text()
    v2_doc = (cqpl / "capabilities/allocation_disposition_v2.md").read_text()
    if "frozen" not in v1_doc.lower() or "allocation_disposition_v2" not in v1_doc:
        fail("v1 documentation does not state frozen/refinement boundary")
    if RUST_DOC not in v2_doc:
        fail("v2 documentation lacks official CString reference")

    print("B1_1_CSTRING_PROTOCOL: PASS")
    print("allocation_disposition_v1=frozen-7")
    print("allocation_disposition_v2=cstring-2")
    print("certainty=may_abstract-only")
    print("truth_semantics=unchanged")


if __name__ == "__main__":
    main()
