#!/usr/bin/env python3
import argparse, json
from pathlib import Path
from collections import Counter

KEYS = {
    "nofree", "nocapture", "returned", "return_noalias", "alloc_family",
    "alloc-family", "alloc_kinds", "allockind", "allocptr", "alloc_size",
    "allocsize", "tli_recognized", "tli_libfunc", "memory", "access"
}

def walk(x, path=()):
    if isinstance(x, dict):
        yield path, x
        for k,v in x.items():
            yield from walk(v, path+(str(k),))
    elif isinstance(x, list):
        for i,v in enumerate(x):
            yield from walk(v, path+(str(i),))

def evidence_counts(doc):
    c=Counter()
    caps=set(doc.get("capabilities") or [])
    if "llvm_memory_effects_v1" in caps:
        c["artifacts_with_llvm_memory_effects_v1"] += 1
    llvm=doc.get("llvm_memory_effects")
    if llvm is None:
        return c, []
    samples=[]
    for path,obj in walk(llvm):
        for k,v in obj.items():
            lk=k.lower()
            if lk == "tli_recognized" and v is True: c["tli_recognized_true"] += 1
            if lk == "nofree" and v is True: c["nofree_true"] += 1
            if lk == "nocapture" and v is True: c["nocapture_true"] += 1
            if lk == "returned" and v is True: c["returned_true"] += 1
            if lk in ("return_noalias","noalias") and v is True: c["return_noalias_true"] += 1
            if lk in ("alloc_family","alloc-family") and v not in (None,"",[],{}):
                c["alloc_family_present"] += 1
            if lk in ("alloc_kinds","allockind"):
                vals=v if isinstance(v,list) else [v]
                for raw in vals:
                    s=str(raw).lower()
                    for kind in ("alloc","realloc","free"):
                        if kind in s: c[f"allockind_{kind}"] += 1
            if lk in ("memory","access","encoded"):
                s=json.dumps(v,sort_keys=True).lower()
                if "argmem" in s and "read" in s: c["argmem_read_occurrences"] += 1
                if "argmem" in s and "write" in s: c["argmem_write_occurrences"] += 1
                if "read" in s: c["memory_read_occurrences"] += 1
                if "write" in s: c["memory_write_occurrences"] += 1
        if len(samples)<20 and any(k.lower() in KEYS for k in obj):
            samples.append({"path":"/".join(path), "keys":sorted(obj.keys())})
    return c,samples

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--artifact-root", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args=ap.parse_args()
    files=sorted(args.artifact_root.rglob("annotated_icfg_v2.json"))
    total=Counter()
    per=[]
    sample=[]
    for f in files:
        try:
            doc=json.loads(f.read_text())
        except Exception as e:
            per.append({"artifact":str(f),"error":str(e)})
            continue
        c,s=evidence_counts(doc)
        total.update(c)
        if c:
            per.append({"artifact":str(f),"counts":dict(c)})
        for x in s:
            if len(sample)<30:
                sample.append({"artifact":str(f),**x})
    report={
        "schema":"cqpl_bodyless_ffi_evidence_inventory_v1",
        "artifact_root":str(args.artifact_root),
        "annotated_artifacts":len(files),
        "evidence_occurrence_counts":dict(sorted(total.items())),
        "important_limitation":{
            "exact_reachable_bodyless_call_count_available":False,
            "reason":"the current annotated artifact does not expose a normative represented-body/bodyless bit per external call; this audit deliberately does not infer it from node-name heuristics"
        },
        "sample_payload_shapes":sample,
        "per_artifact_nonzero":per,
        "status":"PASS" if files else "FAIL_NO_ARTIFACTS"
    }
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    print(json.dumps({
        "status":report["status"],
        "annotated_artifacts":len(files),
        "evidence_occurrence_counts":report["evidence_occurrence_counts"],
        "exact_bodyless_count":"not derivable without heuristic"
    },indent=2,sort_keys=True))
    raise SystemExit(0 if files else 2)

if __name__=="__main__":
    main()
