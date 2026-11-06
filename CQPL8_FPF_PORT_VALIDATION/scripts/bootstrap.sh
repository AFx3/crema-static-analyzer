#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
DEFAULT_WORKTREE=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
WORKTREE=${WORKTREE:-$DEFAULT_WORKTREE}
OUT=${1:-/mnt/wsx-storage/af/cqpl8-fpf-port-bootstrap-$(date -u +%Y%m%dT%H%M%SZ)}
MODE=${2:-build}
EXPECTED_HEAD=${EXPECTED_HEAD:-$(git -C "$WORKTREE" rev-parse HEAD)}
TOOLCHAIN=nightly-2024-11-21

test "$(git -C "$WORKTREE" rev-parse HEAD)" = "$EXPECTED_HEAD" || {
  echo "Refusing: unexpected HEAD in $WORKTREE" >&2; exit 2;
}
if [[ "$MODE" == ffi-smoke ]]; then
  test -f "$OUT/PROVENANCE.json" || { echo "Missing bootstrap provenance in $OUT" >&2; exit 2; }
  python3 - "$OUT/PROVENANCE.json" "$WORKTREE" "$EXPECTED_HEAD" <<'PY'
import hashlib,json,sys
from pathlib import Path
p=json.loads(Path(sys.argv[1]).read_text())
assert p['worktree']==str(Path(sys.argv[2]).resolve()) and p['head']==sys.argv[3]
for binary in p['binaries'].values():
 path=Path(binary['path']);assert path.is_file()
 assert hashlib.sha256(path.read_bytes()).hexdigest()==binary['sha256']
PY
  export RUSTUP_TOOLCHAIN=$TOOLCHAIN
  export TMPDIR="$OUT/tmp"
  export CARGO_INCREMENTAL=0
  SYSROOT=$(rustup run "$TOOLCHAIN" rustc --print sysroot)
  export LD_LIBRARY_PATH="$SYSROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  python3 "$WORKTREE/CQPL8_FPF_PORT_VALIDATION/scripts/export_one_target.py" \
    --root "$WORKTREE" \
    --relative-path a-code_c_to_rust_alloc/c_malloc_c_free_clean \
    --out "$OUT/ffi-smoke" \
    --toolchain "$TOOLCHAIN"
  test -f "$OUT/ffi-smoke/REPORT.json"
  ! grep -R -F "stale FFI sentinel survived extraction" "$OUT/ffi-smoke"
  echo "$OUT"
  exit 0
fi
test "$MODE" = build || { echo "Usage: $0 OUT {build|ffi-smoke}" >&2; exit 2; }
test ! -e "$OUT" || { echo "Refusing to overwrite $OUT" >&2; exit 2; }
mkdir -p "$OUT" "$OUT/tmp" "$OUT/targets/crema" "$OUT/targets/cqpl" "$OUT/bin"

export RUSTUP_TOOLCHAIN=$TOOLCHAIN
export TMPDIR="$OUT/tmp"
export CARGO_INCREMENTAL=0
SYSROOT=$(rustup run "$TOOLCHAIN" rustc --print sysroot)
export LD_LIBRARY_PATH="$SYSROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

CARGO_TARGET_DIR="$OUT/targets/crema" cargo build --manifest-path "$WORKTREE/crema/Cargo.toml"
CARGO_TARGET_DIR="$OUT/targets/cqpl" cargo build --manifest-path "$WORKTREE/cqpl/cqpl_checker/Cargo.toml"
ln -s "$OUT/targets/crema/debug/crema" "$OUT/bin/crema"
ln -s "$OUT/targets/cqpl/debug/cqpl_checker" "$OUT/bin/cqpl_checker"

mkdir -p "$WORKTREE/crema/target/debug" "$WORKTREE/cqpl/cqpl_checker/target/debug"
ln -sfn "$OUT/bin/crema" "$WORKTREE/crema/target/debug/crema"
ln -sfn "$OUT/bin/cqpl_checker" "$WORKTREE/cqpl/cqpl_checker/target/debug/cqpl_checker"

python3 - "$WORKTREE" "$OUT" "$EXPECTED_HEAD" "$TOOLCHAIN" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
w,o,head,toolchain=Path(sys.argv[1]),Path(sys.argv[2]),sys.argv[3],sys.argv[4]
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
sources=[w/'crema/src/cqpl_export.rs',w/'crema/src/identity.rs',w/'crema/src/icfg.rs',
         w/'crema/src/structs.rs',w/'crema/src/abstract_domain.rs',
         w/'cqpl/cqpl_checker/src/explain.rs',w/'cqpl/cqpl_checker/src/kripke.rs']
doc={'schema':'cqpl8_fpf_port_bootstrap_v1','worktree':str(w.resolve()),'head':head,
     'git_status':subprocess.check_output(['git','-C',str(w),'status','--porcelain=v1'],text=True).splitlines(),
     'toolchain':toolchain,'sources':{str(p.relative_to(w)):sha(p) for p in sources},
     'binaries':{'crema':{'path':str((o/'bin/crema').resolve()),'sha256':sha((o/'bin/crema').resolve())},
                 'cqpl_checker':{'path':str((o/'bin/cqpl_checker').resolve()),'sha256':sha((o/'bin/cqpl_checker').resolve())}}}
(o/'PROVENANCE.json').write_text(json.dumps(doc,indent=2,sort_keys=True)+'\n')
PY

echo "$OUT"
