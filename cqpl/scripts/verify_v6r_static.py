#!/usr/bin/env python3
from __future__ import annotations
import hashlib, json, sys
from pathlib import Path

root=Path(sys.argv[1] if len(sys.argv)>1 else Path(__file__).resolve().parents[1]).resolve()
workspace=root.parent
# Standalone candidate packages carry CANDIDATE_SHA256SUMS next to cqpl/.
# An installed repository tree does not. Package-hygiene checks must never
# recurse through the whole repository, because local repro-results and Cargo
# target directories are legitimate untracked experimental state.
standalone_package=(workspace/'CANDIDATE_SHA256SUMS').is_file()
errors=[]

def check(cond,msg):
    if not cond: errors.append(msg)

def sha(p:Path): return hashlib.sha256(p.read_bytes()).hexdigest()

def candidate_files():
    out={}
    for p in root.rglob('*'):
        if not p.is_file(): continue
        if '__pycache__' in p.parts or p.suffix=='.pyc' or 'target' in p.parts: continue
        out[p.relative_to(root).as_posix()]=sha(p)
    return out

baseline=json.loads((root/'artifact/V6R_BASELINE_SHA256.json').read_text())
check(baseline.get('baseline_commit')=='bbca5f09096d77718624564e0aafff9d87a96e6e','baseline commit mismatch')

# Exact frozen CQPL semantic boundary inherited from v6Q-r1c.
for rel,expected in baseline.get('immutable_files',{}).items():
    p=workspace/rel
    check(p.is_file(),f'missing immutable baseline file {rel}')
    if p.is_file(): check(sha(p)==expected,f'observational boundary changed {rel}')

# CREMA producer/transfer sources remain byte-identical.
for rel,expected in baseline.get('immutable_crema_files',{}).items():
    p=workspace/rel
    check(p.is_file(),f'missing immutable CREMA file {rel}')
    if p.is_file(): check(sha(p)==expected,f'CREMA observational boundary changed {rel}')

# model_checker.rs may differ only by the two visibility promotions needed by explain.rs.
mc=root/'cqpl_checker/src/model_checker.rs'
text=mc.read_text()
canon=text.replace("    pub(crate) k: &'a Kripke,","    k: &'a Kripke,")
canon=canon.replace('    pub(crate) fn eval_all(&self, formula: &StateFormula, env: &Env) -> Result<Valuation, String> {','    fn eval_all(&self, formula: &StateFormula, env: &Env) -> Result<Valuation, String> {')
check(hashlib.sha256(canon.encode()).hexdigest()==baseline['model_checker_baseline_sha256'],
      'model_checker semantic body differs from v6Q-r1c beyond visibility changes')

# Stable taxonomy and invariant-bearing runtime surface.
explain=(root/'cqpl_checker/src/explain.rs').read_text()
required_codes=[
'MAY_ALLOCATION','MAY_DEALLOCATION','MAY_USE','MAY_OWNERSHIP','ALIAS_JOIN',
'ABSTRACT_COMPONENT_MERGE','ABSTRACT_TOP_STATE','UNRESOLVED_CONTRACT','EXTERNAL_EFFECT',
'UNRESOLVED_ESCAPE','GLOBAL_TOP_EFFECT','CONTROL_FLOW_UNRESOLVED','HIGHER_ORDER_UNRESOLVED',
'PATH_JOIN','QUERY_THREE_VALUED_PROPAGATION']
for code in required_codes: check(f'"{code}"' in explain,f'missing taxonomy code {code}')
for token in ['cqpl_explanation_v1','cqpl_uncertainty_reasons_v1','reserved_reason_codes_not_inferred',
              'producer_provenance_capability_present','complete_dependency_trace',
              'unk result has no atomically supported uncertainty origin','tt result has no atomic witness endpoint']:
    check(token in explain,f'explain.rs missing invariant token {token}')

main=(root/'cqpl_checker/src/main.rs').read_text()
for token in ['--explain-json','--explain-max-witnesses','semantic result {} != explanation result {}']:
    check(token in main,f'main.rs missing explainability gate {token}')
lib=(root/'cqpl_checker/src/lib.rs').read_text()
check('pub mod explain;' in lib,'lib.rs does not export explain module')

# Runtime-validated source boundary: documentation freeze must not alter the implementation that passed 1344/1344.
rvsrc=json.loads((root/'artifact/V6R_RUNTIME_VALIDATED_SOURCE_SHA256.json').read_text())
check(rvsrc.get('runtime_evidence_sha256')=='f41117d1d2fc1838cc1ee830071b07673811765884c9d96e4229ebbd187247d4',
      'runtime evidence SHA mismatch')
for rel,expected in rvsrc.get('files',{}).items():
    p=root/rel
    check(p.is_file(),f'missing runtime-validated source file {rel}')
    if p.is_file(): check(sha(p)==expected,f'runtime-validated source changed after validation: {rel}')

version=(root/'VERSION').read_text()
for token in ['CREMA-CQPL-v6R-r1','semantic_baseline=CREMA-CQPL-v6Q-r1c',
              'status=explainability-observational-runtime-validated-freeze-candidate',
              'runtime_validation=112-subjects-12-queries-1344-attempts-zero-result-mismatch',
              'runtime_result_counts=ff650-unk468-tt226']:
    check(token in version,f'VERSION missing {token}')

# Documentation contract and tutorial presence.
doc=(root/'EXPLAINABILITY.md').read_text()
guide=(root/'EXPLAINABILITY_GUIDE.md').read_text()
readme=(root/'README.md').read_text()
language=(root/'LANGUAGE.md').read_text()
analysis=(root/'ANALYSIS_GUIDE.md').read_text()
check('truth = "unknown"' not in doc,'EXPLAINABILITY.md uses stale truth token "unknown" instead of "unk"')
for token in ['reason_frontier','atomic_observations','complete_dependency_trace','MAY_ALLOCATION']:
    check(token in doc,f'EXPLAINABILITY.md missing {token}')
for token in [
    'a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_bool',
    'run_one_target_v6q_r1c.py','leak_alloc.explain.json','atomic_observations',
    'std::boxed::Box::<bool>::new','MAY_ALLOCATION']:
    check(token in guide,f'EXPLAINABILITY_GUIDE.md missing tutorial token {token}')
for name,textdoc in [('README.md',readme),('LANGUAGE.md',language),('ANALYSIS_GUIDE.md',analysis)]:
    check('EXPLAINABILITY_GUIDE.md' in textdoc,f'{name} does not link the explainability tutorial')

# Frozen leak/precision baseline remains unchanged.
pre=json.loads((root/'artifact/V6R_LEAK_PREANALYSIS.json').read_text())
for q in ['leak_alloc','leak_alloc_state']:
    check(pre['queries'][q]['unknown']==105,f'{q} frozen unknown count !=105')
    check(pre['queries'][q]['observed_graph_feature_presence']['has_may_alloc']==105,f'{q} MAY alloc presence !=105')

metrics=json.loads((root/'artifact/V6R_BASELINE_PRECISION_METRICS.json').read_text())
expected={'ML':33,'DF':29,'UAF':22,'UB_FFI':18}
for family,count in expected.items():
    m=metrics['families'][family]
    check(m['reviewed_positive']==count,f'{family} reviewed positive count mismatch')
    check(m['unexpected_ff_on_reviewed_positive']==0,f'{family} has unexpected ff baseline')
    check(m['vulnerable_non_refutation_rate']==1.0,f'{family} non-refutation rate !=1')

# Runtime validation summary is machine-readable and agrees with the accepted run.
rv=json.loads((root/'artifact/V6R_RUNTIME_VALIDATION.json').read_text())
check(rv.get('subjects')==112,'runtime subjects !=112')
check(rv.get('queries')==12,'runtime queries !=12')
check(rv.get('attempts')==1344,'runtime attempts !=1344')
check(rv.get('baseline_result_mismatches')==0,'runtime result mismatches !=0')
check(rv.get('failures')==0,'runtime failures !=0')
check(rv.get('result_counts')=={'ff':650,'unk':468,'tt':226},'runtime truth counts mismatch')
for key in ['unknown_without_reason_frontier','unknown_without_specific_origin','true_without_witness','true_without_atomic_witness']:
    check(rv['explainability_gates'].get(key)==0,f'runtime explainability gate {key} !=0')
for q in ['leak_alloc','leak_alloc_state']:
    check(rv['leak_frontier'][q]['unknown']==105,f'{q} runtime unknown !=105')
    check(rv['leak_frontier'][q]['may_allocation_frontier']==105,f'{q} runtime MAY_ALLOCATION !=105')
check(rv['runtime_evidence_archive']['sha256']=='f41117d1d2fc1838cc1ee830071b07673811765884c9d96e4229ebbd187247d4',
      'runtime evidence archive SHA mismatch')

for rel in ['scripts/run_explainability_matrix.py','scripts/analyze_leak_unknowns.py','scripts/compute_precision_metrics.py','scripts/verify_v6r_static.py']:
    src=(root/rel).read_text()
    try: compile(src,str(root/rel),'exec')
    except SyntaxError as e: errors.append(f'python syntax {rel}: {e}')

# Exact delta proof: recompute differences against the full v6Q-r1c baseline tree.
base_all=baseline.get('all_cqpl_files',{})
cand=candidate_files()
changed={rel for rel,h in cand.items() if base_all.get(rel)!=h}
deleted=set(base_all)-set(cand)
stage=[]
for raw in (root/'artifact/V6R_STAGE_PATHS.txt').read_text().splitlines():
    raw=raw.strip()
    if not raw or raw.startswith('#'): continue
    check(raw.startswith('cqpl/'),f'non-CQPL path in V6R_STAGE_PATHS: {raw}')
    stage.append(raw[len('cqpl/'):])
check(not deleted,f'candidate deletes baseline CQPL files: {sorted(deleted)}')
check(changed==set(stage),f'exact delta/stage mismatch changed_only={sorted(changed-set(stage))} staged_only={sorted(set(stage)-changed)}')
check(len(stage)==len(set(stage)),'duplicate V6R_STAGE_PATHS entries')
check(len(stage)==24,f'expected 24 final-freeze CQPL delta files, found {len(stage)}')

# Candidate manifest covers every shipped CQPL file except itself/cache/target.
manifest_path=root/'MANIFEST_SHA256.json'
try:
    manifest=json.loads(manifest_path.read_text())
    shipped={rel:h for rel,h in cand.items() if rel!='MANIFEST_SHA256.json'}
    check(set(manifest)==set(shipped),'candidate manifest file-set mismatch')
    for rel in set(manifest)&set(shipped):
        check(shipped[rel]==manifest[rel],f'candidate manifest hash mismatch {rel}')
except Exception as e:
    errors.append(f'manifest validation: {e}')

# Package hygiene is meaningful only for a standalone distributable package.
# In installed-tree mode, generated local build/repro directories outside the
# tracked CQPL delta are intentionally ignored; Git staging gates police what
# can enter the commit.
if standalone_package:
    for p in workspace.rglob('*'):
        if not p.is_file():
            continue
        rel=p.relative_to(workspace).as_posix()
        if '/target/' in f'/{rel}/' or '__pycache__' in p.parts or p.suffix=='.pyc':
            errors.append(f'generated build/cache file shipped: {rel}')

if errors:
    print('V6R_STATIC_VERIFY: FAIL')
    for e in errors: print(' -',e)
    raise SystemExit(1)
print('V6R_STATIC_VERIFY: PASS')
print('verification_mode='+('standalone-package' if standalone_package else 'installed-tree'))
print('baseline_commit=bbca5f09096d77718624564e0aafff9d87a96e6e')
print('semantic_delta=none; model_checker_visibility_only=true')
print('runtime_validated_source_boundary=byte-identical')
print('exact_cqpl_delta_files='+str(len(stage)))
print('frozen_queries=12 byte-identical')
print('crema_source_boundary=byte-identical')
print('runtime_validation=112x12=1344 mismatches=0 failures=0')
print('explainability_gates=all-zero')
print('leak_unknown=105/112 each; MAY_ALLOCATION_frontier=105/105')
print('reviewed_positive_nonrefutation=ML33/33 DF29/29 UAF22/22 UB_FFI18/18')
