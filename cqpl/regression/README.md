# Running CQPL on CREMA's target corpus

From the repository root:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope phase5-focus16
```

Run the historical frozen protocol:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope frozen92
```

Run every currently discovered target, including slow targets and later additions:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope all
```

Debug one target without changing the suite:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope all \
  --only c_malloc_rust_free_then_use_uaf
```

Outputs are written under `repro-results/cqpl-<scope>-<timestamp>/` and include
per-target K# files, CREMA logs, per-query JSON, aggregate results, status TSV,
environment data, an explicitly unreviewed candidate oracle, and SHA-256 hashes.

Inspect the selected corpus without running analysis:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope all \
  --list-targets
```

Validate the Python harness itself:

```bash
python3 cqpl/regression/scripts/test_regression_harness.py -v
```

After installing this suite, rerun the Rust semantic tests:

```bash
cd cqpl/cqpl_checker
cargo +nightly-2024-11-21 test -- --nocapture
```


## Semantic scope and timeouts

See [`SEMANTIC_SCOPE.md`](SEMANTIC_SCOPE.md) for the deliberate distinction
between Leak/DF/UAF and the currently unmodeled legacy `UB_FFI` class, and for
why the runner does not report a `NO_ERRORS` CQPL verdict.

See [`PERFORMANCE_NOTES.md`](PERFORMANCE_NOTES.md) for the `square` performance
investigation.

Each CQPL query has a wall-clock timeout of 120 seconds by default:

```bash
python3 cqpl/regression/scripts/run_target_repo_cqpl.py \
  --root /home/af/Documenti/a-phd \
  --scope frozen92 \
  --query-timeout-seconds 120
```

Use a non-positive value only when deliberately disabling the guard.
