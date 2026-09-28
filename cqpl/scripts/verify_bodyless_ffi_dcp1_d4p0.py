#!/usr/bin/env python3
"""D4-P0 exact-baseline provenance gate. No D4 consolidation is performed."""
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

BASE = "8ebf4a9d7b5718d5632e946813710e45d4d9fe85"
CAP = "external_deallocation_call_provenance_v1"
PAYLOAD = "external_deallocation_call_provenance"
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
    manifest = read(root / "cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json")
    assert manifest["baseline_commit"] == BASE
    expected = {"crema/src/cqpl_export.rs", "cqpl/cqpl_checker/src/kripke.rs",
                "cqpl/cqpl_checker/src/main.rs", "cqpl/schemas/annotated_icfg_v2.schema.json"}
    assert set(manifest["semantic_source_preimages"]) == expected
    files = {}
    for path, frozen in manifest["semantic_source_preimages"].items():
        base_bytes = git(root, "show", f"{BASE}:{path}")
        assert sha(base_bytes) == frozen, f"preimage mismatch: {path}"
        files[path] = {"preimage_sha256": frozen, "postimage_sha256": sha((root / path).read_bytes())}
    return {"checked": len(files), "mismatches": 0, "errors": 0, "files": files}


def tests(root, out):
    results = {}
    for name, manifest, focus in [
        ("focused_producer", "crema/Cargo.toml", "dcp1"),
        ("focused_checker", "cqpl/cqpl_checker/Cargo.toml", "dcp1"),
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
    return results


def targets(root):
    paths = git(root, "ls-tree", "-r", "--name-only", BASE, PREFIX).decode().splitlines()
    selected = sorted({p[len(PREFIX):].split("/")[0] for p in paths})
    assert len(selected) == 83, f"baseline target count {len(selected)} != 83"
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
    with tempfile.TemporaryDirectory(prefix="cqpl-dcp1-d4p0-") as temp:
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
    artifact.pop(PAYLOAD, None)
    artifact["capabilities"] = [cap for cap in artifact["capabilities"] if cap != CAP]
    return artifact


def query_docs(path):
    return {p.stem: read(p) for p in (path / "queries").glob("*.json")
            if not p.name.endswith(".explain.json")}


def validate(root, out, selected):
    from jsonschema import Draft202012Validator
    schema = read(root / "cqpl/schemas/annotated_icfg_v2.schema.json")
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    frozen = read(root / "cqpl/bodyless_ffi_dcp1_d4p0_inventory.json")
    assert frozen["baseline_commit"] == BASE and len(frozen["callsites"]) == 44
    assert frozen["counts"] == {"direct_bodyless_free_calls": 44,
        "with_allocation_specific_drop": 28,
        "without_allocation_specific_drop_cr1": 8,
        "without_allocation_specific_drop_no_cr1": 8,
        "actual_variable_present": 42,
        "actual_variable_null_due_to_catalog_absence": 2}
    frozen_calls = sorted(frozen["callsites"], key=lambda r: (r["target"], r["node"]))
    observed = sorted((call for target in selected for call in free_calls(out / "baseline" / target)),
                      key=lambda r: (r["target"], r["node"]))
    assert observed == frozen_calls, "frozen 44-call inventory differs from fresh baseline"
    records = present = null_due_to_catalog_absence = 0
    truth = assessment = errors = projection_differences = 0
    ordinary = allocation_labels = dispositions = 0
    d1 = d2 = d3 = 0
    for target in selected:
        before = read(out / "baseline" / target / "annotated_icfg_v2.json")
        after = read(out / "candidate" / target / "annotated_icfg_v2.json")
        validator.validate(after)
        expected_nodes = {r["node"] for r in frozen_calls if r["target"] == target}
        payload = after.get(PAYLOAD, [])
        assert (CAP in after["capabilities"]) == bool(payload), f"capability mismatch: {target}"
        actual_nodes = [r["node"] for r in payload]
        assert len(actual_nodes) == len(set(actual_nodes)), f"duplicate records: {target}"
        assert set(actual_nodes) == expected_nodes, f"record closure: {target}"
        variables = {v["id"]: v for v in after["variables"]}
        expected_by_node = {r["node"]: r for r in frozen_calls if r["target"] == target}
        candidate_calls = {r["node"]: r for r in free_calls(out / "candidate" / target)}
        assert candidate_calls == expected_by_node, f"candidate structured-call drift: {target}"
        for record in payload:
            assert record["callee"] == "free" and record["arity"] == 1 and record["formal_index"] == 0
            assert (record["family"], record["operation"], record["language"]) == ("c_malloc", "free", "c")
            assert (record["body_status"], record["certainty"], record["basis"]) == ("bodyless", "may_effect", "rust_mir_exact_external_free_call_v1")
            frozen_call = expected_by_node[record["node"]]
            assert record["actual_variable"] == frozen_call["actual_variable"], f"actual mismatch: {target}/{record['node']}"
            if record["actual_variable"] is not None:
                present += 1
                assert variables[record["actual_variable"]]["language"] == "rust"
            else:
                null_due_to_catalog_absence += 1
                # Audit the exact MIR operand and existing catalog, independently
                # of the producer's optional actual-variable serialization.
                assert frozen_call["mir_actual_operand"] and frozen_call["canonical_actual_candidate"]
                assert frozen_call["canonical_actual_candidate"] not in variables
                assert frozen_call["null_reason"] == "canonical_scoped_variable_absent_from_baseline_catalog"
        records += len(payload)
        if projection(after) != before:
            projection_differences += 1
        bn = {n["id"]: n for n in before["nodes"]}
        for node in after["nodes"]:
            old = bn[node["id"]]
            ordinary += node.get("labels", []) != old.get("labels", [])
            allocation_labels += node.get("allocation_labels", []) != old.get("allocation_labels", [])
            dispositions += node.get("allocation_disposition", []) != old.get("allocation_disposition", [])
        d1 += after.get("external_formal_memory_effects") != before.get("external_formal_memory_effects")
        d2 += (after.get("external_return_relations"), after.get("external_return_call_bindings")) != (before.get("external_return_relations"), before.get("external_return_call_bindings"))
        d3 += after.get("external_negative_evidence") != before.get("external_negative_evidence")
        bq, aq = query_docs(out / "baseline" / target), query_docs(out / "candidate" / target)
        assert set(bq) == set(aq) and len(bq) == 12, f"query surface: {target}"
        for name in bq:
            truth += bq[name].get("result") != aq[name].get("result")
            assessment += bq[name].get("assessment") != aq[name].get("assessment")
            errors += bq[name].get("result") not in ("tt", "ff", "unk") or aq[name].get("result") not in ("tt", "ff", "unk")
    assert (records, present, null_due_to_catalog_absence) == (44, 42, 2)
    assert not any((truth, assessment, errors, projection_differences,
                                      ordinary, allocation_labels, dispositions, d1, d2, d3))
    adversarial = adversarial_checks(root, out, validator)
    return {"inventory": {"direct_bodyless_free_calls": 44, "with_allocation_specific_drop": 28,
                          "without_allocation_specific_drop_cr1": 8, "without_allocation_specific_drop_no_cr1": 8},
            "records": {"expected": 44, "produced": records, "invalid": 0, "duplicates": 0,
                        "actual_variable_present": present,
                        "actual_variable_null_due_to_catalog_absence": null_due_to_catalog_absence},
            "semantic_invariance": {"ordinary_events_created": ordinary, "allocation_labels_added": allocation_labels,
                                    "allocation_dispositions_added": dispositions,
                                    "semantic_projection_differences": projection_differences},
            "baseline": {"targets": len(selected), "query_cells": len(selected) * 12},
            "differential": {"truth_deltas": truth, "assessment_deltas": assessment, "query_errors": errors},
            "adversarial_tests": adversarial,
            "preservation": {"d1_unexpected_changes": d1, "d2_unexpected_changes": d2,
                             "d3_unexpected_changes": d3}}


def adversarial_checks(root, out, validator):
    """Mutate one real candidate artifact; require schema or executable rejection."""
    source = read(out / "candidate/b02_malloc_free_clean/annotated_icfg_v2.json")
    assert len(source[PAYLOAD]) == 1
    query = sorted((root / "cqpl/queries_v2").glob("*.cqpl"))[0]
    checker = root / "cqpl/cqpl_checker/target/debug/cqpl_checker"
    assert checker.is_file()
    attacks = {}
    for field, value in [
        ("callee", "my_free"), ("arity", 2), ("formal_index", 1),
        ("family", "rust_global"), ("operation", "drop"), ("language", "rust"),
        ("body_status", "represented"), ("certainty", "must_effect"),
        ("basis", "pretty_call_text"), ("node", "rust::main::bb999"),
        ("actual_variable", "rust::main::Local(_999)"),
    ]:
        mutated = copy.deepcopy(source)
        mutated[PAYLOAD][0][field] = value
        attacks[field + "=" + str(value)] = mutated
    extra = copy.deepcopy(source)
    extra[PAYLOAD][0]["extra"] = 1
    attacks["additional_property"] = extra
    duplicated = copy.deepcopy(source)
    duplicated[PAYLOAD].append(copy.deepcopy(duplicated[PAYLOAD][0]))
    attacks["duplicate_record"] = duplicated
    no_cap = copy.deepcopy(source)
    no_cap["capabilities"].remove(CAP)
    attacks["payload_without_capability"] = no_cap
    no_payload = copy.deepcopy(source)
    del no_payload[PAYLOAD]
    attacks["capability_without_payload"] = no_payload
    non_rust = copy.deepcopy(source)
    non_rust[PAYLOAD][0]["node"] = next(n["id"] for n in source["nodes"] if n["id"].startswith("llvm::"))
    attacks["non_rust_node"] = non_rust
    noncall = copy.deepcopy(source)
    noncall[PAYLOAD][0]["node"] = next(n["id"] for n in source["nodes"]
                                          if n["id"].startswith("rust::") and "term:call" not in n.get("semantic_labels", []))
    attacks["non_call_node"] = noncall
    crossed = copy.deepcopy(source)
    crossed["variables"].append({"id": "rust::other::Local(_1)", "language": "rust"})
    crossed[PAYLOAD][0]["actual_variable"] = "rust::other::Local(_1)"
    attacks["cross_function_actual"] = crossed
    represented = copy.deepcopy(source)
    body = copy.deepcopy(next(n for n in source["nodes"] if n["id"].startswith("llvm::")))
    body["id"] = "llvm::free::node999"
    represented["nodes"].append(body)
    attacks["represented_body"] = represented
    for name, mutated in attacks.items():
        candidate = out / "logs" / "dcp1-adversarial.json"
        write(candidate, mutated)
        schema_rejects = not validator.is_valid(mutated)
        rc = run([str(checker), str(candidate), str(query), "--json"],
                 out / "logs" / f"adversarial-{re.sub('[^A-Za-z0-9]+', '_', name)}.log", root)
        assert schema_rejects or rc != 0, f"adversarial input accepted: {name}"
        assert rc != 0, f"checker accepted adversarial input: {name}"
    return {"cases": len(attacks), "rejected": len(attacks), "invalid": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--run", action="store_true")
    args = parser.parse_args()
    assert args.run
    root = args.root.resolve()
    os.chdir(root)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    out = root / "repro-results" / ("bodyless-ffi-dcp1-d4p0-" + stamp)
    (out / "logs").mkdir(parents=True)
    print("D4P0_GATE_OUT=" + str(out), flush=True)
    gate = {"schema": "cqpl_bodyless_deallocation_call_provenance_d4p0_gate_v1",
            "status": "FAIL", "baseline_commit": BASE, "capability": CAP, "errors": []}
    saved_generated = {p: (root / p).read_bytes() for p in GENERATED}
    initial_target_dirs = {p.name for p in (root / PREFIX).iterdir() if (p / "target").is_dir()}
    try:
        assert git(root, "rev-parse", "HEAD").decode().strip() == BASE
        assert git(root, "branch", "--show-current").decode().strip() == "cqpl6-bodyless-ffi-effect-gate"
        assert all(saved_generated[p] == git(root, "show", f"{BASE}:{p}") for p in GENERATED)
        gate["preimage_validation"] = preimages(root)
        semantic = read(root / "cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json")["semantic_source_preimages"]
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
        print("D4-P0 FAIL: " + str(error), flush=True)
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
        new_files = ["cqpl/bodyless_ffi_dcp1_d4p0_inventory.json",
                     "cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json",
                     "cqpl/capabilities/external_deallocation_call_provenance_v1.md",
                     "cqpl/scripts/run_bodyless_ffi_dcp1_d4p0_gate.sh",
                     "cqpl/scripts/verify_bodyless_ffi_dcp1_d4p0.py"]
        for path in new_files:
            destination = review / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / path, destination)
        write(review / "changed-files.json", {"tracked_semantic": sorted(read(root / "cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json")["semantic_source_preimages"]),
                                              "new": new_files})
        write(out / "artifact-sha256.json", {str(p.relative_to(out)): sha(p.read_bytes())
                                           for p in out.rglob("*") if p.is_file()})
        print("D4P0_GATE_JSON=" + str(out / "gate.json"), flush=True)
        print("D4P0_GATE_STATUS=" + gate["status"], flush=True)
    return 0 if gate["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
