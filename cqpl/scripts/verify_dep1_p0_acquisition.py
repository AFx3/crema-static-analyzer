#!/usr/bin/env python3
"""Verify and summarize the DEP1-P0 Cargo acquisition experiment."""
import hashlib
import json
import pathlib
import sys

out, root = map(pathlib.Path, sys.argv[1:3])
metadata = json.loads((out / "cargo-metadata.json").read_text())
ids = {p["name"]: p["id"] for p in metadata["packages"]}
records = []
for mode in ("standard", "forced"):
    msgs = []
    for line in (out / f"cargo-{mode}.jsonl").read_text().splitlines():
        try:
            m = json.loads(line)
        except json.JSONDecodeError:
            continue
        if m.get("reason") == "compiler-artifact":
            msgs.append({"package_id": m.get("package_id"), "target": m.get("target", {}).get("name"),
                         "kind": m.get("target", {}).get("kind"), "filenames": m.get("filenames", []),
                         "fresh": m.get("fresh")})
    records.append({"mode": mode, "artifacts": msgs,
                    "rustflags": "-Zalways-encode-mir=yes" if mode == "forced" else None})
report = {
    "schema": "crema_dep1_p0_acquisition_v1",
    "toolchain": "nightly-2024-11-21 (rustc 1.84.0-nightly 3fee0f12e)",
    "repository": {"head": "0169ce3da4e865e1750e22b9fc14631b0efcca98", "branch": "cqpl7-dependency-body-ingestion"},
    "cargo_package_ids": ids,
    "selected_target": "dep1_app binary",
    "call_site_represented": "Cargo compiled the selected app and runtime dependency packages; MIR call-site census requires the compiler callback phase.",
    "standard_metadata": {k: "error" for k in ["non_generic_non_inline", "inline", "generic", "trait_method", "transitive"]},
    "body_availability_detail": {k: {"call_site_represented": None, "callee_body_represented": None,
        "body_unavailable": None, "body_capture_query_error": "No foreign-DefId optimized_mir callback was executed in this P0 harness; Cargo artifact presence is not treated as body evidence."}
        for k in ["non_generic_non_inline", "inline", "generic", "trait_method", "transitive"]},
    "forced_mir_encoding": {"supported_by_pinned_toolchain": True,
        "flag": "-Zalways-encode-mir=yes", "cargo_rustflags_builds": records,
        "propagation_observation": "Compare compiler-artifact logs and forced build stderr; Cargo-wide RUSTFLAGS applies to dependency rustc invocations when those crates are compiled, evidenced by successful forced build (see logs).",
        "results": {k: "error" for k in ["non_generic_non_inline", "inline", "generic", "trait_method", "transitive"]}},
    "runtime_vs_host": {"runtime_packages": [ids.get("dep1_app"), ids.get("dep"), ids.get("dep2")],
        "build_script_or_proc_macro_packages": [], "note": "No build scripts or proc macros are part of this fixture."},
    "wrapper_capture_required": None,
    "recommended_architecture": None,
    "architecture_decision": "INCONCLUSIVE: foreign MIR was not queried. Do not choose an architecture from Cargo artifacts alone; implement the rustc_private foreign-DefId callback experiment before DEP1-P0 review.",
    "source_provenance": "Synthetic local path packages; compiler call/body provenance was not obtained.",
    "hashes": {}
}
for f in sorted(out.iterdir()):
    if f.is_file() and f.name != "SHA256SUMS":
        report["hashes"][f.name] = hashlib.sha256(f.read_bytes()).hexdigest()
(out / "DEP1_P0_ACQUISITION_REPORT.json").write_text(json.dumps(report, indent=2) + "\n")
lines = [f"{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.relative_to(out)}" for f in sorted(out.rglob("*")) if f.is_file() and f.name != "SHA256SUMS"]
(out / "SHA256SUMS").write_text("\n".join(lines) + "\n")
print(out / "DEP1_P0_ACQUISITION_REPORT.json")
