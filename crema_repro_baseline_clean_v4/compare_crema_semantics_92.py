#!/usr/bin/env python3
import json, sys
from pathlib import Path

EXCLUDED = {"openapi-client-gen"}

def canon(key, vals):
    s=set(vals)
    if key == "shared-register" and "ML" in s and s <= {"ML","DF"}:
        return frozenset({"ML","<OPTIONAL_DF>"})
    return frozenset(s)

if len(sys.argv)!=3:
    raise SystemExit("usage: compare_crema_semantics_92.py BASELINE.json CANDIDATE.json")
a=json.loads(Path(sys.argv[1]).read_text()); b=json.loads(Path(sys.argv[2]).read_text())
for k in EXCLUDED:
    a.pop(k,None); b.pop(k,None)
keys=sorted(set(a)|set(b)); bad=[]
for k in keys:
    av=a.get(k,["<MISSING>"]); bv=b.get(k,["<MISSING>"])
    if canon(k,av)!=canon(k,bv): bad.append((k,sorted(av),sorted(bv)))
print(f"excluded:       {', '.join(sorted(EXCLUDED))}")
print(f"baseline keys:  {len(a)}")
print(f"candidate keys: {len(b)}")
print(f"mismatches:     {len(bad)}")
for k,av,bv in bad:
    print(f"\n{k}\n  baseline : {av}\n  candidate: {bv}")
raise SystemExit(1 if bad else 0)
