#!/usr/bin/env python3
import argparse, json, re
from pathlib import Path

def load(p):
    return json.loads(Path(p).read_text())

def checker(run_root, target, query):
    p=Path(run_root)/target/f"{query}.checker.log"
    if not p.exists():
        return None
    text=p.read_text(errors="replace")
    def field(label):
        m=re.search(rf"^{re.escape(label)}:\s*(\S+)", text, re.M)
        return m.group(1) if m else None
    return {
        "truth":field("CQPL result"),
        "assessment":field("CQPL subresult"),
        "strength":field("CQPL strength"),
    }

def rust_ccalls(doc, allocator=None):
    out=[]
    for a in doc.get("allocations",[]):
        site=a.get("site") or {}
        if site.get("kind")!="c_call":
            continue
        if not str(site.get("node_id","")).startswith("rust::"):
            continue
        if allocator is not None and site.get("allocator")!=allocator:
            continue
        out.append(a)
    return out

def node_by_id(doc,nid):
    for n in doc.get("nodes",[]):
        if n.get("id")==nid:
            return n
    return None

def validate_contract_allocation(doc, allocator, count, label, errors):
    allocs=rust_ccalls(doc,allocator)
    if len(allocs)!=count:
        errors.append(f"{label}: expected {count} rust-callsite c_call/{allocator}, got {len(allocs)}")
        return allocs
    for a in allocs:
        aid=a.get("id")
        site=a.get("site") or {}
        nid=site.get("node_id")
        contract=a.get("allocator_contract") or {}
        if contract != {"family":"c_malloc","operation":allocator,"language":"c"}:
            errors.append(f"{label}:{nid}: unexpected allocator contract {contract}")
        n=node_by_id(doc,nid)
        if n is None:
            errors.append(f"{label}:{nid}: missing callsite node")
            continue
        labels=[
            x for x in (n.get("allocation_labels") or [])
            if x.get("predicate")=="alloc" and x.get("allocation")==aid
        ]
        if len(labels)!=1:
            errors.append(f"{label}:{nid}: expected one alloc label, got {len(labels)}")
        elif labels[0].get("certainty")!="may_abstract":
            errors.append(f"{label}:{nid}: expected may_abstract, got {labels[0].get('certainty')}")
        cells=[
            x for x in ((n.get("allocation_post") or {}).get("cells") or [])
            if x.get("allocation")==aid
        ]
        if len(cells)!=1 or cells[0].get("value")!="TOP":
            errors.append(f"{label}:{nid}: expected allocation_post TOP, got {cells}")
        pts=((n.get("identity") or {}).get("points_to") or [])
        rustvars=[
            x.get("variable") for x in pts
            if aid in (x.get("allocations") or [])
            and str(x.get("variable","")).startswith("rust::")
        ]
        if not rustvars:
            errors.append(f"{label}:{nid}: identity not attached to a Rust variable")
        prov=n.get("source_provenance") or {}
        if prov.get("language")!="rust":
            errors.append(f"{label}:{nid}: bodyless alloc provenance must be rust, got {prov.get('language')!r}")
    return allocs

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--run-root",type=Path,required=True)
    ap.add_argument("--out",type=Path,required=True)
    args=ap.parse_args()
    errors=[]
    evidence={}

    b01=load(args.run_root/"b01_malloc_leak/annotated_icfg_v2.json")
    validate_contract_allocation(b01,"malloc",1,"B01",errors)
    q=checker(args.run_root,"b01_malloc_leak","leak_alloc_state")
    evidence["B01_leak"]=q
    if not q or q["truth"]!="unk" or q["assessment"]!="unk_true":
        errors.append(f"B01: expected leak unk/unk_true, got {q}")

    b08=load(args.run_root/"b08b_c_malloc_then_rust_dealloc_mismatch/annotated_icfg_v2.json")
    validate_contract_allocation(b08,"malloc",1,"B08b",errors)
    q=checker(args.run_root,"b08b_c_malloc_then_rust_dealloc_mismatch","allocator_mismatch_ub_v2")
    evidence["B08b_mismatch"]=q
    if not q or q["truth"]!="unk" or q["assessment"]!="unk_true":
        errors.append(f"B08b: expected mismatch unk/unk_true, got {q}")

    b16=load(args.run_root/"b16_two_bodyless_malloc_callsites/annotated_icfg_v2.json")
    a16=validate_contract_allocation(b16,"malloc",2,"B16",errors)
    if len({a.get("id") for a in a16})!=len(a16):
        errors.append("B16: two static callsites collapsed to one AbstractAllocId")
    if len({(a.get("site") or {}).get("node_id") for a in a16})!=len(a16):
        errors.append("B16: two allocations do not preserve distinct callsite node IDs")

    b17=load(args.run_root/"b17_loop_bodyless_malloc_site_reuse/annotated_icfg_v2.json")
    validate_contract_allocation(b17,"malloc",1,"B17",errors)
    q=checker(args.run_root,"b17_loop_bodyless_malloc_site_reuse","double_free_alloc_state")
    evidence["B17_double_free"]=q
    if q and q.get("assessment")=="unk_true":
        errors.append(f"B17: static-site reuse alone oriented double-free positive: {q}")

    b18=load(args.run_root/"b18_getenv_pointer_return_not_alloc/annotated_icfg_v2.json")
    if rust_ccalls(b18):
        errors.append(f"B18: getenv/pointer return materialized C allocation: {rust_ccalls(b18)}")

    b19=load(args.run_root/"b19_calloc_leak/annotated_icfg_v2.json")
    validate_contract_allocation(b19,"calloc",1,"B19",errors)
    q=checker(args.run_root,"b19_calloc_leak","leak_alloc_state")
    evidence["B19_leak"]=q
    if not q or q["truth"]!="unk" or q["assessment"]!="unk_true":
        errors.append(f"B19: expected leak unk/unk_true, got {q}")

    report={
        "schema":"cqpl_bodyless_ffi_phase_b_minimal_v1",
        "strategy":"new AbstractAllocId + existing TOP",
        "status":"PASS" if not errors else "FAIL",
        "criteria":{
            "b01_bodyless_malloc_exports":not any(x.startswith("B01:") for x in errors),
            "b08b_mismatch_connected":not any(x.startswith("B08b:") for x in errors),
            "two_callsites_distinct":not any(x.startswith("B16:") for x in errors),
            "site_reuse_fail_closed":not any(x.startswith("B17:") for x in errors),
            "pointer_return_not_alloc":not any(x.startswith("B18:") for x in errors),
            "calloc_supported":not any(x.startswith("B19:") for x in errors),
        },
        "evidence":evidence,
        "errors":errors,
    }
    args.out.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    print("CQPL_BODYLESS_FFI_PHASE_B_MINIMAL:",report["status"])
    for k,v in report["criteria"].items():
        print(f"  {k}={v}")
    for e in errors:
        print("  ERROR:",e)
    raise SystemExit(0 if not errors else 2)

if __name__=="__main__":
    main()
