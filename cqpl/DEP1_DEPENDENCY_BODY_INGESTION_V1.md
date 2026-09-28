# CQPL7 / DEP1 — Dependency Body Ingestion V1

**Status:** design and implementation protocol<br>
**Branch:** `cqpl7-dependency-body-ingestion`<br>
**Frozen parent:** `0169ce3da4e865e1750e22b9fc14631b0efcca98`<br>
**Pinned toolchain:** `nightly-2024-11-21`<br>
**Capability candidate:** `dependency_body_ingestion_v1`<br>
**Date:** 2026-09-28

---

## 0. Purpose

This document defines the scientific and engineering protocol for extending CREMA so that analysis of a selected Cargo target can include MIR bodies from relevant Rust dependencies, rather than treating all cross-crate Rust calls as permanently opaque external calls.

The capability is **not** specific to RustSec, `emap`, or any one crate. It is an analysis-scope capability required for realistic Cargo applications, where security-relevant behavior frequently lives in dependency crates.

The motivating pilot established a concrete scope limitation:

- the selected reproducer binary was analyzed;
- call sites into a dependency were represented;
- the corresponding dependency MIR bodies were not represented;
- therefore memory events inside those dependency bodies could not participate in CREMA allocation identity or CQPL lifecycle properties.

The motivating subject MUST NOT be used as the development oracle for DEP1. DEP1 must be designed, implemented, and validated using synthetic cross-crate fixtures and frozen regression corpora first. Only after the DEP1 gate passes may the real-world pilot be rerun as a held-out evaluation.

---

## 1. Scientific question

### RQ-DEP1

Can CREMA extend its interprocedural MIR model across Cargo crate boundaries while preserving:

1. exact compiler-derived call identity;
2. call/return control-flow semantics;
3. allocation/resource identity across crate boundaries;
4. existing same-crate semantics;
5. fail-closed behavior when a dependency body is unavailable?

### Primary hypothesis

For Rust dependency items whose MIR is available through rustc metadata or through a sound Cargo-integrated capture path, CREMA can ingest the body and connect it to the originating call site without reconstructing semantic identity from pretty-printed text.

### Null / failure conditions

DEP1 is not accepted if any of the following is required:

- matching callees by pretty-printed function strings;
- synthesizing MIR bodies from source text;
- silently treating unavailable dependency bodies as analyzed;
- collapsing distinct versions of the same crate;
- conflating build dependencies / procedural macros with target-runtime dependencies;
- changing CQPL truth semantics to compensate for missing MIR;
- introducing target-specific rules for the motivating RustSec subject.

---

## 2. Official Rust/Cargo basis

Implementation decisions must be checked against the pinned toolchain. Current official documentation is architectural evidence, **not an API-version guarantee** for `nightly-2024-11-21`.

### 2.1 Cargo dependency graph

`cargo metadata --format-version 1` provides machine-readable package structure and the resolved dependency graph, including package IDs, dependency edges, enabled features, targets, source information, and workspace membership.

Official documentation:

- Cargo Book — `cargo metadata`<br>
  https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
- Cargo Book — External tools<br>
  https://doc.rust-lang.org/cargo/reference/external-tools.html

DEP1 should use Cargo package identity as a build-graph identity and must not identify packages only by crate name.

### 2.2 Actual Cargo build artifacts

Cargo's JSON message stream can identify the package, manifest, target, crate type, features, profile, produced `rlib`/`rmeta` artifacts, and whether an artifact was fresh.

Official documentation:

- Cargo Book — External tools / JSON messages<br>
  https://doc.rust-lang.org/cargo/reference/external-tools.html
- Cargo Book — `cargo build`<br>
  https://doc.rust-lang.org/cargo/commands/cargo-build.html

This is preferable to guessing artifact names under `target/`.

### 2.3 Cargo compiler invocation scope

`cargo rustc` compiles the selected target and its dependencies, but extra arguments after `--` are passed only to the final compiler invocation, not automatically to dependency compiler invocations. Cargo documents `RUSTFLAGS` / `build.rustflags` as mechanisms affecting compiler invocations more broadly.

Official documentation:

- Cargo Book — `cargo rustc`<br>
  https://doc.rust-lang.org/cargo/commands/cargo-rustc.html
- Cargo Book — Configuration / `rustflags` / `rustc-wrapper`<br>
  https://doc.rust-lang.org/cargo/reference/config.html

This distinction matters if DEP1 needs dependency crates to encode additional MIR metadata.

### 2.4 rustc driver integration

The rustc development guide recommends `rustc_driver` as the normal integration layer for running compiler phases. `rustc_driver::run_compiler` accepts rustc arguments and callbacks that can execute custom logic during compilation.

Official documentation:

- Rust Compiler Development Guide — `rustc_driver` and `rustc_interface`<br>
  https://rustc-dev-guide.rust-lang.org/rustc-driver/intro.html

CREMA must continue to obtain semantic information from rustc rather than reparsing Rust source.

### 2.5 MIR bodies

The official compiler documentation defines `rustc_middle::mir::Body` as the MIR representation of a single function, with basic blocks, locals, source scopes, statements, and terminators.

Official documentation:

- Rust Compiler Development Guide — MIR<br>
  https://rustc-dev-guide.rust-lang.org/mir/index.html
- nightly rustc API — `rustc_middle::mir`<br>
  https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/
- nightly rustc API — `Body`<br>
  https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/struct.Body.html

### 2.6 Foreign MIR queries

The rustc development guide explicitly states that `optimized_mir(def_id)` may be requested for a function and that for a **foreign DefId** rustc reads MIR from the other crate's metadata.

Official documentation:

- Rust Compiler Development Guide — MIR queries and passes<br>
  https://rustc-dev-guide.rust-lang.org/mir/passes.html
- Rust Compiler Development Guide — Query system / external crate metadata<br>
  https://rustc-dev-guide.rust-lang.org/query.html

This is the preferred first mechanism to investigate.

### 2.7 MIR in crate metadata is conditional

The rustc development guide documents that `rlib` / `rmeta` contain rustc metadata and that encoded MIR is optional. MIR is encoded when required for downstream code generation; `cargo check` may omit optimized MIR for performance.

Official documentation:

- Rust Compiler Development Guide — Libraries and metadata<br>
  https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html
- rustc API — `should_encode_mir`<br>
  https://doc.rust-lang.org/nightly/nightly-rustc/rustc_metadata/rmeta/encoder/fn.should_encode_mir.html

Current rustc documentation also exposes an unstable `always_encode_mir` option. Its existence and exact command-line spelling MUST be verified on the pinned 2024 toolchain before use.

Official documentation:

- nightly rustc API — `UnstableOptions::always_encode_mir`<br>
  https://doc.rust-lang.org/nightly/nightly-rustc/rustc_session/config/struct.UnstableOptions.html

### 2.8 Generics and monomorphized instances

Rust performs monomorphization for concrete generic instances. Cross-crate generic and inline functions have different code-generation behavior from ordinary non-generic functions.

Official documentation:

- Rust Compiler Development Guide — Monomorphization<br>
  https://rustc-dev-guide.rust-lang.org/backend/monomorph.html

DEP1 must therefore distinguish a source-level `DefId` from a concrete callable `Instance` where CREMA's existing call-resolution semantics require it.

---

## 3. Terminology and identity model

DEP1 must distinguish at least:

- **Cargo package identity**: Cargo `PackageId`, including version/source disambiguation.
- **rustc crate identity**: compiler crate identity, preferably stable crate identity / crate disambiguator information available from rustc.
- **definition identity**: compiler `DefId` / stable def-path identity.
- **instance identity**: resolved callable instance including generic substitutions when relevant.
- **selected target**: the Cargo target explicitly chosen for analysis.
- **runtime dependency**: a dependency that participates in the selected target's compiled/runtime Rust graph.
- **host/build dependency**: build scripts and procedural-macro-side dependencies that are not ordinary runtime bodies of the selected target.
- **body availability**: explicit producer-certified status describing whether MIR can be obtained.
- **call-site representation**: a MIR call terminator is present in the caller.
- **callee-body representation**: the MIR body of the target function is actually represented as ICFG nodes.
- **cross-crate binding**: a producer-certified relation between one call site and one represented dependency callable instance.

Never treat `function_called` or another pretty-printed path string as semantic identity.

---

## 4. Mandatory design invariants

DEP1 is accepted only if all invariants below hold.

### I1 — Compiler-derived identity

Cross-crate call matching must use rustc semantic identity. Pretty-printed names may be exported for diagnostics only.

### I2 — Crate disambiguation

Two crate instances with the same crate name but different Cargo package/version/source identity must remain distinct.

### I3 — Exact call/body distinction

Every dependency call must expose one of:

- `represented_body`;
- `body_unavailable`;
- `intentionally_opaque`;
- `unresolved`.

A call site must never imply that its body is represented.

### I4 — Fail closed

If MIR cannot be obtained, the call remains external/opaque under existing semantics. DEP1 must not synthesize memory events, allocation identities, return aliases, or control-flow interiors from source text.

### I5 — No build/proc-macro pollution

Build scripts, compiler plugins, proc macros, and host-only dependency executions must not be merged into the selected application's runtime ICFG merely because Cargo compiled them.

### I6 — Cross-crate call/return correctness

A represented dependency body must have:

- an exact call edge from the caller;
- an exact body entry;
- normal return flow back to the correct continuation;
- unwind flow where available under existing CREMA semantics.

No body may be globally connected to all call sites with the same textual name.

### I7 — Allocation identity continuity

Existing allocation/resource identity must survive:

- caller argument -> dependency formal;
- dependency return -> caller destination;
- dependency allocation -> caller-owned return;
- caller allocation -> dependency deallocation/use;

when already representable by CREMA's domain.

### I8 — No CQPL semantic compensation

CQPL checker semantics and the canonical 12 queries must not be changed to make DEP1 fixtures pass.

### I9 — Opt-in initial capability

During development DEP1 must be explicitly enabled, e.g.:

`--dependency-body-ingestion-v1`

With the flag disabled, the accepted pre-DEP1 behavior must remain byte/semantically equivalent modulo permitted environmental metadata.

### I10 — Provenance

Every ingested body must record enough provenance to audit:

- Cargo package identity;
- rustc crate identity;
- def-path identity;
- instance identity when applicable;
- acquisition mechanism;
- body availability;
- source span/source file when available;
- MIR phase/query used.

---

## 5. Phase DEP1-P0 — acquisition feasibility probe

**Do not implement graph semantics before this phase is complete.**

The agent must first determine how dependency MIR can be obtained on the pinned toolchain.

### 5.1 Synthetic probe project

Create a dedicated fixture family outside the production RustSec subject. At minimum:

- `app` binary crate;
- direct Rust library dependency `dep`;
- transitive Rust library dependency `dep2`.

`dep` must expose:

1. ordinary non-generic non-`#[inline]` function;
2. `#[inline]` function;
3. generic function instantiated from `app`;
4. trait method implementation called from `app`;
5. function calling into `dep2`.

The bodies must contain unique MIR operations that can be verified structurally without relying on names alone.

### 5.2 Probe A — standard Cargo metadata path

Under `nightly-2024-11-21`, from the selected application's rustc session:

1. resolve actual foreign callees using existing rustc semantic call resolution;
2. for each foreign `DefId`, test whether the pinned equivalent of `tcx.optimized_mir(def_id)` is legally available;
3. record success/failure separately for:
   - ordinary non-inline;
   - inline;
   - generic;
   - trait method;
   - transitive dependency.

Do not catch a compiler failure and silently reinterpret it as "no body". Produce structured evidence.

### 5.3 Probe B — encoded-MIR forcing

Check the pinned toolchain first:

```bash
rustup run nightly-2024-11-21 rustc -Z help | grep -E 'always.*encode.*mir|encode.*mir'
```

If supported, experimentally determine whether forcing MIR encoding for dependency compilations makes previously unavailable foreign MIR queryable.

Important: `cargo rustc -- <flags>` is not sufficient if the flag must affect dependencies. Verify Cargo propagation behavior using documented mechanisms (`RUSTFLAGS`, Cargo config, or a selective wrapper).

Do not permanently add such flags until their effects are measured.

### 5.4 Probe C — Cargo compiler-wrapper capture, only if needed

If foreign metadata remains incomplete, evaluate a Cargo-integrated rustc-wrapper approach.

Cargo officially supports `build.rustc-wrapper` / environment configuration for wrapping rustc invocations.

A wrapper strategy must:

- observe the exact rustc arguments Cargo chose;
- distinguish target-runtime crate compilations from build scripts/proc macros;
- preserve Cargo's build behavior;
- capture compiler-semantic MIR per dependency crate;
- record package/target/artifact identity;
- avoid recompiling a guessed approximation of Cargo's build command.

### 5.5 Required P0 result

Produce:

`repro-results/dep1-p0-<timestamp>/DEP1_P0_ACQUISITION_REPORT.json`

with a matrix such as:

```json
{
  "schema": "crema_dep1_p0_acquisition_v1",
  "toolchain": "nightly-2024-11-21",
  "standard_metadata": {
    "non_generic_non_inline": "available|unavailable|error",
    "inline": "available|unavailable|error",
    "generic": "available|unavailable|error",
    "trait_method": "available|unavailable|error",
    "transitive": "available|unavailable|error"
  },
  "forced_mir_encoding": {
    "supported_by_pinned_toolchain": true,
    "results": {}
  },
  "wrapper_capture_required": null,
  "recommended_architecture": "metadata|metadata_plus_forced_encoding|wrapper_capture|hybrid"
}
```

Stop for review after P0. Do not proceed to production implementation until the acquisition architecture is justified by evidence.

---

## 6. Preferred architecture order

Use the least invasive sound mechanism that satisfies coverage.

### Architecture A — foreign MIR from rustc metadata

Preferred when sufficient.

During traversal of a represented caller:

1. resolve the call to compiler-semantic target instance(s);
2. identify foreign `DefId`;
3. query foreign optimized MIR;
4. ingest the returned `mir::Body`;
5. recursively traverse newly represented calls.

Advantages:

- single compiler semantic universe;
- direct `DefId` linkage;
- no textual stitching;
- natural access to dependency metadata.

Limitation:

- optimized MIR may not have been encoded for all upstream items.

### Architecture B — force dependency MIR encoding

If the pinned compiler supports it and the experimental gate shows no unacceptable side effects, ensure dependencies are compiled with MIR needed by downstream queries.

Requirements:

- Cargo build configuration must remain reproducible;
- exact flags must be recorded;
- build/proc-macro effects must be audited;
- this mechanism must not silently change subject semantics;
- `cargo check` must not be substituted for a build when optimized MIR is required.

### Architecture C — rustc-wrapper capture

Use only if A/B cannot cover ordinary dependency bodies.

Each real Cargo rustc invocation becomes a producer run. The captured per-crate MIR must later be merged through stable semantic identities.

This is more complex and requires explicit proofs for:

- package/crate disambiguation;
- cross-session identity;
- generic instances;
- call/return binding;
- build-script/proc-macro exclusion;
- duplicate dependency versions.

### Hybrid architecture

A hybrid may use metadata for bodies available directly and wrapper capture for bodies unavailable in metadata, but it must have one canonical identity layer and deterministic precedence. Duplicate representations are forbidden.

---

## 7. Reachability policy

DEP1 must not blindly ingest every body from every package.

Initial v1 policy:

1. start from the already-selected CREMA entry roots;
2. inspect represented MIR calls;
3. resolve each call using compiler semantics;
4. if the callee body is available and allowed by policy, enqueue it;
5. iterate to fixed point;
6. deduplicate by canonical callable instance identity.

Record why each body entered the model:

```text
entry_root
reachable_direct_call
reachable_transitive_call
drop_glue_or_shim
other_compiler_required_body
```

Dynamic dispatch must remain fail-closed unless rustc supplies a finite certified target set already compatible with CREMA's existing semantics.

---

## 8. Proposed exported capability records

Do not commit to exact schema names before reviewing current v2 schema conventions, but the capability should expose equivalent structured evidence.

### 8.1 Body provenance

Conceptual record:

```json
{
  "capability": "dependency_body_ingestion_v1",
  "package_id": "...",
  "crate_identity": "...",
  "def_path": "...",
  "instance_identity": "...",
  "origin": "local|dependency_metadata|dependency_capture",
  "mir_query": "optimized_mir",
  "body_status": "represented|unavailable|opaque|unresolved",
  "source": "...",
  "entry_node": "..."
}
```

### 8.2 Cross-crate call binding

Conceptual record:

```json
{
  "call_node": "...",
  "caller_instance": "...",
  "callee_instance": "...",
  "callee_package_id": "...",
  "body_entry": "...",
  "normal_continuation": "...",
  "unwind_continuation": "...",
  "binding_kind": "exact"
}
```

The checker must consume producer-certified structure, not reconstruct bindings from strings.

---

## 9. Node and function identity

Current same-crate IDs such as:

`rust::main::bb4`

are insufficient for a multi-crate graph if dependency functions can collide.

DEP1 must introduce or extend a canonical identity containing a crate discriminator.

Conceptually:

```text
rust::<crate-identity>::<def-path-or-instance>::bbN
```

Exact encoding is an implementation decision, but must satisfy:

- deterministic under repeated runs;
- unique across multiple versions of the same crate;
- no absolute checkout paths in semantic identity;
- stable enough for baseline/candidate comparisons;
- derived from compiler/Cargo identity, not display strings.

Before changing IDs globally, evaluate compatibility impact on the 83-target frozen corpus. Prefer an opt-in DEP1 namespace if needed to preserve disabled-mode neutrality.

---

## 10. Required synthetic fixtures

Create a new fixture family such as:

`tests_and_target_repos/a-code_dependency_body_ingestion_gate/`

Minimum fixtures:

### d01 — direct non-generic dependency body

`app -> dep::f`.

Proves body ingestion and exact call/return binding.

### d02 — generic dependency instance

`app -> dep::f::<String>` or another concrete type.

Proves instance identity and no conflation of monomorphizations.

### d03 — trait method dependency call

Call a trait implementation living in the dependency.

Proves resolved instance binding beyond a simple free-function path.

### d04 — transitive dependency

`app -> dep1 -> dep2`.

Proves fixed-point cross-crate reachability.

### d05 — allocation returned by dependency

Dependency allocates an owned resource; caller later uses/frees it.

Proves allocation identity across dependency return.

### d06 — caller allocation consumed/deallocated in dependency

Caller creates/owns a resource; dependency consumes or destroys it.

Proves formal/actual and cross-crate lifecycle continuity.

### d07 — same crate name / distinct package versions

Two dependency instances that would collide under name-only matching.

Proves package/crate disambiguation.

### d08 — unavailable body control

A call whose body cannot legitimately be ingested.

Required result: explicit unavailable/opaque status and no fabricated body/events.

### d09 — build dependency / proc-macro exclusion

Cargo builds host-side code, but DEP1 must not merge it into the runtime application ICFG.

### d10 — feature/target-specific dependency resolution

Proves that the analyzed dependency set corresponds to the selected Cargo build configuration.

### d11 — dependency panic/unwind edge

If current CREMA panic lifecycle semantics support the case, prove cross-crate normal/unwind continuation correctness.

### d12 — bodyless C/FFI regression control

Proves that DEP1 does not convert genuine bodyless FFI into Rust dependency bodies and preserves ELE1 semantics.

---

## 11. Cross-crate allocation/resource tests

Body presence alone is not sufficient.

At least two fixtures must produce a resource whose identity crosses the crate boundary.

### Return-flow invariant

```text
dep allocation A
    -> dep return value
    -> caller destination
    -> caller use/drop of A
```

The same abstract allocation identifier must be observable across the return boundary.

### Argument-flow invariant

```text
caller allocation A
    -> actual argument
    -> dependency formal
    -> dependency use/deallocation of A
```

The same abstract allocation identifier must be observable inside the dependency body.

A fixture that merely shows both bodies in the ICFG does not satisfy DEP1.

---

## 12. Validation strategy

### Gate G0 — static/source protocol

Verify:

- capability opt-in;
- no query/checker semantic edits;
- no pretty-string callee matching;
- canonical crate/package identity;
- explicit body availability status;
- synthetic fixture manifest frozen.

### Gate G1 — unit/integration tests

Run complete CREMA and CQPL tests under `nightly-2024-11-21`.

### Gate G2 — disabled-mode neutrality

With DEP1 disabled, compare against parent commit:

`0169ce3da4e865e1750e22b9fc14631b0efcca98`

Required:

- frozen targets: `83`;
- canonical queries: `12`;
- cells: `996`;
- truth deltas: `0`;
- assessment deltas: `0`;
- query errors: `0`;
- semantic projection diffs: `0`;
- allocation identity projection diffs: `0`;
- D1 preservation failures: `0`;
- D2 preservation failures: `0`;
- D3 preservation failures: `0`;
- D4-P0 preservation failures: `0`;
- D4/ELE1 preservation failures: `0`;
- INFRA1 behavior preserved.

### Gate G3 — enabled-mode authorized behavior

DEP1-enabled execution must:

- pass all new d01-d12 fixtures;
- represent exactly the expected dependency bodies;
- preserve exact call/return bindings;
- preserve cross-crate allocation/resource identities;
- reject invalid/ambiguous body bindings;
- keep unavailable bodies fail-closed.

If existing frozen targets change under DEP1-enabled mode, every delta must be enumerated and justified by newly represented dependency MIR. No unexplained truth or assessment delta is acceptable.

### Gate G4 — adversarial mutations

Mutate exported evidence and require verification failure for at least:

- wrong Cargo package identity;
- wrong crate identity;
- wrong def-path identity;
- call bound to a different dependency body;
- duplicate body entry;
- same textual crate name but wrong version;
- missing normal continuation;
- fabricated body availability;
- body marked dependency but originating from build script/proc macro;
- allocation resource rebound across unrelated calls.

### Gate G5 — deterministic replay

Run representative dependency fixtures twice from different checkout roots and normalize only environmental paths.

Required semantic difference: `0`.

---

## 13. RustSec / real-world holdout policy

Do **not** use `RUSTSEC-2026-0128` / `emap` to tune DEP1 implementation.

The existing pilot result is historical motivation only.

The development sequence is:

```text
DEP1-P0 acquisition study
        ↓
architecture decision
        ↓
synthetic fixtures
        ↓
DEP1 implementation
        ↓
disabled-mode neutrality
        ↓
enabled-mode semantic gate
        ↓
independent audit
        ↓
DEP1 commit/freeze
        ↓
only then rerun emap
```

When `emap` is rerun, the primary question becomes:

- is `Keys::next` body now represented?
- is its vulnerable raw-pointer move/read represented?
- is `Map::get` body represented?
- can CREMA correlate the same resource across first destruction, later use, and second destruction?

Only after body coverage is established may the result be interpreted as detector capability (`P0-A` vs `P0-B`).

---

## 14. Performance and scale requirements

"Whole crate" must not mean uncontrolled graph explosion.

Collect for every DEP1 run:

- Cargo packages in resolved graph;
- dependency packages actually compiled;
- bodies available;
- bodies represented;
- bodies unavailable;
- local vs dependency nodes;
- cross-crate call bindings;
- fixed-point iterations;
- wall time;
- peak RSS if available;
- annotated ICFG node/edge counts.

Add guardrails only after measurement. Do not impose arbitrary depth limits that silently truncate semantics.

If a resource limit is introduced, it must fail explicitly as incomplete analysis rather than silently produce a complete-looking `ff`.

---

## 15. Failure semantics and completeness

DEP1 must expose analysis completeness.

At minimum distinguish:

```text
complete_for_reachable_supported_bodies
dependency_body_unavailable
dependency_body_capture_failed
dynamic_target_unresolved
resource_limit_reached
compiler_query_failed
```

A query over a graph known to omit a reachable dependency body must not be presented as evidence that the real program lacks the corresponding behavior without surfacing the model incompleteness.

Do not change existing CQPL three-valued semantics casually. First export completeness evidence at the producer layer; any checker-level use requires a separate reviewed protocol.

---

## 16. Implementation hygiene

During DEP1 development:

- no commits/pushes until a requested gate passes;
- no edits to the motivating RustSec subject;
- no source rewriting of third-party crates;
- no vendoring modifications solely to expose MIR;
- no `git add -A`;
- preserve exact toolchain;
- restore generated tracked artifacts after runs;
- delete generated `target/` directories only when ownership is clear;
- remove generated `__pycache__`;
- keep full logs and SHA-256 manifests.

---

## 17. First assignment to the code agent

The first agent task is **DEP1-P0 only**.

Do not ask the agent to implement the full capability in the first turn.

Use this instruction:

> Study how CREMA can obtain MIR bodies for Rust dependency callees under the pinned `nightly-2024-11-21` toolchain. Build synthetic app/lib/transitive fixtures and experimentally compare standard foreign `optimized_mir` queries, forced MIR metadata encoding if supported by the pinned compiler, and a Cargo rustc-wrapper capture design only if necessary. Produce a structured acquisition report. Do not modify analyzer semantics, CQPL queries, or the RustSec subject. Stop after recommending an acquisition architecture with evidence.

Required first-turn artifacts:

```text
cqpl/DEP1_DEPENDENCY_BODY_INGESTION_V1.md
cqpl/dep1_p0_fixture_manifest.json
cqpl/scripts/run_dep1_p0_acquisition_probe.sh
cqpl/scripts/verify_dep1_p0_acquisition.py
tests_and_target_repos/a-code_dependency_body_ingestion_gate/...
repro-results/dep1-p0-<timestamp>/...
```

The markdown protocol itself may be committed only after independent review.

---

## 18. DEP1-P0 acceptance criteria

P0 is accepted as a research result when:

1. exact parent commit and toolchain are recorded;
2. all probe fixtures build without source modification;
3. foreign-body availability is measured structurally;
4. ordinary non-inline, inline, generic, trait, and transitive cases are distinguished;
5. the pinned compiler's MIR-encoding options are empirically recorded;
6. Cargo flag propagation is measured rather than assumed;
7. wrapper capture is proposed only if metadata approaches are insufficient;
8. no analyzer/query semantics changed;
9. no RustSec target used to choose the architecture;
10. complete logs and checksums are preserved.

P0 acceptance does **not** imply DEP1 acceptance.

---

## 19. Final DEP1 acceptance statement

Only after all gates and independent review may the capability be described as:

```text
DEP1 / dependency_body_ingestion_v1: ACCEPTED
```

That statement must include:

- implementation commit;
- pinned toolchain;
- acquisition mechanism;
- number of new dependency fixtures;
- disabled-mode 83/996 neutrality result;
- enabled-mode fixture results;
- cross-crate resource identity results;
- body-unavailable fail-closed controls;
- adversarial mutation count;
- deterministic replay result;
- source and gate SHA-256 values.

Until then the capability is experimental.

---

## 20. References — official documentation

1. Cargo metadata<br>
   https://doc.rust-lang.org/cargo/commands/cargo-metadata.html

2. Cargo external tools and JSON build messages<br>
   https://doc.rust-lang.org/cargo/reference/external-tools.html

3. Cargo rustc<br>
   https://doc.rust-lang.org/cargo/commands/cargo-rustc.html

4. Cargo configuration (`rustflags`, `rustc-wrapper`)<br>
   https://doc.rust-lang.org/cargo/reference/config.html

5. rustc driver / compiler interface<br>
   https://rustc-dev-guide.rust-lang.org/rustc-driver/intro.html

6. MIR overview<br>
   https://rustc-dev-guide.rust-lang.org/mir/index.html

7. MIR queries and passes (`optimized_mir`, including foreign DefIds)<br>
   https://rustc-dev-guide.rust-lang.org/mir/passes.html

8. rustc query system and external crate metadata<br>
   https://rustc-dev-guide.rust-lang.org/query.html

9. Libraries and rustc metadata (`rlib`, `rmeta`, encoded MIR)<br>
   https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html

10. Monomorphization<br>
    https://rustc-dev-guide.rust-lang.org/backend/monomorph.html

11. nightly rustc `rustc_middle::mir` API<br>
    https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/

12. nightly rustc `Body` API<br>
    https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/struct.Body.html

13. nightly rustc metadata MIR encoding policy (`should_encode_mir`)<br>
    https://doc.rust-lang.org/nightly/nightly-rustc/rustc_metadata/rmeta/encoder/fn.should_encode_mir.html

14. nightly rustc unstable options (`always_encode_mir`)<br>
    https://doc.rust-lang.org/nightly/nightly-rustc/rustc_session/config/struct.UnstableOptions.html

---

## 21. Critical versioning note

The project is intentionally pinned to `nightly-2024-11-21`, while the online nightly rustc API documentation tracks newer compilers. `rustc_private` is explicitly unstable.

Therefore the code agent must verify all concrete APIs against the pinned toolchain before changing CREMA. Useful checks include:

```bash
rustup run nightly-2024-11-21 rustc --version --verbose
rustup run nightly-2024-11-21 rustc -Z help
rustup run nightly-2024-11-21 rustc --print sysroot
```

When source components are installed, inspect the pinned sysroot/compiler sources rather than assuming current nightly signatures.

The architectural principles in this protocol are stable requirements; exact rustc-private function signatures are not.
