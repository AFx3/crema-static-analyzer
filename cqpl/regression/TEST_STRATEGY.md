# CQPL regression strategy over CREMA targets

This suite deliberately separates three validation layers.

## Layer 1 — CQPL unit semantics

The Rust unit tests validate the CQPL parser, the three-valued truth lattice,
cross-language quantification, alias-aware atomic predicates, strong `X`, and
least/greatest fixed-point implementations of existential/universal `F`, `G`
and `U`.

These tests do **not** duplicate CREMA transfer-function or lattice tests.

## Layer 2 — CREMA → CQPL boundary

`crema/src/cqpl_export.rs` is tested inside CREMA. The checker independently
validates `AnnotatedIcfg` schema version 1, graph closure, declared variables,
per-program-point alias components, syntactic labels, and the post-state used by
semantic may predicates.

## Layer 3 — target repository replication

Every selected Cargo analysis root under `tests_and_target_repos/` is:

1. built with the pinned Rust toolchain;
2. analyzed by CREMA with `--only-icfg-annotated`;
3. validated as an `AnnotatedIcfg`;
4. checked with the same official `Leak`, `DF`, and `UAF` CQPL formulas;
5. recorded as `ff`, `unk`, or `tt` together with reproducibility evidence.

The runner is serial by design because CREMA/SVF use shared intermediate files.
Parallel execution would risk cross-target contamination.

## Target discovery

A target is a **minimal Cargo root**: a directory containing `Cargo.toml` that
is not nested below another Cargo root inside `tests_and_target_repos/`.
This includes the literal-box micro-targets while avoiding duplicate analysis of
workspace members when a workspace root already exists.

Scopes:

- `all`: every currently discovered minimal Cargo root. No silent exclusion.
- `frozen92`: historical frozen protocol. It excludes `openapi-client-gen` and
  `a-code_full_rust/drop_raw_ptr_no_free`, and excludes later Phase-5 C-origin
  additions. Exactly 92 targets are required.
- `phase5-focus16`: exactly the 16 targets under `a-code_c_to_rust_alloc/`.

## Legacy results are not CQPL ground truth

The frozen CREMA detector result is retained only as a **differential reference**.
For a memory-error class:

- legacy positive + CQPL `unk|tt`: old detector reports it and CQPL does not refute it;
- legacy positive + CQPL `ff`: review required;
- legacy negative + CQPL `unk|tt`: CQPL-only non-refuting result, potentially a
  conservative false positive or a semantic difference;
- legacy negative + CQPL `ff`: both analyses do not report/non-refute that class.

This relation is engineering evidence, not a soundness theorem.

`UB_FFI` is outside the three current CQPL queries and is therefore retained as
an unmodeled legacy class, never coerced into Leak/DF/UAF.

## Oracle policy

`oracles/reviewed_cqpl.json` contains only target/query outcomes that have been
actually inspected and accepted. Missing entries mean **unreviewed**, not
"don't care" in a scientific claim.

Every complete run writes `candidate_oracle.UNREVIEWED.json`. It must never be
promoted automatically. Review the corresponding K#, labels, abstract states,
and source target before copying an outcome into the reviewed oracle.
