#!/usr/bin/env python3
"""Fail unless focused smoke outputs preserve the required properties."""
import argparse,csv,json
from pathlib import Path
ap=argparse.ArgumentParser();ap.add_argument('--root',type=Path,default=Path('/mnt/wsx-storage/af'));a=ap.parse_args()
checks=[
 ('smoke-closure-df','DF_NORMAL',True),('smoke-real-uaf','UAF',True),
 ('smoke-real-df','DF_NORMAL',True),('smoke-ml-malloc','ML',True),
 ('smoke-ml-calloc','ML',True),('smoke-b20b','ML',True),
 ('smoke-b20b','DF_NORMAL',True),('smoke-b20c','ML',False),('smoke-b39','ML',True),
]
failures=[]
for directory,query,expected in checks:
 path=a.root/directory/'ALL_FIVE_QUERY_RESULTS.tsv'
 if not path.is_file():failures.append(f'missing {path}');continue
 with path.open(newline='') as stream:rows=list(csv.DictReader(stream,delimiter='\t'))
 selected=[r for r in rows if r['query_id']==query]
 if len(selected)!=1:failures.append(f'{directory}: expected one {query}, got {len(selected)}');continue
 row=selected[0];actual=row['operational_detection']=='true';findings=int(row.get('positive_finding_count') or 0)
 if actual!=expected:failures.append(f'{directory}/{query}: detection={actual}, expected={expected}')
 if expected and findings<1:failures.append(f'{directory}/{query}: positive detection lacks finding')
 if not expected and findings:failures.append(f'{directory}/{query}: negative control has {findings} positive findings')
if failures:raise SystemExit('\n'.join(failures))
print(json.dumps({'status':'PASS','checks':len(checks)},indent=2))
