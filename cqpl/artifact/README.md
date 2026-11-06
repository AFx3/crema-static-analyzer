# Reproducing RQ1

From any working directory, run:

```bash
bash /path/to/a-phd/cqpl/artifact/run_rq1.sh
```

An explicit fresh output location may be supplied with `--out`. An alternative
oracle is accepted only through `--oracle PATH` and is recorded as an override;
the default is the byte-frozen oracle shipped beside the script.

Requirements: Linux, Bash, Python 3, Cargo/rustup, Git, GNU-compatible core
utilities, the installed `nightly-2024-11-21` toolchain, and sufficient disk
space for two Rust target directories plus 175 exported graphs. In the
reference environment output defaults to `/mnt/wsx-storage/af`; otherwise it
falls back to `${TMPDIR:-/tmp}`.

Each run uses separate `bootstrap/`, `campaign/`, and `tables/` directories and
refuses an existing output path. It compiles binaries from the current checkout,
runs the FFI stale-artifact guard, executes five queries for every crate, and
requires 875/875 successful query executions and 700/700 unique primary cells.
`DF_NORMAL` is the primary DF query; `DF_FULL` is diagnostic only.

`RQ1_PER_QUERY.{csv,tex}` reports ML, DF_NORMAL, UAF and ALLOC_MISMATCH
separately. `RQ1_GLOBAL_MICRO.{csv,tex}` is the micro-aggregate of their cell
counts, not an average of percentages. Execution failures terminate the
one-command artifact and are never converted to confusion-matrix cells.

The canonical CStringCargo fixture removes an unrelated reachable dynamic
logging dispatch while retaining both consecutive `CString::from_raw` calls.
Its source hash and the target-specific A3 capability are checked and recorded.
Campaign and report provenance contain hashes for the oracle, fixture, target
source trees, relevant analyzer sources, binaries, queries and capability
manifest.

The script exits non-zero on missing dependencies, wrong toolchain/input hash,
existing output, incomplete execution, duplicate cells, missing artifacts or
reporting failure. It does not delete historical output.
