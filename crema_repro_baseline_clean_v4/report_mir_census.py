#!/usr/bin/env python3
import json, sys
from pathlib import Path

if len(sys.argv) != 2:
    raise SystemExit('usage: report_mir_census.py mir-census.aggregate.json')

d = json.loads(Path(sys.argv[1]).read_text())
print(f"targets with census: {d.get('targets', 0)}")
print('\nUnhandled MIR rvalue forms:')
un = d.get('rvalue_unhandled_counts', {})
if not un:
    print('  none')
else:
    for k, v in sorted(un.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f'  {k}: {v}')
        for ex in d.get('rvalue_examples', {}).get(k, [])[:5]:
            print(f'    - {ex}')

print('\nObserved memory-related standard-library calls:')
mem = d.get('memory_api_counts', {})
if not mem:
    print('  none')
else:
    for k, v in sorted(mem.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f'  {k}: {v}')
        for ex in d.get('memory_api_examples', {}).get(k, [])[:3]:
            print(f'    - {ex}')
