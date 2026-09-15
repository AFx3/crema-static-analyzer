#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json, subprocess
from collections import Counter
from pathlib import Path


def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--checker', type=Path, required=True)
    ap.add_argument('--queries', type=Path, required=True)
    ap.add_argument('--subject', action='append', default=[], help='GROUP:NAME:ARTIFACT')
    ap.add_argument('--subject-tsv', type=Path, help='tab-separated group,name,artifact; no header required')
    ap.add_argument('--out', type=Path, required=True)
    args=ap.parse_args()

    subjects=[]
    if args.subject_tsv:
        for line in args.subject_tsv.read_text().splitlines():
            if not line.strip() or line.startswith('#'):
                continue
            parts=line.split('\t')
            if parts[0]=='group' and len(parts)>2 and parts[1]=='name':
                continue
            if len(parts)!=3:
                raise SystemExit(f'bad subject row: {line!r}')
            subjects.append((parts[0],parts[1],Path(parts[2])))
    for spec in args.subject:
        try:
            g,n,p=spec.split(':',2)
        except ValueError:
            raise SystemExit(f'bad --subject {spec!r}')
        subjects.append((g,n,Path(p)))
    if not subjects:
        raise SystemExit('no subjects')

    queries=sorted(args.queries.glob('*.cqpl'))
    if len(queries)!=12:
        raise SystemExit(f'expected exactly 12 queries_v2, found {len(queries)}')
    if not args.checker.is_file():
        raise SystemExit(f'missing checker {args.checker}')

    args.out.mkdir(parents=True,exist_ok=True)
    rdir=args.out/'results'; rdir.mkdir(exist_ok=True)
    long=[]; wide=[]; counts=Counter(); missing=[]
    for group,name,artifact in subjects:
        row=[group,name]
        if not artifact.is_file():
            missing.append((group,name,str(artifact)))
            for q in queries:
                val='ARTIFACT_MISSING'
                long.append([group,name,str(artifact),q.stem,'-',val])
                row.append(val); counts[val]+=1
            wide.append(row)
            continue
        td=rdir/name; td.mkdir(parents=True,exist_ok=True)
        for q in queries:
            out=td/f'{q.stem}.json'; err=td/f'{q.stem}.stderr.log'
            cp=subprocess.run(
                [str(args.checker),str(artifact),str(q),'--json'],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            out.write_text(cp.stdout); err.write_text(cp.stderr)
            if cp.returncode==0:
                try:
                    val=str(json.loads(cp.stdout)['result'])
                except Exception as e:
                    val=f'ERROR(json:{type(e).__name__})'
            else:
                first=' '.join(x.strip() for x in cp.stderr.splitlines() if x.strip())[:240]
                val=f'ERROR(rc={cp.returncode}:{first})'
            long.append([group,name,str(artifact),q.stem,cp.returncode,val])
            row.append(val); counts[val]+=1
        wide.append(row)

    with (args.out/'query-results-long.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t')
        w.writerow(['group','target','artifact','query','rc','result'])
        w.writerows(long)
    with (args.out/'query-results-wide.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t')
        w.writerow(['group','target']+[q.stem for q in queries])
        w.writerows(wide)
    summary={
        'subjects':len(subjects),
        'queries':len(queries),
        'attempts':len(subjects)*len(queries),
        'missing_artifacts':missing,
        'result_counts':dict(sorted(counts.items())),
    }
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(summary,indent=2))
    return 4 if missing else 0

if __name__=='__main__':
    raise SystemExit(main())
