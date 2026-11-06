#!/usr/bin/env python3
"""Replay the frozen nine-project, five-query RQ2 suite with resume support."""
import argparse,concurrent.futures,hashlib,json,os,shlex,subprocess
from pathlib import Path
BASE=Path('/mnt/wsx-storage/af/cqpl-nine-fresh/20261015T115923Z-1240770')
QUERIES={'ALLOC_MISMATCH':'allocator_mismatch_full.cqpl','DF_FULL':'double_free_full.cqpl','DF_NORMAL':'../queries_experimental/double_free_alloc_state_no_unwind_path.cqpl','LEAK':'leak_normal_completion.cqpl','UAF':'use_after_free_full.cqpl'}
def sha(p):
 h=hashlib.sha256();h.update(p.read_bytes());return h.hexdigest()
def replace(v,f,x):v[v.index(f)+1]=str(x)
ap=argparse.ArgumentParser();ap.add_argument('--worktree',type=Path,required=True);ap.add_argument('--bootstrap',type=Path,required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--jobs',type=int,default=4);ap.add_argument('--resume',action='store_true');a=ap.parse_args()
w=a.worktree.resolve();out=a.out.resolve();prov=json.loads((a.bootstrap/'PROVENANCE.json').read_text());crema=Path(prov['binaries']['crema']['path']);cqpl=Path(prov['binaries']['cqpl_checker']['path'])
if prov['worktree']!=str(w) or sha(crema)!=prov['binaries']['crema']['sha256'] or sha(cqpl)!=prov['binaries']['cqpl_checker']['sha256']:ap.error('bootstrap provenance mismatch')
if any(sha(w/relative)!=digest for relative,digest in prov['sources'].items()):ap.error('worktree source drift after bootstrap')
if out.exists() and not a.resume:ap.error('--out exists; use --resume');out.mkdir(parents=True,exist_ok=True)
out.mkdir(parents=True,exist_ok=True);env=os.environ.copy();env.update({'RUSTUP_TOOLCHAIN':'nightly-2024-11-21','CARGO_INCREMENTAL':'0','TMPDIR':str(a.bootstrap/'tmp')});sysroot=subprocess.check_output(['rustup','run','nightly-2024-11-21','rustc','--print','sysroot'],text=True).strip();env['LD_LIBRARY_PATH']=str(Path(sysroot)/'lib')+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
(out/'PROVENANCE.json').write_text(json.dumps({'bootstrap':prov,'baseline':str(BASE),'queries':{k:sha((w/'cqpl/queries_v3'/v).resolve()) for k,v in QUERIES.items()}},indent=2)+'\n')
for bp in sorted((BASE/'projects').iterdir()):
 p=bp.name;o=out/'projects'/p;o.mkdir(parents=True,exist_ok=True)
 if a.resume and (o/'annotated_icfg.json').is_file():continue
 cmd=shlex.split((bp/'crema-command.sh').read_text().strip());cmd[0]=str(crema);cmd[1]=str(BASE/'source-copies'/p);replace(cmd,'--annotated-icfg-out',o/'annotated_icfg.json');replace(cmd,'--allocation-identity-out',o/'allocation_identity.json');replace(cmd,'--analysis-out-dir',o/'analysis');replace(cmd,'--semantic-coverage-out',o/'semantic_coverage.json');replace(cmd,'--cargo-plan-out',o/'cargo_plan.json')
 e=env.copy();e['CARGO_TARGET_DIR']=str(out/'build-targets'/p)
 with (o/'crema.log').open('w') as log:
  rc=subprocess.run(cmd,cwd=w/'crema',env=e,stdout=log,stderr=subprocess.STDOUT).returncode
 if rc:raise SystemExit(f'CREMA export failed: {p}: {rc}')
def job(item):
 bp,qid,rel=item;p=bp.name;o=out/'projects'/p/'queries';o.mkdir(exist_ok=True);r=o/f'{qid}.json';x=o/f'{qid}.explain.json'
 if a.resume and r.is_file() and x.is_file():
  try:json.loads(r.read_text());json.loads(x.read_text());return p,qid,0
  except json.JSONDecodeError:pass
 q=(w/'cqpl/queries_v3'/rel).resolve();base=json.loads((bp/'queries'/f'{qid}.json').read_text());cp=subprocess.run([str(cqpl),str(out/'projects'/p/'annotated_icfg.json'),str(q),'--entry',base['entry'],'--json','--explain-json',str(x),'--explain-max-witnesses','128'],cwd=w,env=env,text=True,capture_output=True);r.write_text(cp.stdout);(o/f'{qid}.stderr').write_text(cp.stderr);return p,qid,cp.returncode
jobs=[(p,q,r) for p in sorted((BASE/'projects').iterdir()) for q,r in QUERIES.items()]
with concurrent.futures.ThreadPoolExecutor(max_workers=a.jobs) as pool:
 failures=[x for x in pool.map(job,jobs) if x[2]]
if failures:raise SystemExit(f'RQ2 failures: {failures}')
