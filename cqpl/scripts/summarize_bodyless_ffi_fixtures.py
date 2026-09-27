#!/usr/bin/env python3
import argparse, json, re
from pathlib import Path
from collections import Counter

QUERIES = [
    "leak_alloc_state",
    "double_free_alloc_state",
    "use_after_free_alloc_state",
    "allocator_mismatch_ub_v2",
]

def parse_log(path):
    text = path.read_text(errors="replace")
    def match(rx):
        m = re.search(rx, text, re.M)
        return m.group(1) if m else None
    return {
        "truth": match(r"^CQPL result:\s*(\S+)"),
        "assessment": match(r"^CQPL subresult:\s*(\S+)"),
        "strength": match(r"^CQPL strength:\s*(\S+)"),
    }

def load_status(path):
    rows={}
    if not path.exists():
        return rows
    for line in path.read_text().splitlines():
        if not line.strip() or line.startswith("target\t"):
            continue
        target,status,detail=(line.split("\t",2)+["",""])[:3]
        rows[target]={"producer_status":status,"producer_detail":detail}
    return rows

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--run-root",type=Path,required=True)
    ap.add_argument("--manifest",type=Path,required=True)
    ap.add_argument("--producer-status",type=Path,required=True)
    ap.add_argument("--out",type=Path,required=True)
    a=ap.parse_args()

    manifest=json.loads(a.manifest.read_text())
    statuses=load_status(a.producer_status)
    rows=[]
    failures=[]
    counts=Counter()

    for fx in manifest["fixtures"]:
        name=fx["target"]
        d=a.run_root/name
        observed=statuses.get(name,{"producer_status":"missing","producer_detail":"no producer status record"})
        expected=fx["phase_a_expected_producer_status"]
        status=observed["producer_status"]
        counts[status]+=1

        row={**fx,**observed,"producer_expectation_matches":status==expected,"queries":{}}
        if status != expected:
            failures.append(f"{name}:producer-status expected={expected} observed={status}")

        ann=d/"annotated_icfg_v2.json"
        row["annotated_icfg_exists"]=ann.exists()
        if status=="pass":
            if not ann.exists():
                failures.append(f"{name}:producer-pass-without-annotated-icfg")
            else:
                doc=json.loads(ann.read_text())
                row["capabilities"]=doc.get("capabilities",[])
                row["nodes"]=len(doc.get("nodes",[]))
                row["allocations"]=len(doc.get("allocations",[]))
                row["has_llvm_memory_effects"]=doc.get("llvm_memory_effects") is not None

            for q in QUERIES:
                lp=d/f"{q}.checker.log"
                item=parse_log(lp) if lp.exists() else {"truth":None,"assessment":None,"strength":None}
                row["queries"][q]=item
                if item["truth"] is None:
                    failures.append(f"{name}:{q}:missing-result")
        else:
            # A structured fail-closed baseline is itself the observation.
            # CQPL must not be run on a missing/invalid annotated artifact.
            for q in QUERIES:
                row["queries"][q]={"truth":None,"assessment":None,"strength":None,"not_run":"producer did not export a valid schema-v2 artifact"}
            if ann.exists():
                failures.append(f"{name}:{status}:unexpected-annotated-icfg-present")

        alloc_id=d/"allocation_identity.json"
        row["allocation_identity_exists"]=alloc_id.exists()
        if alloc_id.exists():
            try:
                ident=json.loads(alloc_id.read_text())
                rows_by_node=ident.get("event_by_node",[])
                row["event_identity_points_to_entries"]=sum(
                    len((r.get("memory") or {}).get("points_to") or []) for r in rows_by_node
                )
            except Exception as e:
                row["allocation_identity_parse_error"]=str(e)

        rows.append(row)

    report={
        "schema":"cqpl_bodyless_ffi_fixture_baseline_v2",
        "status":"PASS" if not failures else "FAIL",
        "phase":"A-characterization-no-producer-semantics-change",
        "fixture_count":len(rows),
        "producer_status_counts":dict(sorted(counts.items())),
        "expected_structured_fail_closed_count":sum(
            1 for f in manifest["fixtures"] if f["phase_a_expected_producer_status"]!="pass"
        ),
        "failures":failures,
        "fixtures":rows,
        "interpretation":{
            "PASS":"all fixtures matched the explicitly frozen Phase-A producer behavior; fail-closed rows are documented gaps, not semantic success",
            "future_goal":"semantic implementation phases should convert applicable fail-closed/unknown characterization rows into provenance-backed analyzable effects"
        }
    }
    a.out.parent.mkdir(parents=True,exist_ok=True)
    a.out.write_text(json.dumps(report,indent=2,sort_keys=True)+"\n")
    print(f"BODYLESS_FFI_FIXTURE_BASELINE: {report['status']} fixtures={len(rows)} producer_status_counts={dict(counts)} failures={len(failures)}")
    if failures:
        for x in failures: print("  ",x)
        raise SystemExit(2)

if __name__=="__main__":
    main()
