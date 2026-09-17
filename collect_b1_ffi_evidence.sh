#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-/home/af/Documenti/a-phd}"
NIGHTLY="${NIGHTLY:-nightly-2024-11-21}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT:-$ROOT/repro-results/b1-ffi-bridge-input-r0-$STAMP}"

CQPL="$ROOT/cqpl"
CREMA="$ROOT/crema"
TESTS="$ROOT/tests_and_target_repos"
RUN_ONE="$CQPL/scripts/run_one_target_v6q_r1c.py"
CHECKER="$CQPL/cqpl_checker/target/debug/cqpl_checker"

TARGETS=(
  clean_mul_fn_ffi_no_errors
  cstr_expect_uaf_and_ub_ffi
  warning_ub_mult
  cstr_cargo_df_ffi
)

QUERIES=(
  double_free_alloc_state
  use_after_free_alloc_state
  allocator_mismatch_ub_v2
  leak_alloc_state
)

fail() {
  echo "B1 EVIDENCE FAIL: $*" >&2
  exit 1
}

for x in rustup cargo python3 jq clang sha256sum find tar; do
  command -v "$x" >/dev/null || fail "missing command: $x"
done

[[ -f "$RUN_ONE" ]] \
  || fail "missing runner: $RUN_ONE"

[[ -f "$CREMA/Cargo.toml" ]] \
  || fail "missing CREMA manifest"

[[ -x "$ROOT/SVF-example/src/svf-example" ]] \
  || fail "SVF driver not built: $ROOT/SVF-example/src/svf-example"

mkdir -p "$OUT"

###############################################################################
# Environment / reproducibility
###############################################################################

{
  echo "timestamp_utc=$STAMP"
  echo "root=$ROOT"
  echo "toolchain=$NIGHTLY"
  echo "git_head=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo NA)"

  echo "git_status_begin"
  git -C "$ROOT" status --porcelain=v1 2>/dev/null || true
  echo "git_status_end"

  echo
  rustup run "$NIGHTLY" rustc --version --verbose
  cargo +"$NIGHTLY" --version
  clang --version

  if command -v llvm-dis >/dev/null; then
    llvm-dis --version | head -n 1
  fi

  if command -v llvm-as >/dev/null; then
    llvm-as --version | head -n 1
  fi
} > "$OUT/environment.txt" 2>&1


###############################################################################
# Four focused FFI subjects
###############################################################################

for name in "${TARGETS[@]}"; do

  REL="a-code_c_ffi/$name"
  TARGET="$TESTS/$REL"

  CASE="$OUT/$name"
  CAN="$CASE/canonical"
  TEL="$CASE/telemetry-v6o"

  [[ -f "$TARGET/Cargo.toml" ]] \
    || fail "missing target $TARGET"

  mkdir -p "$CAN" "$TEL"

  echo
  echo "============================================================"
  echo "TARGET: $name"
  echo "============================================================"


  ###########################################################################
  # Input source fingerprint
  ###########################################################################

  find "$TARGET" -type f \
    \( \
      -name 'Cargo.toml' \
      -o -name 'Cargo.lock' \
      -o -name 'build.rs' \
      -o -name '*.rs' \
      -o -name '*.c' \
      -o -name '*.h' \
    \) \
    -not -path '*/target/*' \
    -print0 \
    | sort -z \
    | xargs -0 sha256sum \
    > "$CASE/source-SHA256SUMS"


  ###########################################################################
  # PASS A
  #
  # Canonical v6Q-r1c.
  #
  # Questo è il pass da usare per:
  #   - CQPL truth values
  #   - annotated_icfg_v2
  #   - allocation_identity
  #   - explainability
  #   - exact SVF/LLVM artifacts consumed by the canonical CREMA path
  ###########################################################################

  echo
  echo "===== $name : canonical v6Q-r1c ====="

  MARK="$CASE/.canonical-svf-start"
  touch "$MARK"

  python3 "$RUN_ONE" \
    --root "$ROOT" \
    --relative-path "$REL" \
    --out "$CAN" \
    --toolchain "$NIGHTLY" \
    --explain-unk-verbose \
    2>&1 | tee "$CASE/canonical-console.log"


  ###########################################################################
  # Preserve canonical global ICFG immediately:
  # CREMA/global_icfg.json is process-global and would otherwise be overwritten
  # by the next target.
  ###########################################################################

  [[ -s "$CREMA/global_icfg.json" ]] \
    || fail "$name: canonical global_icfg.json missing"

  cp -f \
    "$CREMA/global_icfg.json" \
    "$CAN/global_icfg.json"


  ###########################################################################
  # Find exactly the fresh isolated SVF directory produced by this invocation.
  ###########################################################################

  SVF_RUN="$(
    find "$TARGET/target/crema-svf" \
      -mindepth 1 \
      -maxdepth 1 \
      -type d \
      -newer "$MARK" \
      -printf '%T@ %p\n' 2>/dev/null \
      | sort -nr \
      | head -n1 \
      | cut -d' ' -f2-
  )"

  [[ -n "$SVF_RUN" && -d "$SVF_RUN" ]] \
    || fail "$name: cannot identify fresh canonical SVF run"

  mkdir -p "$CAN/svf"

  cp -a \
    "$SVF_RUN"/. \
    "$CAN/svf/"

  printf '%s\n' "$SVF_RUN" \
    > "$CAN/svf-run-source-path.txt"


  ###########################################################################
  # SVF can produce several *_A_FINAL_ICFG.json files.
  # Preserve all of them: B1 needs the whole C/SVF function surface,
  # not only an arbitrarily selected file.
  ###########################################################################

  [[ -s "$CAN/svf/ffi.ll" ]] \
    || fail "$name: exact canonical ffi.ll missing"

  ACOUNT="$(
    find "$CAN/svf" \
      -maxdepth 1 \
      -type f \
      -name '*_A_FINAL_ICFG.json' \
      | wc -l
  )"

  [[ "$ACOUNT" -gt 0 ]] \
    || fail "$name: no *_A_FINAL_ICFG.json produced"

  echo "$ACOUNT" \
    > "$CAN/svf-a-final-icfg-count.txt"

  find "$CAN/svf" \
    -maxdepth 1 \
    -type f \
    -name '*_A_FINAL_ICFG.json' \
    -printf '%f\n' \
    | sort \
    > "$CAN/svf-a-final-icfg-files.txt"


  ###########################################################################
  # Optional exact bitcode.
  #
  # IMPORTANT:
  # derive it from the exact ffi.ll produced by CREMA rather than recompiling
  # the C source with potentially different flags.
  ###########################################################################

  if command -v llvm-as >/dev/null; then
    llvm-as \
      "$CAN/svf/ffi.ll" \
      -o "$CAN/svf/ffi.bc" \
      > "$CAN/svf/llvm-as.stdout.log" \
      2> "$CAN/svf/llvm-as.stderr.log" \
      || true
  fi

  if [[ -s "$CAN/svf/ffi.bc" ]] \
     && command -v llvm-dis >/dev/null; then

    llvm-dis \
      "$CAN/svf/ffi.bc" \
      -o "$CAN/svf/ffi.roundtrip.ll" \
      > "$CAN/svf/llvm-dis.stdout.log" \
      2> "$CAN/svf/llvm-dis.stderr.log" \
      || true
  fi


  ###########################################################################
  # Force explanations for the four B1 queries.
  #
  # run_one_target already creates *.explain.json for unk.
  #
  # Here we deliberately generate an explanation for ALL results, including ff,
  # so that the B1 evidence interface is uniform.
  ###########################################################################

  for q in "${QUERIES[@]}"; do

    QFILE="$CQPL/queries_v2/$q.cqpl"

    [[ -f "$QFILE" ]] \
      || fail "$name: missing query $QFILE"

    "$CHECKER" \
      "$CAN/annotated_icfg_v2.json" \
      "$QFILE" \
      --json \
      --explain-json "$CAN/queries/$q.explain.json" \
      --explain-max-witnesses 32 \
      > "$CAN/queries/$q.recheck.json" \
      2> "$CAN/queries/$q.recheck.stderr.log"

    BASE_RESULT="$(
      jq -r '.result' \
        "$CAN/queries/$q.json"
    )"

    RECHECK_RESULT="$(
      jq -r '.result' \
        "$CAN/queries/$q.recheck.json"
    )"

    [[ "$BASE_RESULT" == "$RECHECK_RESULT" ]] \
      || fail \
        "$name/$q: result drift $BASE_RESULT -> $RECHECK_RESULT"

  done


  ###########################################################################
  # Extract the LLVM declarations/attributes relevant to B1.
  #
  # The complete ffi.ll remains preserved; this small file is only an index.
  ###########################################################################

  grep -nE \
    '^declare |^define |malloc|calloc|realloc|free|alloc-family|allockind|allocptr|nofree|memory\(|captures\(' \
    "$CAN/svf/ffi.ll" \
    > "$CAN/ffi-llvm-memory-contract-lines.txt" \
    || true


  ###########################################################################
  # Re-freeze canonical evidence AFTER all B1 rechecks and copied artifacts.
  #
  # run_one_target writes its own canonical/SHA256SUMS before this collector
  # rewrites selected *.explain.json files and before we add global_icfg/SVF
  # evidence.  Regenerate the per-target manifest here so it describes the
  # final canonical bytes rather than an intermediate state.
  ###########################################################################

  (
    cd "$CAN"

    find . \
      -type f \
      ! -name 'SHA256SUMS' \
      -printf '%P\0' \
      | sort -z \
      | xargs -0 -r sha256sum \
      > SHA256SUMS

    sha256sum -c SHA256SUMS
  )


  ###########################################################################
  # PASS B
  #
  # v6O telemetry.
  #
  # This exists ONLY to obtain:
  #   semantic_coverage.json
  #   analysis_manifest.json
  #   cargo_analysis_plan.json
  #
  # Do not use this pass to replace the canonical query truth values.
  ###########################################################################

  echo
  echo "===== $name : v6O telemetry pass ====="

  MARK2="$CASE/.telemetry-svf-start"
  touch "$MARK2"

  mkdir -p "$TEL/queries"

  (
    cd "$CREMA"

    cargo +"$NIGHTLY" run \
      --manifest-path "$CREMA/Cargo.toml" \
      -- \
      "$TARGET" \
      --analysis-mode application \
      --cargo-kind bin \
      --entry main \
      --only-icfg-annotated \
      --cqpl-schema-version 2 \
      --annotated-icfg-out "$TEL/annotated_icfg_v2.json" \
      --allocation-identity-out "$TEL/allocation_identity.json" \
      --cargo-plan-out "$TEL/cargo_analysis_plan.json" \
      --semantic-coverage-out "$TEL/semantic_coverage.json" \
      --analysis-out-dir "$TEL" \
      --mir-semantics-v2

  ) > "$TEL/crema-export.log" 2>&1


  ###########################################################################
  # Required telemetry outputs
  ###########################################################################

  [[ -s "$TEL/semantic_coverage.json" ]] \
    || fail "$name: telemetry semantic_coverage missing"

  [[ -s "$TEL/analysis_manifest.json" ]] \
    || fail "$name: telemetry analysis_manifest missing"

  [[ -s "$TEL/cargo_analysis_plan.json" ]] \
    || fail "$name: telemetry cargo analysis plan missing"


  ###########################################################################
  # Copy exact root-specific ICFG referenced by the manifest.
  ###########################################################################

  TEL_ICFG="$(
    jq -r \
      '.artifacts[0].icfg // empty' \
      "$TEL/analysis_manifest.json"
  )"

  [[ -n "$TEL_ICFG" && -s "$TEL_ICFG" ]] \
    || fail \
      "$name: manifest does not reference readable telemetry ICFG"

  cp -f \
    "$TEL_ICFG" \
    "$TEL/global_icfg.json"


  ###########################################################################
  # Preserve telemetry SVF input/output independently as well.
  ###########################################################################

  SVF_RUN2="$(
    find "$TARGET/target/crema-svf" \
      -mindepth 1 \
      -maxdepth 1 \
      -type d \
      -newer "$MARK2" \
      -printf '%T@ %p\n' 2>/dev/null \
      | sort -nr \
      | head -n1 \
      | cut -d' ' -f2-
  )"

  [[ -n "$SVF_RUN2" && -d "$SVF_RUN2" ]] \
    || fail \
      "$name: cannot identify fresh telemetry SVF run"

  mkdir -p "$TEL/svf"

  cp -a \
    "$SVF_RUN2"/. \
    "$TEL/svf/"

  printf '%s\n' "$SVF_RUN2" \
    > "$TEL/svf-run-source-path.txt"

  if [[ -s "$CREMA/ffi_functions.json" ]]; then
    cp -f \
      "$CREMA/ffi_functions.json" \
      "$TEL/ffi_functions.json"
  fi


  ###########################################################################
  # Per-target inventory
  ###########################################################################

  jq -n \
    --arg target "$REL" \
    --arg canonical "$CAN" \
    --arg telemetry "$TEL" \
    --argjson a_final_count "$ACOUNT" \
    '{
       target: $target,
       canonical_dir: $canonical,
       telemetry_dir: $telemetry,
       a_final_icfg_files: $a_final_count
     }' \
    > "$CASE/inventory.json"

done


###############################################################################
# Freeze all evidence bytes
###############################################################################

(
  cd "$OUT"

  find . \
    -type f \
    ! -path './SHA256SUMS' \
    -printf '%P\0' \
    | sort -z \
    | xargs -0 -r sha256sum \
    > SHA256SUMS

  sha256sum -c SHA256SUMS
)


###############################################################################
# Package
###############################################################################

ARCHIVE="${OUT}.tar.gz"

tar \
  -C "$(dirname "$OUT")" \
  -czf "$ARCHIVE" \
  "$(basename "$OUT")"

(
  cd "$(dirname "$ARCHIVE")"
  sha256sum "$(basename "$ARCHIVE")" \
    > "$(basename "$ARCHIVE").sha256"
)

echo
echo "============================================================"
echo "B1 evidence collection complete"
echo "============================================================"
echo "directory: $OUT"
echo "archive:   $ARCHIVE"
echo "sha256:    $ARCHIVE.sha256"
