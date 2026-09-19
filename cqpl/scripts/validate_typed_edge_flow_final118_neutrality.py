#!/usr/bin/env python3
import argparse, csv, json, subprocess, tempfile
from pathlib import Path

UNWIND = {
    'Call unwind','Drop unwind','Assert unwind','InlineAsm unwind',
    'Rust unwind propagate','Rust drop unwind propagate'
}

def run_checker(checker: Path, artifact: Path, query: Path):
    p = subprocess.run([str(checker), str(artifact), str(query), '--json'], text=True, capture_output=True)
    if p.returncode != 0:
        raise RuntimeError(f"checker failed artifact={artifact} query={query}\nSTDOUT:\n{p.stdout}\nSTDERR:\n{p.stderr}")
    x=json.loads(p.stdout)
    return x['result'], x['assessment']

def validate_r2_artifact(x, path):
    caps=set(x.get('capabilities',[]))
    if 'typed_edge_flow_v1' not in caps:
        raise RuntimeError(f"{path}: missing typed_edge_flow_v1")
    edges=x.get('typed_edges')
    if not isinstance(edges,list):
        raise RuntimeError(f"{path}: missing typed_edges array")
    nodes={n['id']:n for n in x['nodes']}
    proj={k:set() for k in nodes}
    seen=set()
    for e in edges:
        key=(e['source'],e['destination'],e['flow'],e.get('label'),e.get('source_label'),e.get('destination_label'))
        if key in seen:
            raise RuntimeError(f"{path}: duplicate typed edge {key}")
        seen.add(key)
        if e['source'] not in nodes or e['destination'] not in nodes:
            raise RuntimeError(f"{path}: typed edge outside node domain: {key}")
        expected='unwind' if e.get('label') in UNWIND else 'normal'
        if e['flow'] != expected:
            raise RuntimeError(f"{path}: flow/label mismatch: {key}, expected {expected}")
        proj[e['source']].add(e['destination'])
    for node_id,n in nodes.items():
        if set(n.get('successors',[])) != proj[node_id]:
            raise RuntimeError(f"{path}: projection mismatch at {node_id}")

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--subjects', required=True, type=Path, help='TSV: group<TAB>target<TAB>annotated_icfg_v2.json')
    ap.add_argument('--checker', required=True, type=Path)
    ap.add_argument('--queries-dir', required=True, type=Path)
    ap.add_argument('--out', required=True, type=Path)
    args=ap.parse_args()
    queries=sorted(args.queries_dir.glob('*.cqpl'))
    if len(queries)!=12:
        raise SystemExit(f"expected 12 queries, found {len(queries)}")
    rows=[]
    subjects=[]
    with args.subjects.open() as f:
        for line in f:
            if not line.strip(): continue
            parts=line.rstrip('\n').split('\t')
            if len(parts)<3: raise SystemExit(f"bad SUBJECTS.tsv line: {line!r}")
            subjects.append((parts[0],parts[1],Path(parts[2])))
    if len(subjects)!=118:
        raise SystemExit(f"expected 118 subjects, found {len(subjects)}")
    args.out.mkdir(parents=True, exist_ok=True)
    deltas=0
    attempts=0
    edge_counts={'normal':0,'unwind':0}
    with tempfile.TemporaryDirectory(prefix='gate-a-r2-final118-') as td:
        td=Path(td)
        for si,(group,target,artifact) in enumerate(subjects,1):
            x=json.load(open(artifact))
            validate_r2_artifact(x, artifact)
            for e in x['typed_edges']:
                edge_counts[e['flow']]+=1
            stripped=json.loads(json.dumps(x))
            stripped.pop('typed_edges',None)
            stripped['capabilities']=[c for c in stripped.get('capabilities',[]) if c!='typed_edge_flow_v1']
            sp=td/f'{si:03d}.json'
            json.dump(stripped,open(sp,'w'))
            for q in queries:
                a=run_checker(args.checker,artifact,q)
                b=run_checker(args.checker,sp,q)
                attempts+=1
                same=a==b
                if not same: deltas+=1
                rows.append({
                    'group':group,'target':target,'query':q.name,
                    'r2_truth':a[0],'stripped_truth':b[0],
                    'truth_same':a[0]==b[0],
                    'assessment_same':a[1]==b[1],
                })
            print(f'[{si:03d}/118] {group}/{target}: PASS')
    with (args.out/'FINAL118_R2_NEUTRALITY.tsv').open('w',newline='') as f:
        w=csv.DictWriter(f,fieldnames=rows[0].keys(),delimiter='\t')
        w.writeheader(); w.writerows(rows)
    summary={
        'schema':'gate_a_r2_final118_neutrality_v1',
        'subjects':len(subjects),'queries':len(queries),'attempts':attempts,
        'truth_or_assessment_delta_count':deltas,
        'typed_edge_counts':edge_counts,
        'status':'PASS' if deltas==0 else 'FAIL'
    }
    json.dump(summary,open(args.out/'FINAL118_R2_NEUTRALITY.json','w'),indent=2,sort_keys=True)
    print(json.dumps(summary,indent=2,sort_keys=True))
    if deltas:
        raise SystemExit(1)

if __name__=='__main__': main()
