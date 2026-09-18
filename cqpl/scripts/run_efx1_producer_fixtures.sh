#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
SVF_EXAMPLE="${SVF_EXAMPLE:-$ROOT/SVF-example/src/svf-example}"
FIXTURES="$ROOT/cqpl/fixtures/efx1"
VALIDATOR="$ROOT/cqpl/scripts/validate_efx1_evidence.py"

if [[ ! -x "$SVF_EXAMPLE" ]]; then
  echo "EFX1 fixtures: missing executable $SVF_EXAMPLE" >&2
  exit 2
fi

TMP="${TMPDIR:-/tmp}/crema-efx1-fixtures-$$"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP"

run_one() {
  local name="$1"
  local out="$TMP/$name"
  mkdir -p "$out"
  # SVF also writes auxiliary files (e.g. callgraph/.svf.bc) relative to CWD.
  # Run inside the isolated fixture directory so validation never dirties the repo.
  (
    cd "$out"
    CREMA_SVF_OUTPUT_DIR="$out" "$SVF_EXAMPLE" "$FIXTURES/$name.ll"       >/dev/null 2>"$out/stderr.log"
  )
  python3 "$VALIDATOR" \
    --effects "$out/LLVM_MEMORY_EFFECTS_V1.json" \
    --pts "$out/SVF_SOLVED_POINTS_TO_V1.json"
}

run_one explicit_effects
run_one tli_libfuncs
run_one tli_negative

# Bpta-R1 non-empty witness. Generate IR with the frozen historical frontend
# rather than hand-writing a target datalayout; the gate checks a genuine
# internal C actual->formal flow and remains MAY-only.
CLANG14="${CREMA_CLANG:-/usr/bin/clang-14}"
LLVM16_AS="$ROOT/SVF/llvm-16.0.0.obj/bin/llvm-as"
[[ -x "$CLANG14" ]] || { echo "EFX1 fixtures: missing frozen Clang14 $CLANG14" >&2; exit 2; }
[[ -x "$LLVM16_AS" ]] || { echo "EFX1 fixtures: missing LLVM16 llvm-as $LLVM16_AS" >&2; exit 2; }
PTA_OUT="$TMP/pta_nonempty"
mkdir -p "$PTA_OUT"
"$CLANG14" -S -c -fno-discard-value-names -emit-llvm \
  "$FIXTURES/pta_nonempty.c" -o "$PTA_OUT/pta_nonempty.ll"
"$LLVM16_AS" "$PTA_OUT/pta_nonempty.ll" -o "$PTA_OUT/pta_nonempty.bc"
(
  cd "$PTA_OUT"
  CREMA_SVF_OUTPUT_DIR="$PTA_OUT" "$SVF_EXAMPLE" "$PTA_OUT/pta_nonempty.ll" \
    >/dev/null 2>"$PTA_OUT/stderr.log"
)
python3 "$VALIDATOR" \
  --effects "$PTA_OUT/LLVM_MEMORY_EFFECTS_V1.json" \
  --pts "$PTA_OUT/SVF_SOLVED_POINTS_TO_V1.json"
python3 - "$PTA_OUT/SVF_SOLVED_POINTS_TO_V1.json" <<'PYCHECK'
import json, sys
j=json.load(open(sys.argv[1]))
assert j['schema']=='svf_solved_points_to_v1'
assert j['analysis']=='AndersenWaveDiff'
assert j['semantics']=='may'
funcs={f['function']:f for f in j['functions']}
assert 'sink' in funcs, funcs.keys()
formals=funcs['sink']['formals']
assert len(formals)==1, formals
pts=formals[0]['points_to']
assert pts, formals[0]
assert len(pts)==len(set(pts)), pts
print('BPTA_R1_NONEMPTY_WITNESS: PASS')
PYCHECK

python3 "$VALIDATOR" \
  --effects "$TMP/tli_libfuncs/LLVM_MEMORY_EFFECTS_V1.json" \
  --pts "$TMP/tli_libfuncs/SVF_SOLVED_POINTS_TO_V1.json" \
  --require-libfunc free \
  --require-libfunc malloc \
  --require-libfunc calloc \
  --require-libfunc realloc

python3 - "$TMP/tli_negative/LLVM_MEMORY_EFFECTS_V1.json" <<'PY'
import json, sys
j=json.load(open(sys.argv[1]))
funcs={f['name']:f for m in j['modules'] for f in m['functions']}
assert funcs['free']['tli_recognized'] is False, funcs['free']
assert funcs['free']['tli_libfunc'] is None, funcs['free']
assert funcs['free']['tli_changed'] is False, funcs['free']
assert funcs['malloc']['explicit']['nobuiltin'] is True, funcs['malloc']
assert funcs['malloc']['tli_recognized'] is False, funcs['malloc']
assert funcs['malloc']['tli_libfunc'] is None, funcs['malloc']
assert funcs['malloc']['tli_changed'] is False, funcs['malloc']
print('EFX1_NEGATIVE_TLI_FIXTURE: PASS')
PY

echo "EFX1_PRODUCER_FIXTURES: PASS"
