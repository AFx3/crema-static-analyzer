#!/usr/bin/env python3
import argparse, collections, json
from pathlib import Path
ap=argparse.ArgumentParser()
ap.add_argument('root',type=Path)
ap.add_argument('out',type=Path)
args=ap.parse_args()
files=sorted(args.root.glob('*/semantic_coverage.json'))
if not files: raise SystemExit('no semantic_coverage.json inputs')
stmt=collections.Counter(); rv=collections.Counter(); ext=collections.Counter(); gaps=collections.Counter(); calls=collections.Counter(); totals=collections.Counter(); crates=[]
complete=0; incomplete=0; versions=set(); profiles=set()
for f in files:
    d=json.loads(f.read_text()); version=d.get('schema_version')
    assert version in (2,3), (f,version); versions.add(version)
    profiles.add(d.get('transfer_profile','legacy_v6O'))
    manifest_path=f.parent/'analysis_manifest.json'
    manifest=json.loads(manifest_path.read_text()) if manifest_path.is_file() else {}
    status=manifest.get('analysis_status','unknown')
    cqpl_ok=manifest.get('cqpl_export_incomplete_roots',0)==0 and status=='complete'
    complete += int(cqpl_ok); incomplete += int(not cqpl_ok)
    crates.append({'package':d['package_name'],'version':d['package_version'],'file':str(f),'analysis_status':status,'cqpl_complete':cqpl_ok,'schema_version':version,'transfer_profile':d.get('transfer_profile','legacy_v6O')})
    for r in d['roots']:
        stmt.update(r['unmodeled_statement_kinds']); rv.update(r['unmodeled_rvalue_families']); ext.update(r['external_opaque_callees'])
        gaps.update(r.get('unresolved_control_flow_summaries',{})); calls.update(r['calls'])
        for k in ('statements','rvalues','terminators'):
            for c,n in r[k].items(): totals[f'{k}.{c}']+=n
out={
 'schema_version':3 if 3 in versions else 2,
 'telemetry_only':True,
 'input_schema_versions':sorted(versions),
 'transfer_profiles':sorted(profiles),
 'crates':crates,
 'crate_count':len(crates),
 'cqpl_complete_crates':complete,
 'cqpl_incomplete_crates':incomplete,
 'aggregate_counts':dict(sorted(totals.items())),
 'aggregate_calls':dict(sorted(calls.items())),
 'ranked_unresolved_control_flow_summaries':gaps.most_common(),
 'ranked_unmodeled_statement_kinds':stmt.most_common(),
 'ranked_unmodeled_rvalue_families':rv.most_common(),
 'ranked_external_opaque_callees':ext.most_common(),
 'selection_rule':'semantic extensions must be justified by observed rank/frequency and a documented Rust contract; conservative TOP is preferred to fabricated precision',
}
args.out.write_text(json.dumps(out,indent=2)+'\n')
print(f"V6P_COVERAGE_AGGREGATE_V{out['schema_version']}: PASS crates={len(crates)} cqpl_complete={complete} cqpl_incomplete={incomplete}")
