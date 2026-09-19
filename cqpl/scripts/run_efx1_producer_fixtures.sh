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

# Live-producer contract preflight.  A stale binary can still exit 0 and emit
# legacy *_A_FINAL_ICFG.json files, so executable presence alone is not enough.
for marker in \
  'LLVM_MEMORY_EFFECTS_V1.json' \
  'SVF_SOLVED_POINTS_TO_V1.json' \
  'formal_param_mapping_schema' \
  'svf_formal_arg_index_v1'
do
  if ! grep -aFq "$marker" "$SVF_EXAMPLE"; then
    echo "EFX1 fixtures: stale/incompatible SVF producer; binary lacks marker '$marker': $SVF_EXAMPLE" >&2
    echo "Perform a clean rebuild of target svf-example before running the fixtures." >&2
    exit 3
  fi
done

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

# Gate A/Bmulti: two source-language formals must retain declaration-order
# identity in both the final ICFG and solved Andersen sidecar.  This is the
# minimal producer fixture that detects a legacy first-formal-only binary.
FORMAL_OUT="$TMP/formal_two_args"
mkdir -p "$FORMAL_OUT"
"$CLANG14" -S -c -fno-discard-value-names -emit-llvm \
  "$FIXTURES/formal_two_args.c" -o "$FORMAL_OUT/formal_two_args.ll"
(
  cd "$FORMAL_OUT"
  CREMA_SVF_OUTPUT_DIR="$FORMAL_OUT" "$SVF_EXAMPLE" "$FORMAL_OUT/formal_two_args.ll" \
    >/dev/null 2>"$FORMAL_OUT/stderr.log"
)
python3 "$VALIDATOR" \
  --effects "$FORMAL_OUT/LLVM_MEMORY_EFFECTS_V1.json" \
  --pts "$FORMAL_OUT/SVF_SOLVED_POINTS_TO_V1.json"
python3 - \
  "$FORMAL_OUT/free_second_A_FINAL_ICFG.json" \
  "$FORMAL_OUT/SVF_SOLVED_POINTS_TO_V1.json" <<'PYFORMAL'
import json, sys
icfg=json.load(open(sys.argv[1]))
pts=json.load(open(sys.argv[2]))
assert icfg.get('formal_param_mapping_schema') == 'svf_formal_arg_index_v1', icfg.keys()
ids=icfg.get('formal_param_var_ids')
assert isinstance(ids, list) and len(ids) == 2 and len(set(ids)) == 2, ids
funcs={f['function']: f for f in pts['functions']}
assert 'free_second' in funcs, funcs.keys()
formals=funcs['free_second']['formals']
assert [f['formal_index'] for f in formals] == [0, 1], formals
assert [f['svf_var_id'] for f in formals] == ids, (formals, ids)
print('BMULTI_PRODUCER_POSITIONAL_CERTIFICATE: PASS')
PYFORMAL

echo "EFX1_PRODUCER_FIXTURES: PASS"
