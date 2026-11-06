# CQPL8 RQ1 integration manifest

## Validated lineage

The source branch `port/rq1-ml-fn-to-cqpl8` is a descendant of
`cqpl8-heap-transfer-semantics` (`a52cb4a`). It already contains:

- `0ab9d0b`: ML production changes in `crema/src/cqpl_export.rs`,
  `cqpl/cqpl_checker/src/explain.rs`, and `cqpl/cqpl_checker/src/kripke.rs`;
- `10b243b`: permanent CQPL ML regression tests in
  `cqpl/cqpl_checker/src/explain.rs`.

The remaining working-tree changes port P0/P1/P2, make CStringCargo canonical,
and package the RQ1 artifact. Histories must still be checked immediately
before integration; do not assume a fast-forward after either branch changes.

## Files that must be versioned

### Analyzer and regression implementation

- `crema/src/abstract_domain.rs`
- `crema/src/cqpl_export.rs`
- `crema/src/icfg.rs`
- `crema/src/identity.rs`
- `crema/src/structs.rs`
- `cqpl/cqpl_checker/src/explain.rs` (already in `0ab9d0b`, `10b243b`)
- `cqpl/cqpl_checker/src/kripke.rs` (already in `0ab9d0b`)

### Canonical fixture

- `tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/src/main.rs`
- `tests_and_target_repos/a-code_full_rust/cstringcargo_enum_df_only_rust/README.md`

`Cargo.toml` and `Cargo.lock` are deliberately unchanged.

### RQ1 protocol and reporting

- `CQPL8_FPF_PORT_VALIDATION/A3_TARGET_CAPABILITIES.tsv`
- `CQPL8_FPF_PORT_VALIDATION/PORTING_CHANGELOG.md`
- `CQPL8_FPF_PORT_VALIDATION/REGRESSION_TESTS_ADDED.md`
- `CQPL8_FPF_PORT_VALIDATION/RQ1_CSTRINGCARGO_FINDING_AUDIT.md`
- `CQPL8_FPF_PORT_VALIDATION/RQ1_CSTRINGCARGO_FINDING_AUDIT.tsv`
- `CQPL8_FPF_PORT_VALIDATION/VALIDATION_MANUAL.md`
- `CQPL8_FPF_PORT_VALIDATION/INTEGRATION_MANIFEST.md`
- `CQPL8_FPF_PORT_VALIDATION/scripts/bootstrap.sh`
- `CQPL8_FPF_PORT_VALIDATION/scripts/export_one_target.py`
- `CQPL8_FPF_PORT_VALIDATION/scripts/report_rq1_tables.py`
- `CQPL8_FPF_PORT_VALIDATION/scripts/rq1_paper_pipeline.sh`
- `CQPL8_FPF_PORT_VALIDATION/scripts/run_rq1_final.py`
- `CQPL8_FPF_PORT_VALIDATION/scripts/run_rq2_final.py`
- `CQPL8_FPF_PORT_VALIDATION/scripts/validate_smokes.py`
- `cqpl/analysis_profiles/full_memory_safety_v1.json`
- `cqpl/evaluation_manifests/full_memory_safety_v1/dataset_targets_196.txt`

### Public artifact

- `cqpl/artifact/run_rq1.sh`
- `cqpl/artifact/README.md`
- `cqpl/artifact/RQ1_ORACLE_175.csv`

The five query files, `cqpl/artifact/TARGET_ANALYSIS_CONFIG.tsv`, and
`cqpl/regression/reference/entry_overrides.json` are already tracked and remain
runtime dependencies; their hashes are recorded by the runner.

## Generated or unrelated files to exclude

Do not stage:

- `SVF-example/callgraph_initial.dot.dot`
- `crema/ffi_functions.json`
- `crema/global_icfg.json`
- `crema/global_icfg_nodes_edges.dot`
- any `target/`, `scratch/`, `__pycache__/`, `.pyc`, binary, checkpoint, smoke,
  bootstrap, graph, or campaign output;
- `cqpl/cqpl_checker/target`;
- personal or historical result directories;
- `RQ1_RQ2_CQPL8_RECONCILIATION/scripts/__pycache__/`.

The additional untracked `cqpl/scripts/*.py` reconciliation/corpus utilities
are not called by `run_rq1.sh` and are not required for this artifact. Review
and integrate them separately, if desired.

## Runtime dependency closure

`run_rq1.sh` checks that every artifact-specific dependency is tracked. The
execution chain is:

`run_rq1.sh` -> `bootstrap.sh` -> `export_one_target.py` and
`run_rq1_final.py` -> `report_rq1_tables.py`.

The exporter additionally reads tracked `TARGET_ANALYSIS_CONFIG.tsv`,
`entry_overrides.json`, the analysis profile and the selected source crates.
The runner reads the 175-target manifest, five tracked queries, the canonical
oracle and A3 capability manifest.

## Integration procedure

1. Review and stage only the exact files above, in logical commits (analyzer;
   canonical fixture; protocol/artifact).
2. Run unit tests and the one-command artifact from a clean committed checkout.
3. Confirm 875/875 query executions, 700/700 primary cells, the expected
   row-level comparison, and report provenance.
4. Create a separate clean integration worktree from
   `cqpl8-heap-transfer-semantics` because the author checkout is dirty.
5. Verify ancestry. If the destination is still an ancestor, fast-forward it
   to the reviewed source branch; otherwise perform an explicit reviewed merge
   or cherry-pick of the logical commits. Never copy generated files.
6. Re-run the artifact on the exact integrated commit before pushing.

No test or artifact execution was performed while preparing this manifest.
