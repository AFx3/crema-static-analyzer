#!/usr/bin/env python3
"""Export one RQ1 subject with the pinned full profile and FFI stale guard."""
import argparse,hashlib,json,os,re,shutil,subprocess
from pathlib import Path
EXPECTED_RUSTC='rustc 1.84.0-nightly (3fee0f12e 2024-11-20)'
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def config(path):
 out={}
 for raw in path.read_text().splitlines():
  if not raw or raw.lstrip().startswith('#'):continue
  parts=raw.split('\t',2)
  if len(parts)!=3:raise SystemExit(f'invalid target config: {raw!r}')
  out[parts[0]]='' if parts[1]=='-' else parts[1]
 return out
ap=argparse.ArgumentParser();ap.add_argument('--root',type=Path,required=True);ap.add_argument('--relative-path',required=True);ap.add_argument('--out',type=Path,required=True);ap.add_argument('--toolchain',default='nightly-2024-11-21');ap.add_argument('--a3-bodyless-enum-constructor-callback-v1',action='store_true');a=ap.parse_args()
root=a.root.resolve();target=(root/'tests_and_target_repos'/a.relative_path).resolve();out=a.out.resolve();crema=root/'crema';cqpl=root/'cqpl';binary=crema/'target/debug/crema';checker=cqpl/'cqpl_checker/target/debug/cqpl_checker'
if (root/'tests_and_target_repos').resolve() not in target.parents or not (target/'Cargo.toml').is_file():ap.error('invalid target')
if out.exists():shutil.rmtree(out)
out.mkdir(parents=True)
rustc=subprocess.check_output(['rustup','run',a.toolchain,'rustc','--version'],text=True).strip()
if rustc!=EXPECTED_RUSTC:raise SystemExit(f'unexpected rustc: {rustc}')
env=os.environ.copy();env['CARGO_INCREMENTAL']='0';sysroot=subprocess.check_output(['rustup','run',a.toolchain,'rustc','--print','sysroot'],text=True).strip();env['LD_LIBRARY_PATH']=str(Path(sysroot)/'lib')+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
tenv=env.copy();tenv['CARGO_TARGET_DIR']=str(target/'target');build=['cargo',f'+{a.toolchain}','build','--manifest-path',str(target/'Cargo.toml')]
if (target/'Cargo.lock').is_file():build.append('--locked')
with (out/'subject-build.log').open('w') as log:brc=subprocess.run(build,cwd=target,env=tenv,stdout=log,stderr=subprocess.STDOUT).returncode
if brc:raise SystemExit('subject build failed')
target_cfg=config(cqpl/'artifact/TARGET_ANALYSIS_CONFIG.tsv');entries=json.loads((cqpl/'regression/reference/entry_overrides.json').read_text());cargo_target=target_cfg.get(a.relative_path,'');entry=entries.get(a.relative_path)
svf=root/'SVF-example/output';svf.mkdir(parents=True,exist_ok=True);sentinel=svf/f'ZZ_CQPL_FULL_STALE_{os.getpid()}_A_FINAL_ICFG.json';sentinel.write_text('{invalid-stale-global-svf\n');(crema/'ffi_functions.json').write_text('{"ffi_functions":["__CQPL_FULL_STALE__"]}\n')
annotated=out/'annotated_icfg_v2.json';identity=out/'allocation_identity.json';cmd=[str(binary),str(target),'--analysis-mode','application']
if entry:cmd+=['--entry',entry]
if cargo_target:cmd+=['--cargo-target',cargo_target]
cmd+=['--dependency-body-ingestion-v1','--dependency-drop-glue-ingestion-v1','--dependency-resource-identity-continuity-v1','--dependency-resource-return-continuity-v1','--dependency-place-resource-relations-v1','--dependency-drop-receiver-resource-continuity-v1']
if a.a3_bodyless_enum_constructor_callback_v1:cmd+=['--a3-bodyless-enum-constructor-callback-v1']
cmd+=['--cqpl-schema-version','2','--mir-semantics-v2','--panic-unwind-lifecycle-v1','--only-icfg-annotated','--annotated-icfg-out',str(annotated),'--allocation-identity-out',str(identity),'--analysis-out-dir',str(out/'analysis'),'--semantic-coverage-out',str(out/'semantic_coverage.json'),'--cargo-plan-out',str(out/'cargo_plan.json')]
(out/'crema-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (out/'crema-export.log').open('w') as log:rc=subprocess.run(cmd,cwd=crema,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
sentinel.unlink(missing_ok=True)
if rc or not annotated.is_file() or not identity.is_file():raise SystemExit(f'CREMA export failed: {rc}')
log=(out/'crema-export.log').read_text(errors='replace')
if sentinel.name in log:raise SystemExit('stale SVF sentinel was consumed')
ffi=crema/'ffi_functions.json';shutil.copy2(ffi,out/'ffi_functions.json');fd=json.loads(ffi.read_text())
if '__CQPL_FULL_STALE__' in fd.get('ffi_functions',[]):raise SystemExit('stale FFI sentinel survived extraction')
graph=json.loads(annotated.read_text());profile=json.loads((cqpl/'analysis_profiles/full_memory_safety_v1.json').read_text());missing=sorted(set(profile['required_capabilities'])-set(graph.get('capabilities',[])))
if missing:raise SystemExit(f'missing capabilities: {missing}')
report={'schema':'cqpl8_fpf_port_export_v1','relative_path':a.relative_path,'rustc':rustc,'crema_binary_sha256':sha(binary),'checker_binary_sha256':sha(checker),'artifact_sha256':sha(annotated),'status':'PASS'};(out/'REPORT.json').write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
