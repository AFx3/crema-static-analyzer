#!/usr/bin/env python3
"""Generate property-level RQ1 CSV/LaTeX tables from one coherent campaign.

This reporter never executes CREMA/CQPL and never infers cells from findings.
Its unit is exactly one (target_id, primary query/property) observation.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from collections import Counter
from pathlib import Path

PROPERTIES=("ML","DF","UAF","ALLOC_MISMATCH")
QUERY_FOR={"ML":"ML","DF":"DF_NORMAL","UAF":"UAF","ALLOC_MISMATCH":"ALLOC_MISMATCH"}
COUNT_FOR={"ML":"ml_count","DF":"df_count","UAF":"uaf_count","ALLOC_MISMATCH":"allocator_mismatch_count"}
ORACLE_COLUMNS={"test_name","ml_count","df_count","uaf_count","allocator_mismatch_count","total_errors","is_clean","review_status"}

def sha(path:Path)->str:
 d=hashlib.sha256()
 with path.open('rb') as f:
  for block in iter(lambda:f.read(1<<20),b''):d.update(block)
 return d.hexdigest()

def ratio(n:int,d:int): return None if d==0 else n/d

def metrics(c:Counter[str])->dict[str,object]:
 tp,fp,tn,fn=(c[x] for x in ("TP","FP","TN","FN"));n=tp+fp+tn+fn
 return {"evaluable":n,"TP":tp,"FP":fp,"TN":tn,"FN":fn,
         "precision":ratio(tp,tp+fp),"recall":ratio(tp,tp+fn),
         "f1_score":ratio(2*tp,2*tp+fp+fn),"accuracy":ratio(tp+tn,n),
         "specificity":ratio(tn,tn+fp)}

def load_oracle(path:Path)->dict[tuple[str,str],bool]:
 with path.open(newline='',encoding='utf-8-sig') as f:
  reader=csv.DictReader(f);fields=set(reader.fieldnames or [])
  if not ORACLE_COLUMNS<=fields: raise SystemExit(f'unsupported oracle columns: {sorted(fields)}')
  rows=list(reader)
 if len(rows)!=175: raise SystemExit(f'oracle must contain 175 rows, observed {len(rows)}')
 truth={};targets=set()
 for line,row in enumerate(rows,2):
  target=(row.get('test_name') or '').strip()
  if not target or target in targets: raise SystemExit(f'invalid/duplicate oracle target at line {line}: {target!r}')
  targets.add(target);counts={}
  for prop,column in COUNT_FOR.items():
   raw=(row.get(column) or '').strip()
   if not raw.isdecimal(): raise SystemExit(f'non-integer {column} at oracle line {line}: {raw!r}')
   counts[prop]=int(raw)
  total=(row.get('total_errors') or '').strip()
  if not total.isdecimal() or int(total)!=sum(counts.values()):raise SystemExit(f'inconsistent total_errors at oracle line {line}')
  if (row.get('review_status') or '').strip()!='VERIFIED':raise SystemExit(f'non-evaluable oracle target {target}: review_status={row.get("review_status")!r}')
  clean=(row.get('is_clean') or '').strip().lower()
  if clean not in {'true','false'} or (clean=='true')!=(sum(counts.values())==0):raise SystemExit(f'inconsistent is_clean for {target}')
  for prop,count in counts.items():truth[(target,prop)]=count>0
 if len(truth)!=700:raise SystemExit(f'oracle expansion must yield 700 cells, observed {len(truth)}')
 return truth

def read_primary(path:Path)->list[dict[str,str]]:
 with path.open(newline='',encoding='utf-8-sig') as f:rows=list(csv.DictReader(f,delimiter='\t'))
 if len(rows)!=700:raise SystemExit(f'PRIMARY_RESULTS.tsv must contain 700 rows, observed {len(rows)}')
 seen=set()
 for line,row in enumerate(rows,2):
  target=(row.get('target_id') or '').strip();prop=(row.get('property') or '').strip();query=(row.get('query_id') or '').strip()
  key=(target,prop)
  if not target or prop not in PROPERTIES or query!=QUERY_FOR[prop]:raise SystemExit(f'invalid primary row at line {line}: {key!r}, query={query!r}')
  if (row.get('primary') or '').lower()!='true':raise SystemExit(f'non-primary row at line {line}')
  if key in seen:raise SystemExit(f'duplicate primary key: {key!r}')
  seen.add(key)
 return rows

def bool_field(value:str,key:tuple[str,str])->bool:
 value=value.strip().lower()
 if value not in {'true','false'}:raise SystemExit(f'invalid operational_detection for {key}: {value!r}')
 return value=='true'

def fmt(value:object)->str:
 return 'NA' if value is None else (f'{100*value:.2f}%' if isinstance(value,float) else str(value))

def write_csv(path:Path,rows:list[dict[str,object]],fields:list[str]):
 with path.open('w',newline='',encoding='utf-8') as f:
  w=csv.DictWriter(f,fieldnames=fields,lineterminator='\n');w.writeheader();w.writerows(rows)

def tex_escape(s:str)->str:return s.replace('\\','\\textbackslash{}').replace('_','\\_').replace('%','\\%')

def write_tex(path:Path,rows:list[dict[str,object]],fields:list[str],caption:str,label:str):
 align='l'+'r'*(len(fields)-1)
 lines=['\\begin{table}[t]','\\centering',f'\\caption{{{caption}}}',f'\\label{{{label}}}',f'\\begin{{tabular}}{{{align}}}','\\hline',
        ' & '.join(tex_escape(x) for x in fields)+' \\\\','\\hline']
 for row in rows:lines.append(' & '.join(tex_escape(fmt(row[x])) for x in fields)+' \\\\')
 lines+=['\\hline','\\end{tabular}','\\end{table}']
 path.write_text('\n'.join(lines)+'\n',encoding='utf-8')

def main()->int:
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--campaign',type=Path,required=True);ap.add_argument('--oracle',type=Path,required=True);ap.add_argument('--capabilities',type=Path,required=True);ap.add_argument('--fixture-source',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);a=ap.parse_args()
 if a.out.exists():ap.error('--out already exists; choose a fresh directory')
 primary=a.campaign/'PRIMARY_RESULTS.tsv';provenance=a.campaign/'PROVENANCE.json';summary=a.campaign/'EXECUTION_SUMMARY.json'
 for p in (primary,provenance,summary,a.oracle,a.capabilities,a.fixture_source):
  if not p.is_file():raise SystemExit(f'missing input: {p}')
 prov=json.loads(provenance.read_text());run_summary=json.loads(summary.read_text());truth=load_oracle(a.oracle);rows=read_primary(primary)
 if prov.get('policy')!='evidence-qualified':raise SystemExit('campaign policy is not evidence-qualified')
 if prov.get('oracle_sha256')!=sha(a.oracle):raise SystemExit('campaign oracle hash differs from supplied oracle')
 if prov.get('a3_target_capabilities_sha256')!=sha(a.capabilities):raise SystemExit('campaign capability-manifest hash differs')
 variant=prov.get('source_level_variants',{}).get('a-code_full_rust/cstringcargo_enum_df_only_rust',{})
 if variant.get('changed_file_sha256')!=sha(a.fixture_source):raise SystemExit('fixture source hash differs from campaign provenance')
 if run_summary.get('primary_rows')!=700:raise SystemExit('campaign does not contain all 700 expected primary rows')
 if set(truth)!={(r['target_id'],r['property']) for r in rows}:raise SystemExit('campaign target/property inventory differs from oracle')
 by_prop={p:Counter() for p in PROPERTIES};excluded={p:0 for p in PROPERTIES};classified=[]
 for row in rows:
  key=(row['target_id'],row['property']);status=row.get('execution_status','')
  if status!='OK':excluded[row['property']]+=1;classification='EXCLUDED'
  else:
   det=bool_field(row.get('operational_detection',''),key);present=truth[key]
   classification=('TP' if det else 'FN') if present else ('FP' if det else 'TN');by_prop[row['property']][classification]+=1
  classified.append({**row,'ground_truth':'PRESENT' if truth[key] else 'ABSENT','oracle_positive':str(truth[key]).lower(),'classification':classification})
 if sum(sum(c.values()) for c in by_prop.values())+sum(excluded.values())!=700:raise SystemExit('evaluable + excluded does not equal 700')
 a.out.mkdir(parents=True)
 class_fields=list(classified[0]);
 with (a.out/'RQ1_CLASSIFICATION.tsv').open('w',newline='',encoding='utf-8') as f:
  w=csv.DictWriter(f,delimiter='\t',fieldnames=class_fields,lineterminator='\n');w.writeheader();w.writerows(classified)
 fields=['query','expected_observations','evaluable_observations','excluded_observations','TP','FP','TN','FN','precision','recall','f1_score','accuracy','specificity']
 per=[];total=Counter()
 for prop in PROPERTIES:
  m=metrics(by_prop[prop]);total.update(by_prop[prop]);per.append({'query':QUERY_FOR[prop],'expected_observations':175,'evaluable_observations':m.pop('evaluable'),'excluded_observations':excluded[prop],**m})
 micro=metrics(total);global_rows=[{'query':'MICRO_TOTAL','expected_observations':700,'evaluable_observations':micro.pop('evaluable'),'excluded_observations':sum(excluded.values()),**micro}]
 write_csv(a.out/'RQ1_PER_QUERY.csv',per,fields);write_tex(a.out/'RQ1_PER_QUERY.tex',per,fields,'RQ1 operational results by primary query (evidence-qualified policy).','tab:rq1-per-query')
 write_csv(a.out/'RQ1_GLOBAL_MICRO.csv',global_rows,fields);write_tex(a.out/'RQ1_GLOBAL_MICRO.tex',global_rows,fields,'RQ1 micro-aggregate over the four primary queries.','tab:rq1-global-micro')
 report_prov={'schema':'rq1_table_report_provenance_v1','campaign':str(a.campaign.resolve()),'inputs_sha256':{'PRIMARY_RESULTS.tsv':sha(primary),'PROVENANCE.json':sha(provenance),'EXECUTION_SUMMARY.json':sha(summary),'oracle':sha(a.oracle),'capability_manifest':sha(a.capabilities),'normalized_fixture_source':sha(a.fixture_source)},'campaign_query_sha256':prov.get('queries',{}),'campaign_binary_sha256':{k:v.get('sha256') for k,v in prov.get('bootstrap',{}).get('binaries',{}).items()},'policy':'evidence-qualified','df_primary':'DF_NORMAL','df_full_included':False,'expected_cells':700,'evaluable_cells':sum(total.values()),'excluded_cells':sum(excluded.values()),'source_level_variant':variant}
 (a.out/'RQ1_REPORT_PROVENANCE.json').write_text(json.dumps(report_prov,indent=2,sort_keys=True)+'\n')
 return 0
if __name__=='__main__':raise SystemExit(main())
