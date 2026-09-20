#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json
from collections import Counter
from pathlib import Path


def rows(path: Path):
    with path.open(newline='', encoding='utf-8') as f:
        return list(csv.DictReader(f, delimiter='\t'))


def idx(rs, *fields):
    out={}
    for r in rs:
        k=tuple(r[f] for f in fields)
        if k in out:
            raise SystemExit(f'duplicate key {k}')
        out[k]=r
    return out


def non_normal_edges(report: dict) -> list[dict]:
    bad=[]
    for cert in report.get('diagnostic_certificates', []):
        for e in cert.get('abstract_witness', {}).get('edges', []):
            flows=e.get('flows', [])
            if e.get('basis') == 'typed_edge_flow_v1' and 'normal' not in flows:
                bad.append(e)
    return bad


def main() -> int:
    ap=argparse.ArgumentParser()
    ap.add_argument('--canonical-matrix', type=Path, required=True)
    ap.add_argument('--experimental-matrix', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args=ap.parse_args(); args.out.mkdir(parents=True,exist_ok=True)

    can_long=rows(args.canonical_matrix/'query-results-long.tsv')
    exp_long=rows(args.experimental_matrix/'query-results-long.tsv')
    can=idx(can_long,'group','target','query')
    exp=idx(exp_long,'group','target','query')
    exp_queries={k[2] for k in exp}
    if exp_queries != {'double_free_alloc_state_no_unwind_path'}:
        raise SystemExit(f'unexpected experimental query set: {exp_queries}')
    subjects={(r['group'],r['target']) for r in exp_long}
    if len(subjects)!=118:
        raise SystemExit(f'expected 118 experimental subjects, found {len(subjects)}')

    truth_mismatch=[]
    for group,target in sorted(subjects):
        old=can[(group,target,'double_free_alloc_state')]['result']
        new=exp[(group,target,'double_free_alloc_state_no_unwind_path')]['result']
        if old != new:
            truth_mismatch.append({'group':group,'target':target,'canonical':old,'experimental':new})

    exp_unk=idx(rows(args.experimental_matrix/'unknown-explanations.tsv'),'group','target','query')
    assessment_counts=Counter()
    for r in exp_unk.values():
        assessment_counts[r['subresult']] += 1

    controls={}
    expected={
        ('corpus','clean_alloc_read_and_drop'):('unk','unk_false'),
        ('corpus','boxed_bool__df'):('unk','unk_true'),
        ('corpus','boxed_char__df'):('unk','unk_true'),
    }
    control_ok=True
    for (g,t),(want_truth,want_sub) in expected.items():
        lr=exp.get((g,t,'double_free_alloc_state_no_unwind_path'))
        ur=exp_unk.get((g,t,'double_free_alloc_state_no_unwind_path'))
        got=(lr['result'] if lr else None, ur['subresult'] if ur else None)
        controls[f'{g}/{t}']={'expected':[want_truth,want_sub],'observed':list(got),'pass':got==(want_truth,want_sub)}
        control_ok &= controls[f'{g}/{t}']['pass']

    normal_cert_failures=[]
    for key,r in sorted(exp_unk.items()):
        p=args.experimental_matrix/r['explanation']
        report=json.loads(p.read_text())
        for i,cert in enumerate(report.get('diagnostic_certificates', [])):
            if cert.get('assessment_scope') != 'normal_execution':
                normal_cert_failures.append({'key':key,'certificate':i,'error':'assessment_scope is not normal_execution'})
                continue
            bad=non_normal_edges({'diagnostic_certificates':[cert]})
            if bad:
                normal_cert_failures.append({'key':key,'certificate':i,'error':'witness requires non-normal edge','edges':bad})

    # Audit the existing UAF assessment without changing its semantics.
    can_unk=idx(rows(args.canonical_matrix/'unknown-explanations.tsv'),'group','target','query')
    uaf_reports=0; uaf_positive_certs=0; uaf_requires_non_normal=0; uaf_examples=[]
    old_df_positive_certs=0; old_df_requires_non_normal=0
    for key,r in sorted(can_unk.items()):
        if key[2] not in {'use_after_free_alloc_state','double_free_alloc_state'}:
            continue
        report=json.loads((args.canonical_matrix/r['explanation']).read_text())
        for cert in report.get('diagnostic_certificates', []):
            if key[2]=='use_after_free_alloc_state' and cert.get('finding_kind')=='drop_then_use_without_reallocation':
                uaf_reports += 1
                uaf_positive_certs += 1
                bad=non_normal_edges({'diagnostic_certificates':[cert]})
                if bad:
                    uaf_requires_non_normal += 1
                    if len(uaf_examples)<10:
                        uaf_examples.append({'group':key[0],'target':key[1],'edges':bad})
            if key[2]=='double_free_alloc_state' and cert.get('finding_kind')=='repeated_drop_without_reallocation':
                old_df_positive_certs += 1
                if non_normal_edges({'diagnostic_certificates':[cert]}):
                    old_df_requires_non_normal += 1

    can_summary=json.loads((args.canonical_matrix/'summary.json').read_text())
    exp_summary=json.loads((args.experimental_matrix/'summary.json').read_text())
    report={
        'schema':'cqpl_full13_double_free_normal_execution_v1',
        'subjects':118,
        'canonical_queries':12,
        'experimental_queries':1,
        'combined_attempts':can_summary['attempts']+exp_summary['attempts'],
        'expected_combined_attempts':1534,
        'truth_equivalence':{
            'compared_subjects':118,
            'mismatch_count':len(truth_mismatch),
            'mismatches':truth_mismatch,
        },
        'experimental_truth_counts':exp_summary['result_counts'],
        'experimental_unknown_assessment_counts':dict(sorted(assessment_counts.items())),
        'controls':controls,
        'normal_certificate_failures':normal_cert_failures,
        'uaf_unwind_audit':{
            'positive_certificates':uaf_positive_certs,
            'positive_certificates_requiring_non_normal_edge':uaf_requires_non_normal,
            'examples':uaf_examples,
        },
        'legacy_double_free_unwind_audit':{
            'positive_certificates':old_df_positive_certs,
            'positive_certificates_requiring_non_normal_edge':old_df_requires_non_normal,
        },
    }
    report['criteria']={
        'combined_attempts_1534':report['combined_attempts']==1534,
        'experimental_truth_equals_canonical_truth_118_of_118':not truth_mismatch,
        'known_controls_pass':control_ok,
        'normal_execution_certificates_use_only_normal_edges':not normal_cert_failures,
    }
    report['status']='PASS' if all(report['criteria'].values()) else 'FAIL'
    out=args.out/'FULL13_DOUBLE_FREE_REPORT.json'
    out.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    print(json.dumps(report,indent=2,sort_keys=True))
    return 0 if report['status']=='PASS' else 2

if __name__=='__main__':
    raise SystemExit(main())
