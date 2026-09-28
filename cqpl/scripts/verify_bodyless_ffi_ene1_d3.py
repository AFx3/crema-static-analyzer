#!/usr/bin/env python3
"""Independent ENE1 acceptance checks and immutable exact-baseline replay.

No candidate output is used as the oracle. New conformance sources are replayed
with baseline semantic sources as well, to check every forbidden promotion.
"""
import argparse
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

BASELINE = "da9555a6fa6a99f3607112be6b3d36c7acc01e84"
CAP = "external_negative_evidence_v1"
PREFIX = "tests_and_target_repos/a-code_c_ffi_bodyless_gate/"
SEMANTIC = {"crema/src/cqpl_export.rs", "cqpl/cqpl_checker/src/kripke.rs",
            "cqpl/cqpl_checker/src/main.rs", "cqpl/schemas/annotated_icfg_v2.schema.json"}
AUX = {"cqpl/cqpl_checker/src/model_checker.rs", "cqpl/cqpl_checker/src/explain.rs",
       "cqpl/cqpl_checker/tests/ctl_semantic_laws.rs", "cqpl/cqpl_checker/tests/v6i_query_no_refutation.rs"}
GENERATED = ["crema/ffi_functions.json", "crema/global_icfg.json",
             "crema/global_icfg_nodes_edges.dot", "SVF-example/callgraph_initial.dot.dot"]
KINDS = ("no_free_function", "no_free_formal", "no_capture_formal")


def unique(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate JSON key: {k}")
        out[k] = v
    return out


def read(p):
    return json.loads(p.read_text(), object_pairs_hook=unique)


def digest(b):
    return hashlib.sha256(b).hexdigest()


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def write(p, d):
    p.write_text(json.dumps(d, indent=2) + "\n")


def source_paths(root):
    # Includes untracked source/harness bytes, excludes generated build trees.
    paths = set(SEMANTIC) | AUX
    paths.update(read(root / "cqpl/bodyless_ffi_ene1_d3_preimage_sha256.json")["frozen_capability_hashes"])
    paths.update(p.as_posix() for p in Path("cqpl").glob("*ene1_d3*.json"))
    paths.update({"cqpl/D3_EXTERNAL_NEGATIVE_EVIDENCE_V1_GATE.md",
                  "cqpl/capabilities/external_negative_evidence_v1.md",
                  "cqpl/scripts/run_bodyless_ffi_ene1_d3_gate.sh",
                  "cqpl/scripts/verify_bodyless_ffi_ene1_d3.py"})
    for rel in ["cqpl/fixtures/ene1_external_library"] + [PREFIX + f["target"] for f in read(root / "cqpl/bodyless_ffi_ene1_d3_fixture_manifest.json")["fixtures"]]:
        paths.update(str(p.relative_to(root)) for p in (root / rel).rglob("*")
                     if p.is_file() and "target" not in p.relative_to(root / rel).parts)
    return sorted(paths)


def preimages(root):
    pre = read(root / "cqpl/bodyless_ffi_ene1_d3_preimage_sha256.json")
    assert pre["baseline_commit"] == pre["head_commit"] == BASELINE
    assert set(pre["semantic_source_preimages"]) == SEMANTIC
    aux = read(root / "cqpl/bodyless_ffi_ene1_d3_test_constructor_preimages.json")
    assert set(aux) == AUX
    checked = {}
    for p, h in pre["semantic_source_preimages"].items():
        b = git(root, "show", BASELINE + ":" + p)
        assert digest(b) == h, p
        checked[p] = {"preimage_sha256": h, "postimage_sha256": digest((root / p).read_bytes())}
    for p, h in aux.items():
        b = git(root, "show", BASELINE + ":" + p)
        stripped = b"".join(l for l in (root / p).read_bytes().splitlines(keepends=True)
                            if l.strip() != b"external_negative_evidence: vec![],")
        assert digest(b) == h and stripped == b, f"non-constructor semantic change: {p}"
    tracked = git(root, "ls-tree", "-r", "--name-only", BASELINE).decode().splitlines()
    # Independent closure, not merely trusting the manifest's list.
    required = {p for p in tracked if re.search(r"/b(?:4[4-9]|5\d|6[01])_", p)
                or p in ["cqpl/D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS_GATE.md", "cqpl/D2_EXTERNAL_RETURN_RELATIONS_V1_GATE.md"]
                or (p.startswith("cqpl/") and any(s in p for s in ["external_formal_memory_effects_v", "external_return_relations_v", "bodyless_ffi_efm2_d1", "bodyless_ffi_err1_d2"]))}
    assert set(pre["frozen_capability_hashes"]) == required
    for p, h in pre["frozen_capability_hashes"].items():
        assert digest(git(root, "show", BASELINE + ":" + p)) == h == digest((root / p).read_bytes()), p
    changed = set(git(root, "diff", "--name-only", BASELINE).decode().splitlines())
    assert changed <= SEMANTIC | AUX, f"unfrozen semantic source: {changed - SEMANTIC - AUX}"
    return {"checked": len(checked), "mismatches": 0, "errors": 0, "files": checked}, len(required)


def run_command(cmd, log, cwd):
    with log.open("w") as out:
        return subprocess.run(cmd, cwd=cwd, stdout=out, stderr=subprocess.STDOUT).returncode


def tests(root, out, toolchain):
    results = {}
    for name, manifest, focus in [
        ("focused_producer", "crema/Cargo.toml", True),
        ("focused_checker", "cqpl/cqpl_checker/Cargo.toml", True),
        ("full_cqpl", "cqpl/cqpl_checker/Cargo.toml", False),
        ("full_crema", "crema/Cargo.toml", False),
    ]:
        cmd = ["cargo", "+" + toolchain, "test", "--manifest-path", str(root / manifest), "--locked"]
        if focus:
            cmd.append("ene1")
        rc = run_command(cmd, out / "logs" / (name + ".log"), root)
        content = (out / "logs" / (name + ".log")).read_text()
        matches = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", content)
        results[name] = {"exit_code": rc, "passed": sum(int(a) for a, _ in matches),
                         "failures": sum(int(b) for _, b in matches)}
        assert rc == 0 and matches, name
        if focus:
            assert results[name]["passed"] >= (1 if name == "focused_producer" else 2), name
    return results


def replay(tree, out, side, targets, toolchain):
    for i, target in enumerate(targets, 1):
        cmd = ["python3", str(tree / "cqpl/scripts/run_one_target_v6q_r1c.py"),
               "--root", str(tree), "--relative-path", "a-code_c_ffi_bodyless_gate/" + target,
               "--out", str(out / side / target), "--toolchain", toolchain]
        rc = run_command(cmd, out / "logs" / f"{side}-{target}.log", tree)
        with (out / "runner-status.tsv").open("a") as f:
            f.write(f"{side}\t{target}\t{rc}\n")
        print(f"{side} {i}/{len(targets)} {target}: exit {rc}", flush=True)
        assert rc == 0, f"replay failed: {side}/{target}"
        shutil.copy2(tree / "crema/global_icfg.json", out / side / target / "raw-global-icfg.json")
        # Archive actual normal frontend/SVF outputs before generated cleanup.
        package = tree / PREFIX / target
        runs = list((package / "target/crema-svf").glob("run-*"))
        if runs:
            evidence = out / side / target / "compiler-evidence"
            evidence.mkdir()
            latest = max(runs, key=lambda p: p.stat().st_mtime_ns)
            for p in latest.iterdir():
                if p.is_file() and (p.suffix == ".ll" or p.name in ["LLVM_MEMORY_EFFECTS_V1.json", "EFX0_C_FRONTEND_PROVENANCE.txt", "SVF_SOLVED_POINTS_TO_V1.json"]):
                    shutil.copy2(p, evidence / p.name)


def query_docs(directory):
    return {p.stem: read(p) for p in (directory / "queries").glob("*.json")
            if not p.name.endswith(".explain.json")}


def verify(root, out, existing, fixtures):
    from jsonschema import Draft202012Validator
    schema = read(root / "cqpl/schemas/annotated_icfg_v2.schema.json")
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    query_names = {p.stem for p in (root / "cqpl/queries_v2").glob("*.cqpl")}
    assert len(query_names) == 12
    artifacts, queries = {}, {}
    new = [f["target"] for f in fixtures]
    for side in ["baseline", "candidate"]:
        for target in existing + new:
            path = out / side / target
            d = read(path / "annotated_icfg_v2.json")
            validator.validate(d)
            docs = query_docs(path)
            assert set(docs) == query_names and all(v["result"] in ["tt", "ff", "unk"] and "assessment" in v for v in docs.values())
            artifacts[side, target], queries[side, target] = d, docs
    truth = assessment = 0
    anti = {k: 0 for k in ["post_call_liveness_promotions", "allocation_wide_noescape_promotions",
                          "capture_inferences_from_absence", "free_inferences_from_absence",
                          "ordinary_semantic_events_created_by_ene1"]}
    fixture_changes = {}
    for target in existing + new:
        before, after = queries["baseline", target], queries["candidate", target]
        dt = sum(before[q]["result"] != after[q]["result"] for q in query_names)
        da = sum(before[q]["assessment"] != after[q]["assessment"] for q in query_names)
        if target in existing:
            truth += dt
            assessment += da
        else:
            fixture_changes[target] = {"truth_deltas": dt, "assessment_deltas": da}
        b, a = artifacts["baseline", target], artifacts["candidate", target]
        assert a.get("allocations") == b.get("allocations"), f"allocation change: {target}"
        bn = {n["id"]: n for n in b["nodes"]}
        assert set(bn) == {n["id"] for n in a["nodes"]}
        # Exact ordinary-state/event equality is stronger than counting only
        # promotions. ENE1 cannot even weaken any existing lifecycle/escape state.
        for n in a["nodes"]:
            old = bn[n["id"]]
            state = ["pre", "post", "identity", "event_identity", "allocation_post", "allocation_disposition"]
            if any(n.get(k) != old.get(k) for k in state):
                anti["post_call_liveness_promotions"] += 1
                anti["allocation_wide_noescape_promotions"] += 1
            for field in ["labels", "allocation_labels"]:
                if n.get(field, []) != old.get(field, []):
                    anti["ordinary_semantic_events_created_by_ene1"] += 1
                    anti["free_inferences_from_absence"] += 1
                    anti["capture_inferences_from_absence"] += 1
        assert a.get("external_return_relations", []) == b.get("external_return_relations", []), f"D2 changed: {target}"
        assert a.get("external_formal_memory_effects", []) == b.get("external_formal_memory_effects", []), f"D1 changed: {target}"
    assert truth == assessment == 0
    assert all(v == 0 for v in anti.values()), anti
    assert all(d["truth_deltas"] == d["assessment_deltas"] == 0 for d in fixture_changes.values())
    by_kind = collections.Counter({k: 0 for k in KINDS})
    by_source = collections.Counter()
    coverage = {category: {k: 0 for k in ["function_nofree", "formal_nofree", "formal_nocapture"]}
                for category in ["natural", "synthetic_compiler_verified"]}
    inventory = {k: 0 for k in ["explicit_function_nofree_calls", "tli_verified_function_nofree_calls",
                               "explicit_formal_nofree_calls", "explicit_formal_nocapture_calls",
                               "unresolved_formal_identity", "represented_body_candidates"]}
    outcomes = {}
    fixture_by_target = {f["target"]: f for f in fixtures}
    for target in existing + new:
        d = artifacts["candidate", target]
        nodes = {n["id"]: n for n in d["nodes"]}
        records = d.get("external_negative_evidence", [])
        assert bool(records) == (CAP in d["capabilities"])
        raw = read(out / "candidate" / target / "raw-global-icfg.json")
        raw_nodes = dict(raw["ordered_nodes"])
        ffi = set(read(out / "candidate" / target / "ffi_functions.json")["ffi_functions"])
        expected = set()
        def actual(raw_arg, scope):
            match = re.fullmatch(r"(?:Local\(_([0-9]+)\)|_([0-9]+))(?: \[mutable\])?", raw_arg.strip())
            return scope + "::Local(_" + (match[1] or match[2]) + ")" if match else None
        for node_id, raw_node in raw_nodes.items():
            term = raw_node.get("node_data", {}).get("terminator") if raw_node["node_type"] == "Mir" else None
            if not term or term.get("kind") != "Call" or term.get("function_called", "").strip() not in ffi:
                continue
            name = term["function_called"].strip()
            functions = [f for m in d.get("llvm_memory_effects", {}).get("modules", []) for f in m["functions"] if f["name"] == name]
            if len(functions) != 1 or not functions[0]["is_declaration"] or any(n.startswith("llvm::" + name + "::") for n in nodes):
                continue
            f = functions[0]
            args = term["arguments"]
            if len(args) != len(f["explicit"]["formals"]):
                continue
            if f["explicit"]["nofree"] or (f["tli_changed"] and f["tli_recognized"] and f["tli_inferred"]["nofree"]):
                expected.add((node_id, name, "no_free_function", None))
            for formal in f["explicit"]["formals"]:
                if not formal["pointer_typed"]:
                    continue
                for attr, kind in [("nofree", "no_free_formal"), ("nocapture", "no_capture_formal")]:
                    if formal[attr]:
                        if actual(args[formal["index"]]["arg"], node_id.rsplit("::bb",1)[0]) is None:
                            inventory["unresolved_formal_identity"] += 1
                        else:
                            expected.add((node_id, name, kind, formal["index"]))
        seen = set()
        for r in records:
            key = (r["node"], r["callee"], r["evidence_kind"], r.get("formal_index"))
            assert key not in seen
            seen.add(key)
            term = raw_nodes[r["node"]]["node_data"]["terminator"]
            assert term["kind"] == "Call" and term["function_called"].strip() == r["callee"]
            scope = r["node"].rsplit("::bb", 1)[0]
            assert r["call_arguments"] == [actual(arg["arg"], scope) for arg in term["arguments"]]
            assert "term:call" in nodes[r["node"]]["semantic_labels"]
            assert not any(n.startswith("llvm::" + r["callee"] + "::") for n in nodes)
            e = d["llvm_memory_effects"]
            f = e["modules"][r["evidence_module"]]["functions"][r["evidence_function"]]
            assert f["name"] == r["callee"] and f["is_declaration"]
            assert len(r["call_arguments"]) == len(f["explicit"]["formals"])
            kind, source = r["evidence_kind"], r["evidence_source"]
            if kind == "no_free_function":
                assert "actual_variable" not in r and "formal_index" not in r
                if source == "llvm16_explicit_ir":
                    assert f["explicit"]["nofree"]
                    inventory["explicit_function_nofree_calls"] += 1
                else:
                    assert f["tli_changed"] and f["tli_recognized"] and f["tli_inferred"]["nofree"]
                    inventory["tli_verified_function_nofree_calls"] += 1
            else:
                i = r["formal_index"]
                formal = f["explicit"]["formals"][i]
                assert formal["index"] == i and formal["pointer_typed"]
                assert formal["nofree" if kind == "no_free_formal" else "nocapture"]
                assert r["actual_variable"] == r["call_arguments"][i]
                inventory["explicit_formal_nofree_calls" if kind == "no_free_formal" else "explicit_formal_nocapture_calls"] += 1
            category = "synthetic_compiler_verified" if target in fixture_by_target else "natural"
            ck = {"no_free_function": "function_nofree", "no_free_formal": "formal_nofree", "no_capture_formal": "formal_nocapture"}[kind]
            coverage[category][ck] += 1
            by_kind[kind] += 1
            by_source[kind + "/" + source] += 1
        assert seen == expected, f"ENE1 inventory/record closure mismatch: {target}"
        if target in fixture_by_target:
            fixture = fixture_by_target[target]
            selected = [r for r in records if r["callee"] == fixture["callee"]]
            if fixture["evidence_kind"]:
                assert len(selected) == 1 and selected[0]["evidence_kind"] == fixture["evidence_kind"]
                assert selected[0]["evidence_source"] == fixture["evidence_source"]
                evidence = out / "candidate" / target / "compiler-evidence"
                assert (evidence / "ffi.ll").is_file()
                archived = read(evidence / "LLVM_MEMORY_EFFECTS_V1.json")
                assert archived == d["llvm_memory_effects"], "archived evidence mismatch"
                if fixture["callee"] == "d3_observe":
                    assert re.search(r"declare void @d3_observe\([^\n]*nocapture", (evidence / "ffi.ll").read_text())
                    assert not re.search(r"define[^\n]*@d3_observe\(", (evidence / "ffi.ll").read_text())
                    r = selected[0]
                    assert r["formal_index"] == 0
                    if fixture["fixture"] == "b65":
                        assert r["call_arguments"][0] == r["call_arguments"][1]
                        f = d["llvm_memory_effects"]["modules"][r["evidence_module"]]["functions"][r["evidence_function"]]
                        assert not f["explicit"]["formals"][1]["nocapture"]
                        # Same actual has an actual allocation in this control.
                        assert any(rel["variable"] == r["actual_variable"] and rel["allocations"]
                                   for rel in nodes[r["node"]].get("event_identity", {}).get("points_to", []))
            else:
                assert not selected
                if fixture["fixture"] == "b67":
                    assert any(n.startswith("llvm::d3_observe::") for n in nodes)
                    inventory["represented_body_candidates"] += 1
            outcomes[fixture["fixture"]] = {"status": "PASS", "records": len(selected), **fixture_changes[target]}
            if fixture["fixture"] == "b64":
                outcomes["b64"]["formal_nofree_end_to_end_available"] = False
                outcomes["b64"]["fallback"] = "verified_function_nofree"
            if fixture["fixture"] == "b69":
                outcomes["b69"]["status"] = "PASS_CONTROL_COVERAGE_UNAVAILABLE"
                outcomes["b69"]["end_to_end_conjunction_available"] = False
    return {"baseline": {"targets": len(existing), "query_cells": sum(len(queries["baseline", t]) for t in existing)},
            "differential": {"truth_deltas": truth, "assessment_deltas": assessment, "query_errors": 0},
            "anti_overinterpretation": anti, "inventory": inventory,
            "evidence_records": {**by_kind, "invalid": 0}, "evidence_records_by_kind_source": dict(by_source),
            "negative_evidence_coverage": coverage, "end_to_end_conjunction_available": False,
            "formal_nofree_end_to_end_available": False,
            "new_fixtures": {"expected": 8, "passed": len(outcomes), "failed": 0, "end_to_end_conformance_fixtures_passed": 7, "conjunction_coverage_unavailable": 1, "outcomes": outcomes},
            "d1_d2_behavior_preservation": {"targets_checked": len(existing), "d1_deltas": 0, "d2_deltas": 0}}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", type=Path, required=True)
    ap.add_argument("--run", action="store_true")
    args = ap.parse_args()
    root = args.root.resolve()
    os.chdir(root)
    assert args.run
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    out = root / "repro-results" / ("bodyless-ffi-ene1-d3-" + stamp)
    out.mkdir(exist_ok=False)
    (out / "logs").mkdir()
    print("D3_GATE_OUT=" + str(out), flush=True)
    gate = {"schema": "cqpl_external_negative_evidence_d3_gate_v1", "status": "FAIL",
            "baseline_commit": BASELINE, "capability": CAP, "errors": []}
    saved = {p: (root / p).read_bytes() for p in GENERATED}
    try:
        assert git(root, "rev-parse", "HEAD").decode().strip() == BASELINE
        gate["head_commit"] = BASELINE
        gate["branch"] = git(root, "branch", "--show-current").decode().strip()
        assert gate["branch"] == "cqpl6-bodyless-ffi-effect-gate"
        for p, b in saved.items():
            assert b == git(root, "show", "HEAD:" + p), f"dirty generated input: {p}"
        gate["preimage_validation"], frozen_count = preimages(root)
        gate["preservation"] = {"d1_unexpected_modifications": 0, "d2_unexpected_modifications": 0, "frozen_files_checked": frozen_count}
        sources = {p: digest((root / p).read_bytes()) for p in source_paths(root)}
        write(out / "source-postimages-at-start.json", sources)
        write(out / "semantic-postimages.json", {p: digest((root / p).read_bytes()) for p in sorted(SEMANTIC)})
        (out / "environment.txt").write_text(git(root, "status", "--short").decode() +
            subprocess.check_output(["rustup", "run", "nightly-2024-11-21", "rustc", "--version"]).decode() +
            subprocess.check_output(["cargo", "+nightly-2024-11-21", "--version"]).decode() +
            subprocess.check_output(["python3", "--version"]).decode())
        (out / "runner-status.tsv").write_text("side\ttarget\trc\n")
        toolchain = "nightly-2024-11-21"
        gate["software_tests"] = tests(root, out, toolchain)
        focused = (out / "logs/focused_checker.log").read_text()
        assert "ene1_closed_provenance_identity_and_contradictions ... ok" in focused
        assert "ene1_raw_boundary_atomicity_and_null_attacks ... ok" in focused
        gate["contradiction_tests"] = {"no_free_formal_plus_FreeArg": "PASS_FAIL_CLOSED",
            "no_free_formal_plus_ReallocArg": "PASS_FAIL_CLOSED",
            "function_nofree_plus_precise_preexisting_deallocation": "PASS_FAIL_CLOSED"}
        manifest = read(root / "cqpl/bodyless_ffi_ene1_d3_fixture_manifest.json")
        assert manifest["baseline_commit"] == BASELINE and len(manifest["fixtures"]) == 8
        fixtures = manifest["fixtures"]
        new = [f["target"] for f in fixtures]
        assert {f["fixture"] for f in fixtures} == {"b" + str(n) for n in range(62, 70)}
        paths = git(root, "ls-tree", "-r", "--name-only", BASELINE, PREFIX).decode().splitlines()
        existing = sorted({p[len(PREFIX):].split("/")[0] for p in paths})
        assert len(existing) == 75, f"baseline count discrepancy: {len(existing)}; human review required"
        (out / "existing-targets.txt").write_text("\n".join(existing) + "\n")
        with tempfile.TemporaryDirectory(prefix="cqpl-ene1-d3-baseline-") as temp:
            baseline = Path(temp) / "repo"
            assert run_command(["git", "clone", "--shared", "--no-checkout", str(root), str(baseline)], out / "logs/baseline-clone.log", root) == 0
            assert run_command(["git", "-C", str(baseline), "checkout", "--detach", BASELINE], out / "logs/baseline-checkout.log", root) == 0
            assert git(baseline, "rev-parse", "HEAD").decode().strip() == BASELINE
            # Same new fixture sources, baseline semantic producer and checker.
            for target in new:
                src = root / PREFIX / target
                shutil.copytree(src, baseline / PREFIX / target, ignore=shutil.ignore_patterns("target"))
            shutil.copytree(root / "cqpl/fixtures/ene1_external_library", baseline / "cqpl/fixtures/ene1_external_library")
            replay(baseline, out, "baseline", existing + new, toolchain)
            replay(root, out, "candidate", existing + new, toolchain)
        gate.update(verify(root, out, existing, fixtures))
        assert gate["baseline"] == {"targets": 75, "query_cells": 900}
        for p, b in saved.items():
            (root / p).write_bytes(b)
        preimages(root)
        assert all(digest((root / p).read_bytes()) == h for p, h in sources.items()), "source/harness changed during gate"
        gate["run_source_stability"] = {"files_checked": len(sources), "changed": 0}
        gate["status"] = "PASS"
    except Exception as error:
        gate["errors"].append(str(error))
        print("D3 FAIL: " + str(error), flush=True)
    finally:
        for p, b in saved.items():
            (root / p).write_bytes(b)
        manifest = read(root / "cqpl/bodyless_ffi_ene1_d3_fixture_manifest.json")
        for f in manifest["fixtures"]:
            p = root / PREFIX / f["target"] / "target"
            assert not git(root, "ls-files", "--", str(p.relative_to(root))).strip(), "refusing tracked cleanup"
            if p.exists():
                shutil.rmtree(p)
        diff_rc = run_command(["git", "-C", str(root), "diff", "--check"], out / "git-diff-check.log", root)
        dirs = [root / PREFIX / f["target"] for f in manifest["fixtures"]]
        hygiene = {"fixture_target_directories": sum((p / "target").exists() for p in dirs),
                   "pycache": sum(1 for p in dirs + [root / "cqpl/scripts", root / "crema/src", root / "cqpl/cqpl_checker/src"] for _ in p.rglob("__pycache__")),
                   "generated_global_icfg": sum(1 for p in dirs for _ in p.rglob("global_icfg*.json")),
                   "dirty_generated_entries": sum((root / p).read_bytes() != git(root, "show", "HEAD:" + p) for p in GENERATED),
                   "git_diff_check_rc": diff_rc}
        gate["hygiene"] = hygiene
        if any(hygiene.values()):
            gate["status"] = "FAIL"
            gate["errors"].append("hygiene failure")
        write(out / "gate.json", gate)
        (out / "gate.json.sha256").write_text(digest((out / "gate.json").read_bytes()) + "  gate.json\n")
        (out / "git-status.txt").write_bytes(git(root, "status", "--short"))
        (out / "tracked.patch").write_bytes(git(root, "diff", "--binary", BASELINE))
        sums = {str(p.relative_to(out)): digest(p.read_bytes()) for p in out.rglob("*") if p.is_file()}
        write(out / "artifact-sha256.json", sums)
        print("D3_GATE_JSON=" + str(out / "gate.json"), flush=True)
        print("D3_GATE_STATUS=" + gate["status"], flush=True)
    return 0 if gate["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
