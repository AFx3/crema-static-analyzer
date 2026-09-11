#!/usr/bin/env bash
set -uo pipefail

ROOT="${CREMA_PHD_ROOT:-/home/af/Documenti/a-phd}"
CREMA="$ROOT/crema"
SVF_EXAMPLE="$ROOT/SVF-example"
BASE="${PHASE5_TARGET_BASE:-$ROOT/tests_and_target_repos/a-code_c_to_rust_alloc}"
NORMALIZER="$ROOT/crema_repro_baseline_clean_v4/normalize_results.py"
TOOLCHAIN="${CREMA_TOOLCHAIN:-nightly-2024-11-21}"
OUT="${1:-$ROOT/repro-results/phase5-c-origin-focus}"

mkdir -p "$OUT/raw"
printf "key\ttarget\texit_code\n" > "$OUT/status.tsv"

clean_analysis_artifacts() {
  rm -f "$CREMA/ffi_functions.json" \
        "$CREMA/global_icfg.json" \
        "$CREMA/global_icfg_nodes_edges.dot" \
        "$SVF_EXAMPLE/ffi.ll" || true

  if [[ -d "$SVF_EXAMPLE/output" ]]; then
    find "$SVF_EXAMPLE/output" -maxdepth 1 -type f \
      -name '*_A_FINAL_ICFG.json' -delete 2>/dev/null || true
  fi
}

archive_analysis_artifacts() {
  local key="$1"
  local dst="$OUT/raw/$key.analysis"
  mkdir -p "$dst"

  [[ -f "$CREMA/global_icfg.json" ]] \
    && cp "$CREMA/global_icfg.json" "$dst/global_icfg.json"
  [[ -f "$CREMA/ffi_functions.json" ]] \
    && cp "$CREMA/ffi_functions.json" "$dst/ffi_functions.json"

  if [[ -d "$SVF_EXAMPLE/output" ]]; then
    find "$SVF_EXAMPLE/output" -maxdepth 1 -type f \
      -name '*_A_FINAL_ICFG.json' -exec cp {} "$dst/" \; 2>/dev/null || true
  fi
}

run_one() {
  local key="$1"
  local target="$BASE/$key"

  clean_analysis_artifacts
  rm -rf "$target/target" || true
  cargo +"$TOOLCHAIN" clean --manifest-path "$target/Cargo.toml" \
    >/dev/null 2>&1 || true

  echo "BUILD $key"
  cargo +"$TOOLCHAIN" build --manifest-path "$target/Cargo.toml" \
    >/dev/null 2>&1
  local brc=$?
  if [[ $brc -ne 0 ]]; then
    echo "  build failed rc=$brc"
    printf "%s\t%s\t125\n" "$key" "$target" >> "$OUT/status.tsv"
    return
  fi

  # Remove only analyzer artifacts. Keep target build products.
  clean_analysis_artifacts

  local raw="$OUT/raw/$key.log"
  echo "RUN $key"
  (cd "$CREMA" && cargo +"$TOOLCHAIN" run -- "$target") >"$raw" 2>&1
  local rc=$?
  printf "%s\t%s\t%s\n" "$key" "$target" "$rc" >> "$OUT/status.tsv"

  archive_analysis_artifacts "$key"

  local wrapped="$OUT/raw/$key.wrapped.log"
  { echo "=== $key ==="; cat "$raw"; } > "$wrapped"
  python3 "$NORMALIZER" "$wrapped" \
    > "$OUT/raw/$key.normalized.json"
  rm -f "$wrapped"

  python3 - "$OUT/raw/$key.normalized.json" "$key" "$rc" <<'PY'
import json, sys
p, key, rc = sys.argv[1:]
d = json.load(open(p))
print(f"  rc={rc} classes={d.get(key)}")
PY
}

python3 - "$BASE/ORACLE.json" <<'PY' > "$OUT/keys.txt"
import json, sys
for key in sorted(json.load(open(sys.argv[1]))):
    print(key)
PY

while IFS= read -r key; do
  run_one "$key"
done < "$OUT/keys.txt"

python3 - "$OUT" "$BASE/ORACLE.json" <<'PY'
import json, pathlib, sys

out = pathlib.Path(sys.argv[1])
oracle = json.load(open(sys.argv[2]))
bad = []

status = {}
for line in (out / "status.tsv").read_text().splitlines()[1:]:
    if not line.strip():
        continue
    key, _target, rc = line.split("\t")
    status[key] = int(rc)

for key, spec in sorted(oracle.items()):
    want = spec["expected_classes"]

    if status.get(key) != 0:
        bad.append((key, want, f"rc={status.get(key)}"))
        continue

    p = out / "raw" / f"{key}.normalized.json"
    if not p.exists():
        bad.append((key, want, "<missing>"))
        continue

    got = json.load(open(p)).get(key)
    if sorted(got or []) != sorted(want):
        bad.append((key, want, got))

if bad:
    print("PHASE5 C-ORIGIN FOCUS: FAIL")
    for key, want, got in bad:
        print(f"  {key}: expected={want} got={got}")
    raise SystemExit(1)

print(f"PHASE5 C-ORIGIN FOCUS: PASS ({len(oracle)}/{len(oracle)})")
PY
