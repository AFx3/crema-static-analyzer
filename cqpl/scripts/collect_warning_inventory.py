#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json, re
from collections import Counter, defaultdict
from pathlib import Path

WARN_RE=re.compile(r'^warning:\s*(.*)$')
LOC_RE=re.compile(r'^\s*-->\s*(.+)$')

def category(path: Path) -> str:
    s=path.as_posix()
    if '/raw/' in s and path.name=='build.log': return 'subject_build'
    if '/raw/' in s and path.name=='crema-export.log': return 'crema_export'
    if '/registry/' in s: return 'registry'
    if path.name.endswith('.stderr.log'): return 'query_stderr'
    if 'crema-tests' in path.name: return 'crema_tests'
    if 'cqpl-tests' in path.name: return 'cqpl_tests'
    if 'cqpl-build' in path.name: return 'cqpl_build'
    return 'other'

def main() -> int:
    ap=argparse.ArgumentParser(); ap.add_argument('--root',type=Path,required=True); ap.add_argument('--out',type=Path,required=True)
    args=ap.parse_args(); args.out.mkdir(parents=True,exist_ok=True)
    records=[]
    for p in sorted(args.root.rglob('*.log')):
        lines=p.read_text(errors='replace').splitlines()
        for i,line in enumerate(lines):
            m=WARN_RE.match(line)
            if not m: continue
            message=m.group(1).strip()
            loc=''
            for j in range(i+1,min(len(lines),i+8)):
                lm=LOC_RE.match(lines[j])
                if lm:
                    loc=lm.group(1).strip(); break
                if WARN_RE.match(lines[j]): break
            records.append({'category':category(p),'message':message,'location':loc,'log':p.relative_to(args.root).as_posix()})
    by_message=Counter((r['category'],r['message']) for r in records)
    summary=Counter(r['category'] for r in records)
    payload={
        'schema':'cqpl_warning_inventory_v1',
        'warning_occurrences':len(records),
        'by_category':dict(sorted(summary.items())),
        'unique_category_message_pairs':len(by_message),
        'top':[
            {'category':c,'message':m,'occurrences':n}
            for (c,m),n in by_message.most_common(100)
        ],
    }
    (args.out/'WARNING_INVENTORY.json').write_text(json.dumps(payload,indent=2,sort_keys=True)+'\n')
    with (args.out/'WARNING_INVENTORY.tsv').open('w',newline='') as f:
        w=csv.DictWriter(f,delimiter='\t',fieldnames=['category','message','location','log']); w.writeheader(); w.writerows(records)
    print(json.dumps(payload,indent=2,sort_keys=True))
    return 0

if __name__=='__main__': raise SystemExit(main())
