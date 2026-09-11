#!/usr/bin/env python3
import re, sys, json
from pathlib import Path

def classify(text):
    out=set()
    if "Double Free Issues" in text:
        out.add("DF")
    if "Never Free Issues" in text:
        out.add("ML")
    if "Use-After-Free Issues / Undefined behaviour" in text or "Use detected at source line" in text:
        out.add("UAF")
    if "Possible UNDEFINED BEHAVIOUR!" in text or "allocated in Rust and then freed in C" in text:
        out.add("UB_FFI")
    if not out:
        if "Potential memory issues detected" in text:
            out.add("POTENTIAL_UNCLASSIFIED")
        elif "NO Issues detected" not in text and "(no memory" not in text:
            out.add("NO_CLASSIFIER_OUTPUT")
    return sorted(out)

def split_projects(text):
    chunks={}
    current=None
    buf=[]
    for line in text.splitlines():
        m=re.match(r"^=== (.+?) ===$", line)
        if m:
            if current is not None:
                chunks[current]="\n".join(buf)
            current=m.group(1)
            buf=[]
        elif current is not None:
            buf.append(line)
    if current is not None:
        chunks[current]="\n".join(buf)
    return chunks

if len(sys.argv)!=2:
    print("usage: normalize_results.py LOG", file=sys.stderr)
    sys.exit(2)

text=Path(sys.argv[1]).read_text(errors="replace")
print(json.dumps({k:classify(v) for k,v in split_projects(text).items()}, indent=2, sort_keys=True))
