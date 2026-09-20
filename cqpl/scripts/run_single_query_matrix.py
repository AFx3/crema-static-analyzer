#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json, subprocess
from collections import Counter
from pathlib import Path

from unknown_explanations import UnknownExplanationError, explain_unknown


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument('--checker', type=Path, required=True)
    ap.add_argument('--query', type=Path, required=True)
    ap.add_argument('--subject-tsv', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--explain-unk-verbose', action='store_true')
    args = ap.parse_args()

    if not args.checker.is_file():
        raise SystemExit(f'missing checker {args.checker}')
    if not args.query.is_file():
        raise SystemExit(f'missing query {args.query}')

    subjects=[]
    for line in args.subject_tsv.read_text().splitlines():
        if not line.strip() or line.startswith('#'):
            continue
        parts=line.split('\t')
        if len(parts)!=3:
            raise SystemExit(f'bad subject row: {line!r}')
        subjects.append((parts[0],parts[1],Path(parts[2])))
    if not subjects:
        raise SystemExit('no subjects')

    args.out.mkdir(parents=True,exist_ok=True)
    rdir=args.out/'results'; rdir.mkdir(exist_ok=True)
    stem=args.query.stem
    long=[]; wide=[]; counts=Counter(); missing=[]
    unknown_explanations=[]; explanation_failures=[]

    for group,name,artifact in subjects:
        if not artifact.is_file():
            missing.append((group,name,str(artifact)))
            long.append([group,name,str(artifact),stem,'-','ARTIFACT_MISSING'])
            wide.append([group,name,'ARTIFACT_MISSING'])
            counts['ARTIFACT_MISSING'] += 1
            continue

        td=rdir/name; td.mkdir(parents=True,exist_ok=True)
        out=td/f'{stem}.json'; err=td/f'{stem}.stderr.log'
        cp=subprocess.run(
            [str(args.checker),str(artifact),str(args.query),'--json'],
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
        long.append([group,name,str(artifact),stem,cp.returncode,val])
        wide.append([group,name,val]); counts[val]+=1

        if val == 'unk':
            explain=td/f'{stem}.explain.json'
            try:
                record=explain_unknown(
                    checker=args.checker, artifact=artifact, query=args.query, result=val,
                    explanation=explain, verbose=args.explain_unk_verbose,
                )
            except UnknownExplanationError as e:
                explanation_failures.append({
                    'group':group,'target':name,'artifact':str(artifact),
                    'query':stem,'error':str(e),
                })
                print(f'{group}/{name} {stem}=unk EXPLANATION_FAIL: {e}')
            else:
                assert record is not None
                record.update({
                    'group':group,'target':name,'artifact':str(artifact),
                    'explanation':explain.relative_to(args.out).as_posix(),
                })
                unknown_explanations.append(record)
                print(
                    f"{group}/{name} {stem}=unk explanation={record['explanation']} "
                    f"reasons={';'.join(record['reason_frontier'])} "
                    f"supporting_findings={record['supporting_findings']} "
                    f"refuting_findings={record['refuting_findings']}"
                )

    with (args.out/'query-results-long.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t')
        w.writerow(['group','target','artifact','query','rc','result'])
        w.writerows(long)
    with (args.out/'query-results-wide.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t')
        w.writerow(['group','target',stem]); w.writerows(wide)

    unknown_expected=counts.get('unk',0)
    unknown_complete=(len(unknown_explanations)==unknown_expected and not explanation_failures)
    fields=[
        'group','target','artifact','query','result','subresult','direction','strength','assessment_schema','assessment_basis','assessment_caveats','explanation','reason_frontier',
        'witnesses','supporting_findings','supporting_finding_kinds','supporting_finding_strengths',
        'refuting_findings','refuting_finding_kinds','refuting_finding_strengths',
    ]
    with (args.out/'unknown-explanations.tsv').open('w',newline='') as f:
        w=csv.DictWriter(f,delimiter='\t',fieldnames=fields); w.writeheader()
        for record in unknown_explanations:
            row=dict(record)
            for key in [
                'reason_frontier','supporting_finding_kinds','supporting_finding_strengths',
                'refuting_finding_kinds','refuting_finding_strengths','assessment_basis','assessment_caveats'
            ]:
                row[key]=';'.join(row[key])
            w.writerow({key:row.get(key,'') for key in fields})

    unknown_summary={
        'schema':'cqpl_unknown_explanations_v1',
        'policy':'every_unknown_requires_valid_specific_explanation',
        'unknown_results':unknown_expected,
        'explanations_generated':len(unknown_explanations),
        'complete':unknown_complete,
        'failures':explanation_failures,
        'reports':unknown_explanations,
    }
    (args.out/'unknown-explanations-summary.json').write_text(json.dumps(unknown_summary,indent=2,sort_keys=True)+'\n')
    summary={
        'subjects':len(subjects),'queries':1,'attempts':len(subjects),
        'query':stem,'missing_artifacts':missing,'result_counts':dict(sorted(counts.items())),
    }
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2,sort_keys=True)+'\n')
    print(json.dumps(summary,indent=2,sort_keys=True))
    if not unknown_complete:
        return 5
    return 4 if missing else 0

if __name__=='__main__':
    raise SystemExit(main())
