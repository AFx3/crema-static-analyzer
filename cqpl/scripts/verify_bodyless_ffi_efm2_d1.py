#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import subprocess
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator


BASELINE = "39886ed5cea289c1016e808d1c1e4348582c481c"
CAP_V1 = "external_formal_memory_effects_v1"
CAP_V2 = "external_formal_memory_effects_v2"
BASIS_V2 = "crema_efm2_closed_contract_v1"
CALLEES = ["strlen", "memcmp", "memcpy", "memmove", "memset", "memchr", "strchr", "write"]
EXPECTED_PREIMAGES = {
    "crema/src/cqpl_export.rs",
    "cqpl/cqpl_checker/src/kripke.rs",
    "cqpl/cqpl_checker/src/main.rs",
    "cqpl/schemas/annotated_icfg_v2.schema.json",
}
D1_TARGETS = (
    "b44_memmove_freed_src_uaf_read", "b45_memmove_freed_dst_uaf_write",
    "b46_memchr_freed_buffer_uaf_read", "b47_strchr_freed_string_uaf_read",
    "b48_memmove_zero_extent_no_memory_event", "b49_memchr_zero_extent_no_memory_event",
    "b50_body_present_memmove_control", "b51_memmove_dynamic_extent_may_effect",
)


def validate_preimages(root: Path, path: Path) -> tuple[dict[str, Any], dict[str, Any], list[str]]:
    errors: list[str] = []

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                errors.append(f"duplicate preimage JSON entry: {key}")
            result[key] = value
        return result

    try:
        doc = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, ValueError) as error:
        return {}, {"checked": 0, "mismatches": 0, "files": {}, "errors": [str(error)]}, [f"cannot read preimage record: {error}"]
    sources = doc.get("source_preimages")
    if not isinstance(sources, dict):
        sources = {}
        errors.append("source_preimages must be an object")
    if set(sources) != EXPECTED_PREIMAGES:
        errors.append(f"preimage source closure mismatch: missing={sorted(EXPECTED_PREIMAGES - set(sources))} extra={sorted(set(sources) - EXPECTED_PREIMAGES)}")
    files = {}
    mismatches = 0
    for source, recorded in sorted(sources.items()):
        completed = subprocess.run(
            ["git", "-C", str(root), "show", f"{BASELINE}:{source}"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        actual = hashlib.sha256(completed.stdout).hexdigest() if completed.returncode == 0 else None
        matches = completed.returncode == 0 and actual == recorded
        files[source] = {"recorded_sha256": recorded, "baseline_sha256": actual, "git_show_rc": completed.returncode, "matches": matches}
        if not matches:
            mismatches += 1
            errors.append(f"preimage validation failed for {source}: git_show_rc={completed.returncode} recorded={recorded} actual={actual}")
    return doc, {"checked": len(files), "mismatches": mismatches, "files": files, "errors": errors.copy()}, errors

SOURCES = {
    "strlen": ["svf_absextapi_strlen_semantics_v1", "llvm16_tli_strlen_argmem_read_semantics_v1"],
    "memset": ["svf_extapi_memset_semantics_v1", "llvm16_tli_memset_arg0_writeonly_semantics_v1"],
    ("memcpy", 0): ["svf_extapi_memcpy_semantics_v1", "llvm16_tli_memcpy_arg0_writeonly_semantics_v1"],
    ("memcpy", 1): ["svf_extapi_memcpy_semantics_v1", "llvm16_tli_memcpy_arg1_readonly_semantics_v1"],
    "memcmp": ["llvm16_tli_memcmp_argmem_read_semantics_v1", "llvm16_memorylocation_memcmp_formal_semantics_v1"],
    "write": ["posix_write_buffer_semantics_v1", "llvm16_tli_write_arg1_readonly_semantics_v1"],
    "memmove": ["posix_memmove_n_byte_copy_semantics_v1", "llvm16_memmove_formal_semantics_v1", "llvm16_tli_memmove_recognition_v1"],
    "memchr": ["posix_memchr_bounded_read_semantics_v1", "llvm16_tli_memchr_recognition_v1"],
    "strchr": ["posix_strchr_c_string_read_semantics_v1"],
}


def contract(
    callee: str,
    semantic_class: str,
    formal: int,
    access: str,
    extent_kind: str,
    extent_argument_index: int | None,
    sources: list[str],
) -> dict[str, Any]:
    return {
        "callee": callee,
        "semantic_class": semantic_class,
        "formal_index": formal,
        "access": access,
        "extent_kind": extent_kind,
        "extent_argument_index": extent_argument_index,
        "basis": BASIS_V2,
        "semantic_sources": sources,
    }


CLOSED_CONTRACT = {
    ("strlen", 0, "read"): contract("strlen", "strlen_read_c_string_v1", 0, "read", "c_string_until_nul", None, SOURCES["strlen"]),
    ("memset", 0, "write"): contract("memset", "memset_v1", 0, "write", "bytes_from_formal", 2, SOURCES["memset"]),
    ("memcpy", 0, "write"): contract("memcpy", "memcpy_v1", 0, "write", "bytes_from_formal", 2, SOURCES[("memcpy", 0)]),
    ("memcpy", 1, "read"): contract("memcpy", "memcpy_v1", 1, "read", "bytes_from_formal", 2, SOURCES[("memcpy", 1)]),
    ("memcmp", 0, "read"): contract("memcmp", "memcmp_v1", 0, "read", "bytes_from_formal", 2, SOURCES["memcmp"]),
    ("memcmp", 1, "read"): contract("memcmp", "memcmp_v1", 1, "read", "bytes_from_formal", 2, SOURCES["memcmp"]),
    ("write", 1, "read"): contract("write", "posix_write_v1", 1, "read", "bytes_from_formal", 2, SOURCES["write"]),
    ("memmove", 0, "write"): contract("memmove", "memmove_v1", 0, "write", "bytes_from_formal", 2, SOURCES["memmove"]),
    ("memmove", 1, "read"): contract("memmove", "memmove_v1", 1, "read", "bytes_from_formal", 2, SOURCES["memmove"]),
    ("memchr", 0, "read"): contract("memchr", "memchr_bounded_read_v1", 0, "read", "bytes_from_formal", 2, SOURCES["memchr"]),
    ("strchr", 0, "read"): contract("strchr", "strchr_read_c_string_v1", 0, "read", "c_string_until_nul", None, SOURCES["strchr"]),
}


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(root: Path, *args: str) -> tuple[int, str]:
    completed = subprocess.run(
        ["git", "-C", str(root), *args],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    return completed.returncode, completed.stdout.strip()


def load_status(path: Path) -> dict[tuple[str, str], int]:
    status: dict[tuple[str, str], int] = {}
    if not path.is_file():
        return status
    with path.open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            status[(row["side"], row["target"])] = int(row["rc"])
    return status


def query_docs(target_root: Path) -> dict[str, dict[str, Any]]:
    query_root = target_root / "queries"
    result: dict[str, dict[str, Any]] = {}
    if not query_root.is_dir():
        return result
    for path in sorted(query_root.glob("*.json")):
        if path.name.endswith(".explain.json"):
            continue
        doc = load_json(path)
        if isinstance(doc, dict) and "result" in doc and "assessment" in doc:
            result[path.stem] = doc
    return result


def projection(record: dict[str, Any]) -> dict[str, Any]:
    return {
        "formal_index": record.get("formal_index"),
        "access": record.get("access"),
        "extent_kind": record.get("extent_kind"),
        "extent_argument_index": record.get("extent_argument_index"),
    }


def nodes_at_terminator_span(
    artifact: dict[str, Any], expected_span: dict[str, int]
) -> list[dict[str, Any]]:
    return [
        node
        for node in artifact.get("nodes", [])
        if any(
            anchor.get("kind") == "mir_terminator"
            and all(
                anchor.get("parsed_span", {}).get(field) == value
                for field, value in expected_span.items()
            )
            for anchor in node.get("source_provenance", {}).get("anchors", [])
        )
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify D1 EFM2 artifacts and the 39886ed differential")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--run-root", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--status-tsv", type=Path, required=True)
    parser.add_argument("--cqpl-test-rc", type=int, required=True)
    parser.add_argument("--crema-test-rc", type=int, required=True)
    parser.add_argument("--legacy-checker-rc", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    root = args.root.resolve()
    run_root = args.run_root.resolve()
    baseline_root = run_root / "baseline"
    candidate_root = run_root / "candidate"
    manifest = load_json(args.manifest)
    statuses = load_status(args.status_tsv)
    failures: list[str] = []

    def require(condition: bool, message: str) -> bool:
        if not condition:
            failures.append(message)
        return condition

    rc, head = git(root, "rev-parse", "HEAD")
    require(rc == 0, "cannot resolve candidate HEAD")
    rc, branch = git(root, "branch", "--show-current")
    require(rc == 0, "cannot resolve candidate branch")
    ancestor_rc, _ = git(root, "merge-base", "--is-ancestor", BASELINE, "HEAD")
    diff_check_rc, diff_check_output = git(root, "diff", "--check")
    require(ancestor_rc == 0, f"candidate HEAD does not descend from {BASELINE}")
    require(diff_check_rc == 0, f"git diff --check failed: {diff_check_output}")
    require(branch == "cqpl6-bodyless-ffi-effect-gate", f"unexpected branch {branch!r}")

    preimage_path = root / "cqpl/bodyless_ffi_efm2_d1_preimage_sha256.json"
    preimage, preimage_validation, preimage_errors = validate_preimages(root, preimage_path)
    failures.extend(preimage_errors)
    require(preimage.get("baseline_commit") == BASELINE, "preimage baseline SHA mismatch")
    require(preimage.get("head_commit") == BASELINE, "preimage HEAD SHA mismatch")
    frozen_v1 = root / "cqpl/capabilities/external_formal_memory_effects_v1.md"
    expected_v1_hash = preimage.get("frozen_compatibility_reference", {}).get("cqpl/capabilities/external_formal_memory_effects_v1.md")
    require(sha256(frozen_v1) == expected_v1_hash, "frozen EFM1 capability document changed")

    forbidden_truth_files = {
        "cqpl/cqpl_checker/src/ast.rs",
        "cqpl/cqpl_checker/src/truth.rs",
        "cqpl/cqpl_checker/src/model_checker.rs",
        "crema/src/identity.rs",
        "crema/src/abstract_domain.rs",
        "cqpl/cqpl_checker/src/explain.rs",
    }
    _, changed_text = git(root, "diff", "--name-only")
    changed_paths = {line for line in changed_text.splitlines() if line}
    require(not (changed_paths & forbidden_truth_files), f"forbidden D1 source changes: {sorted(changed_paths & forbidden_truth_files)}")

    schema_path = root / "cqpl/schemas/annotated_icfg_v2.schema.json"
    schema = load_json(schema_path)
    try:
        Draft202012Validator.check_schema(schema)
        schema_well_formed = True
    except Exception as error:  # pragma: no cover - gate error path
        schema_well_formed = False
        failures.append(f"invalid JSON Schema: {error}")
    validator = Draft202012Validator(schema)

    candidate_artifacts: dict[str, dict[str, Any]] = {}
    baseline_artifacts: dict[str, dict[str, Any]] = {}
    query_errors = 0
    for (side, target), target_rc in sorted(statuses.items()):
        require(target_rc == 0, f"{side} runner failed for {target}: rc={target_rc}")
        artifact_path = run_root / side / target / "annotated_icfg_v2.json"
        if target_rc != 0 or not artifact_path.is_file():
            query_errors += 1
            failures.append(f"missing annotated artifact for {side}/{target}")
            continue
        artifact = load_json(artifact_path)
        schema_errors = sorted(validator.iter_errors(artifact), key=lambda error: list(error.absolute_path))
        require(not schema_errors, f"schema rejected {side}/{target}: {schema_errors[0].message if schema_errors else ''}")
        if side == "candidate":
            candidate_artifacts[target] = artifact
        elif side == "baseline":
            baseline_artifacts[target] = artifact

        results_path = run_root / side / target / "query-results.tsv"
        docs = query_docs(run_root / side / target)
        if not results_path.is_file() or len(docs) != 12:
            query_errors += 1
            failures.append(f"incomplete query outputs for {side}/{target}: docs={len(docs)}")

    legacy_schema_artifacts = [
        artifact for artifact in baseline_artifacts.values()
        if CAP_V1 in artifact.get("capabilities", [])
    ]
    require(bool(legacy_schema_artifacts), "no legacy EFM1 baseline artifact was exercised")
    require(args.legacy_checker_rc == 0, f"candidate checker rejected frozen EFM1 artifact: rc={args.legacy_checker_rc}")

    all_candidate_records: list[dict[str, Any]] = []
    all_candidate_callees: set[str] = set()
    record_validation_errors = 0
    for target, artifact in sorted(candidate_artifacts.items()):
        caps = artifact.get("capabilities", [])
        records = artifact.get("external_formal_memory_effects", [])
        require(not (CAP_V1 in caps and CAP_V2 in caps), f"{target} declares both EFM versions")
        require((CAP_V2 in caps) == bool(records), f"{target} violates EFM2 capability/payload atomicity")
        require(CAP_V1 not in caps, f"new producer emitted frozen EFM1 for {target}")
        variables = {entry.get("id"): entry for entry in artifact.get("variables", [])}
        nodes = {node.get("id"): node for node in artifact.get("nodes", [])}
        seen: set[tuple[Any, ...]] = set()
        for record in records:
            all_candidate_records.append(record)
            all_candidate_callees.add(record.get("callee", ""))
            key = (record.get("callee"), record.get("formal_index"), record.get("access"))
            expected = CLOSED_CONTRACT.get(key)
            actual = {
                "callee": record.get("callee"),
                "semantic_class": record.get("semantic_class"),
                "formal_index": record.get("formal_index"),
                "access": record.get("access"),
                "extent_kind": record.get("extent_kind"),
                "extent_argument_index": record.get("extent_argument_index"),
                "basis": record.get("basis"),
                "semantic_sources": record.get("semantic_sources"),
            }
            if expected != actual:
                record_validation_errors += 1
                failures.append(f"{target} has non-closed EFM2 tuple: {actual}")
            allowed_keys = {
                "node", "callee", "semantic_class", "formal_index", "access",
                "event_variable", "actual_variable", "extent_kind",
                "extent_argument_index", "basis", "semantic_sources",
            }
            require(set(record) <= allowed_keys, f"{target} EFM2 record has forbidden fields: {sorted(set(record) - allowed_keys)}")
            require("size_argument_index" not in record, f"{target} EFM2 record uses legacy extent encoding")
            node = nodes.get(record.get("node"))
            require(node is not None, f"{target} EFM2 record references unknown node")
            if node is not None:
                require(record["node"].startswith("rust::"), f"{target} EFM2 node is not Rust")
                require("term:call" in node.get("semantic_labels", []), f"{target} EFM2 node is not term:call")
                event = {"predicate": record.get("access"), "variable": record.get("event_variable")}
                require(event in node.get("labels", []), f"{target} EFM2 record lacks matching ordinary event")
            scope = record.get("node", "").rsplit("::bb", 1)[0]
            expected_actual = f"{scope}::{record.get('event_variable')}"
            require(record.get("actual_variable") == expected_actual, f"{target} formal/actual identity mismatch")
            actual_var = variables.get(record.get("actual_variable"))
            require(actual_var is not None and actual_var.get("language") == "rust", f"{target} actual is not a declared Rust variable")
            duplicate_key = (record.get("node"), record.get("callee"), record.get("formal_index"), record.get("access"))
            require(duplicate_key not in seen, f"{target} has duplicate EFM2 record {duplicate_key}")
            seen.add(duplicate_key)

    require(all_candidate_callees == set(CALLEES), f"candidate EFM2 callee closure mismatch: {sorted(all_candidate_callees)}")

    schema_adversarial: dict[str, bool] = {}
    efm2_doc = next((doc for doc in candidate_artifacts.values() if doc.get("external_formal_memory_effects")), None)
    if efm2_doc is None:
        failures.append("no candidate EFM2 document available for schema adversarial checks")
    else:
        def rejected(mutator) -> bool:
            candidate = json.loads(json.dumps(efm2_doc))
            mutator(candidate)
            return any(validator.iter_errors(candidate))

        schema_adversarial["both_versions_rejected"] = rejected(
            lambda doc: doc["capabilities"].append(CAP_V1)
        )
        schema_adversarial["payload_without_capability_rejected"] = rejected(
            lambda doc: doc.__setitem__("capabilities", [cap for cap in doc["capabilities"] if cap != CAP_V2])
        )
        schema_adversarial["capability_without_payload_rejected"] = rejected(
            lambda doc: doc.pop("external_formal_memory_effects")
        )
        def legacy_extent(doc: dict[str, Any]) -> None:
            record = doc["external_formal_memory_effects"][0]
            record["size_argument_index"] = record.pop("extent_argument_index", 2)
            record.pop("extent_kind", None)
        schema_adversarial["v2_legacy_extent_rejected"] = rejected(legacy_extent)
        for name, value in schema_adversarial.items():
            require(value, f"JSON Schema adversarial check failed: {name}")

    fixture_outcomes: dict[str, dict[str, Any]] = {}
    new_passed = 0
    new_failed = 0
    represented_body_not_duplicated = False
    zero_extent_suppressed = True
    dynamic_extent_remains_may = False
    targeted_uaf_ok = True
    for fixture in manifest["fixtures"]:
        target = fixture["target"]
        outcome_failures_before = len(failures)
        artifact = candidate_artifacts.get(target)
        if artifact is None:
            failures.append(f"new fixture artifact missing: {target}")
            fixture_outcomes[target] = {"status": "FAIL", "reason": "artifact missing"}
            new_failed += 1
            continue
        records = [
            record for record in artifact.get("external_formal_memory_effects", [])
            if record.get("callee") == fixture["callee"]
        ]
        actual_effects = sorted((projection(record) for record in records), key=lambda item: (item["formal_index"], item["access"]))
        expected_effects = sorted(fixture["expected_effects"], key=lambda item: (item["formal_index"], item["access"]))
        require(actual_effects == expected_effects, f"{target} effect mismatch: {actual_effects} != {expected_effects}")

        if "uaf" in fixture:
            qpath = candidate_root / target / "queries/use_after_free_alloc_state.json"
            explain_path = candidate_root / target / "queries/use_after_free_alloc_state.explain.json"
            require(qpath.is_file(), f"{target} missing UAF query JSON")
            require(explain_path.is_file(), f"{target} missing UAF explanation JSON")
            if qpath.is_file():
                qdoc = load_json(qpath)
                expected_uaf = fixture["uaf"]
                observed = {
                    "truth": qdoc.get("result"),
                    "subresult": qdoc.get("assessment", {}).get("subresult"),
                    "direction": qdoc.get("assessment", {}).get("direction"),
                    "strength": qdoc.get("assessment", {}).get("strength"),
                }
                ok = observed == expected_uaf
                targeted_uaf_ok &= ok
                require(ok, f"{target} UAF outcome mismatch: {observed}")
                fixture_outcomes.setdefault(target, {})["uaf"] = observed
            mismatch_doc = query_docs(candidate_root / target).get("allocator_mismatch_ub_v2", {})
            require(mismatch_doc.get("result") == "ff", f"{target} introduced allocator-mismatch evidence")

        if fixture.get("zero_extent"):
            call_nodes = nodes_at_terminator_span(artifact, fixture["call_node_span"])
            call_rw = [
                label
                for node in call_nodes
                for label in node.get("labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            call_allocation_rw = [
                label
                for node in call_nodes
                for label in node.get("allocation_labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            ok = (
                len(call_nodes) == 1
                and not records
                and not call_rw
                and not call_allocation_rw
                and CAP_V2 not in artifact.get("capabilities", [])
            )
            zero_extent_suppressed &= ok
            require(
                ok,
                f"{target} exact-zero call-node proof failed: "
                f"nodes={len(call_nodes)} labels={call_rw} allocation_labels={call_allocation_rw}",
            )

        if fixture.get("represented_body"):
            represented_nodes = [
                node for node in artifact.get("nodes", [])
                if node.get("id", "").startswith("llvm::memmove::")
            ]
            call_nodes = nodes_at_terminator_span(artifact, fixture["call_node_span"])
            call_rw = [
                label
                for node in call_nodes
                for label in node.get("labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            call_allocation_rw = [
                label
                for node in call_nodes
                for label in node.get("allocation_labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            represented_body_rw = [
                label
                for node in represented_nodes
                for label in node.get("allocation_labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            allocation_events = [
                (node.get("id"), label.get("predicate"), label.get("allocation"))
                for node in artifact.get("nodes", [])
                for label in node.get("allocation_labels", [])
                if label.get("predicate") in {"read", "write"}
            ]
            ok = (
                len(call_nodes) == 1
                and bool(represented_nodes)
                and bool(represented_body_rw)
                and not records
                and not call_rw
                and not call_allocation_rw
                and len(allocation_events) == len(set(allocation_events))
            )
            represented_body_not_duplicated = ok
            require(ok, f"{target} represented memmove body was absent or duplicated by EFM2")

        if fixture.get("dynamic_extent"):
            dynamic_extent_remains_may = len(records) == 2
            require(dynamic_extent_remains_may, f"{target} dynamic extent lost EFM2 effects")

        passed = len(failures) == outcome_failures_before
        fixture_outcomes.setdefault(target, {})["status"] = "PASS" if passed else "FAIL"
        if passed:
            new_passed += 1
        else:
            new_failed += 1

    focused_expected = {
        "b24_memcmp_freed_left_uaf": [(0, "read"), (1, "read")],
        "b25_memcmp_freed_right_uaf": [(0, "read"), (1, "read")],
        "b26_write_after_free_uaf": [(1, "read")],
        "b29_strchr_return_not_alloc": [(0, "read")],
        "b34_memmove_returned_alias": [(0, "write"), (1, "read")],
        "b35_memchr_return_not_alloc": [(0, "read")],
    }
    focused_existing: dict[str, bool] = {}
    for target, expected_pairs in focused_expected.items():
        artifact = candidate_artifacts.get(target)
        ok = artifact is not None
        if artifact is not None:
            pairs = sorted(
                (record["formal_index"], record["access"])
                for record in artifact.get("external_formal_memory_effects", [])
            )
            ok = pairs == sorted(expected_pairs)
        focused_existing[target] = ok
        require(ok, f"focused existing proof records mismatch for {target}")

    for target in ["b29_strchr_return_not_alloc", "b34_memmove_returned_alias", "b35_memchr_return_not_alloc"]:
        before = baseline_artifacts.get(target, {}).get("allocations")
        after = candidate_artifacts.get(target, {}).get("allocations")
        require(before == after, f"EFM2 changed allocation identity/catalog for {target}")

    truth_deltas: list[dict[str, Any]] = []
    assessment_deltas: list[dict[str, Any]] = []
    existing_targets = sorted(baseline_artifacts)
    require(set(existing_targets) == set(target for side, target in statuses if side == "baseline"), "baseline artifact set incomplete")
    for target in existing_targets:
        before_docs = query_docs(baseline_root / target)
        after_docs = query_docs(candidate_root / target)
        all_queries = sorted(set(before_docs) | set(after_docs))
        for query in all_queries:
            before = before_docs.get(query)
            after = after_docs.get(query)
            if before is None or after is None:
                query_errors += 1
                failures.append(f"missing differential cell {target}/{query}")
                continue
            if before.get("result") != after.get("result"):
                truth_deltas.append({
                    "target": target,
                    "query": query,
                    "before": before.get("result"),
                    "after": after.get("result"),
                })
            if before.get("assessment") != after.get("assessment"):
                assessment_deltas.append({
                    "target": target,
                    "query": query,
                    "before": before.get("assessment"),
                    "after": after.get("assessment"),
                })

    require(not truth_deltas, f"existing corpus has {len(truth_deltas)} truth deltas")
    require(not assessment_deltas, f"existing corpus has {len(assessment_deltas)} assessment deltas")
    require(query_errors == 0, f"existing/new corpus has {query_errors} query errors")

    no_return_relation = all(
        not any("return" in key or "alias" in key for key in record)
        for record in all_candidate_records
    ) and all(
        "external_return_relations_v1" not in artifact.get("capabilities", [])
        for artifact in candidate_artifacts.values()
    )
    require(no_return_relation, "D1 artifact introduced a return/alias relation")

    fixture_root = root / "tests_and_target_repos/a-code_c_ffi_bodyless_gate"
    d1_roots = [fixture_root / target for target in D1_TARGETS]
    target_paths = sorted(str(path.relative_to(root)) for tree in d1_roots for path in tree.rglob("target"))
    cache_paths = sorted(str(path.relative_to(root)) for tree in [*d1_roots, root / "cqpl/scripts"] for path in tree.rglob("__pycache__"))
    global_icfg_paths = sorted(str(path.relative_to(root)) for tree in d1_roots for path in tree.rglob("global_icfg*.json"))
    callgraph_rc, callgraph_status = git(root, "status", "--porcelain", "--untracked-files=all", "--", "SVF-example/callgraph_initial.dot.dot")
    require(not target_paths, f"D1 fixture target artifacts remain: {target_paths}")
    require(not cache_paths, f"D1 source/script Python caches remain: {cache_paths}")
    require(not global_icfg_paths, f"D1 fixture generated global ICFG files remain: {global_icfg_paths}")
    require(callgraph_rc == 0 and not callgraph_status, f"callgraph_initial.dot.dot is dirty/generated: {callgraph_status}")
    status = "PASS" if not failures else "FAIL"
    result = {
        "schema": "cqpl_bodyless_ffi_efm2_d1_gate_v1",
        "status": status,
        "baseline_commit": BASELINE,
        "candidate_commit_or_worktree": head,
        "branch": branch,
        "toolchain": "nightly-2024-11-21",
        "capability": CAP_V2,
        "preimage_record": str(preimage_path),
        "preimage_validation": preimage_validation,
        "legacy_efm1": {
            "accepted": bool(legacy_schema_artifacts) and args.legacy_checker_rc == 0,
            "schema_artifacts_checked": len(legacy_schema_artifacts),
            "checker_rc": args.legacy_checker_rc,
            "frozen_document_sha256": sha256(frozen_v1),
        },
        "closed_contract": {
            "callees": CALLEES,
            "tuples": len(CLOSED_CONTRACT),
            "observed_callees": sorted(all_candidate_callees),
            "invalid_records": record_validation_errors,
        },
        "proof_obligations": {
            "exact_symbol": record_validation_errors == 0,
            "exact_arity": record_validation_errors == 0,
            "per_formal_only": record_validation_errors == 0,
            "explicit_extent": record_validation_errors == 0,
            "zero_extent_suppressed": zero_extent_suppressed,
            "dynamic_extent_remains_may": dynamic_extent_remains_may,
            "represented_body_not_duplicated": represented_body_not_duplicated,
            "formal_actual_separation": record_validation_errors == 0,
            "generic_argmem_not_overassigned": record_validation_errors == 0,
            "no_return_relation_added": no_return_relation,
            "truth_semantics_unchanged": not (changed_paths & forbidden_truth_files),
        },
        "schema_validation": {
            "draft_2020_12_valid": schema_well_formed,
            "candidate_artifacts_checked": len(candidate_artifacts),
            "adversarial": schema_adversarial,
        },
        "unit_tests": {
            "cqpl_rc": args.cqpl_test_rc,
            "crema_rc": args.crema_test_rc,
        },
        "new_fixtures": {
            "passed": new_passed,
            "failed": new_failed,
            "targeted_uaf_assessments_match": targeted_uaf_ok,
            "outcomes": fixture_outcomes,
        },
        "focused_existing": focused_existing,
        "existing_corpus": {
            "targets": len(existing_targets),
            "query_cells": sum(len(query_docs(baseline_root / target)) for target in existing_targets),
            "truth_deltas": len(truth_deltas),
            "assessment_deltas": len(assessment_deltas),
            "query_errors": query_errors,
            "truth_delta_details": truth_deltas,
            "assessment_delta_details": assessment_deltas,
        },
        "hygiene": {
            "d1_fixture_target_directories_remaining": len(target_paths),
            "d1_fixture_target_paths": target_paths,
            "python_cache_directories_remaining": len(cache_paths),
            "python_cache_paths": cache_paths,
            "generated_global_icfg_files_remaining": len(global_icfg_paths),
            "generated_global_icfg_paths": global_icfg_paths,
            "callgraph_initial_clean": callgraph_rc == 0 and not callgraph_status,
            "callgraph_initial_status_rc": callgraph_rc,
            "callgraph_initial_dirty_entries": len(callgraph_status.splitlines()),
            "git_diff_check_rc": diff_check_rc,
            "frozen_efm1_document_unchanged": sha256(frozen_v1) == expected_v1_hash,
            "forbidden_truth_or_identity_files_changed": sorted(changed_paths & forbidden_truth_files),
        },
        "failures": failures,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"CQPL_BODYLESS_FFI_EFM2_D1_GATE: {status}")
    print(f"gate_json={args.out}")
    print(f"existing_truth_deltas={len(truth_deltas)}")
    print(f"existing_assessment_deltas={len(assessment_deltas)}")
    print(f"new_fixtures_passed={new_passed}")
    print(f"new_fixtures_failed={new_failed}")
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
