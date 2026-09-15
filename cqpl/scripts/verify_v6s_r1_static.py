#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

BASELINE_COMMIT = "aa7ed4edcb4b6240aa14e3d23c7d7b848bfa2fb6"
BASELINE_VERSION = "CREMA-CQPL-v6R-r1"
CAPABILITY = "allocation_disposition_v1"
FROZEN_QUERY_COUNT = 12

R1B_PRODUCTION_PREFIX_SHA256 = {
    "crema/src/identity.rs": "94e275232ee5aa3aa9c4dea401eb16ccdbdc1f7bbb55676f4ef2e4e0c5da781d",
    "crema/src/cqpl_export.rs": "fa606a56846a56a1f4230c2a9f1903566f4ec3ea8cae6fc2296e1851cce1732d",
    "crema/src/main.rs": "c42279aaf2e207ef02609d2b4415f00c1e1622728327634c848c6cc06609f4e3",
}

R1B_WHOLE_FILE_SHA256 = {
    "crema/src/identity.rs": "98a8b8209f5c159047ac73435d32145ea9c635abb9901bd7f8bfcb781501b0d4",
    "crema/src/cqpl_export.rs": "47fce3cb1149bcc56aae102a2f4bb5512414732bc4c2c88d9f3753a60bdc25fa",
    "crema/src/main.rs": "717285f631fca9c2da18e9a4a857898768f9c234d57f7755c7036a01962429b5",
}

DISPOSITION_CONTRACT = {
    "box_into_raw": ("preserve_manual_obligation", "rustc_box_into_raw_v1", True),
    "box_from_raw": ("restore_raii_obligation", "rustc_box_from_raw_v1", True),
    "box_leak": ("preserve_persistent_obligation", "rustc_box_leak_v1", True),
    "mem_forget_owned_box": ("preserve_unreclaimed_obligation", "rustc_mem_forget_owned_box_v1", True),
    "raw_pointer_drop_noop": ("no_pointee_lifecycle_effect", "rustc_mem_drop_raw_pointer_v1", True),
    "return_escape": ("may_escape_to_caller", "rust_return_identity_v1", False),
    "may_deallocate": ("may_discharge", "allocation_drop_label_v1", False),
}

GENERATED_PARTS = {"target", "__pycache__", "repro-results"}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def die(errors: list[str], msg: str) -> None:
    errors.append(msg)


def load_json(path: Path, errors: list[str]):
    try:
        return json.loads(path.read_text())
    except Exception as exc:
        die(errors, f"invalid JSON {path}: {exc}")
        return None


def prefix_sha(path: Path) -> str:
    text = path.read_text()
    prefix = text.split("#[cfg(test)]", 1)[0]
    return sha256_bytes(prefix.encode())


def source_files(root: Path) -> set[str]:
    out: set[str] = set()
    for p in root.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(root)
        if any(part in GENERATED_PARTS for part in rel.parts):
            continue
        if p.suffix == ".pyc":
            continue
        out.add(rel.as_posix())
    return out


def main() -> int:
    cqpl = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).resolve().parents[1]).resolve()
    repo = cqpl.parent
    crema_src = repo / "crema" / "src"
    standalone = (repo / "CANDIDATE_PROVENANCE.json").is_file()
    mode = "standalone-package" if standalone else "installed-tree"
    errors: list[str] = []

    if not cqpl.is_dir():
        die(errors, f"missing cqpl root: {cqpl}")
    if not crema_src.is_dir():
        die(errors, f"missing CREMA source boundary: {crema_src}")

    baseline_path = cqpl / "artifact" / "V6S_R1_BASELINE_SHA256.json"
    baseline = load_json(baseline_path, errors) or {}
    if baseline.get("schema") != "cqpl_v6s_r1_baseline_sha256_v1":
        die(errors, "bad/missing v6S baseline SHA schema")
    if baseline.get("baseline_commit") != BASELINE_COMMIT:
        die(errors, "v6S baseline commit mismatch")
    if baseline.get("baseline_version") != BASELINE_VERSION:
        die(errors, "v6S baseline version mismatch")

    base_cqpl: dict[str, str] = baseline.get("all_cqpl_files_excluding_manifest", {})
    base_crema: dict[str, str] = baseline.get("crema_src_files", {})
    frozen_queries: dict[str, str] = baseline.get("frozen_query_sha256", {})
    prefix_hashes: dict[str, str] = baseline.get("production_prefix_sha256", {})
    if len(frozen_queries) != FROZEN_QUERY_COUNT:
        die(errors, f"expected {FROZEN_QUERY_COUNT} frozen query hashes, got {len(frozen_queries)}")
    if len(base_crema) != 9:
        die(errors, f"expected 9 baseline CREMA source hashes, got {len(base_crema)}")

    # Frozen query semantics must remain byte-identical to v6R-r1.
    for name, expected in sorted(frozen_queries.items()):
        rel = f"queries_v2/{name}"
        p = cqpl / rel
        if not p.is_file():
            die(errors, f"missing frozen query: cqpl/{rel}")
        elif sha256(p) != expected:
            die(errors, f"frozen query changed: cqpl/{rel}")

    # The logical evaluator/explainer production code remains frozen to v6R.
    # r1b intentionally changes CREMA identity/export plumbing, so identity.rs
    # is excluded from the legacy prefix freeze and pinned below to the exact
    # dual-view implementation instead.
    for rel, expected in sorted(prefix_hashes.items()):
        if rel == "crema/src/identity.rs":
            continue
        p = repo / rel
        if not p.is_file():
            die(errors, f"missing production-prefix file: {rel}")
        elif prefix_sha(p) != expected:
            die(errors, f"production prefix changed unexpectedly: {rel}")

    # Exact r1b implementation boundary: production prefixes and full files
    # (including the regression tests) are pinned for the three files whose
    # behavior differs from r1a.
    for rel, expected in sorted(R1B_PRODUCTION_PREFIX_SHA256.items()):
        p = repo / rel
        if not p.is_file():
            die(errors, f"missing r1b implementation file: {rel}")
        elif prefix_sha(p) != expected:
            die(errors, f"r1b production prefix mismatch: {rel}")
    for rel, expected in sorted(R1B_WHOLE_FILE_SHA256.items()):
        p = repo / rel
        if not p.is_file():
            die(errors, f"missing r1b implementation file: {rel}")
        elif sha256(p) != expected:
            die(errors, f"r1b whole-file SHA mismatch: {rel}")

    # Core CQPL grammar/truth files must remain whole-file identical.
    for rel in [
        "cqpl_checker/src/ast.rs",
        "cqpl_checker/src/parser.rs",
        "cqpl_checker/src/truth.rs",
    ]:
        expected = base_cqpl.get(rel)
        p = cqpl / rel
        if not expected or not p.is_file() or sha256(p) != expected:
            die(errors, f"frozen CQPL semantic core changed: cqpl/{rel}")

    # Derive the actual repository delta independently from the allow-list.
    actual_delta: set[str] = set()
    for rel, expected in sorted(base_cqpl.items()):
        p = cqpl / rel
        if not p.is_file():
            die(errors, f"baseline CQPL file deleted: cqpl/{rel}")
            continue
        if sha256(p) != expected:
            actual_delta.add(f"cqpl/{rel}")

    # MANIFEST existed in v6R and is intentionally refreshed in v6S.
    actual_delta.add("cqpl/MANIFEST_SHA256.json")

    for name, expected in sorted(base_crema.items()):
        rel = f"crema/src/{name}"
        p = repo / rel
        if not p.is_file():
            die(errors, f"baseline CREMA file deleted: {rel}")
            continue
        if sha256(p) != expected:
            actual_delta.add(rel)

    stage_path = cqpl / "artifact" / "V6S_R1_STAGE_PATHS.txt"
    if not stage_path.is_file():
        die(errors, "missing cqpl/artifact/V6S_R1_STAGE_PATHS.txt")
        stage_set: set[str] = set()
    else:
        stage_set = {
            line.strip()
            for line in stage_path.read_text().splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        }

    # New files are those in the stage-set not present in the v6R inventory.
    # They must exist; baseline files are already handled above.
    for rel in sorted(stage_set):
        if rel.startswith("cqpl/"):
            qrel = rel.removeprefix("cqpl/")
            if qrel == "MANIFEST_SHA256.json" or qrel in base_cqpl:
                continue
            if not (cqpl / qrel).is_file():
                die(errors, f"staged new CQPL file missing: {rel}")
            actual_delta.add(rel)
        elif rel.startswith("crema/src/"):
            # Existing CREMA files are already compared to baseline.  No new
            # CREMA source file is part of v6S-r1.
            if rel.removeprefix("crema/src/") not in base_crema:
                die(errors, f"unexpected new CREMA source path in v6S-r1 stage list: {rel}")
        else:
            die(errors, f"v6S stage path outside allowed source roots: {rel}")

    if actual_delta != stage_set:
        for rel in sorted(actual_delta - stage_set):
            die(errors, f"delta path missing from V6S_R1_STAGE_PATHS.txt: {rel}")
        for rel in sorted(stage_set - actual_delta):
            die(errors, f"V6S_R1_STAGE_PATHS.txt path is not an actual v6R->v6S delta: {rel}")

    if any(rel.startswith("cqpl/queries_v2/") for rel in stage_set):
        die(errors, "v6S stage set must not contain frozen query files")

    # Manifest is an exact CQPL shipping boundary, excluding itself and build/cache files.
    manifest_path = cqpl / "MANIFEST_SHA256.json"
    manifest = load_json(manifest_path, errors)
    if not isinstance(manifest, dict):
        die(errors, "MANIFEST_SHA256.json must be a flat object")
        manifest = {}
    cqpl_files = source_files(cqpl) - {"MANIFEST_SHA256.json"}
    if set(manifest) != cqpl_files:
        for rel in sorted(cqpl_files - set(manifest)):
            die(errors, f"CQPL manifest missing file: {rel}")
        for rel in sorted(set(manifest) - cqpl_files):
            die(errors, f"CQPL manifest contains non-shipped file: {rel}")
    for rel, expected in manifest.items():
        p = cqpl / rel
        if p.is_file() and sha256(p) != expected:
            die(errors, f"CQPL manifest SHA mismatch: {rel}")

    # Validate JSON schema and the closed disposition vocabulary mechanically.
    schema = load_json(cqpl / "schemas" / "annotated_icfg_v2.schema.json", errors) or {}
    try:
        props = schema["$defs"]["allocationDispositionRecord"]["properties"]
        kinds = set(props["kind"]["enum"])
        effects = set(props["obligation_effect"]["enum"])
        bases = set(props["basis"]["enum"])
        if kinds != set(DISPOSITION_CONTRACT):
            die(errors, f"schema disposition kind set mismatch: {sorted(kinds)}")
        if effects != {v[0] for v in DISPOSITION_CONTRACT.values()}:
            die(errors, "schema disposition obligation-effect set mismatch")
        if bases != {v[1] for v in DISPOSITION_CONTRACT.values()}:
            die(errors, "schema disposition proof-basis set mismatch")
        if props["certainty"].get("const") != "may_abstract":
            die(errors, "schema disposition certainty must be may_abstract")
        conditional_present = any(
            branch.get("if", {})
            .get("properties", {})
            .get("capabilities", {})
            .get("contains", {})
            .get("const") == CAPABILITY
            and "allocation_disposition" in (
                branch.get("then", {})
                .get("properties", {})
                .get("nodes", {})
                .get("items", {})
                .get("required", [])
            )
            for branch in schema.get("allOf", [])
        )
        if not conditional_present:
            die(errors, "schema must require node allocation_disposition when capability is declared")
    except Exception as exc:
        die(errors, f"schema missing allocationDispositionRecord contract: {exc}")

    # Static raw-pointer semantic invariant across producer, transfer, exporter, consumer.
    icfg = (crema_src / "icfg.rs").read_text()
    absd = (crema_src / "abstract_domain.rs").read_text()
    export = (crema_src / "cqpl_export.rs").read_text()
    kripke = (cqpl / "cqpl_checker" / "src" / "kripke.rs").read_text()
    main_rs = (cqpl / "cqpl_checker" / "src" / "main.rs").read_text()
    structs = (crema_src / "structs.rs").read_text()
    identity = (crema_src / "identity.rs").read_text()
    crema_main = (crema_src / "main.rs").read_text()

    required_tokens = {
        "crema/src/icfg.rs": [
            "RustAllocationDispositionEvidenceKind::MemDropRawPointer",
            "TyKind::RawPtr(..)",
            'tcx.crate_name(def_id.krate).as_str() == "core"',
            'matches!(tcx.def_kind(parent), DefKind::Mod)',
            'tcx.item_name(parent).as_str() == "mem"',
        ],
        "crema/src/abstract_domain.rs": [
            "certified_raw_pointer_drop",
            "(CellValue::BOXTIMES, new_mem.clone())",
            "v6s_certified_raw_pointer_mem_drop_preserves_pointee_allocation",
        ],
        "crema/src/cqpl_export.rs": [
            "certified_raw_pointer_drop",
            "&& !certified_raw_pointer_drop",
            "v6s_raw_pointer_mem_drop_is_not_exported_as_deallocation_label",
            '"raw_pointer_drop_noop"',
            '"no_pointee_lifecycle_effect"',
        ],
        "cqpl/cqpl_checker/src/kripke.rs": [
            "RawPointerDropNoop",
            "NoPointeeLifecycleEffect",
            "v6s_allocation_disposition_rejects_wrong_raw_pointer_drop_effect",
        ],
        "cqpl/cqpl_checker/src/main.rs": [
            '"raw_pointer_drop_noop" => ("no_pointee_lifecycle_effect", "rustc_mem_drop_raw_pointer_v1", true)',
        ],
        "crema/src/structs.rs": [
            "RustAllocationDispositionEvidenceKind",
            "MemDropRawPointer",
        ],
        "crema/src/identity.rs": [
            "IdentityTransferProfile::LegacyFrozen",
            "IdentityTransferProfile::DispositionV6S",
            "fixed_point_disposition_identity_analysis",
            "direct_assignment_local",
            "v6s_legacy_identity_keeps_frozen_projected_lhs_behavior",
            "v6s_projected_deref_write_preserves_base_pointer_identity",
        ],
        "crema/src/main.rs": [
            "fixed_point_identity_analysis",
            "fixed_point_disposition_identity_analysis",
            "allocation_identity_state",
            "disposition_identity_state",
        ],
    }
    text_by_name = {
        "crema/src/icfg.rs": icfg,
        "crema/src/abstract_domain.rs": absd,
        "crema/src/cqpl_export.rs": export,
        "cqpl/cqpl_checker/src/kripke.rs": kripke,
        "cqpl/cqpl_checker/src/main.rs": main_rs,
        "crema/src/structs.rs": structs,
        "crema/src/identity.rs": identity,
        "crema/src/main.rs": crema_main,
    }
    for name, tokens in required_tokens.items():
        for token in tokens:
            if token not in text_by_name[name]:
                die(errors, f"raw-pointer/disposition invariant token missing in {name}: {token}")

    # r1b dual-view separation and schema-closure invariants.
    for token in [
        "disposition_event_identity",
        "disposition_post_identity",
        "record.source_variable.as_ref()",
        "record.target_variable.as_ref()",
        "variable_ids.insert(variable.clone())",
    ]:
        if token not in export:
            die(errors, f"r1b disposition-only schema-closure token missing in cqpl_export.rs: {token}")

    # r1a compiler-safety invariant: never ask rustc for the item name of an
    # arbitrary parent DefId. Associated methods can have an `Impl` parent,
    # which has no name and caused an ICE on the pinned compiler during the
    # first fresh FFI subjects of the superseded r1 validation attempt.
    if "tcx.item_name(tcx.parent(def_id))" in icfg:
        die(errors, "unsafe rustc parent item_name query remains in crema/src/icfg.rs")
    if icfg.count("matches!(tcx.def_kind(parent), DefKind::Mod)") < 2:
        die(errors, "expected DefKind::Mod guards for alloc::alloc and core::mem parent-name queries")

    # Do not allow r1 to claim MUST semantics in the serialized contract.
    exporter_slice = export[export.find("struct AllocationDispositionRecord"):export.find("fn allocation_contract", export.find("struct AllocationDispositionRecord"))]
    if 'certainty: "must' in exporter_slice or 'certainty: "exact' in exporter_slice:
        die(errors, "v6S-r1 disposition exporter contains non-MAY certainty")

    # Documentation must include official Rust semantic references and no accidental
    # escaped-newline blocks from package assembly.
    docs = [
        cqpl / "README.md",
        cqpl / "LANGUAGE.md",
        cqpl / "ANNOTATED_ICFG.md",
        cqpl / "V6S_R1_GUIDE.md",
        cqpl / "capabilities" / "allocation_disposition_v1.md",
    ]
    joined_docs = "\n".join(p.read_text() for p in docs if p.is_file())
    for url in [
        "https://doc.rust-lang.org/reference/types/pointer.html",
        "https://doc.rust-lang.org/std/boxed/struct.Box.html",
        "https://doc.rust-lang.org/std/mem/fn.drop.html",
        "https://doc.rust-lang.org/std/mem/fn.forget.html",
    ]:
        if url not in joined_docs and url not in icfg and url not in structs:
            die(errors, f"missing official Rust documentation reference: {url}")
    for p in docs:
        if p.is_file() and "\\n\\n##" in p.read_text():
            die(errors, f"literal escaped newline block in documentation: {p.name}")

    expected_gates = load_json(cqpl / "artifact" / "V6S_R1_EXPECTED_GATES.json", errors) or {}
    if expected_gates.get("baseline_commit") != BASELINE_COMMIT:
        die(errors, "V6S_R1_EXPECTED_GATES baseline mismatch")
    if expected_gates.get("baseline_attempts") != 1344 or expected_gates.get("required_result_mismatches") != 0:
        die(errors, "V6S_R1_EXPECTED_GATES must require 1344 attempts and zero result mismatch")
    if expected_gates.get("baseline_leak_alloc_unknown") != 105:
        die(errors, "V6S_R1_EXPECTED_GATES leak baseline must be 105")
    if expected_gates.get("implementation_revision") != "CREMA-CQPL-v6S-r1b":
        die(errors, "V6S_R1_EXPECTED_GATES implementation revision must be r1b")
    if "dual-view" not in expected_gates.get("identity_observation_model", ""):
        die(errors, "V6S_R1_EXPECTED_GATES must declare dual-view identity observation")

    # Standalone packages additionally enforce archive/source hygiene. Installed
    # trees intentionally do not scan sibling repro-results/targets.
    if standalone:
        forbidden = []
        for p in repo.rglob("*"):
            if p.is_symlink():
                forbidden.append(f"symlink:{p.relative_to(repo)}")
                continue
            if not p.is_file():
                continue
            rel = p.relative_to(repo)
            if any(part in GENERATED_PARTS for part in rel.parts) or p.suffix == ".pyc":
                forbidden.append(rel.as_posix())
        if forbidden:
            for item in forbidden[:50]:
                die(errors, f"generated/cache/symlink file shipped: {item}")

    if errors:
        print("V6S_R1_STATIC_VERIFY: FAIL")
        for e in errors:
            print(" -", e)
        return 1

    print("V6S_R1_STATIC_VERIFY: PASS")
    print(f"verification_mode={mode}")
    print(f"baseline_commit={BASELINE_COMMIT}")
    print("implementation_revision=CREMA-CQPL-v6S-r1b")
    print("semantic_goal=observational-allocation-disposition; old-query-truth-frozen")
    print(f"exact_repository_delta_files={len(stage_set)}")
    print(f"frozen_queries={FROZEN_QUERY_COUNT} byte-identical")
    print("cqpl_logic_production_prefix=byte-identical-v6R")
    print("identity_observation_model=dual-view; legacy-frozen; disposition-v6s")
    print("disposition_certainty=may_abstract-only")
    print("raw_pointer_mem_drop=pointee-noop-structurally-certified")
    print("rustc_parent_name_guard=DefKind::Mod-before-item_name")
    print("runtime_acceptance_required=112x12=1344 zero-result-mismatch + fixture gates")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
