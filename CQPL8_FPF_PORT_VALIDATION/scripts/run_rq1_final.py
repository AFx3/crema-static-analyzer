#!/usr/bin/env python3
"""Run the five-query RQ1 protocol with atomic per-target checkpoints.

Primary properties are ML, DF_NORMAL, UAF and ALLOC_MISMATCH. DF_FULL is
retained as a diagnostic query. This runner records logical results unchanged;
its operational_detection column applies the evidence-qualified policy only.
"""
import argparse,csv,hashlib,json,os,shutil,subprocess,sys,tempfile,time
from pathlib import Path

GROUPS={'a-code_full_rust/':61,'a-code_c_ffi/':15,'a-code_c_to_rust_alloc/':16,'a-code_c_ffi_bodyless_gate/':83}
QUERIES={
 'ML':('cqpl/queries_v3/leak_normal_completion.cqpl',True),
 'DF_FULL':('cqpl/queries_v3/double_free_full.cqpl',False),
 'DF_NORMAL':('cqpl/queries_experimental/double_free_alloc_state_no_unwind_path.cqpl',True),
 'UAF':('cqpl/queries_v3/use_after_free_full.cqpl',True),
 'ALLOC_MISMATCH':('cqpl/queries_v3/allocator_mismatch_full.cqpl',True),
}
FIELDS=['target_id','query_id','property','primary','execution_status','result','subresult','direction','strength','finding_count','positive_finding_count','operational_detection','detection_reason','artifact_sha256','query_sha256','crema_binary_sha256','cqpl_binary_sha256','result_json','explanation_json','detail']

def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def source_tree_sha(root):
 h=hashlib.sha256()
 for p in sorted(x for x in root.rglob('*') if x.is_file()
                 and 'target' not in x.relative_to(root).parts
                 and '.git' not in x.relative_to(root).parts):
  rel=p.relative_to(root).as_posix().encode()
  h.update(len(rel).to_bytes(8,'big'));h.update(rel)
  with p.open('rb') as f:
   for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def atomic_json(path,obj):
 tmp=path.with_suffix(path.suffix+'.tmp');tmp.write_text(json.dumps(obj,indent=2,sort_keys=True)+'\n');os.replace(tmp,path)
def atomic_tsv(path,rows):
 tmp=path.with_suffix(path.suffix+'.tmp')
 with tmp.open('w',newline='') as f:
  w=csv.DictWriter(f,delimiter='\t',fieldnames=FIELDS,extrasaction='ignore');w.writeheader();w.writerows(rows)
 os.replace(tmp,path)
def detection(result,sub,count):
 if result=='tt':return True,'tt'
 if result=='ff':return False,'ff'
 if result!='unk':return None,'unrecognized_logical_result'
 if sub in ('unk_true','unk_mixed'):return True,sub
 if sub=='unk_unoriented':return (count>0),('unk_unoriented_with_finding' if count else 'unk_unoriented_without_finding')
 if sub=='unk_false':return False,'unk_false'
 return (count>0),('unrecognized_unknown_with_finding' if count else 'unrecognized_unknown_without_finding')
def load_rows(checkpoints):
 rows=[]
 for p in sorted(checkpoints.glob('*/ROWS.json')):rows.extend(json.loads(p.read_text()))
 return sorted(rows,key=lambda r:(r['target_id'],r['query_id']))
def target_capabilities(path):
 capabilities={}
 allowed={'a3_bodyless_enum_constructor_callback_v1'}
 with path.open(newline='') as stream:
  for row in csv.DictReader(stream,delimiter='\t'):
   target=row.get('target_id','');capability=row.get('capability','')
   if not target or capability not in allowed:raise SystemExit('invalid A3 target capability row')
   capabilities.setdefault(target,set()).add(capability)
 return capabilities

ap=argparse.ArgumentParser()
ap.add_argument('--worktree',type=Path,required=True);ap.add_argument('--bootstrap',type=Path,required=True)
ap.add_argument('--out',type=Path,required=True);ap.add_argument('--oracle',type=Path,required=True)
ap.add_argument('--resume',action='store_true');ap.add_argument('--only');ap.add_argument('--timeout',type=int,default=1800)
a=ap.parse_args();w=a.worktree.resolve();boot=a.bootstrap.resolve();out=a.out.resolve()
if out.exists() and not a.resume:ap.error('--out exists; use --resume or a fresh directory')
out.mkdir(parents=True,exist_ok=True);checkpoints=out/'checkpoints';checkpoints.mkdir(exist_ok=True)
prov=json.loads((boot/'PROVENANCE.json').read_text());crema=Path(prov['binaries']['crema']['path']);checker=Path(prov['binaries']['cqpl_checker']['path'])
if prov['worktree']!=str(w) or sha(crema)!=prov['binaries']['crema']['sha256'] or sha(checker)!=prov['binaries']['cqpl_checker']['sha256']:ap.error('bootstrap provenance mismatch')
if any(sha(w/relative)!=digest for relative,digest in prov['sources'].items()):ap.error('worktree source drift after bootstrap')
if sha(a.oracle)!=sha(a.oracle.resolve()):ap.error('oracle path is unstable')
manifest=w/'cqpl/evaluation_manifests/full_memory_safety_v1/dataset_targets_196.txt'
capability_manifest=w/'CQPL8_FPF_PORT_VALIDATION/A3_TARGET_CAPABILITIES.tsv';capabilities=target_capabilities(capability_manifest)
targets=[x.strip() for x in manifest.read_text().splitlines() if x.strip() and not x.lstrip().startswith('#') and any(x.startswith(g) for g in GROUPS)]
if len(targets)!=175 or len(set(targets))!=175 or {g:sum(t.startswith(g) for t in targets) for g in GROUPS}!=GROUPS:ap.error('invalid 175-target inventory')
if not set(capabilities).issubset(targets):ap.error('A3 capability manifest contains a target outside the 175-target inventory')
if a.only:
 if a.only not in targets:ap.error('--only is not in the inventory')
 targets=[a.only]
query_hashes={q:sha(w/rel) for q,(rel,_) in QUERIES.items()}
target_source_hashes={target:source_tree_sha(w/'tests_and_target_repos'/target) for target in targets}
cstring_source=w/'tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs'
atomic_json(out/'PROVENANCE.json',{'bootstrap':prov,'oracle':str(a.oracle.resolve()),'oracle_sha256':sha(a.oracle),'manifest_sha256':sha(manifest),'a3_target_capabilities_sha256':sha(capability_manifest),'queries':query_hashes,'policy':'evidence-qualified','targets':targets,'target_source_tree_sha256':target_source_hashes,'source_level_variants':{'a-code_full_rust/cstringcargo_enum_df_only_rust':{'status':'normalized_source_variant','changed_file':'src/main.rs','changed_file_sha256':sha(cstring_source),'rationale':'replace reachable log::error dynamic-dispatch control dependency in CString::new Err arm with abort; double from_raw sequence unchanged'}}})
env=os.environ.copy();env.update({'RUSTUP_TOOLCHAIN':'nightly-2024-11-21','CARGO_INCREMENTAL':'0','TMPDIR':str(boot/'tmp')})
sysroot=subprocess.check_output(['rustup','run','nightly-2024-11-21','rustc','--print','sysroot'],text=True).strip();env['LD_LIBRARY_PATH']=str(Path(sysroot)/'lib')+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
for n,target in enumerate(targets,1):
 slug=target.replace('/','__');final=checkpoints/slug
 if a.resume and (final/'COMPLETE.json').is_file():continue
 scratch=out/'scratch';scratch.mkdir(exist_ok=True)
 temp=Path(tempfile.mkdtemp(prefix=slug+'.',dir=scratch))
 run=temp/'producer';cmd=[sys.executable,str(w/'CQPL8_FPF_PORT_VALIDATION/scripts/export_one_target.py'),'--root',str(w),'--relative-path',target,'--out',str(run),'--toolchain','nightly-2024-11-21']
 if 'a3_bodyless_enum_constructor_callback_v1' in capabilities.get(target,set()):cmd.append('--a3-bodyless-enum-constructor-callback-v1')
 log=(temp/'producer.log').open('w')
 try:rc=subprocess.run(cmd,cwd=w,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=a.timeout).returncode
 except subprocess.TimeoutExpired:rc=124;log.write('\nTIMEOUT\n')
 finally:log.close()
 artifact=run/'annotated_icfg_v2.json';rows=[]
 for qid,(rel,primary) in QUERIES.items():
  q=w/rel;rjson=temp/f'{qid}.json';ejson=temp/f'{qid}.explain.json';detail=''
  if rc or not artifact.is_file():status='EXECUTION_FAILURE';doc={};ex={};detail=f'producer_rc={rc}'
  else:
   try:
    cp=subprocess.run([str(checker),str(artifact),str(q),'--json','--explain-json',str(ejson),'--explain-max-witnesses','128'],cwd=w,env=env,text=True,capture_output=True,timeout=a.timeout)
    rjson.write_text(cp.stdout);(temp/f'{qid}.stderr').write_text(cp.stderr)
    doc=json.loads(cp.stdout);ex=json.loads(ejson.read_text());status='OK' if cp.returncode==0 else 'QUERY_FAILURE'
   except subprocess.TimeoutExpired:
    doc={};ex={};status='QUERY_FAILURE';detail='query timeout'
   except Exception as exc:doc={};ex={};status='QUERY_FAILURE';detail=str(exc)
  ass=doc.get('assessment',{}) if isinstance(doc.get('assessment'),dict) else {};count=len(ex.get('supporting_findings',[]));det,reason=detection(doc.get('result',''),ass.get('subresult',''),count) if status=='OK' else (None,'execution_failure')
  rows.append({'target_id':target,'query_id':qid,'property':'DF' if qid.startswith('DF') else qid,'primary':str(primary).lower(),'execution_status':status,'result':doc.get('result',''),'subresult':ass.get('subresult',''),'direction':ass.get('direction',''),'strength':ass.get('strength',''),'finding_count':count if status=='OK' else '','positive_finding_count':count if status=='OK' else '','operational_detection':'' if det is None else str(det).lower(),'detection_reason':reason,'artifact_sha256':sha(artifact) if artifact.is_file() else '','query_sha256':query_hashes[qid],'crema_binary_sha256':prov['binaries']['crema']['sha256'],'cqpl_binary_sha256':prov['binaries']['cqpl_checker']['sha256'],'result_json':str((final/f'{qid}.json').resolve()),'explanation_json':str((final/f'{qid}.explain.json').resolve()),'detail':detail})
 atomic_json(temp/'ROWS.json',rows);atomic_json(temp/'COMPLETE.json',{'target_id':target,'producer_rc':rc,'completed_at':time.time()})
 if final.exists():shutil.rmtree(final)
 os.replace(temp,final);all_rows=load_rows(checkpoints);atomic_tsv(out/'ALL_FIVE_QUERY_RESULTS.tsv',all_rows);atomic_tsv(out/'PRIMARY_RESULTS.tsv',[r for r in all_rows if r['primary']=='true'])
 print(f'[{n}/{len(targets)}] {target}',flush=True)

all_rows=load_rows(checkpoints)
primary=[r for r in all_rows if r['primary']=='true']
summary={
 'target_count':len(targets),
 'five_query_rows':len(all_rows),
 'primary_rows':len(primary),
 'successful_five_query_rows':sum(r['execution_status']=='OK' for r in all_rows),
 'successful_primary_rows':sum(r['execution_status']=='OK' for r in primary),
 'df_primary_query_ids':sorted({r['query_id'] for r in primary if r['property']=='DF'}),
 'status':'PASS' if len(all_rows)==len(targets)*5 and len(primary)==len(targets)*4
                  and all(r['execution_status']=='OK' for r in all_rows)
                  and {r['query_id'] for r in primary if r['property']=='DF'}=={'DF_NORMAL'}
          else 'INCOMPLETE',
}
atomic_json(out/'EXECUTION_SUMMARY.json',summary)
if summary['status']!='PASS':raise SystemExit('RQ1 execution incomplete; see EXECUTION_SUMMARY.json')
