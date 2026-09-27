#!/usr/bin/env python3
"""Fail-closed, independent ERR1 artifact and exact-baseline acceptance audit."""
from __future__ import annotations
import argparse
import copy
import csv
import hashlib
import json
import re
import runpy
import subprocess
from pathlib import Path
from jsonschema import Draft202012Validator

BASELINE = "94a9834b7f36c1ebb63369130f6c669c50846a51"
CAP = "external_return_relations_v1"
PREFIX = "tests_and_target_repos/a-code_c_ffi_bodyless_gate/"
SEMANTIC_FILES = {
    "crema/src/identity.rs", "crema/src/cqpl_export.rs",
    "cqpl/cqpl_checker/src/kripke.rs", "cqpl/cqpl_checker/src/main.rs",
    "cqpl/schemas/annotated_icfg_v2.schema.json",
}
CONTRACT = {
    "memcpy": (3, "memcpy_return_dst_v1", "exact_argument_alias", "posix_memcpy_returns_destination_v1"),
    "memmove": (3, "memmove_return_dst_v1", "exact_argument_alias", "posix_memmove_returns_destination_v1"),
    "memset": (3, "memset_return_dst_v1", "exact_argument_alias", "posix_memset_returns_destination_v1"),
    "memchr": (3, "memchr_return_derived_v1", "nullable_derived_alias", "posix_memchr_nullable_derived_return_v1"),
    "strchr": (2, "strchr_return_derived_v1", "nullable_derived_alias", "posix_strchr_nullable_derived_return_v1"),
    "getenv": (1, "getenv_borrowed_environment_v1", "nullable_borrowed_external", "posix_getenv_nullable_borrowed_environment_v1"),
}


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read(path):
    return json.loads(path.read_text(), object_pairs_hook=unique)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], capture_output=True)


def identities(identity, variable, access=True):
    """Resolve ordinary locals and stack references without inventing identities."""
    out, seen, pending = set(), set(), [variable]
    while pending:
        variable = pending.pop()
        if variable in seen:
            continue
        seen.add(variable)
        for field in (["points_to", "access_bases"] if access else ["points_to"]):
            for item in identity.get(field, []):
                if item["variable"] == variable:
                    out.update(item["allocations"])
        for item in identity.get("stack_refs", []):
            if item["variable"] != variable:
                continue
            for place in item["places"]:
                if not place["projection"]:
                    base = place["base"]
                    if base["kind"] == "rust":
                        pending.append(f"rust::{base['function']}::Local(_{base['local']})")
                else:
                    for projected in identity.get("place_points_to", []):
                        if projected["place"] == place:
                            out.update(projected["allocations"])
    return out


def query_docs(directory):
    return {p.stem: read(p) for p in sorted((directory / "queries").glob("*.json"))
            if not p.name.endswith(".explain.json")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("root", "run-root", "manifest", "status-tsv", "out"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("cqpl-test-rc", "crema-test-rc", "legacy-checker-rc"):
        parser.add_argument("--" + name, type=int, required=True)
    args = parser.parse_args()
    root, run = args.root.resolve(), args.run_root.resolve()
    errors = []

    def require(condition, message):
        if not condition:
            errors.append(message)

    gate = {
        "schema": "cqpl_external_return_relations_d2_gate_v1", "status": "FAIL",
        "baseline_commit": BASELINE, "capability": CAP,
        "software_tests": {"cqpl_exit_code": args.cqpl_test_rc, "crema_exit_code": args.crema_test_rc,
                           "baseline_efm2_acceptance_exit_code": args.legacy_checker_rc},
        "errors": errors,
    }
    try:
        for name, rc in gate["software_tests"].items():
            require(rc == 0, f"{name}={rc}")
        focused = {
            "err1-producer-tests.log": ["err1_closed_classifier_and_zero_orthogonality", "err1_aliases_preserve_candidates_without_granting_derived_base_frees", "err1_unknown_zero_and_interior_source_provenance", "err1_records_bind_real_result_and_source_without_projected_guesses"],
            "err1-checker-tests.log": ["err1_accepts_and_rejects_closed_tuple_and_call_identity_attacks", "err1_derived_and_borrowed_do_not_certify_base_deallocation", "err1_raw_boundary_is_atomic_nonempty_and_requires_mir"],
        }
        test_results = {}
        for log, names in focused.items():
            content = (run / "logs" / log).read_text()
            for name in names:
                okay = re.search(r"^test [^\n]*::" + re.escape(name) + r" \.\.\. ok$", content, re.M) is not None
                require(okay, f"focused test missing/failed: {name}")
                test_results[name] = okay
        gate["focused_tests"] = {"groups_checked": len(test_results), "groups_passed": sum(test_results.values()), "groups": test_results}
        require(git(root, "merge-base", "--is-ancestor", BASELINE, "HEAD").returncode == 0, "baseline ancestry")
        gate["head_commit"] = git(root, "rev-parse", "HEAD").stdout.decode().strip()
        gate["branch"] = git(root, "branch", "--show-current").stdout.decode().strip()
        require(gate["branch"] == "cqpl6-bodyless-ffi-effect-gate", "wrong branch")
        pre = read(root / "cqpl/bodyless_ffi_err1_d2_preimage_sha256.json")
        require(pre["baseline_commit"] == BASELINE and pre["head_commit"] == BASELINE, "preimage baseline")
        require(set(pre["source_preimages"]) == SEMANTIC_FILES, "semantic source preimage closure")
        checked = {}
        for path, expected in pre["source_preimages"].items():
            result = git(root, "show", f"{BASELINE}:{path}")
            actual = digest(result.stdout) if result.returncode == 0 else None
            matches = result.returncode == 0 and actual == expected
            require(matches, f"preimage mismatch/missing: {path}")
            checked[path] = {"recorded_sha256": expected, "baseline_sha256": actual,
                             "git_show_rc": result.returncode, "matches": matches,
                             "postimage_sha256": digest((root / path).read_bytes())}
        gate["preimage_validation"] = {"checked": len(checked), "mismatches": sum(not v["matches"] for v in checked.values()),
                                       "errors": sum(v["git_show_rc"] != 0 for v in checked.values()), "files": checked}
        # Auxiliary edits must be test-constructor initialization only, byte-for-byte.
        aux = read(root / "cqpl/bodyless_ffi_err1_d2_test_constructor_preimages.json")
        aux_results = {}
        require(set(aux) == {"cqpl/cqpl_checker/src/model_checker.rs", "cqpl/cqpl_checker/src/explain.rs", "cqpl/cqpl_checker/tests/ctl_semantic_laws.rs", "cqpl/cqpl_checker/tests/v6i_query_no_refutation.rs"}, "test constructor preimage closure")
        for path, expected in aux.items():
            baseline = git(root, "show", f"{BASELINE}:{path}")
            current = (root / path).read_bytes()
            stripped = b"".join(line for line in current.splitlines(keepends=True)
                                if line.strip() not in (b"external_return_relations: vec![],", b"external_return_call_bindings: vec![],"))
            okay = baseline.returncode == 0 and digest(baseline.stdout) == expected and stripped == baseline.stdout
            require(okay, f"non-constructor truth/explanation change: {path}")
            aux_results[path] = okay
        gate["truth_implementation_preservation"] = aux_results
        changed = set(git(root, "diff", "--name-only", BASELINE).stdout.decode().splitlines())
        allowed_tracked = SEMANTIC_FILES | set(aux) | {"crema/ffi_functions.json", "crema/global_icfg.json", "crema/global_icfg_nodes_edges.dot", "SVF-example/callgraph_initial.dot.dot"}
        require(changed <= allowed_tracked, f"unfrozen tracked change surface: {sorted(changed - allowed_tracked)}")
        frozen = pre["frozen_capability_hashes"]
        frozen_results = {}
        for path, expected in frozen.items():
            baseline = git(root, "show", f"{BASELINE}:{path}")
            okay = baseline.returncode == 0 and digest(baseline.stdout) == expected and digest((root / path).read_bytes()) == expected
            require(okay, f"D1 frozen file changed: {path}")
            frozen_results[path] = okay
        required_frozen = {
            "cqpl/D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS_GATE.md", "cqpl/bodyless_ffi_efm2_d1_fixture_manifest.json",
            "cqpl/bodyless_ffi_efm2_d1_preimage_sha256.json", "cqpl/capabilities/external_formal_memory_effects_v1.md",
            "cqpl/capabilities/external_formal_memory_effects_v2.md", "cqpl/scripts/run_bodyless_ffi_efm2_d1_gate.sh",
            "cqpl/scripts/verify_bodyless_ffi_efm2_d1.py",
        }
        tracked = git(root, "ls-tree", "-r", "--name-only", BASELINE, PREFIX).stdout.decode().splitlines()
        required_frozen.update(p for p in tracked if re.match(re.escape(PREFIX) + r"b(?:4[4-9]|5[01])_", p))
        require(set(frozen) == required_frozen, "D1 frozen-file closure")
        gate["d1_preservation"] = {"checked": len(frozen_results), "frozen_files_modified": sum(not v for v in frozen_results.values()), "files": frozen_results}
        start_images = read(run / "source-postimages-at-start.json")
        require(SEMANTIC_FILES | set(aux) | set(frozen) <= set(start_images), "run-start source hash closure")
        changed_during_run = [p for p, h in start_images.items() if not (root / p).is_file() or digest((root / p).read_bytes()) != h]
        require(not changed_during_run, f"source/harness changed during gate: {changed_during_run}")
        gate["run_source_stability"] = {"files_checked": len(start_images), "changed": len(changed_during_run), "changed_paths": changed_during_run, "start_sha256": start_images}
        manifest = read(args.manifest)
        require(manifest["schema"] == "cqpl_bodyless_ffi_err1_d2_fixture_manifest_v1" and manifest["baseline_commit"] == BASELINE, "manifest baseline/schema")
        fixtures = manifest["fixtures"]
        require(len(fixtures) == 10 and {int(f["target"].split("_")[0][1:]) for f in fixtures} == set(range(52, 62)), "fixture closure")
        fixture_contracts = {
            52: ("memcpy", "exact_argument_alias"), 53: ("memmove", "exact_argument_alias"),
            54: ("memset", "exact_argument_alias"), 55: ("memchr", "nullable_derived_alias"),
            56: ("strchr", "nullable_derived_alias"), 57: ("getenv", "nullable_borrowed_external"),
            58: ("memmove", "exact_argument_alias"), 59: ("memchr", None),
            60: ("memmove", None), 61: ("opaque_ptr_fn", None),
        }
        for fixture in fixtures:
            number = int(fixture["target"].split("_")[0][1:])
            require((fixture["callee"], fixture["relation_kind"]) == fixture_contracts[number], f"fixture contract {number}")
            if 52 <= number <= 56:
                require(fixture.get("uaf") == {"truth": "unk", "subresult": "unk_true", "direction": "true", "strength": "strong_abstract_evidence"}, f"frozen D2 UAF oracle {number}")
        existing = sorted({p[len(PREFIX):].split("/")[0] for p in tracked})
        require(len(existing) == 65, "baseline target count !=65")
        require((run / "existing-targets.txt").read_text().splitlines() == existing, "baseline target inventory")
        status = {}
        for row in csv.DictReader(args.status_tsv.open(), delimiter="\t"):
            key = (row["side"], row["target"])
            require(key not in status, f"duplicate runner status: {key}")
            status[key] = int(row["rc"])
        expected_status = {("baseline", t) for t in existing} | {("candidate", t) for t in existing + [f["target"] for f in fixtures]}
        require(set(status) == expected_status, "runner status closure")
        for key, rc in status.items():
            require(rc == 0, f"runner failure {key}: {rc}")
        schema = read(root / "cqpl/schemas/annotated_icfg_v2.schema.json")
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        query_names = {p.stem for p in (root / "cqpl/queries_v2").glob("*.cqpl")}
        require(len(query_names) == 12, "query inventory !=12")
        artifacts, queries = {}, {}
        query_errors = 0
        for side, target in sorted(expected_status):
            directory = run / side / target
            artifact = read(directory / "annotated_icfg_v2.json")
            schema_errors = list(validator.iter_errors(artifact))
            require(not schema_errors, f"schema {side}/{target}: {[e.message for e in schema_errors][:3]}")
            artifacts[side, target] = artifact
            queries[side, target] = query_docs(directory)
            docs = queries[side, target]
            bad = set(docs) != query_names or any(d.get("result") not in {"tt", "ff", "unk"} or "assessment" not in d for d in docs.values())
            query_errors += int(bad)
            require(not bad, f"query output closure/error {side}/{target}")
        truth, assessment = [], []
        for target in existing:
            before, after = queries["baseline", target], queries["candidate", target]
            for name in sorted(query_names):
                if before[name]["result"] != after[name]["result"]:
                    truth.append({"target": target, "query": name, "baseline": before[name]["result"], "candidate": after[name]["result"]})
                if before[name]["assessment"] != after[name]["assessment"]:
                    assessment.append({"target": target, "query": name, "baseline": before[name]["assessment"], "candidate": after[name]["assessment"]})
            require(artifacts["baseline", target].get("allocations", []) == artifacts["candidate", target].get("allocations", []), f"existing allocation catalog changed: {target}")
        gate["baseline"] = {"targets": len(existing), "query_cells": sum(len(queries["baseline", t]) for t in existing)}
        gate["differential"] = {"truth_deltas": len(truth), "assessment_deltas": len(assessment), "query_errors": query_errors,
                                "truth_changes": truth, "assessment_changes": assessment}
        require(gate["baseline"]["query_cells"] == 780 and not truth and not assessment and query_errors == 0, "existing differential is not zero")
        kinds = {kind: 0 for kind in ("exact_argument_alias", "nullable_derived_alias", "nullable_borrowed_external")}
        seen_callees, invalid, safety = set(), [], {"fresh_allocations_from_err1": 0, "derived_alias_base_free_certificates": 0, "borrowed_external_allocations": 0, "allocator_family_mutations_from_err1": 0}
        for (side, target), artifact in artifacts.items():
            if side != "candidate":
                continue
            nodes = {n["id"]: n for n in artifact["nodes"]}
            variables = {v["id"] for v in artifact["variables"]}
            records, bindings = artifact.get("external_return_relations", []), artifact.get("external_return_call_bindings", [])
            require(bool(records) == (CAP in artifact["capabilities"]) == bool(bindings), f"ERR1 capability atomicity {target}")
            calls = {b["node"]: b for b in bindings}
            require(len(calls) == len(bindings) == len(records), f"ERR1 binding closure {target}")
            seen = set()
            for record in records:
                start = len(errors)
                callee, kind = record["callee"], record["relation_kind"]
                expected = CONTRACT.get(callee)
                require(expected is not None, f"unclosed callee {target}: {callee}")
                if expected is None:
                    invalid.append(record)
                    continue
                seen_callees.add(callee)
                kinds[kind] += 1
                borrowed = callee == "getenv"
                require((record["arity"], record["semantic_class"], kind, record["semantic_sources"]) == (expected[0], expected[1], expected[2], [expected[3]]), f"closed tuple {target}")
                require(record["basis"] == "crema_err1_closed_contract_v1" and record["ownership"] == ("borrowed_external" if borrowed else "alias_existing") and record["nullability"] == ("same_as_source" if kind == "exact_argument_alias" else "nullable"), f"ERR1 contract {target}")
                node = nodes[record["node"]]
                require("term:call" in node["semantic_labels"], f"not call {target}")
                require(not any(n.startswith(f"llvm::{callee}::") for n in nodes), f"represented body {target}")
                call = calls[record["node"]]
                require(call["body_status"] == "bodyless" and call["basis"] == "rustc_mir_call_binding_v1" and call["callee"] == callee and len(call["arguments"]) == expected[0] and call["result_variable"] == record["result_variable"], f"MIR binding {target}")
                scope = record["node"].rsplit("::bb", 1)[0]
                def valid_local(v):
                    return v in variables and re.fullmatch(re.escape(scope) + r"::Local\(_[0-9]+\)", v) is not None
                require(valid_local(record["result_variable"]), f"result scope {target}")
                if borrowed:
                    require("source_formal_index" not in record and "source_actual_variable" not in record, f"getenv source alias {target}")
                else:
                    require(record.get("source_formal_index") == 0 and valid_local(record.get("source_actual_variable", "")) and record["source_actual_variable"] == call["arguments"][0], f"source binding {target}")
                key = (record["node"], callee, record["result_variable"], kind)
                require(key not in seen, f"duplicate ERR1 {target}")
                seen.add(key)
                fresh = sum(a.get("site", {}).get("node_id") == record["node"] for a in artifact.get("allocations", []))
                fresh += sum(label["predicate"] == "alloc" for label in node.get("allocation_labels", []))
                safety["fresh_allocations_from_err1"] += fresh
                require(fresh == 0, f"ERR1 fresh allocation {target}")
                post, prestate = node.get("identity", {}), node.get("event_identity", {})
                result_access = identities(post, record["result_variable"])
                result_base = identities(post, record["result_variable"], False)
                if kind == "exact_argument_alias":
                    source_access = identities(prestate, record["source_actual_variable"])
                    source_base = identities(prestate, record["source_actual_variable"], False)
                    require(result_access == source_access and result_base == source_base, f"exact alias provenance changed {target}")
                elif kind == "nullable_derived_alias":
                    safety["derived_alias_base_free_certificates"] += len(result_base)
                    require(not result_base and result_access == identities(prestate, record["source_actual_variable"]), f"derived provenance {target}")
                else:
                    safety["borrowed_external_allocations"] += len(result_access) + fresh
                    require(not result_access, f"borrowed allocation {target}")
                if len(errors) != start:
                    invalid.append(record)
        require(seen_callees == set(CONTRACT), "closed ERR1 callee coverage")
        gate["closed_contract"] = {"callees": len(seen_callees), "relations": len(CONTRACT), "invalid_records": len(invalid)}
        gate["relation_kinds"], gate["identity_safety"] = kinds, safety
        outcomes = {}
        for fixture in fixtures:
            target = fixture["target"]
            start = len(errors)
            artifact = artifacts["candidate", target]
            records = artifact.get("external_return_relations", [])
            selected = [r for r in records if r["callee"] == fixture["callee"]]
            if fixture["relation_kind"] is None:
                require(not selected, f"control ERR1 relation {target}")
            else:
                require(len(selected) == 1 and selected[0]["relation_kind"] == fixture["relation_kind"], f"fixture relation {target}")
            if "uaf" in fixture:
                doc = queries["candidate", target]["use_after_free_alloc_state"]
                require(doc["result"] == fixture["uaf"]["truth"] and all(doc["assessment"].get(k) == fixture["uaf"][k] for k in ("subresult", "direction", "strength")), f"UAF assessment {target}")
                explanation = read(run / "candidate" / target / "queries/use_after_free_alloc_state.explain.json")
                source = identities(next(n for n in artifact["nodes"] if n["id"] == selected[0]["node"])["event_identity"], selected[0]["source_actual_variable"])
                require(bool(source) and any(f.get("allocation") in source and f.get("kind") == "drop_then_use_without_reallocation" and f.get("strength") == "strong_abstract_evidence" and f.get("use_node") != selected[0]["node"] for f in explanation["supporting_findings"]), f"return-use UAF proof {target}")
            if target.startswith(("b58_", "b59_")):
                require(not artifact.get("external_formal_memory_effects"), f"zero memory effect {target}")
            if target.startswith("b58_"):
                call_node = next(n for n in artifact["nodes"] if n["id"] == selected[0]["node"])
                base = identities(call_node["event_identity"], selected[0]["source_actual_variable"], False)
                require(bool(base) and any(label["predicate"] == "drop" and label["allocation"] in base for n in artifact["nodes"] for label in n.get("allocation_labels", [])), "zero exact alias lost source/deallocation identity")
            if target.startswith("b57_"):
                require(not artifact.get("allocations"), f"borrowed allocation catalog {target}")
            if target.startswith("b60_"):
                require(any(n["id"].startswith("llvm::memmove::") for n in artifact["nodes"]), "body control not represented")
            if target.startswith(("b59_", "b60_", "b61_")):
                require(all(a.get("site", {}).get("callee") != fixture["callee"] for a in artifact.get("allocations", [])), f"control fresh return allocation {target}")
            outcomes[target] = {"status": "PASS" if len(errors) == start else "FAIL", "records": len(selected), "errors": errors[start:]}
        gate["new_fixtures"] = {"expected": 10, "passed": sum(o["status"] == "PASS" for o in outcomes.values()), "failed": sum(o["status"] != "PASS" for o in outcomes.values()), "outcomes": outcomes}
        # Strict schema attacks are replayed independently of Rust unit validation.
        exemplar = artifacts["candidate", fixtures[0]["target"]]
        attacks = [("callee", "other"), ("arity", 2), ("semantic_class", "wrong"), ("relation_kind", "nullable_derived_alias"), ("source_formal_index", 1), ("nullability", "nullable"), ("ownership", "borrowed_external"), ("basis", "wrong"), ("semantic_sources", [])]
        rejected = 0
        for field, value in attacks:
            bad = copy.deepcopy(exemplar)
            bad["external_return_relations"][0][field] = value
            rejected += int(not validator.is_valid(bad))
        for mode in range(4):
            bad = copy.deepcopy(exemplar)
            if mode == 0:
                bad["capabilities"].remove(CAP)
            elif mode == 1:
                del bad["external_return_relations"]
            elif mode == 2:
                bad["external_return_relations"] = []
            else:
                del bad["external_return_relations"][0]["source_actual_variable"]
            rejected += int(not validator.is_valid(bad))
        require(rejected == len(attacks) + 4, "schema attack accepted")
        gate["schema_validation"] = {"artifacts_checked": len(artifacts), "adversarial_checked": len(attacks) + 4, "adversarial_rejected": rejected}
        # Frozen D1 contract is consumed unchanged, not redefined for ERR1.
        efm_contract = runpy.run_path(str(root / "cqpl/scripts/verify_bodyless_ffi_efm2_d1.py"))["CLOSED_CONTRACT"]
        efm_seen, efm_invalid = set(), 0
        for (side, target), artifact in artifacts.items():
            if side != "candidate":
                continue
            for record in artifact.get("external_formal_memory_effects", []):
                key = (record["callee"], record["formal_index"], record["access"])
                expected = efm_contract.get(key)
                okay = expected is not None and all(record.get(k) == v for k, v in expected.items())
                efm_invalid += int(not okay)
                efm_seen.add(key)
        require(efm_invalid == 0 and efm_seen == set(efm_contract), "EFM2 closed contract preservation")
        gate["efm2_preservation"] = {"callees": len({k[0] for k in efm_seen}), "tuples": len(efm_seen), "invalid_records": efm_invalid}
        legacy = copy.deepcopy(artifacts["baseline", "b14a_bodyless_strlen"])
        legacy["capabilities"].remove("external_formal_memory_effects_v2")
        legacy["capabilities"].append("external_formal_memory_effects_v1")
        for record in legacy["external_formal_memory_effects"]:
            record.pop("extent_kind")
            record.pop("extent_argument_index", None)
            record["basis"] = "crema_efm1_closed_contract_v1"
        legacy_path = run / "legacy-efm1-input.json"
        legacy_path.write_text(json.dumps(legacy, indent=2) + "\n")
        require(validator.is_valid(legacy), "legacy EFM1 schema acceptance")
        completed = subprocess.run([str(root / "cqpl/cqpl_checker/target/debug/cqpl_checker"), str(legacy_path), str(root / "cqpl/queries_v2/use_after_free_alloc_state.cqpl"), "--json"], capture_output=True)
        (run / "legacy-efm1-candidate-checker.json").write_bytes(completed.stdout)
        (run / "logs/legacy-efm1-candidate-checker.log").write_bytes(completed.stderr)
        gate["efm1_acceptance"] = {"checker_exit_code": completed.returncode, "schema_valid": validator.is_valid(legacy)}
        require(completed.returncode == 0, "legacy EFM1 checker acceptance")
        dirs = [root / PREFIX / f["target"] for f in fixtures]
        pycache = sorted({str(p.relative_to(root)) for d in dirs + [root / "cqpl/scripts", root / "crema/src", root / "cqpl/cqpl_checker/src"] for p in d.rglob("__pycache__")})
        generated = [str(p.relative_to(root)) for d in dirs for p in d.rglob("global_icfg*.json")]
        targets = [str(p.relative_to(root)) for d in dirs for p in d.rglob("target") if p.is_dir() or p.is_symlink()]
        dirty = {}
        for path in ("SVF-example/callgraph_initial.dot.dot", "crema/ffi_functions.json", "crema/global_icfg.json", "crema/global_icfg_nodes_edges.dot"):
            result = git(root, "status", "--porcelain", "--", path)
            dirty[path] = result.returncode != 0 or bool(result.stdout.strip())
        diff = git(root, "diff", "--check")
        hygiene = {"fixture_target_directories": len(targets), "pycache": len(pycache), "generated_global_icfg": len(generated), "dirty_callgraph_entries": int(dirty["SVF-example/callgraph_initial.dot.dot"]), "dirty_generated_entries": sum(dirty.values()), "git_diff_check_rc": diff.returncode, "targets": targets, "pycache_paths": pycache, "generated_paths": generated, "dirty_files": dirty}
        gate["hygiene"] = hygiene
        require(not targets and not pycache and not generated and not any(dirty.values()) and diff.returncode == 0, "generated artifact hygiene")
    except Exception as error:
        errors.append(f"fail-closed verification error: {type(error).__name__}: {error}")
    gate["status"] = "PASS" if not errors else "FAIL"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(gate, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": gate["status"], "gate_json": str(args.out), "errors": errors}, indent=2))
    return 0 if not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
