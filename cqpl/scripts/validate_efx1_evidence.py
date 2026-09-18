#!/usr/bin/env python3
"""Strict validator for EFX1 LLVM-effect and solved-SVF-PTA evidence.

This validator is intentionally independent of model-checker truth semantics.
It verifies provenance, closed vocabularies, index discipline, and MAY-only PTA.
"""
from __future__ import annotations
import argparse, json, sys
from pathlib import Path

ACCESS = {"none", "read", "write", "readwrite"}
ALLOC_KINDS = {"alloc", "realloc", "free", "uninitialized", "zeroed", "aligned"}

class ValidationError(Exception):
    pass

def need(cond: bool, msg: str) -> None:
    if not cond:
        raise ValidationError(msg)

def exact_keys(obj, expected, where):
    need(isinstance(obj, dict), f"{where}: expected object")
    got = set(obj)
    expected = set(expected)
    need(got == expected, f"{where}: fields mismatch; missing={sorted(expected-got)} extra={sorted(got-expected)}")

def read_json(path: Path):
    try:
        return json.loads(path.read_text())
    except Exception as e:
        raise ValidationError(f"cannot parse {path}: {e}") from e

def validate_memory(m, where):
    exact_keys(m, {"argmem", "inaccessiblemem", "other", "encoded"}, where)
    for key in ("argmem", "inaccessiblemem", "other"):
        need(m.get(key) in ACCESS, f"{where}: {key} must be one of {sorted(ACCESS)}")
    need(isinstance(m.get("encoded"), int) and 0 <= m["encoded"] <= 63, f"{where}: encoded must be LLVM16 MemoryEffects value in [0, 63]")

def validate_snapshot(s, where):
    exact_keys(s, {"nofree", "nosync", "willreturn", "nobuiltin", "optnone", "memory_explicit", "memory", "alloc_kind", "alloc_family", "return_noalias", "alloc_size", "formals"}, where)
    for b in ("nofree", "nosync", "willreturn", "nobuiltin", "optnone", "memory_explicit", "return_noalias"):
        need(isinstance(s.get(b), bool), f"{where}: {b} must be bool")
    validate_memory(s.get("memory"), where + ".memory")
    kinds = s.get("alloc_kind")
    need(isinstance(kinds, list), f"{where}: alloc_kind must be list")
    need(len(kinds) == len(set(kinds)), f"{where}: duplicate alloc_kind")
    need(all(k in ALLOC_KINDS for k in kinds), f"{where}: invalid alloc_kind")
    fam = s.get("alloc_family")
    need(fam is None or (isinstance(fam, str) and fam), f"{where}: alloc_family must be null or non-empty string")
    formals = s.get("formals")
    need(isinstance(formals, list), f"{where}: formals must be list")
    idxs = []
    returned = 0
    for i, f in enumerate(formals):
        fw = f"{where}.formals[{i}]"
        exact_keys(f, {"index", "pointer_typed", "nofree", "nocapture", "returned", "readnone", "readonly", "writeonly", "allocptr", "allocalign"}, fw)
        idx = f.get("index")
        need(isinstance(idx, int) and idx >= 0, f"{fw}: invalid index")
        idxs.append(idx)
        for b in ("pointer_typed", "nofree", "nocapture", "returned", "readnone", "readonly", "writeonly", "allocptr", "allocalign"):
            need(isinstance(f.get(b), bool), f"{fw}: {b} must be bool")
        if f["returned"]:
            returned += 1
        if f["allocptr"] or f["nofree"] or f["nocapture"] or f["readnone"] or f["readonly"] or f["writeonly"]:
            need(f["pointer_typed"], f"{fw}: pointer semantic attribute on non-pointer formal")
    need(len(idxs) == len(set(idxs)), f"{where}: duplicate formal index")
    need(idxs == list(range(len(idxs))), f"{where}: formal indices must be contiguous declaration order")
    need(returned <= 1, f"{where}: LLVM returned may appear on at most one formal")
    alloc_size = s.get("alloc_size")
    if alloc_size is not None:
        exact_keys(alloc_size, {"element_size_arg", "num_elements_arg"}, where + ".alloc_size")
        elem = alloc_size.get("element_size_arg")
        num = alloc_size.get("num_elements_arg")
        need(isinstance(elem, int) and 0 <= elem < len(formals), f"{where}: alloc_size element_size_arg out of range")
        need(num is None or (isinstance(num, int) and 0 <= num < len(formals)), f"{where}: alloc_size num_elements_arg out of range")

def validate_effects(path: Path, expected_llvm: str | None):
    root = read_json(path)
    exact_keys(root, {"schema", "llvm_version", "explicit_basis", "tli_basis", "modules"}, str(path))
    need(root.get("schema") == "llvm_memory_effects_v1", f"{path}: wrong schema")
    llvmv = root.get("llvm_version")
    need(isinstance(llvmv, str) and llvmv, f"{path}: missing llvm_version")
    if expected_llvm:
        need(llvmv == expected_llvm, f"{path}: llvm_version={llvmv!r}, expected {expected_llvm!r}")
    need(root.get("explicit_basis") == "llvm16_explicit_input_ir_v1", f"{path}: wrong explicit_basis")
    need(root.get("tli_basis") == "llvm16_tli_libfunc_attrs_v1", f"{path}: wrong tli_basis")
    modules = root.get("modules")
    need(isinstance(modules, list) and modules, f"{path}: modules must be non-empty list")
    for mi, mod in enumerate(modules):
        mw = f"modules[{mi}]"
        exact_keys(mod, {"input", "target_triple", "input_ir_verified", "tli_clone_verified", "functions", "callsites_explicit", "callsites_tli_inferred"}, mw)
        need(isinstance(mod.get("input"), str) and mod["input"], f"{mw}: missing input")
        need(isinstance(mod.get("target_triple"), str), f"{mw}: target_triple must be string")
        need(mod.get("input_ir_verified") is True, f"{mw}: input_ir_verified must be true")
        need(mod.get("tli_clone_verified") is True, f"{mw}: tli_clone_verified must be true")
        funcs = mod.get("functions")
        need(isinstance(funcs, list), f"{mw}: functions must be list")
        names = []
        for fi, f in enumerate(funcs):
            fw = f"{mw}.functions[{fi}]"
            exact_keys(f, {"name", "is_declaration", "origin_explicit", "explicit", "tli_recognized", "tli_libfunc", "origin_inferred", "tli_inferred", "tli_changed"}, fw)
            name = f.get("name")
            need(isinstance(name, str) and name, f"{fw}: missing name")
            names.append(name)
            need(isinstance(f.get("is_declaration"), bool), f"{fw}: is_declaration must be bool")
            need(f.get("origin_explicit") == "explicit_input_ir", f"{fw}: wrong explicit origin")
            need(f.get("origin_inferred") == "llvm_tli_inferred", f"{fw}: wrong inferred origin")
            need(isinstance(f.get("tli_recognized"), bool), f"{fw}: tli_recognized must be bool")
            need(isinstance(f.get("tli_changed"), bool), f"{fw}: tli_changed must be bool")
            libfunc = f.get("tli_libfunc")
            if f["tli_recognized"]:
                need(isinstance(libfunc, str) and libfunc, f"{fw}: recognized TLI function requires non-empty tli_libfunc")
            else:
                need(libfunc is None, f"{fw}: unrecognized function must have null tli_libfunc")
            validate_snapshot(f.get("explicit"), fw + ".explicit")
            validate_snapshot(f.get("tli_inferred"), fw + ".tli_inferred")
            changed = f["explicit"] != f["tli_inferred"]
            need(f["tli_changed"] == changed, f"{fw}: tli_changed does not match structural delta")
            if f["tli_changed"]:
                need(f["tli_recognized"], f"{fw}: TLI changed an unrecognized function")
                need(f["is_declaration"], f"{fw}: TLI evidence may not mutate a definition in EFX1")
                need(not f["explicit"]["nobuiltin"], f"{fw}: nobuiltin declaration changed")
                need(not f["explicit"]["optnone"], f"{fw}: optnone declaration changed")
        need(len(names) == len(set(names)), f"{mw}: duplicate function names")

        exp_calls = mod.get("callsites_explicit")
        inf_calls = mod.get("callsites_tli_inferred")
        need(isinstance(exp_calls, list) and isinstance(inf_calls, list), f"{mw}: callsite arrays required")
        need(len(exp_calls) == len(inf_calls), f"{mw}: callsite cardinality changed under evidence-only clone")
        for ci, (a, b) in enumerate(zip(exp_calls, inf_calls)):
            cw = f"{mw}.callsites[{ci}]"
            for rec in (a, b):
                exact_keys(rec, {"caller", "ordinal", "direct", "callee", "callsite_memory_explicit", "effective_memory"}, cw)
                need(isinstance(rec.get("caller"), str), f"{cw}: caller must be string")
                need(isinstance(rec.get("ordinal"), int) and rec["ordinal"] >= 0, f"{cw}: invalid ordinal")
                need(isinstance(rec.get("direct"), bool), f"{cw}: direct must be bool")
                need(rec.get("callee") is None or isinstance(rec.get("callee"), str), f"{cw}: callee must be string/null")
                need(isinstance(rec.get("callsite_memory_explicit"), bool), f"{cw}: callsite_memory_explicit must be bool")
                validate_memory(rec.get("effective_memory"), cw + ".effective_memory")
            keya = (a.get("caller"), a.get("ordinal"), a.get("direct"), a.get("callee"))
            keyb = (b.get("caller"), b.get("ordinal"), b.get("direct"), b.get("callee"))
            need(keya == keyb, f"{cw}: TLI clone changed callsite identity")
    return root

def validate_pts(path: Path):
    root = read_json(path)
    exact_keys(root, {"schema", "analysis", "semantics", "formal_mapping_schema", "functions"}, str(path))
    need(root.get("schema") == "svf_solved_points_to_v1", f"{path}: wrong schema")
    need(root.get("analysis") == "AndersenWaveDiff", f"{path}: wrong analysis")
    need(root.get("semantics") == "may", f"{path}: PTA must be MAY")
    need(root.get("formal_mapping_schema") == "svf_formal_arg_index_v1", f"{path}: wrong formal mapping schema")
    funcs = root.get("functions")
    need(isinstance(funcs, list), f"{path}: functions must be list")
    names = []
    for fi, f in enumerate(funcs):
        fw = f"functions[{fi}]"
        exact_keys(f, {"function", "formals"}, fw)
        name = f.get("function")
        need(isinstance(name, str) and name, f"{fw}: missing function")
        names.append(name)
        formals = f.get("formals")
        need(isinstance(formals, list), f"{fw}: formals must be list")
        idxs, vars_ = [], []
        for pi, p in enumerate(formals):
            pw = f"{fw}.formals[{pi}]"
            exact_keys(p, {"formal_index", "svf_var_id", "points_to"}, pw)
            idx = p.get("formal_index")
            var = p.get("svf_var_id")
            pts = p.get("points_to")
            need(isinstance(idx, int) and idx >= 0, f"{pw}: invalid formal_index")
            need(isinstance(var, int) and var >= 0, f"{pw}: invalid svf_var_id")
            need(isinstance(pts, list) and all(isinstance(x, int) and x >= 0 for x in pts), f"{pw}: invalid points_to")
            need(pts == sorted(set(pts)), f"{pw}: points_to must be sorted unique")
            idxs.append(idx); vars_.append(var)
        need(len(idxs) == len(set(idxs)), f"{fw}: duplicate formal_index")
        need(len(vars_) == len(set(vars_)), f"{fw}: duplicate formal svf_var_id")
        need(idxs == list(range(len(idxs))), f"{fw}: formal indices must be contiguous declaration order")
    need(len(names) == len(set(names)), f"{path}: duplicate function records")
    return root

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--effects", type=Path, required=True)
    ap.add_argument("--pts", type=Path, required=True)
    ap.add_argument("--expect-llvm", default="16.0.4")
    ap.add_argument("--require-libfunc", action="append", default=[])
    ns = ap.parse_args()
    try:
        effects = validate_effects(ns.effects, ns.expect_llvm or None)
        validate_pts(ns.pts)
        if ns.require_libfunc:
            by_name = {}
            for m in effects["modules"]:
                by_name.update({f["name"]: f for f in m["functions"]})
            for name in ns.require_libfunc:
                need(name in by_name, f"required libfunc {name!r} missing")
                rec = by_name[name]
                need(rec["tli_recognized"], f"required libfunc {name!r} not TLI-recognized")
                need(rec["tli_changed"], f"required libfunc {name!r} produced no inferred contract delta")
        print("EFX1_EVIDENCE_VALIDATION: PASS")
        return 0
    except ValidationError as e:
        print(f"EFX1_EVIDENCE_VALIDATION: FAIL: {e}", file=sys.stderr)
        return 1

if __name__ == "__main__":
    raise SystemExit(main())
