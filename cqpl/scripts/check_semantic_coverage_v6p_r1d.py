#!/usr/bin/env python3
import json, sys
from pathlib import Path
p=Path(sys.argv[1]); d=json.loads(p.read_text())
version=d.get('schema_version')
assert version == 3, (p,version)
assert d.get('telemetry_only') is True
assert d.get('transfer_profile') == 'mir_semantics_v2_sound_r1d_over_v6O', d.get('transfer_profile')
assert d.get('semantics') == 'observational_coverage_of_v6P_r1d_sound_mir_semantics_v2_extension'
roots=d.get('roots')
assert isinstance(roots,list) and roots, 'missing roots'
for r in roots:
    assert r.get('rust_mir_blocks',0)>0
    for key in ('statements','rvalues','terminators'):
        x=r[key]
        assert x['total']==x['precise']+x['conservative']+x['unmodeled'], (key,x)
    calls=r['calls']
    assert calls['total']==sum(calls[k] for k in ('resolved_local','external_summary','unresolved','external_opaque','mixed_local_external'))
print(f"SEMANTIC_COVERAGE_V{version}: PASS roots={len(roots)} profile={d['transfer_profile']}")
