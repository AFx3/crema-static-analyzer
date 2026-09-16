#!/usr/bin/env python3
from pathlib import Path
import argparse,sys
p=argparse.ArgumentParser(); p.add_argument("root"); p.add_argument("--rust-status",default="candidate"); p.add_argument("--expected-rust-summaries",type=int,default=5); a=p.parse_args()
root=Path(a.root).resolve(); sys.path.insert(0,str(root/"cqpl/scripts"))
from library_effects_v1 import load_registry,validate_registry_set
errors=[]; regs=[]
for path in [root/"cqpl/library_models/rust_std_v1.json",root/"cqpl/library_models/libc_v1.json"]:
    try: regs.append(load_registry(path))
    except Exception as exc: errors.append(f"{path}: {exc}")
if not errors:
    try: validate_registry_set(regs)
    except Exception as exc: errors.append(str(exc))
rust=next((r for r in regs if r.registry_id=="rust_std_v1"),None); libc=next((r for r in regs if r.registry_id=="libc_v1"),None)
if rust is None: errors.append("missing rust_std_v1")
else:
    if rust.status!=a.rust_status: errors.append(f"rust status={rust.status}")
    if len(rust.summaries)!=a.expected_rust_summaries: errors.append(f"rust summaries={len(rust.summaries)}")
if libc is None: errors.append("missing libc_v1")
elif libc.status!="scaffold" or libc.summaries: errors.append("A2 forbids active libc summaries")
print("rust_status =",rust.status if rust else None); print("rust_summaries =",len(rust.summaries) if rust else None); print("libc_summaries =",len(libc.summaries) if libc else None); print("errors =",len(errors))
for e in errors: print("ERROR:",e)
print("LIBRARY_EFFECTS_V1_MODELS:","PASS" if not errors else "FAIL")
raise SystemExit(0 if not errors else 1)
