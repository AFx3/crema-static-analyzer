#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, json
from pathlib import Path

FAMILIES={
    'ML':'leak_alloc',
    'DF':'double_free_alloc',
    'UAF':'use_after_free_alloc',
    'UB_FFI':'allocator_mismatch_ub_v2',
}


def main() -> int:
    ap=argparse.ArgumentParser(description='CQPL v6R reviewed-oracle precision metrics')
    ap.add_argument('--oracle-audit', type=Path, required=True)
    ap.add_argument('--matrix', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    args=ap.parse_args()

    with args.oracle_audit.open(newline='') as f:
        oracle=list(csv.DictReader(f,delimiter='\t'))
    with args.matrix.open(newline='') as f:
        matrix={(r['group'],r['target']):r for r in csv.DictReader(f,delimiter='\t')}

    report={'schema':'cqpl_precision_metrics_v1','families':{}}
    for klass,query in FAMILIES.items():
        positives=[]; negatives=[]
        for row in oracle:
            key=(row['group'],row['target'])
            if key not in matrix or row.get('reference_basis') in ('none',''):
                continue
            classes={x for x in row.get('reference_classes','').split(',') if x}
            explained=row.get('explained_deviation','')
            # A reviewed legacy deviation removes only the explicitly disputed
            # class from the positive oracle. Other classes on the same target
            # remain valid evidence (e.g. df_rand keeps DF/UB_FFI but not UAF).
            disputed = bool(explained) and (f'{klass}:' in explained or explained.startswith(f'{klass}:'))
            result=matrix[key][query]
            is_positive = klass in classes and not disputed
            (positives if is_positive else negatives).append(result)
        p=len(positives); n=len(negatives)
        p_tt=sum(x=='tt' for x in positives); p_unk=sum(x=='unk' for x in positives); p_ff=sum(x=='ff' for x in positives)
        n_tt=sum(x=='tt' for x in negatives); n_unk=sum(x=='unk' for x in negatives); n_ff=sum(x=='ff' for x in negatives)
        report['families'][klass]={
            'query':query,
            'reviewed_positive':p,
            'reviewed_negative':n,
            'unknown_rate_over_reviewed':((p_unk+n_unk)/(p+n) if p+n else None),
            'vulnerable_non_refutation_rate':((p_tt+p_unk)/p if p else None),
            'vulnerable_definite_detection':(p_tt/p if p else None),
            'clean_definite_refutation':(n_ff/n if n else None),
            'unexpected_ff_on_reviewed_positive':p_ff,
            'unexpected_tt_on_reviewed_clean':n_tt,
            'positive_distribution':{'tt':p_tt,'unk':p_unk,'ff':p_ff},
            'negative_distribution':{'tt':n_tt,'unk':n_unk,'ff':n_ff},
        }
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    print(json.dumps(report,indent=2,sort_keys=True))
    return 0

if __name__=='__main__':
    raise SystemExit(main())
