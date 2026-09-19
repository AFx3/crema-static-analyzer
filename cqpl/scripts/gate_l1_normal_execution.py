#!/usr/bin/env python3
import argparse, csv, json
from pathlib import Path

TARGET_QUERY='leak_alloc_state'

def tsv(path):
    with Path(path).open(newline='', encoding='utf-8') as f:
        return list(csv.DictReader(f, delimiter='\t'))

def index(rows, fields):
    out={}
    for r in rows:
        k=tuple(r[x] for x in fields)
        if k in out: raise SystemExit(f'duplicate key {k}')
        out[k]=r
    return out

def classes(raw):
    return {x.strip() for x in raw.split(',') if x.strip()}

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--baseline', type=Path, required=True, help='cqpl/artifact/POST_R2_FINAL118')
    ap.add_argument('--matrix', type=Path, required=True, help='fresh query-matrix directory')
    ap.add_argument('--oracle', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    a=ap.parse_args(); a.out.mkdir(parents=True, exist_ok=True)

    base_long=tsv(a.baseline/'QUERY_RESULTS_LONG.tsv')
    new_long=tsv(a.matrix/'query-results-long.tsv')
    b=index(base_long, ('group','target','query')); n=index(new_long, ('group','target','query'))
    if set(b)!=set(n):
        missing=sorted(set(b)-set(n)); extra=sorted(set(n)-set(b))
        raise SystemExit(f'FINAL118 key mismatch missing={missing[:10]} extra={extra[:10]}')
    truth_deltas=[]
    for k in sorted(b):
        if b[k]['result'] != n[k]['result']:
            truth_deltas.append((*k,b[k]['result'],n[k]['result']))

    base_unk=index(tsv(a.baseline/'UNKNOWN_EXPLANATIONS.tsv'), ('group','target','query'))
    new_unk=index(tsv(a.matrix/'unknown-explanations.tsv'), ('group','target','query'))
    non_target_assessment_deltas=[]
    for k,row in base_unk.items():
        if k[2]==TARGET_QUERY: continue
        other=new_unk.get(k)
        if other is None:
            non_target_assessment_deltas.append((*k,'missing_new_unknown'))
            continue
        before=(row['subresult'],row['direction'],row['strength'])
        after=(other['subresult'],other['direction'],other['strength'])
        if before!=after:
            non_target_assessment_deltas.append((*k,*before,*after))

    oracle={}
    for r in tsv(a.oracle):
        oracle[(r['group'],r['target'])]=r.get('reference_classes','')
    # Six FINAL118 additions not in historical FINAL112 oracle.
    oracle.update({
        ('corpus','clean_alloc_read_and_drop'):'CLEAN',
        ('experimental','bmulti_clean_two_args_ffi'):'CLEAN',
        ('experimental','bmulti_df_second_ffi'):'DF',
        ('experimental','bmulti_leak_second_ffi'):'ML',
        ('experimental','bmulti_uaf_second_ffi'):'UAF',
    })

    true_ml=[]; true_ml_false=[]; true_ml_mixed=[]
    for k,row in new_unk.items():
        if k[2]!=TARGET_QUERY: continue
        if 'ML' in classes(oracle.get((k[0],k[1]),'')):
            true_ml.append(k)
            if row['subresult']=='unk_false': true_ml_false.append(k)
            if row['subresult']=='unk_mixed': true_ml_mixed.append(k)

    def need(group,target):
        k=(group,target,TARGET_QUERY)
        if k not in new_unk: raise SystemExit(f'missing UNKNOWN explanation for {k}')
        return new_unk[k]
    clean=need('corpus','clean_alloc_read_and_drop')
    boxed=need('corpus','boxed_bool__ml')

    clean_ok=(clean['result']=='unk' and clean['subresult']=='unk_false' and clean['direction']=='false'
              and 'assessment_scope:normal_execution' in clean.get('assessment_basis','')
              and 'finding:all_candidate_suffixes_cross_modeled_drop' in clean.get('assessment_basis',''))
    boxed_ok=(boxed['result']=='unk' and boxed['subresult']=='unk_true' and boxed['direction']=='true'
              and 'assessment_scope:normal_execution' in boxed.get('assessment_basis',''))

    summary={
      'schema':'cqpl_gate_l1_normal_execution_v1',
      'truth_delta_count':len(truth_deltas),
      'non_target_assessment_delta_count':len(non_target_assessment_deltas),
      'known_true_ml_count':len(true_ml),
      'known_true_ml_unk_false_count':len(true_ml_false),
      'known_true_ml_unk_mixed_count':len(true_ml_mixed),
      'clean_alloc_read_and_drop':{k:clean.get(k,'') for k in ['result','subresult','direction','strength','assessment_basis']},
      'boxed_bool__ml':{k:boxed.get(k,'') for k in ['result','subresult','direction','strength','assessment_basis']},
      'criteria':{
        'truth_delta_0_of_1416': len(truth_deltas)==0 and len(new_long)==1416,
        'non_target_assessment_delta_0': len(non_target_assessment_deltas)==0,
        'known_true_ml_to_unk_false_0': len(true_ml_false)==0,
        'clean_is_unk_false': clean_ok,
        'boxed_bool_ml_remains_unk_true': boxed_ok,
      },
    }
    summary['status']='PASS' if all(summary['criteria'].values()) else 'FAIL'
    (a.out/'GATE_L1_NORMAL_EXECUTION.json').write_text(json.dumps(summary,indent=2)+'\n')
    with (a.out/'TRUTH_DELTAS.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t'); w.writerow(['group','target','query','before','after']); w.writerows(truth_deltas)
    with (a.out/'NON_TARGET_ASSESSMENT_DELTAS.tsv').open('w',newline='') as f:
        w=csv.writer(f,delimiter='\t'); w.writerow(['record']); [w.writerow(['\t'.join(x)]) for x in non_target_assessment_deltas]
    print(json.dumps(summary,indent=2))
    return 0 if summary['status']=='PASS' else 2

if __name__=='__main__': raise SystemExit(main())
