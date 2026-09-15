#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json, subprocess
from collections import Counter, defaultdict
from pathlib import Path


def read_subjects(path: Path):
    out=[]
    for raw in path.read_text().splitlines():
        if not raw.strip() or raw.startswith('#'):
            continue
        parts=raw.split('\t')
        if len(parts)!=3:
            raise SystemExit(f'bad subject row in {path}: {raw!r}')
        if parts[0]=='group' and parts[1]=='name':
            continue
        out.append((parts[0],parts[1],Path(parts[2])))
    return out


def main() -> int:
    ap=argparse.ArgumentParser(description='CQPL v6R explainability matrix; observational only')
    ap.add_argument('--checker', type=Path, required=True)
    ap.add_argument('--queries', type=Path, required=True)
    ap.add_argument('--subject-tsv', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--max-witnesses', type=int, default=8)
    ap.add_argument('--query', action='append', default=[], help='query stem filter; repeatable')
    ap.add_argument('--baseline-wide', type=Path, help='optional frozen query-results-wide.tsv; enforces result identity')
    args=ap.parse_args()

    if args.max_witnesses < 1:
        raise SystemExit('--max-witnesses must be >= 1')
    if not args.checker.is_file():
        raise SystemExit(f'missing checker: {args.checker}')

    subjects=read_subjects(args.subject_tsv)
    queries=sorted(args.queries.glob('*.cqpl'))
    if args.query:
        wanted=set(args.query)
        queries=[q for q in queries if q.stem in wanted]
        missing=wanted-{q.stem for q in queries}
        if missing:
            raise SystemExit(f'missing requested query stems: {sorted(missing)}')
    if not queries:
        raise SystemExit('no queries selected')

    baseline={}
    if args.baseline_wide:
        with args.baseline_wide.open(newline='') as f:
            for row in csv.DictReader(f,delimiter='\t'):
                for q in queries:
                    baseline[(row['group'],row['target'],q.stem)]=row[q.stem]

    args.out.mkdir(parents=True,exist_ok=True)
    edir=args.out/'explanations'; edir.mkdir(exist_ok=True)
    long=[]
    result_counts=Counter()
    reason_subject_presence=defaultdict(Counter)
    reason_signature_counts=defaultdict(Counter)
    failures=[]
    unknown_without_frontier=[]
    unknown_without_specific_origin=[]
    true_without_witness=[]
    true_without_atomic_witness=[]
    baseline_mismatches=[]

    for group,name,artifact in subjects:
        if not artifact.is_file():
            failures.append({'group':group,'target':name,'error':f'missing artifact {artifact}'})
            continue
        tdir=edir/group/name
        tdir.mkdir(parents=True,exist_ok=True)
        for q in queries:
            explain=tdir/f'{q.stem}.explain.json'
            cp=subprocess.run([
                str(args.checker),str(artifact),str(q),'--json',
                '--explain-json',str(explain),
                '--explain-max-witnesses',str(args.max_witnesses),
            ],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            if cp.returncode != 0:
                failures.append({'group':group,'target':name,'query':q.stem,'rc':cp.returncode,'stderr':cp.stderr[:1000]})
                continue
            try:
                plain=json.loads(cp.stdout)
                report=json.loads(explain.read_text())
            except Exception as e:
                failures.append({'group':group,'target':name,'query':q.stem,'error':f'json:{type(e).__name__}:{e}'})
                continue
            result=str(plain['result'])
            if str(report.get('result')) != result:
                failures.append({'group':group,'target':name,'query':q.stem,'error':'plain/explanation result mismatch'})
                continue
            expected=baseline.get((group,name,q.stem))
            if expected is not None and expected != result:
                baseline_mismatches.append({'group':group,'target':name,'query':q.stem,'baseline':expected,'v6r':result})
            reasons=sorted(set(map(str,report.get('reason_frontier',[]))))
            witnesses=report.get('witnesses',[])
            diagnostics=report.get('diagnostics',{})
            if result=='unk' and not reasons:
                unknown_without_frontier.append((group,name,q.stem))
            if result=='unk' and not diagnostics.get('unknown_has_specific_origin',False):
                unknown_without_specific_origin.append((group,name,q.stem))
            if result=='tt' and not witnesses:
                true_without_witness.append((group,name,q.stem))
            if result=='tt' and not diagnostics.get('true_has_atomic_witness',False):
                true_without_atomic_witness.append((group,name,q.stem))
            result_counts[result]+=1
            key=f'{q.stem}:{result}'
            for reason in reasons:
                reason_subject_presence[key][reason]+=1
            reason_signature_counts[key][';'.join(reasons) if reasons else '<none>']+=1
            long.append({
                'group':group,'target':name,'artifact':str(artifact),'query':q.stem,
                'result':result,'reasons':';'.join(reasons),'witnesses':len(witnesses),
                'complete_witnesses':sum(bool(w.get('complete_dependency_trace')) for w in witnesses),
                'explanation':str(explain),
            })

    with (args.out/'explainability-long.tsv').open('w',newline='') as f:
        fields=['group','target','artifact','query','result','reasons','witnesses','complete_witnesses','explanation']
        w=csv.DictWriter(f,delimiter='\t',fieldnames=fields)
        w.writeheader(); w.writerows(long)

    summary={
        'schema':'cqpl_explainability_matrix_v1',
        'subjects':len(subjects),
        'queries':[q.stem for q in queries],
        'attempts_expected':len(subjects)*len(queries),
        'attempts_completed':len(long),
        'result_counts':dict(sorted(result_counts.items())),
        'reason_subject_presence':{k:dict(sorted(v.items())) for k,v in sorted(reason_subject_presence.items())},
        'reason_signature_counts':{k:dict(sorted(v.items())) for k,v in sorted(reason_signature_counts.items())},
        'unknown_without_reason_frontier':[list(x) for x in unknown_without_frontier],
        'unknown_without_specific_origin':[list(x) for x in unknown_without_specific_origin],
        'true_without_witness':[list(x) for x in true_without_witness],
        'true_without_atomic_witness':[list(x) for x in true_without_atomic_witness],
        'baseline_result_mismatches':baseline_mismatches,
        'failures':failures,
        'scientific_note':'reason_subject_presence is overlapping; counts must not be summed as mutually exclusive causal classes. reason_signature_counts partitions by observed frontier signature.',
    }
    (args.out/'explainability-summary.json').write_text(json.dumps(summary,indent=2,sort_keys=True)+'\n')
    print(json.dumps(summary,indent=2,sort_keys=True))
    ok=(not failures and not unknown_without_frontier and not unknown_without_specific_origin and not true_without_witness and not true_without_atomic_witness and not baseline_mismatches and len(long)==len(subjects)*len(queries))
    print('V6R_EXPLAINABILITY_MATRIX:', 'PASS' if ok else 'FAIL')
    return 0 if ok else 5

if __name__=='__main__':
    raise SystemExit(main())
