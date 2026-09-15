#!/usr/bin/env bash
set -euo pipefail

CQPL_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${CREMA_PHD_ROOT:-${1:-}}"
[[ -n "$ROOT" ]] || { echo 'set CREMA_PHD_ROOT or pass ROOT as argv[1]' >&2; exit 2; }
NIGHTLY="${CREMA_RUST_TOOLCHAIN:-nightly-2024-11-21}"
OUT="${CQPL_REGISTRY_OUT:-$ROOT/repro-results/cqpl-v6q-r1c-registry-3}"
TARGETS="$CQPL_DIR/artifact/CRATES_IO_TARGETS.json"
EXPECTED_RUSTC='rustc 1.84.0-nightly (3fee0f12e 2024-11-20)'
OFFLINE=()
[[ "${CQPL_CRATES_IO_OFFLINE:-0}" == "1" ]] && OFFLINE=(--offline)

fail() { echo "CQPL v6Q-r1c REGISTRY FAIL: $*" >&2; exit 2; }

command -v rustup >/dev/null || fail 'rustup not found'
command -v cargo >/dev/null || fail 'cargo not found'
command -v python3 >/dev/null || fail 'python3 not found'
[[ -f "$TARGETS" ]] || fail "missing $TARGETS"
[[ -f "$ROOT/crema/Cargo.toml" ]] || fail "missing $ROOT/crema/Cargo.toml"

RUSTC_VERSION="$(rustup run "$NIGHTLY" rustc --version)"
[[ "$RUSTC_VERSION" == "$EXPECTED_RUSTC" ]] || fail "unexpected pinned rustc: $RUSTC_VERSION"
HOST="$(rustup run "$NIGHTLY" rustc -vV | sed -n 's/^host: //p')"
[[ -n "$HOST" ]] || fail 'could not resolve rustc host triple'

rm -rf "$OUT"
mkdir -p "$OUT/coverage-inputs"
printf '%s\n' "$HOST" > "$OUT/host-triple.txt"
cp "$TARGETS" "$OUT/CRATES_IO_TARGETS.json"

python3 - "$TARGETS" > "$OUT/cases.tsv" <<'PY'
import json,sys
for c in json.load(open(sys.argv[1]))['crates']:
    print('\t'.join([
        c['name'], c['version'], c['analysis_mode'], c['cargo_kind'], c['api_root'],
        ','.join(c['features']) or '-', '1' if c['no_default_features'] else '0',
    ]))
PY

COMPLETE=0
while IFS=$'\t' read -r NAME VERSION MODE KIND ROOTAPI FEATURES NODEFAULT; do
    echo "=== CQPL v6Q-r1c registry: $NAME $VERSION mode=$MODE kind=$KIND root=$ROOTAPI ==="
    CASE="$OUT/$NAME-$VERSION"
    PROBE="$CASE/registry-probe"
    SRC="$CASE/package-source"
    A="$CASE/analysis"
    mkdir -p "$PROBE/src" "$A"

    cat > "$PROBE/Cargo.toml" <<EOT
[package]
name="cqpl-v6q-r1c-registry-probe"
version="0.0.0"
edition="2021"
[dependencies]
$NAME="=$VERSION"
EOT
    printf 'fn main(){}\n' > "$PROBE/src/main.rs"

    cargo +"$NIGHTLY" metadata \
      --format-version 1 \
      --manifest-path "$PROBE/Cargo.toml" \
      "${OFFLINE[@]}" \
      > "$CASE/registry-metadata.json"

    python3 "$CQPL_DIR/scripts/prepare_registry_crate.py" \
      --metadata "$CASE/registry-metadata.json" \
      --lock "$PROBE/Cargo.lock" \
      --name "$NAME" \
      --version "$VERSION" \
      --dest "$SRC" \
      --provenance-out "$CASE/registry-provenance.json" \
      > "$CASE/source-root.txt"

    ARGS=(
      "$SRC"
      --analysis-mode "$MODE"
      --cargo-kind "$KIND"
      --api-root "$ROOTAPI"
      --target-triple "$HOST"
      --mir-semantics-v2
      --only-icfg-annotated
      --cqpl-schema-version 2
      --annotated-icfg-out "$A/annotated_icfg_v2.json"
      --allocation-identity-out "$A/allocation_identity.json"
      --cargo-plan-out "$A/cargo_analysis_plan.json"
      --semantic-coverage-out "$A/semantic_coverage.json"
      --analysis-out-dir "$A"
    )
    [[ "$NODEFAULT" == 1 ]] && ARGS+=(--no-default-features)
    [[ "$FEATURES" != '-' ]] && ARGS+=(--features "$FEATURES")

    set +e
    (
      cd "$ROOT/crema"
      cargo +"$NIGHTLY" run -- "${ARGS[@]}"
    ) 2>&1 | tee "$A/console.log"
    RC=${PIPESTATUS[0]}
    set -e

    (
      cd "$SRC"
      sha256sum -c PACKAGE_SOURCE_SHA256SUMS
    ) > "$CASE/source-integrity.log"

    [[ "$RC" -eq 0 ]] || fail "$NAME $VERSION CREMA exit=$RC; see $A/console.log"
    for required in \
      "$A/annotated_icfg_v2.json" \
      "$A/allocation_identity.json" \
      "$A/cargo_analysis_plan.json" \
      "$A/semantic_coverage.json" \
      "$A/analysis_manifest.json"
    do
      [[ -s "$required" ]] || fail "$NAME $VERSION missing/empty $required"
    done

    python3 "$CQPL_DIR/scripts/check_semantic_coverage_v6p_r1d.py" "$A/semantic_coverage.json"
    python3 - "$A/cargo_analysis_plan.json" "$A/analysis_manifest.json" "$MODE" "$KIND" "$ROOTAPI" <<'PY'
import json,sys
plan=json.load(open(sys.argv[1])); manifest=json.load(open(sys.argv[2]))
mode,kind,root=sys.argv[3:6]
assert plan['analysis_mode']==mode, (plan['analysis_mode'],mode)
assert plan['selected_target']['kind']==kind, (plan['selected_target']['kind'],kind)
assert plan['analysis_roots']==[root], (plan['analysis_roots'],root)
assert manifest['analysis_status']=='complete', manifest['analysis_status']
PY

    mkdir -p "$OUT/coverage-inputs/$NAME-$VERSION"
    cp "$A/semantic_coverage.json" "$OUT/coverage-inputs/$NAME-$VERSION/"
    cp "$A/analysis_manifest.json" "$OUT/coverage-inputs/$NAME-$VERSION/"
    COMPLETE=$((COMPLETE+1))
    echo "CQPL_V6Q_R1C_REGISTRY_COMPLETE: PASS $NAME $VERSION"
done < "$OUT/cases.tsv"

[[ "$COMPLETE" -eq 3 ]] || fail "expected 3 complete registry crates, got $COMPLETE"

MEMCHR_LOG="$OUT/memchr-2.7.4/analysis/console.log"
grep -F 'V6Q_HIGHER_ORDER_PRODUCER_EVIDENCE: kind=OptionMap callee=' "$MEMCHR_LOG" >/dev/null \
  || fail 'memchr did not exercise producer-certified Option::map evidence'
if grep -R -F 'UNRESOLVED_HIGHER_ORDER:' "$OUT"/*/analysis/console.log >/dev/null; then
  fail 'registry set contains unresolved higher-order control flow'
fi

python3 "$CQPL_DIR/scripts/aggregate_semantic_coverage_v6p.py" \
  "$OUT/coverage-inputs" "$OUT/semantic_coverage_aggregate.json"
python3 - "$OUT/semantic_coverage_aggregate.json" <<'PY'
import json,sys
x=json.load(open(sys.argv[1])); c=x['aggregate_counts']
assert x['crate_count']==3 and x['cqpl_complete_crates']==3 and x['cqpl_incomplete_crates']==0, x
assert c['rvalues.unmodeled']==0, c
assert c['statements.unmodeled']==0, c
assert c['terminators.unmodeled']==0, c
print('CQPL_V6Q_R1C_REGISTRY_COVERAGE: PASS crates=3 unmodeled=0/0/0')
PY

(
  cd "$OUT"
  find . -type f ! -name SHA256SUMS.FINAL -print0 \
    | sort -z \
    | xargs -0 sha256sum > SHA256SUMS.FINAL
  sha256sum -c SHA256SUMS.FINAL
)

echo "CQPL v6Q-r1c REGISTRY: PASS crates=3"
echo "registry_out=$OUT"
