#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json
from collections import Counter
from pathlib import Path

LEAK_QUERIES=('leak_alloc','leak_alloc_state')


def main() -> int:
    ap=argparse.ArgumentParser(description='Summarize v6R leak unknown explanation frontiers')
    ap.add_argument('--long', type=Path, required=True, help='explainability-long.tsv')
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--expect-subjects', type=int, default=112)
    ap.add_argument('--expect-unknown-per-query', type=int, default=105)
    args=ap.parse_args()

    with args.long.open(newline='') as f:
        rows=list(csv.DictReader(f,delimiter='\t'))
    out={
        'schema':'cqpl_v6r_leak_unknown_summary_v1',
        'scientific_note':'reason_presence counts overlap. signature_counts are mutually exclusive by complete frontier signature. No reason is interpreted as producer provenance unless explicitly emitted by the checker.',
        'queries':{},
    }
    ok=True
    for query in LEAK_QUERIES:
        qr=[r for r in rows if r['query']==query]
        unknown=[r for r in qr if r['result']=='unk']
        reason_presence=Counter()
        signatures=Counter()
        for r in unknown:
            reasons=tuple(x for x in r['reasons'].split(';') if x)
            for reason in set(reasons): reason_presence[reason]+=1
            signatures[';'.join(sorted(set(reasons))) if reasons else '<none>']+=1
        qout={
            'subjects':len(qr),
            'unknown':len(unknown),
            'unknown_rate':(len(unknown)/len(qr) if qr else None),
            'reason_presence':dict(sorted(reason_presence.items())),
            'signature_counts':dict(sorted(signatures.items())),
            'all_unknown_have_may_allocation':'MAY_ALLOCATION' in reason_presence and reason_presence['MAY_ALLOCATION']==len(unknown),
        }
        out['queries'][query]=qout
        if len(qr)!=args.expect_subjects or len(unknown)!=args.expect_unknown_per_query or not qout['all_unknown_have_may_allocation']:
            ok=False
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(out,indent=2,sort_keys=True)+'\n')
    print(json.dumps(out,indent=2,sort_keys=True))
    print('V6R_LEAK_UNKNOWN_AUDIT:', 'PASS' if ok else 'FAIL')
    return 0 if ok else 6

if __name__=='__main__':
    raise SystemExit(main())
