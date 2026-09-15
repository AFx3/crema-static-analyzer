#!/usr/bin/env python3
import argparse, hashlib, json, re, shutil
from pathlib import Path

ap=argparse.ArgumentParser()
ap.add_argument('--metadata',type=Path,required=True)
ap.add_argument('--lock',type=Path,required=True)
ap.add_argument('--name',required=True)
ap.add_argument('--version',required=True)
ap.add_argument('--dest',type=Path,required=True)
ap.add_argument('--provenance-out',type=Path,required=True)
args=ap.parse_args()
meta=json.loads(args.metadata.read_text())
packages=[p for p in meta.get('packages',[]) if p.get('name')==args.name and p.get('version')==args.version]
if len(packages)!=1:
    raise SystemExit(f'expected exactly one metadata package {args.name} {args.version}, got {len(packages)}')
p=packages[0]
source=p.get('source')
if not isinstance(source,str) or not source:
    raise SystemExit(f'{args.name} {args.version}: Cargo metadata did not report a dependency source')
manifest=Path(p['manifest_path']).resolve(); src_root=manifest.parent

# Cargo.lock is TOML but Python 3.10 lacks tomllib. Parse only the simple package
# scalar fields needed for registry provenance; fail closed on ambiguity.
text=args.lock.read_text()
blocks=re.split(r'(?m)^\[\[package\]\]\s*$', text)
matches=[]
for b in blocks[1:]:
    fields={}
    for key in ('name','version','source','checksum'):
        m=re.search(rf'(?m)^{key}\s*=\s*"([^"]*)"\s*$', b)
        if m: fields[key]=m.group(1)
    if fields.get('name')==args.name and fields.get('version')==args.version:
        matches.append(fields)
if len(matches)!=1:
    raise SystemExit(f'{args.name} {args.version}: lock package match count={len(matches)}')
lock=matches[0]
if lock.get('source') != source:
    raise SystemExit(f'{args.name}: metadata/lock source mismatch {source!r} vs {lock.get("source")!r}')
checksum=lock.get('checksum')
if not checksum or not re.fullmatch(r'[0-9a-f]{64}',checksum):
    raise SystemExit(f'{args.name}: missing/invalid registry checksum')

if args.dest.exists(): shutil.rmtree(args.dest)
shutil.copytree(src_root,args.dest,symlinks=True)
# A package Cargo.lock is an analysis-generated file, not part of the source
# fingerprint. Remove a published lock if present but record that fact.
published_lock=(args.dest/'Cargo.lock').exists()
if published_lock: (args.dest/'Cargo.lock').unlink()

entries=[]
for f in sorted(args.dest.rglob('*')):
    if not f.is_file() or 'target' in f.relative_to(args.dest).parts: continue
    rel=f.relative_to(args.dest).as_posix()
    if rel=='Cargo.lock': continue
    h=hashlib.sha256(f.read_bytes()).hexdigest(); entries.append((h,rel))
manifest_out=args.dest/'PACKAGE_SOURCE_SHA256SUMS'
manifest_out.write_text(''.join(f'{h}  {rel}\n' for h,rel in entries))
source_tree_digest=hashlib.sha256(manifest_out.read_bytes()).hexdigest()

prov={
 'schema_version':1,
 'registry':'crates.io',
 'package':args.name,
 'version':args.version,
 'metadata_source':source,
 'registry_checksum':checksum,
 'original_registry_manifest_path':str(manifest),
 'copied_source_root':str(args.dest.resolve()),
 'published_package_had_cargo_lock':published_lock,
 'source_file_count':len(entries),
 'source_manifest_sha256':source_tree_digest,
}
args.provenance_out.parent.mkdir(parents=True,exist_ok=True)
args.provenance_out.write_text(json.dumps(prov,indent=2,sort_keys=True)+'\n')
print(str(args.dest.resolve()))
