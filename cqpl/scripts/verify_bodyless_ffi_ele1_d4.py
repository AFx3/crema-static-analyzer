#!/usr/bin/env python3
"""D4 exact-baseline ELE1 consolidation gate."""
from __future__ import annotations

import argparse
import collections
import copy
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tarfile

BASE = "8c7a9b3cb732e8e1760c64889476688559790396"
CAP = "external_library_effects_v1"
FAMILIES = ("allocation_return", "reallocation", "deallocation", "formal_memory", "return_relation", "negative_evidence")
PREFIX = "tests_and_target_repos/a-code_c_ffi_bodyless_gate/"
TOOLCHAIN = "nightly-2024-11-21"
GENERATED = ["crema/ffi_functions.json", "crema/global_icfg.json",
             "crema/global_icfg_nodes_edges.dot", "SVF-example/callgraph_initial.dot.dot"]


def unique(pairs):
    out = {}
    for key, value in pairs:
        if key in out:
            raise ValueError(f"duplicate JSON key: {key}")
        out[key] = value
    return out


def read(path):
    return json.loads(path.read_text(), object_pairs_hook=unique)


def write(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def run(command, logfile, cwd):
    with logfile.open("w") as stream:
        return subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT).returncode


def preimages(root):
    manifest = read(root / "cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json")
    assert manifest["baseline_commit"] == BASE
    expected = {"crema/src/cqpl_export.rs", "cqpl/cqpl_checker/src/main.rs",
                "cqpl/schemas/annotated_icfg_v2.schema.json"}
    assert set(manifest["semantic_source_preimages"]) == expected
    files = {}
    for path, frozen in manifest["semantic_source_preimages"].items():
        base_bytes = git(root, "show", f"{BASE}:{path}")
        assert sha(base_bytes) == frozen, f"preimage mismatch: {path}"
        files[path] = {"preimage_sha256": frozen, "postimage_sha256": sha((root / path).read_bytes())}
    for group in ("frozen_capability_hashes", "frozen_gate_hashes", "frozen_spec_hashes", "frozen_fixture_hashes"):
        for path, frozen in manifest[group].items():
            base_bytes = git(root, "show", f"{BASE}:{path}")
            assert sha(base_bytes) == frozen and (root / path).read_bytes() == base_bytes, path
    return {"checked": len(files) + sum(len(manifest[group]) for group in
            ("frozen_capability_hashes", "frozen_gate_hashes", "frozen_spec_hashes", "frozen_fixture_hashes")),
            "mismatches": 0, "errors": 0, "files": files}


def tests(root, out):
    results = {}
    for name, manifest, focus in [
        ("focused_producer", "crema/Cargo.toml", "ele1_binding_is_extracted_from_structured_mir_call"),
        ("focused_checker", "cqpl/cqpl_checker/Cargo.toml", "ele1"),
        ("full_cqpl", "cqpl/cqpl_checker/Cargo.toml", None),
        ("full_crema", "crema/Cargo.toml", None),
    ]:
        command = ["cargo", "+" + TOOLCHAIN, "test", "--manifest-path", str(root / manifest), "--locked"]
        if focus:
            command.append(focus)
        rc = run(command, out / "logs" / f"{name}.log", root)
        log = (out / "logs" / f"{name}.log").read_text()
        counts = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", log)
        results[name] = {"exit_code": rc, "passed": sum(int(a) for a, _ in counts),
                         "failures": sum(int(b) for _, b in counts)}
        assert rc == 0 and counts, f"{name} failed"
        if focus:
            assert results[name]["passed"] > 0, f"no {name} tests found"
    binary = root / "cqpl/cqpl_checker/target/debug/cqpl_checker"
    rc = run(["cargo", "+" + TOOLCHAIN, "build", "--manifest-path",
              str(root / "cqpl/cqpl_checker/Cargo.toml"), "--locked"],
             out / "logs/checker-binary.log", root)
    assert rc == 0 and binary.is_file()
    results["checker_binary"] = {"exit_code": rc, "path": str(binary)}
    return results


def targets(root):
    paths = git(root, "ls-tree", "-r", "--name-only", BASE, PREFIX).decode().splitlines()
    selected = sorted({p[len(PREFIX):].split("/")[0] for p in paths})
    queries = [p for p in git(root, "ls-tree", "-r", "--name-only", BASE, "cqpl/queries_v2/").decode().splitlines() if p.endswith(".cqpl")]
    assert len(selected) == 83 and len(queries) == 12 and len(selected) * len(queries) == 996
    assert all(not re.match(r"b(?:7\d|[89]\d)_", name) for name in selected)
    return selected


def replay(root, out, side, selected):
    for number, target in enumerate(selected, 1):
        command = [sys.executable, str(root / "cqpl/scripts/run_one_target_v6q_r1c.py"),
                   "--root", str(root), "--relative-path", "a-code_c_ffi_bodyless_gate/" + target,
                   "--out", str(out / side / target), "--toolchain", TOOLCHAIN]
        rc = run(command, out / "logs" / f"{side}-{target}.log", root)
        with (out / "runner-status.tsv").open("a") as stream:
            stream.write(f"{side}\t{target}\t{rc}\n")
        print(f"{side} {number}/{len(selected)} {target}: exit {rc}", flush=True)
        assert rc == 0, f"{side} replay failed: {target}"
        shutil.copy2(root / "crema/global_icfg.json", out / side / target / "raw-global-icfg.json")


def replay_isolated_pair(root, out, selected, semantic):
    # One isolated path for both sides preserves rustc source-span bytes. The
    # baseline phase is an untouched detached checkout at the exact commit.
    with tempfile.TemporaryDirectory(prefix="cqpl-ele1-d4-") as temp:
        tree = Path(temp) / "repo"
        assert run(["git", "clone", "--shared", "--no-checkout", str(root), str(tree)],
                   out / "logs/baseline-clone.log", root) == 0
        assert run(["git", "-C", str(tree), "checkout", "--detach", BASE],
                   out / "logs/baseline-checkout.log", root) == 0
        assert git(tree, "rev-parse", "HEAD").decode().strip() == BASE
        assert all(sha((tree / p).read_bytes()) == semantic[p] for p in semantic)
        replay(tree, out, "baseline", selected)
        for path in semantic:
            shutil.copyfile(root / path, tree / path)
        assert all((tree / p).read_bytes() == (root / p).read_bytes() for p in semantic)
        replay(tree, out, "candidate", selected)


def free_calls(path):
    raw = read(path / "raw-global-icfg.json")
    artifact = read(path / "annotated_icfg_v2.json")
    ffi = set(read(path / "ffi_functions.json")["ffi_functions"])
    represented = {node["node_data"].get("function_name") for _, node in raw["ordered_nodes"]
                   if node["node_type"] == "Llvm"}
    nodes = {node["id"]: node for node in artifact["nodes"]}
    catalog = {variable["id"] for variable in artifact["variables"]}
    cr1 = {dealloc["node"] for record in artifact.get("conditional_reallocations", [])
           for dealloc in record.get("result_deallocations", [])}
    calls = []
    for node_id, node in raw["ordered_nodes"]:
        if node["node_type"] != "Mir":
            continue
        term = node["node_data"].get("terminator") or {}
        if (term.get("kind") != "Call" or term.get("function_called") != "free"
            or term.get("callee_def_path") != "free" or len(term.get("arguments", [])) != 1
            or "free" not in ffi or "free" in represented):
            continue
        annotated = nodes[node_id]
        assert "term:call" in annotated["semantic_labels"]
        arg = term["arguments"][0]["arg"]
        match = re.search(r"Local\(_(\d+)\)|_(\d+)", arg)
        candidate = f"{node_id.rsplit('::bb', 1)[0]}::Local(_{match[1] or match[2]})" if match else None
        actual = candidate if candidate in catalog else None
        calls.append({"target": path.name, "node": node_id, "callee": "free", "formal_index": 0,
                      "drop_label_present": any(x["predicate"] == "drop" for x in annotated["labels"]),
                      "allocation_drop_label_present": any(x["predicate"] == "drop" for x in annotated.get("allocation_labels", [])),
                      "cr1_corroboration_present": node_id in cr1,
                      "mir_actual_operand": arg, "canonical_actual_candidate": candidate,
                      "actual_variable": actual,
                      **({"null_reason": "canonical_scoped_variable_absent_from_baseline_catalog"}
                         if actual is None else {})})
    return calls


def projection(artifact):
    artifact = json.loads(json.dumps(artifact))
    for field in ("external_call_bindings", "external_library_effects", "external_return_call_bindings"):
        artifact.pop(field, None)
    artifact["capabilities"] = [cap for cap in artifact["capabilities"] if cap != CAP]
    return artifact


def query_docs(path):
    return {p.stem: read(p) for p in (path / "queries").glob("*.json")
            if not p.name.endswith(".explain.json")}


def mir_variable(scope, raw, catalog):
    match = re.fullmatch(r"(?:Local\(_(\d+)\)(?: \[mutable\])?|_(\d+))", raw)
    if not match:
        return None
    candidate = f"{scope}::Local(_{match[1] or match[2]})"
    return candidate if candidate in catalog else None


def family_sources(artifact):
    sources = collections.defaultdict(collections.Counter)
    for allocation in artifact.get("allocations", []):
        site = allocation["site"]
        contract = allocation.get("allocator_contract", {})
        if site["kind"] == "c_call" and site["node_id"].startswith("rust::") and contract.get("family") == "c_malloc" and contract.get("operation") in ("malloc", "calloc", "strdup", "realloc"):
            sources[site["node_id"]]["allocation_return"] += 1
    for field, family in (("reallocation_boundaries", "reallocation"),
                          ("external_deallocation_call_provenance", "deallocation"),
                          ("external_formal_memory_effects", "formal_memory"),
                          ("external_return_relations", "return_relation"),
                          ("external_negative_evidence", "negative_evidence")):
        for record in artifact.get(field, []):
            sources[record["node"]][family] += 1
    return sources


def validate(root, out, selected):
    from jsonschema import Draft202012Validator
    schema = read(root / "cqpl/schemas/annotated_icfg_v2.schema.json")
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    frozen = read(root / "cqpl/bodyless_ffi_dcp1_d4p0_inventory.json")
    assert len(frozen["callsites"]) == 44
    assert frozen["counts"]["direct_bodyless_free_calls"] == 44
    assert frozen["counts"]["with_allocation_specific_drop"] == 28
    assert frozen["counts"]["without_allocation_specific_drop_cr1"] == 8
    assert frozen["counts"]["without_allocation_specific_drop_no_cr1"] == 8
    frozen_calls = sorted(frozen["callsites"], key=lambda r: (r["target"], r["node"]))
    observed = sorted((call for target in selected for call in free_calls(out / "baseline" / target)),
                      key=lambda r: (r["target"], r["node"]))
    assert observed == frozen_calls, "D4-P0 44-call inventory differs from baseline replay"
    counters = collections.Counter()
    families = collections.Counter()
    baseline_families = collections.Counter()
    baseline_calls = 0
    legacy = collections.Counter()
    for target in selected:
        before = read(out / "baseline" / target / "annotated_icfg_v2.json")
        after = read(out / "candidate" / target / "annotated_icfg_v2.json")
        validator.validate(before)
        validator.validate(after)
        checker = root / "cqpl/cqpl_checker/target/debug/cqpl_checker"
        query = root / "cqpl/queries_v2/mir_terminator_presence.cqpl"
        assert run([str(checker), str(out / "baseline" / target / "annotated_icfg_v2.json"),
                    str(query), "--json"], out / "logs" / f"legacy-post-d4-{target}.log", root) == 0
        baseline_proof = family_sources(before)
        baseline_calls += len(baseline_proof)
        for baseline_counts in baseline_proof.values():
            baseline_families.update(baseline_counts)
        counters["legacy_err1_bindings"] += len(before.get("external_return_call_bindings", []))
        counters["legacy_artifacts_checked"] += 1
        if projection(before) != projection(after):
            counters["semantic_projection_differences"] += 1
        for field, key in (("external_formal_memory_effects", "d1_unexpected_changes"),
                           ("external_return_relations", "d2_unexpected_changes"),
                           ("external_negative_evidence", "d3_unexpected_changes"),
                           ("external_deallocation_call_provenance", "d4p0_unexpected_changes")):
            counters[key] += before.get(field) != after.get(field)
        assert after.get("external_return_call_bindings") is None
        if any(a.get("labels") != b.get("labels") or a.get("allocation_labels") != b.get("allocation_labels")
               for a, b in zip(before["nodes"], after["nodes"])):
            counters["ordinary_events_created_by_ele1"] += 1
        proof = family_sources(after)
        bindings = after.get("external_call_bindings", [])
        envelopes = after.get("external_library_effects", [])
        assert len(proof) == len(bindings) == len(envelopes)
        assert len({b["node"] for b in bindings}) == len(bindings)
        assert len({b["binding_id"] for b in bindings}) == len(bindings)
        assert len({e["binding_id"] for e in envelopes}) == len(envelopes)
        assert (CAP in after["capabilities"]) == bool(proof)
        raw = read(out / "candidate" / target / "raw-global-icfg.json")
        raw_nodes = dict(raw["ordered_nodes"])
        catalog = {v["id"] for v in after["variables"]}
        for binding in bindings:
            node = binding["node"]
            assert node in proof and binding["binding_id"] == "ele1:" + node
            mir = raw_nodes[node]
            assert mir["node_type"] == "Mir"
            term = mir["node_data"]["terminator"]
            assert term["kind"] == "Call"
            scope = node.rsplit("::bb", 1)[0]
            assert binding["rust_function_scope"] == scope
            assert binding["callee"] == term["function_called"].strip()
            assert binding["arity"] == len(term["arguments"]) == len(binding["arguments"])
            assert binding["arguments"] == [mir_variable(scope, a["arg"], catalog) for a in term["arguments"]]
            assert binding["result_variable"] == mir_variable(scope, term["return_place"], catalog)
            assert binding["body_status"] == "bodyless" and binding["basis"] == "rustc_mir_external_call_binding_v1"
            assert not any(n["node_type"] == "Llvm" and n["node_data"].get("function_name") == binding["callee"] for _, n in raw["ordered_nodes"])
        for envelope in envelopes:
            node = envelope["binding_id"].removeprefix("ele1:")
            assert node in proof and envelope["basis"] == "crema_external_library_effects_v1"
            assert envelope["effect_counts"] == {family: proof[node][family] for family in FAMILIES}
            assert set(envelope["effect_families"]) == {family for family in FAMILIES if proof[node][family]}
        counters["canonical_bindings"] += len(bindings)
        counters["envelopes"] += len(envelopes)
        for node, counts in proof.items():
            families.update(counts)
        bq, aq = query_docs(out / "baseline" / target), query_docs(out / "candidate" / target)
        assert set(bq) == set(aq) and len(bq) == 12
        for name in bq:
            counters["truth_deltas"] += bq[name].get("result") != aq[name].get("result")
            counters["assessment_deltas"] += bq[name].get("assessment") != aq[name].get("assessment")
            counters["query_errors"] += bq[name].get("result") not in ("tt", "ff", "unk") or aq[name].get("result") not in ("tt", "ff", "unk")
        for key, cap in (("efm2", "external_formal_memory_effects_v2"), ("err1_legacy", "external_return_relations_v1"),
                         ("ene1", "external_negative_evidence_v1"), ("d4p0", "external_deallocation_call_provenance_v1")):
            legacy[key] += cap in before["capabilities"]
    assert counters["canonical_bindings"] == counters["envelopes"] == 115
    assert dict(families) == {"allocation_return": 22, "deallocation": 44, "formal_memory": 39,
                             "return_relation": 22, "reallocation": 15, "negative_evidence": 6}
    assert baseline_calls == 115 and baseline_families == families
    assert counters["legacy_err1_bindings"] == 22
    assert counters["legacy_artifacts_checked"] == 83
    manifest = read(root / "cqpl/bodyless_ffi_ele1_d4_consolidation_manifest.json")
    assert manifest["baseline_commit"] == BASE
    controls = manifest["controls"]
    assert all(target in selected for names in controls.values() for target in names)
    for key, names in controls.items():
        for target in names:
            artifact = read(out / "candidate" / target / "annotated_icfg_v2.json")
            envelopes = artifact.get("external_library_effects", [])
            if key in ("represented_body", "unknown_external"):
                assert not envelopes and not artifact.get("external_call_bindings")
            elif key == "zero_extent":
                if target.startswith("b58"):
                    assert sum(e["effect_counts"]["return_relation"] for e in envelopes) == 1
                    assert sum(e["effect_counts"]["formal_memory"] for e in envelopes) == 0
                else:
                    assert not envelopes
            elif key in ("exact_alias", "derived_alias", "negative_memory"):
                pair = {"exact_alias": {"formal_memory", "return_relation"},
                        "derived_alias": {"formal_memory", "return_relation"},
                        "negative_memory": {"formal_memory", "negative_evidence"}}[key]
                assert any(pair <= set(e["effect_families"]) for e in envelopes)
                if key == "derived_alias":
                    assert all(e["effect_counts"]["deallocation"] == 0 for e in envelopes)
            else:
                family = {"nocapture": "negative_evidence"}.get(key, key)
                assert any(e["effect_counts"][family] > 0 for e in envelopes)
    efm1 = root / "repro-results/bodyless-ffi-efm2-d1-20260927T152444Z/baseline/b24_memcmp_freed_left_uaf/annotated_icfg_v2.json"
    assert efm1.is_file() and "external_formal_memory_effects_v1" in read(efm1)["capabilities"]
    validator.validate(read(efm1))
    checker = root / "cqpl/cqpl_checker/target/debug/cqpl_checker"
    query = root / "cqpl/queries_v2/mir_terminator_presence.cqpl"
    assert run([str(checker), str(efm1), str(query), "--json"], out / "logs/legacy-efm1.log", root) == 0
    assert all(counters[k] == 0 for k in ("semantic_projection_differences", "ordinary_events_created_by_ele1",
        "truth_deltas", "assessment_deltas", "query_errors", "d1_unexpected_changes", "d2_unexpected_changes",
        "d3_unexpected_changes", "d4p0_unexpected_changes"))
    adversarial = adversarial_checks(root, out, validator)
    return {"baseline": {"targets": 83, "query_cells": 996},
            "baseline_protocol_inventory": {"effectful_bodyless_calls": baseline_calls,
                                            "effect_family_records": dict(baseline_families),
                                            "legacy_err1_call_bindings": counters["legacy_err1_bindings"],
                                            "d4p0_direct_free_records": baseline_families["deallocation"],
                                            "represented_body_controls": len(controls["represented_body"]),
                                            "unknown_external_controls": len(controls["unknown_external"])},
            "bindings": {"effectful_calls": 115, "canonical_bindings": counters["canonical_bindings"],
                         "duplicate_nodes": 0, "invalid_bindings": 0, "mir_origin_failures": 0},
            "effect_family_records": dict(families),
            "deallocation_closure": {"direct_bodyless_free_calls": 44, "with_allocation_specific_drop": 28,
                                     "without_allocation_specific_drop_cr1": 8,
                                     "without_allocation_specific_drop_no_cr1": 8,
                                     "duplicate_cr1_or_drop_counts": 0},
            "envelopes": {"count": counters["envelopes"], "orphan_underlying_effects": 0,
                          "orphan_envelope_effects": 0, "count_mismatches": 0, "duplicate_effect_materializations": 0},
            "semantic_invariance": {"semantic_projection_differences": 0, "ordinary_events_created_by_ele1": 0},
            "legacy_compatibility": {"efm1_failures": 0, "efm2_failures": 0, "err1_legacy_failures": 0,
                                     "ene1_failures": 0, "baseline_artifacts_checked": 83,
                                     "protocol_targets": dict(legacy), "efm1_artifact_sha256": sha(efm1.read_bytes())},
            "differential": {"truth_deltas": 0, "assessment_deltas": 0, "query_errors": 0},
            "contradictions": {"tests_passed": adversarial["rejected"], "tests_failed": 0},
            "preservation": {"d1_unexpected_changes": 0, "d2_unexpected_changes": 0,
                             "d3_unexpected_changes": 0, "d4p0_unexpected_changes": 0}}


def adversarial_checks(root, out, validator):
    """Mutate real consolidated artifacts; require executable fail-closed rejection."""
    base = read(out / "candidate/b52_memcpy_return_exact_alias_uaf/annotated_icfg_v2.json")
    free = read(out / "candidate/b02_malloc_free_clean/annotated_icfg_v2.json")
    negative = read(out / "candidate/b62_function_nofree/annotated_icfg_v2.json")
    realloc = read(out / "candidate/b20j_second_realloc_chain_partial/annotated_icfg_v2.json")
    borrowed = read(out / "candidate/b57_getenv_borrowed_no_allocation/annotated_icfg_v2.json")
    unknown = read(out / "candidate/b61_unknown_pointer_return_fail_closed/annotated_icfg_v2.json")
    query = sorted((root / "cqpl/queries_v2").glob("*.cqpl"))[0]
    checker = root / "cqpl/cqpl_checker/target/debug/cqpl_checker"
    cases = {}
    def add(name, source, change):
        value = copy.deepcopy(source)
        change(value)
        cases[name] = value
    def move_binding(d, node):
        d["external_call_bindings"][0].update(node=node, binding_id="ele1:" + node)
        d["external_library_effects"][0]["binding_id"] = "ele1:" + node
    add("B1_unknown_node", base, lambda d: move_binding(d, "rust::main::bb999"))
    def non_rust(d):
        move_binding(d, "llvm::main::bb0")
        d["external_call_bindings"][0]["rust_function_scope"] = "llvm::main"
    add("B2_non_rust", base, non_rust)
    add("B3_non_call", base, lambda d: move_binding(d, next(n["id"] for n in d["nodes"] if n["id"].startswith("rust::") and "term:call" not in n["semantic_labels"])))
    add("B4_scope", base, lambda d: d["external_call_bindings"][0].update(rust_function_scope="rust::other"))
    add("B5_arity", base, lambda d: d["external_call_bindings"][0].update(arity=9))
    add("B6_argument_undeclared", base, lambda d: d["external_call_bindings"][0]["arguments"].__setitem__(0, "rust::main::Local(_999)"))
    def cross_argument(d):
        d["variables"].append({"id": "rust::other::Local(_1)", "language": "rust"})
        d["external_call_bindings"][0]["arguments"][0] = "rust::other::Local(_1)"
    add("B7_argument_cross_scope", base, cross_argument)
    add("B8_result_undeclared", base, lambda d: d["external_call_bindings"][0].update(result_variable="rust::main::Local(_999)"))
    def cross_result(d):
        d["variables"].append({"id": "rust::other::Local(_1)", "language": "rust"})
        d["external_call_bindings"][0]["result_variable"] = "rust::other::Local(_1)"
    add("B9_result_cross_scope", base, cross_result)
    add("B10_binding_id", base, lambda d: d["external_call_bindings"][0].update(binding_id="ele1:wrong"))
    add("B11_represented", base, lambda d: d["external_call_bindings"][0].update(body_status="represented"))
    add("B12_duplicate_id", base, lambda d: d["external_call_bindings"].append(copy.deepcopy(d["external_call_bindings"][0])))
    add("B13_duplicate_node", base, lambda d: d["external_call_bindings"].append(dict(d["external_call_bindings"][0], binding_id="ele1:other")))
    add("B14_unknown_envelope", base, lambda d: d["external_library_effects"][0].update(binding_id="ele1:rust::main::bb999"))
    add("B15_missing_envelope", base, lambda d: d["external_library_effects"].clear())
    add("B16_zero_envelope", base, lambda d: d["external_library_effects"][0]["effect_counts"].update({f: 0 for f in FAMILIES}))
    add("B17_family_disagreement", base, lambda d: d["external_library_effects"][0].update(effect_families=["deallocation"]))
    add("X1_efm_formal", base, lambda d: d["external_formal_memory_effects"][0].update(formal_index=9))
    add("X2_efm_actual", base, lambda d: d["external_formal_memory_effects"][0].update(actual_variable="rust::main::Local(_999)"))
    add("X3_err_result", base, lambda d: d["external_return_relations"][0].update(result_variable="rust::main::Local(_999)"))
    add("X4_err_source", base, lambda d: d["external_return_relations"][0].update(source_actual_variable="rust::main::Local(_999)"))
    add("X5_ene_actual", negative, lambda d: d["external_negative_evidence"][0].update(call_arguments=["rust::main::Local(_999)"]))
    add("X6_dcp_actual", free, lambda d: d["external_deallocation_call_provenance"][0].update(actual_variable="rust::main::Local(_999)"))
    add("X7_underlying_missing_count", base, lambda d: d["external_library_effects"][0]["effect_counts"].update(formal_memory=0))
    add("X8_fabricated_count", base, lambda d: d["external_library_effects"][0]["effect_counts"].update(deallocation=1))
    add("X9_duplicate_effect", base, lambda d: d["external_formal_memory_effects"].append(copy.deepcopy(d["external_formal_memory_effects"][0])))
    def represented(d):
        n = copy.deepcopy(d["nodes"][0])
        n["id"] = "llvm::memcpy::node999"
        d["nodes"].append(n)
    add("X10_represented_source", base, represented)
    add("X11_derived_not_deallocation", base, lambda d: d["external_library_effects"][0]["effect_counts"].update(deallocation=1))
    add("X12_borrowed_not_allocation", borrowed, lambda d: d["external_library_effects"][0]["effect_counts"].update(allocation_return=1))
    def nofree_free_conflict(d):
        free_record = d["external_deallocation_call_provenance"][0]
        record = copy.deepcopy(d["external_negative_evidence"][0])
        record["node"] = free_record["node"]
        record["callee"] = "free"
        record["evidence_kind"] = "no_free_function"
        record.pop("formal_index", None)
        record.pop("actual_variable", None)
        record["call_arguments"] = [free_record["actual_variable"]]
        d["external_negative_evidence"].append(record)
    add("X13_nofree_free_conflict", negative, nofree_free_conflict)
    def fabricate_unknown(d):
        assert not d.get("external_library_effects")
        node = next(n["id"] for n in d["nodes"] if n["id"].startswith("rust::") and "term:call" in n["semantic_labels"])
        d["capabilities"].append(CAP)
        d["external_call_bindings"] = [dict(base["external_call_bindings"][0], node=node, binding_id="ele1:" + node,
                                            rust_function_scope=node.rsplit("::bb", 1)[0], callee="unknown")]
        d["external_library_effects"] = [dict(base["external_library_effects"][0], binding_id="ele1:" + node)]
    add("X14_unknown_envelope", unknown, fabricate_unknown)
    add("X15_second_binding_authority", base, lambda d: d.update(external_return_call_bindings=[{"node": d["external_call_bindings"][0]["node"], "callee": "memcpy", "arguments": d["external_call_bindings"][0]["arguments"], "result_variable": d["external_call_bindings"][0]["result_variable"], "body_status": "bodyless", "basis": "rustc_mir_call_binding_v1"}]))
    add("X_realloc_result", realloc, lambda d: next(b for b in d["external_call_bindings"] if b["callee"] == "realloc").update(result_variable="rust::main::Local(_999)"))
    for name, mutated in cases.items():
        path = out / "logs/ele1-adversarial.json"
        write(path, mutated)
        rc = run([str(checker), str(path), str(query), "--json"], out / "logs" / f"adversarial-{name}.log", root)
        assert rc != 0, f"checker accepted adversarial case: {name}"
    return {"cases": len(cases), "rejected": len(cases), "invalid": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--run", action="store_true")
    args = parser.parse_args()
    assert args.run
    root = args.root.resolve()
    os.chdir(root)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    out = root / "repro-results" / ("bodyless-ffi-ele1-d4-" + stamp)
    (out / "logs").mkdir(parents=True)
    print("D4_GATE_OUT=" + str(out), flush=True)
    gate = {"schema": "cqpl_external_library_effects_d4_gate_v1",
            "status": "FAIL", "baseline_commit": BASE, "capability": CAP, "errors": []}
    saved_generated = {p: (root / p).read_bytes() for p in GENERATED}
    initial_target_dirs = {p.name for p in (root / PREFIX).iterdir() if (p / "target").is_dir()}
    try:
        assert not initial_target_dirs, "fixture target directories exist before gate"
        assert git(root, "rev-parse", "HEAD").decode().strip() == BASE
        assert git(root, "branch", "--show-current").decode().strip() == "cqpl6-bodyless-ffi-effect-gate"
        gate["environment"] = {
            "branch": git(root, "branch", "--show-current").decode().strip(),
            "head": git(root, "rev-parse", "HEAD").decode().strip(),
            "git_status_short": git(root, "status", "--short").decode().splitlines(),
            "rustc": subprocess.check_output(["rustc", "+" + TOOLCHAIN, "--version"], text=True).strip(),
            "cargo": subprocess.check_output(["cargo", "+" + TOOLCHAIN, "--version"], text=True).strip(),
            "python": sys.version.split()[0],
        }
        assert all(saved_generated[p] == git(root, "show", f"{BASE}:{p}") for p in GENERATED)
        gate["preimage_validation"] = preimages(root)
        semantic = read(root / "cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json")["semantic_source_preimages"]
        postimages = {p: sha((root / p).read_bytes()) for p in semantic}
        write(out / "semantic-postimages.json", postimages)
        selected = targets(root)
        (out / "targets.txt").write_text("\n".join(selected) + "\n")
        (out / "runner-status.tsv").write_text("side\ttarget\trc\n")
        gate["software_tests"] = tests(root, out)
        replay_isolated_pair(root, out, selected, semantic)
        gate.update(validate(root, out, selected))
        assert all(sha((root / p).read_bytes()) == h for p, h in postimages.items())
        assert set(git(root, "diff", "--name-only", BASE).decode().splitlines()) <= set(semantic)
        gate["status"] = "PASS"
    except Exception as error:
        gate["errors"].append(str(error))
        print("D4 ELE1 FAIL: " + str(error), flush=True)
    finally:
        for p, data in saved_generated.items():
            (root / p).write_bytes(data)
        for target in (root / PREFIX).iterdir():
            generated = target / "target"
            if generated.is_dir() and target.name not in initial_target_dirs:
                assert not git(root, "ls-files", "--", str(generated.relative_to(root))).strip()
                shutil.rmtree(generated)
        new_target_dirs = {p.name for p in (root / PREFIX).iterdir() if (p / "target").is_dir()} - initial_target_dirs
        source_generated_icfg = sum(1 for p in (root / PREFIX).rglob("global_icfg*.json")
                                    if "target" not in p.relative_to(root / PREFIX).parts)
        hygiene = {"fixture_target_directories": len(new_target_dirs),
                   "pycache": sum(1 for p in [root / "cqpl/scripts", root / "crema/src", root / "cqpl/cqpl_checker/src"] for _ in p.rglob("__pycache__")),
                   "generated_global_icfg": source_generated_icfg,
                   "dirty_generated_entries": sum((root / p).read_bytes() != git(root, "show", f"{BASE}:{p}") for p in GENERATED),
                   "git_diff_check_rc": run(["git", "diff", "--check"], out / "git-diff-check.log", root)}
        gate["hygiene"] = hygiene
        if any(hygiene.values()):
            gate["status"] = "FAIL"
            gate["errors"].append("hygiene failure")
        write(out / "gate.json", gate)
        (out / "gate.json.sha256").write_text(sha((out / "gate.json").read_bytes()) + "  gate.json\n")
        (out / "git-status.txt").write_bytes(git(root, "status", "--short"))
        (out / "tracked.patch").write_bytes(git(root, "diff", "--binary", BASE))
        review = out / "source-review"
        review.mkdir()
        new_files = ["cqpl/D4_EXTERNAL_LIBRARY_EFFECTS_V1_CONSOLIDATION_GATE.md",
                     "cqpl/bodyless_ffi_ele1_d4_consolidation_manifest.json",
                     "cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json",
                     "cqpl/capabilities/external_library_effects_v1.md",
                     "cqpl/scripts/run_bodyless_ffi_ele1_d4_gate.sh",
                     "cqpl/scripts/verify_bodyless_ffi_ele1_d4.py"]
        for path in new_files:
            destination = review / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / path, destination)
        write(review / "changed-files.json", {"tracked_semantic": sorted(read(root / "cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json")["semantic_source_preimages"]),
                                              "new": new_files})
        for path in semantic:
            destination = review / "preimages" / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(git(root, "show", f"{BASE}:{path}"))
            postimage = review / "postimages" / path
            postimage.parent.mkdir(parents=True, exist_ok=True)
            postimage.write_bytes((root / path).read_bytes())
        write(out / "artifact-sha256.json", {str(p.relative_to(out)): sha(p.read_bytes())
                                           for p in out.rglob("*") if p.is_file()})
        sums = [f"{sha(p.read_bytes())}  {p.relative_to(out)}" for p in sorted(out.rglob("*")) if p.is_file()]
        (out / "SHA256SUMS").write_text("\n".join(sums) + "\n")
        package = root / "repro-results" / ("bodyless-ffi-ele1-d4-review-" + stamp + ".tar.gz")
        with tarfile.open(package, "w:gz") as archive:
            archive.add(out, arcname=out.name)
        print("D4_GATE_JSON=" + str(out / "gate.json"), flush=True)
        print("D4_REVIEW_PACKAGE=" + str(package), flush=True)
        print("D4_GATE_STATUS=" + gate["status"], flush=True)
    return 0 if gate["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
