#!/usr/bin/env python3
"""Verify an isolated standard/forced explicit-target provenance replay."""
import hashlib, json, pathlib, sys

out, root = map(pathlib.Path, sys.argv[1:3])
standard_path=out/"foreign-mir-standard-explicit-target.json"
forced_path=out/"foreign-mir-forced-explicit-target.json"
frozen={"standard":hashlib.sha256(standard_path.read_bytes()).hexdigest(),"forced":hashlib.sha256(forced_path.read_bytes()).hexdigest()}
for mode,path in [("standard",standard_path),("forced",forced_path)]:
    recorded=(out/f"{mode}-immediate.sha256").read_text().split()[0]
    assert frozen[mode]==recorded, f"{mode} acquisition JSON changed after immediate hash/copy"
    raw_hash=hashlib.sha256((out/"raw"/f"{mode}.json").read_bytes()).hexdigest()
    assert raw_hash==recorded, f"{mode} frozen acquisition JSON differs from compiler callback output"
    assert path.stat().st_mode & 0o222 == 0, f"{mode} acquisition JSON not frozen read-only"

meta=json.loads((out/"cargo-metadata.json").read_text())
package_ids={p["name"]:p["id"] for p in meta["packages"]}
host_triple="x86_64-unknown-linux-gnu"
raws={"standard":json.loads(standard_path.read_text()),"forced":json.loads(forced_path.read_text())}
case_keys=["non_generic_non_inline","inline","generic","trait_method","transitive_root","transitive_dep2"]

def case_for(raw):
    found={k:None for k in case_keys}
    for call in raw.get("calls",[]):
        opname=call["call_operand"]["fn_def_path"]
        crate=call["call_operand"]["crate_identity"]["crate_name"]
        key=None
        if opname.endswith("::ordinary"): key="non_generic_non_inline"
        elif opname.endswith("::inline_body"): key="inline"
        elif opname.endswith("::generic_body"): key="generic"
        elif opname.endswith("::Compute::compute"): key="trait_method"
        elif opname.endswith("::transitive"): key="transitive_root"
        elif crate=="dep2" and opname.endswith("::leaf"): key="transitive_dep2"
        if key and found[key] is None: found[key]=call
    normalized={}
    bodies=raw.get("foreign_bodies",[])
    bypath={b.get("def_path"):b for b in bodies}
    for key,call in found.items():
        if call is None:
            normalized[key]={"call_site_represented":False,"call":None,"mir_query":{"body_status":"unresolved_call","is_mir_available":False,"optimized_mir_attempted":False}}
            continue
        q=call["mir_query"]
        if call["resolved_instance"] is not None:
            assert q["def_id"]==call["resolved_instance"]["def_id"], f"{key}: query DefId != resolved Instance DefId"
        if key=="trait_method":
            assert q["def_path"]=="<dep::Worker as dep::Compute>::compute", "trait query did not target the resolved implementation"
        if key=="transitive_root" and q["body_status"]=="available":
            assert "dep::transitive" in bypath, "transitive root query did not enter traversed foreign body set"
        if key=="transitive_dep2" and q["body_status"]=="available":
            assert "dep2::leaf" in bypath, "dep2 leaf query did not enter traversed foreign body set"
        normalized[key]={"call_site_represented":True,"call":call,"mir_query":q}
    return normalized

matrices={m:case_for(raws[m]) for m in raws}
comparison={m:{k:(v["mir_query"]["body_status"] if v["mir_query"]["body_status"]!="unresolved_call" else "unresolved_call") for k,v in matrices[m].items()} for m in matrices}
expected={"standard":{"non_generic_non_inline":"unavailable","inline":"available","generic":"available","trait_method":"unavailable","transitive_root":"unavailable","transitive_dep2":"unresolved_call"},"forced":{"non_generic_non_inline":"available","inline":"available","generic":"available","trait_method":"available","transitive_root":"available","transitive_dep2":"available"}}
exact_expected=comparison==expected

def load_jsonl(path): return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
builds={}
for mode in ["standard","forced"]:
    inv=load_jsonl(out/f"rustc-{mode}-explicit-target.jsonl")
    runtime={}
    for name in ["dep1_app","dep","dep2"]:
        candidates=[x for x in inv if x.get("crate_name")==name]
        assert candidates, f"missing rustc invocation evidence for runtime crate {name} ({mode})"
        # Cargo may invoke rustc more than once for a crate; preserve all and require consistent mode flags.
        flags={bool(x.get("always_encode_mir")) for x in candidates}
        should=mode=="forced"
        assert flags=={should}, f"unexpected RUSTFLAGS propagation to {name} in {mode}: {flags}"
        for x in candidates:
            argv=x["argv"]
            assert "--target" in argv and host_triple in argv, f"{name} invocation lacks explicit target in {mode}"
        runtime[name]=candidates
    cargo_command=(out/f"commands.txt").read_text()
    command_has_target=f"--target {host_triple}" in cargo_command
    assert command_has_target
    cargo_line=next(x for x in (out/f"cargo-{mode}-explicit-target.jsonl").read_text().splitlines() if x)
    cargo_messages=len((out/f"cargo-{mode}-explicit-target.jsonl").read_text().splitlines())
    stable={"app":raws[mode].get("entry",{}).get("stable_crate_id")}
    for ident in raws[mode].get("loaded_crates",[]):
        if ident["crate_name"] in ("dep","dep2"):
            stable[ident["crate_name"]]=ident["stable_crate_id"]
    for call in raws[mode].get("calls",[]):
        ident=call["call_operand"]["crate_identity"]
        if ident["crate_name"] in ("dep","dep2"):
            stable.setdefault(ident["crate_name"],ident["stable_crate_id"])
    assert all(stable.get(n) for n in ["app","dep","dep2"]), f"missing StableCrateId evidence {mode}: {stable}"
    target_dir=f"target-{mode}-explicit-target"
    builds[mode]={"target_triple":host_triple,"explicit_target":True,"rustflags":"-Zalways-encode-mir=yes" if mode=="forced" else None,"always_encode_mir":mode=="forced","cargo_target_directory":str(out/target_dir),"cargo_command_excerpt":cargo_command.split("\n\n")[0 if mode=="standard" else 1],"cargo_message_lines":cargo_messages,"rustc_runtime_invocations":runtime,"stable_crate_ids":stable,"output_json":standard_path.name if mode=="standard" else forced_path.name,"output_json_sha256":frozen[mode],"cargo_log_first_record":cargo_line}
    assert builds[mode]["explicit_target"] is True
    assert builds[mode]["always_encode_mir"] is (mode=="forced")

# Verify compiler-semantic transitive edge independent of labels used in the comparison.
forced_calls=raws["forced"].get("calls",[])
assert any(c["caller"]["crate_name"]=="dep1_app" and c["caller"]["def_path"]=="main" and c["call_operand"]["fn_def_path"].endswith("::transitive") for c in forced_calls), "missing app -> dep::transitive edge"
assert any(c["caller"]["def_path"].endswith("::transitive") and c["call_operand"]["crate_identity"]["crate_name"]=="dep2" and c["call_operand"]["fn_def_path"].endswith("::leaf") for c in forced_calls), "missing compiler-derived dep::transitive -> dep2::leaf edge"

host_src=out/"host-control-previous"
host_prov=json.loads((host_src/"provenance.json").read_text())
for name,row in host_prov["files"].items():
    assert hashlib.sha256((host_src/name).read_bytes()).hexdigest()==row["copied_sha256"]==row["original_sha256"]
host=json.loads((host_src/"host-unit-control.json").read_text())
assert host["explicit-target"]["units"]["build_script_build"]["always_encode_mir"] is False
assert host["explicit-target"]["units"]["dep1_app"]["always_encode_mir"] is True

git_status=(out/"git-status-short.txt").read_text().splitlines()
report={"schema":"dep1_p0_final_replay_v1","head":(out/"environment.txt").read_text().splitlines()[0],"branch":(out/"environment.txt").read_text().splitlines()[1],"toolchain":{"rustc":next(x for x in (out/"environment.txt").read_text().splitlines() if x.startswith("rustc ")),"cargo":next(x for x in (out/"environment.txt").read_text().splitlines() if x.startswith("cargo "))},"target_triple":host_triple,"standard_explicit_target":matrices["standard"],"forced_explicit_target":matrices["forced"],"observed_comparison":comparison,"expected_comparison":expected,"exact_expected_comparison":exact_expected,"acquisition_build_provenance":builds,"architecture_decision":"metadata_plus_forced_encoding" if exact_expected else "no_architecture_decision_from_expected_pattern","architecture_reason":"The controlled comparison varies only -Zalways-encode-mir=yes at the same explicit target. Architecture remains metadata_plus_forced_encoding only when the complete observed matrix exactly matches the required pattern." if exact_expected else "The observed matrix differs from the specified pattern; do not force the architecture decision.","previous_host_control":{"source_run":host_prov["source_run"],"copied_evidence":host_prov,"decision":"reused without rerunning; source hashes and copied hashes verified"},"package_identity_rule":"Cargo PackageIds remain from Cargo metadata; StableCrateId, DefId, def-path and Instance are rustc identities. The fixed synthetic graph is correlated through metadata and manifest edges, not by parsing rustc crate display names.","analyzer_semantic_modifications":0,"git_diff_check":(out/"git-diff-check.txt").read_text().strip() or "passed","git_status_short":git_status}
(out/"DEP1_P0_FINAL_REPLAY_REPORT.json").write_text(json.dumps(report,indent=2)+"\n")
(out/"DEP1_P0_FINAL_REPLAY_COMPARISON.json").write_text(json.dumps(comparison,indent=2)+"\n")

# Last operation checks: packaging cannot mutate either frozen acquisition JSON.
after={"standard":hashlib.sha256(standard_path.read_bytes()).hexdigest(),"forced":hashlib.sha256(forced_path.read_bytes()).hexdigest()}
assert after==frozen, f"acquisition output changed during packaging: {frozen} -> {after}"
(out/"acquisition-json-sha256.json").write_text(json.dumps({"before_packaging":frozen,"after_packaging":after,"unchanged":True},indent=2)+"\n")
import hashlib as _hashlib
files=sorted(p for p in out.rglob("*") if p.is_file() and p.name!="SHA256SUMS")
(out/"SHA256SUMS").write_text("".join(f"{_hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(out)}\n" for p in files))
# Verify once again after writing the manifest itself.
assert {"standard":hashlib.sha256(standard_path.read_bytes()).hexdigest(),"forced":hashlib.sha256(forced_path.read_bytes()).hexdigest()}==frozen
