# CQPL7 / DEP1-P1 — Production Dependency Body Ingestion

**Status:** implementation protocol  
**Capability:** `dependency_body_ingestion_v1`  
**Branch:** `cqpl7-dependency-body-ingestion`  
**Exact DEP1-P0 freeze:** `446d67db98b20043ec35d49ca042a1f7c86dc9f5`  
**Pinned toolchain:** `nightly-2024-11-21`  
**Rust compiler:** `rustc 1.84.0-nightly (3fee0f12e 2024-11-20)`  
**Date:** 2026-09-28

---

## 0. Scientific status inherited from DEP1-P0

DEP1-P0 established, on the pinned toolchain, that CREMA does **not** need a separate MIR-capture compiler session for each ordinary Rust dependency in the tested Cargo graph.

The accepted acquisition mechanism is:

```text
metadata_plus_forced_encoding
```

The controlled explicit-target replay established:

```text
                         standard          forced
ordinary                 unavailable       available
inline                   available         available
generic                  available         available
trait implementation     unavailable       available
transitive root          unavailable       available
transitive dep2          unresolved_call   available
```

The accepted acquisition chain is:

```text
Cargo target build
    with explicit --target
    and -Zalways-encode-mir=yes
            |
            v
dependency rmeta/rlib contains MIR
            |
            v
selected-target rustc session
            |
            v
MIR Call operand -> FnDef + generic args
            |
            v
Instance::try_resolve(...)
            |
            v
resolved Instance.def_id()
            |
            v
tcx.is_mir_available(def_id)
            |
            +-- false --> explicit unavailable/opaque state
            |
            `-- true  --> tcx.optimized_mir(def_id)
                              |
                              v
                        foreign MIR Body
```

DEP1-P1 shall integrate that mechanism into production CREMA.

The motivating RustSec/`emap` subject remains a held-out evaluation target and MUST NOT be used to choose implementation behavior.

---

## 1. Research question

### RQ-DEP1-P1

Can CREMA construct one semantically coherent interprocedural ICFG across the selected Cargo target and reachable Rust dependency bodies while preserving:

1. compiler-derived callable identity;
2. exact call/return linkage;
3. generic/trait instance resolution;
4. allocation/resource identity across crate boundaries;
5. fail-closed behavior for unavailable or unsupported bodies;
6. all previously accepted CREMA/CQPL semantics when DEP1 is disabled?

### Acceptance hypothesis

With `dependency_body_ingestion_v1` enabled, calls from the selected target into in-scope Rust dependencies become ordinary represented interprocedural calls whenever rustc metadata supplies the resolved callee MIR body.

With the capability disabled, CREMA's existing model must remain semantically unchanged.

---

## 2. Non-goals

DEP1-P1 is **not**:

- a RustSec-specific patch;
- an `emap` detector;
- a source-level Rust parser;
- a textual callee linker;
- a whole-standard-library analysis;
- a proc-macro/build-script analyzer;
- a dynamic-dispatch solver beyond targets already resolved by rustc;
- a replacement for ELE1 bodyless external-library effects;
- a CQPL checker-semantic change;
- a promise that all reachable Rust bodies are always available.

Do not edit the RustSec subject during P1.

Do not run the held-out RustSec pilot until DEP1-P1 has independently passed its gates.

---

## 3. Official compiler/Cargo basis

All concrete rustc-private signatures MUST be checked against `nightly-2024-11-21`. Current online nightly documentation is architectural guidance only.

### 3.1 Foreign MIR

The Rust Compiler Development Guide documents that `optimized_mir(def_id)` returns optimized MIR and that for a foreign `DefId` rustc reads the MIR from the other crate's metadata.

Official documentation:

https://rustc-dev-guide.rust-lang.org/mir/passes.html

### 3.2 Resolved callable instances

`rustc_middle::ty::Instance::try_resolve` resolves a `(DefId, GenericArgs)` pair to the precise callable instance where possible, including concrete trait implementations.

Official current-nightly API documentation:

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.Instance.html

The pinned toolchain API is authoritative.

### 3.3 Stable compiler identity

`DefId` and `CrateNum` are session-local identities. `DefPathHash` is intended to identify a definition across crate/session boundaries, and contains a `StableCrateId` component.

Official current-nightly API documentation:

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/index.html

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefPathHash.html

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.StableCrateId.html

### 3.4 Cargo dependency graph

Use machine-readable Cargo metadata, with explicit format version and platform filtering, to identify the resolved package graph and selected-target dependency context.

Official documentation:

https://doc.rust-lang.org/cargo/commands/cargo-metadata.html

### 3.5 Target-scoped rustflags

Cargo documents that, when an explicit `--target` is used, rustflags are passed to target compilations while host-side build scripts and proc macros do not receive those flags.

Official documentation:

https://doc.rust-lang.org/cargo/reference/config.html

https://doc.rust-lang.org/cargo/reference/build-cache.html

DEP1-P0 independently measured this property on the pinned toolchain.

---

## 4. Mandatory architecture

### 4.1 Feature gate

Production behavior must be opt-in.

Introduce a capability control equivalent to:

```text
--dependency-body-ingestion-v1
```

Exact CLI spelling may follow existing CREMA conventions, but the capability must have a stable structured name:

```text
dependency_body_ingestion_v1
```

Disabled mode is the regression baseline.

### 4.2 Acquisition mode

When DEP1 is enabled:

1. determine the selected Cargo target exactly as CREMA already does;
2. determine the explicit compilation target triple;
3. ensure target-unit dependency builds encode MIR using the pinned equivalent of:
   `-Zalways-encode-mir=yes`;
4. execute the selected target analysis in the real Cargo/rustc semantic environment;
5. use resolved foreign `Instance` identities;
6. query `tcx.is_mir_available(resolved_def_id)`;
7. only if available, query `tcx.optimized_mir(resolved_def_id)`;
8. recursively ingest supported reachable dependency bodies.

Do not call `optimized_mir` after `is_mir_available == false`; DEP1-P0 demonstrated that deliberately querying absent foreign MIR can poison the pinned rustc query context.

### 4.3 Cargo host isolation

DEP1 must use an explicit target when applying forced MIR encoding.

Do not apply `-Zalways-encode-mir=yes` indiscriminately to host-side build-script/proc-macro compilation.

If CREMA already receives an explicit target, preserve it.

If it does not, obtain the actual host target from the pinned compiler and pass it explicitly for the DEP1 Cargo build path.

Record the selected target triple in exported analysis metadata.

### 4.4 Existing rustflags

Do not silently discard user/project compiler flags.

Before implementing forced MIR encoding, inspect how CREMA currently launches Cargo and how the selected subject obtains rustflags from:

- `CARGO_ENCODED_RUSTFLAGS`;
- `RUSTFLAGS`;
- Cargo target-specific configuration;
- Cargo build configuration.

The implementation must either:

1. compose `-Zalways-encode-mir=yes` without changing the effective pre-existing flags; or
2. fail explicitly with a structured DEP1 configuration error when safe composition cannot be guaranteed.

Silent replacement is forbidden.

---

## 5. Scope policy: which crates may enter the ICFG

DEP1 must not recursively ingest everything rustc has loaded.

### 5.1 Cargo-derived package scope

Construct the dependency scope from:

```bash
cargo metadata --format-version 1 --filter-platform <selected-target>
```

and the exact selected package/target/features used for analysis.

The runtime scope must be derived from resolved **normal target dependencies** relevant to the selected target.

Do not include a package merely because Cargo compiled it.

### 5.2 Exclusions

Initial DEP1 v1 scope excludes unless CREMA already has an independently accepted policy:

- build dependencies;
- build-script bodies;
- proc-macro crates;
- host-only dependency units;
- compiler-internal/sysroot crates (`core`, `alloc`, `std`, compiler_builtins, etc.);
- C/FFI libraries represented through existing bodyless mechanisms.

Sysroot calls may remain represented as existing external summaries unless separately modeled.

### 5.3 Transitive runtime dependencies

Normal runtime dependencies of in-scope dependencies are eligible recursively.

The dependency traversal must reach a fixed point.

### 5.4 No name-based allow list

Production code MUST NOT contain logic equivalent to:

```text
crate_name == "dep"
crate_name == "dep2"
```

The P0 probe used synthetic names only to delimit an experiment. That policy is forbidden in production.

---

## 6. Cargo PackageId ↔ rustc crate identity

This is a first-class DEP1 requirement.

Maintain separate identities:

```text
Cargo:
    PackageId
    package version
    source
    manifest
    target/features

rustc:
    StableCrateId
    DefPathHash
    DefId (session-local only)
    Instance
```

Do not manufacture Cargo `PackageId` by parsing `tcx.crate_name()` or a pretty def path.

### Required correlation evidence

The implementation must establish a producer-certified relation between an in-scope Cargo package and the loaded rustc crate corresponding to its compiled target unit.

Acceptable inputs include Cargo's real artifact/compiler metadata and rustc crate metadata.

The exact correlation mechanism is an implementation decision, but the gate must prove that it distinguishes:

- two versions of the same package name;
- two crates with equal crate display names but different package/source identity;
- host/build units from target/runtime units.

If exact correlation cannot be proved for some loaded crate, its dependency body must remain out-of-scope or explicitly `unresolved_scope_identity`.

---

## 7. Callable identity

### 7.1 Session-local semantic key

Inside one rustc session, use the compiler's actual `Instance<'tcx>` / `DefId` values for traversal and visited-set semantics.

Do not use strings as the in-memory semantic key.

### 7.2 Persisted definition identity

Persistent exported definition identity must be based on compiler-stable identity, preferably the pinned-toolchain equivalent of:

```text
DefPathHash
```

rather than `CrateNum` or raw `DefId`.

`CrateNum` is session-local and must not be part of a persisted semantic ID.

### 7.3 Persisted instance identity

Generic and trait instances must not be accidentally conflated.

The agent must inspect the pinned compiler for a suitable compiler-derived deterministic representation/fingerprint of:

```text
InstanceKind + generic arguments
```

Requirements:

- no pretty-name matching;
- no `Debug` string used as the semantic lookup key;
- no source-text hashing;
- deterministic replay across checkout roots;
- distinct concrete generic instances remain distinguishable where semantics differ.

`Instance` debug strings may be exported as diagnostic fields only.

If a stable cross-run generic-instance identifier cannot be implemented soundly in this P1 step, stop for review rather than inventing one.

### 7.4 Human-readable names

`tcx.def_path_str`, crate names, and rendered generic arguments are diagnostic/provenance fields only.

They must never decide graph linkage.

---

## 8. Recursive body ingestion algorithm

Implement a compiler-semantic worklist.

Conceptually:

```text
worklist = selected local entry Instance(s)
visited  = {}

while worklist not empty:
    instance = pop(worklist)

    if instance in visited:
        continue

    visited += instance

    body_def = resolved_body_def(instance)

    if body_def is local:
        obtain existing local MIR path
    else:
        classify Cargo/rustc scope
        if not eligible:
            record intentionally_opaque/out_of_scope
            continue

        if !tcx.is_mir_available(body_def):
            record body_unavailable
            continue

        body = tcx.optimized_mir(body_def)

    ingest body under this Instance context

    for each MIR Call terminator:
        structurally extract FnDef + GenericArgs
        resolve with Instance::try_resolve
        classify exact/unresolved/virtual/unsupported
        enqueue supported exact callee instances
```

This pseudocode is normative in semantics, not in exact Rust API spelling.

### Important

For a resolved trait call, query the resolved implementation body:

```text
resolved_instance.def_id()
```

not the original trait declaration `FnDef`.

DEP1-P0 caught and corrected this exact failure mode.

---

## 9. Generic MIR and substitution context

`optimized_mir(def_id)` returns MIR for the definition; a concrete `Instance` couples that body with concrete arguments.

DEP1 must preserve the `Instance` context while interpreting the MIR.

Do not claim that rustc creates a separate fully monomorphized MIR body for every generic instance unless the pinned compiler proves otherwise.

Where CREMA requires types, call targets, drop glue, layouts, or other type-dependent facts, apply the concrete instance substitutions through compiler APIs.

Add a test with at least two different concrete instantiations of the same generic function and prove they do not become semantically conflated.

---

## 10. ICFG identity and node naming

Current same-crate labels such as:

```text
rust::main::bb4
```

must not collide when foreign bodies are added.

Introduce a canonical internal function/body identity containing at least:

```text
stable definition identity
instance identity
```

Basic-block identity becomes conceptually:

```text
<callable-instance-id>::bbN
```

Human-readable aliases may be included, for example:

```text
rust::<crate-name>::<def-path>::bbN
```

but aliases are not semantic keys.

### Backward compatibility

With DEP1 disabled, preserve existing IDs exactly.

Do not globally rename existing same-crate nodes in disabled mode merely to support DEP1.

Enabled-mode new cross-crate IDs may use a DEP1-specific canonical namespace if necessary.

---

## 11. Cross-crate call/return semantics

A dependency body is not accepted merely because its nodes appear in JSON.

For every represented exact cross-crate call, prove:

```text
caller call node
    -> callee entry

callee normal return
    -> caller normal continuation
```

and, where CREMA already represents it:

```text
callee unwind
    -> caller unwind continuation
```

Reuse existing same-crate call/return machinery where possible.

Do not create a parallel reduced-quality "dependency edge" model.

### Call binding record

Export structured producer-certified evidence equivalent to:

```json
{
  "capability": "dependency_body_ingestion_v1",
  "caller_instance_id": "...",
  "call_node": "...",
  "callee_instance_id": "...",
  "callee_def_path_hash": "...",
  "body_status": "represented",
  "body_entry_node": "...",
  "normal_continuation": "...",
  "unwind_continuation": null,
  "binding_kind": "exact"
}
```

Exact schema must follow existing CREMA conventions.

CQPL or validation tooling must consume structured fields, not re-derive the relation from display names.

---

## 12. Body availability/completeness states

Every reachable dependency call must be classifiable as one of:

```text
represented_body
body_unavailable
intentionally_opaque
out_of_scope
unresolved_instance
virtual_or_dynamic_unresolved
compiler_query_failed
resource_limit_reached
```

Do not collapse these into one generic `"external"` state internally.

Existing output fields may remain for backward compatibility, but DEP1-enabled output must contain sufficient structured evidence to distinguish these states.

A model with a reachable missing dependency body must not silently look complete.

---

## 13. Allocation/resource identity across crate boundaries

This is a mandatory semantic gate.

Body ingestion without resource continuity is insufficient.

### 13.1 Dependency allocation returned to caller

Fixture pattern:

```text
dependency:
    allocate resource A
    return ownership/value representing A

caller:
    receive result
    use/drop/deallocate A
```

Required invariant:

```text
dependency allocation identity A
    ==
caller-side resource identity after return binding
```

### 13.2 Caller allocation passed into dependency

Fixture pattern:

```text
caller:
    allocate/own resource A
    call dependency(A)

dependency:
    receive formal parameter
    use/move/drop/deallocate A
```

Required invariant:

```text
caller actual resource identity A
    ==
dependency formal resource identity A
```

### 13.3 Dependency-mediated UAF/double-destruction structure

At least one synthetic fixture must make a lifecycle path cross the crate boundary, e.g.:

```text
caller allocates A
    -> dependency destroys A
    -> caller later uses/destroys A
```

The gate is structural: prove same-resource identity across call/formal/return bindings.

Do not tune a CQPL truth value by adding special-case rules.

---

## 14. New production fixtures

Extend the DEP1 fixture family beyond the P0 acquisition probe.

At minimum preserve/add:

### D01 — direct ordinary dependency body

Prove exact foreign body representation and call/return.

### D02 — inline dependency body

Prove no regression when body was already available in ordinary metadata.

### D03 — two generic instances

Two calls to the same generic definition with distinct concrete types.

Prove instance separation.

### D04 — concrete trait implementation

Call a trait method; prove:

```text
operand trait DefId != resolved impl DefId
MIR query DefId == resolved impl DefId
```

### D05 — transitive dependency

```text
app -> dep -> dep2
```

Prove fixed-point traversal and return path.

### D06 — dependency allocates and returns resource

Prove resource identity through return.

### D07 — caller resource consumed/destroyed in dependency

Prove actual/formal resource identity.

### D08 — duplicate crate/package names or versions

Construct a graph in which name-only matching would be ambiguous.

Prove Cargo/rustc identity disambiguation.

### D09 — body unavailable control

Body remains unavailable/unsupported.

Prove no fabricated nodes/events and explicit completeness status.

### D10 — build script/proc-macro exclusion

Prove target/runtime scope does not ingest host-only code.

### D11 — sysroot exclusion

Prove ordinary `core`/`std` calls do not trigger uncontrolled whole-sysroot traversal.

### D12 — FFI/ELE1 control

Prove genuine bodyless C/FFI calls remain under existing ELE1/bodyless semantics.

---

## 15. Opt-in neutrality requirement

The DEP1 implementation must not alter accepted behavior when disabled.

Frozen comparison parent:

```text
446d67db98b20043ec35d49ca042a1f7c86dc9f5
```

Required disabled-mode regression gate:

```text
targets:                  83
canonical CQPL queries:   12
query cells:              996

truth deltas:             0
assessment deltas:        0
query errors:             0
semantic projection diffs: 0
allocation identity diffs: 0

D1 preservation failures:     0
D2 preservation failures:     0
D3 preservation failures:     0
D4-P0 preservation failures:  0
D4/ELE1 preservation failures:0
INFRA1 behavior preserved
```

If existing test counts have changed only because DEP1 tests are added, report baseline and candidate counts explicitly rather than comparing raw totals blindly.

---

## 16. Enabled-mode gate

With DEP1 enabled on the synthetic dependency fixture family, require:

- direct dependency body represented;
- inline body represented;
- generic instances handled without conflation;
- trait implementation resolved to concrete impl body;
- transitive `dep2` body represented;
- exact caller -> callee entry linkage;
- exact callee return -> correct continuation linkage;
- resource identity preserved through argument flow;
- resource identity preserved through return flow;
- unavailable body fail-closed;
- build/proc-macro body excluded;
- sysroot traversal bounded by policy;
- FFI behavior preserved.

The verifier must inspect structured semantic fields.

Do not pass fixtures by grepping pretty names alone.

---

## 17. Adversarial validation

Create mutation tests that must fail verification.

At minimum mutate:

1. StableCrateId/package correlation;
2. DefPathHash;
3. resolved trait implementation ID;
4. generic-instance identifier;
5. caller call node;
6. body entry node;
7. return continuation;
8. transitive dep2 binding;
9. body availability status;
10. package version for duplicate-name fixture;
11. build-script classification;
12. sysroot classification;
13. actual/formal resource binding;
14. return-resource binding;
15. allocation identity;
16. body provenance acquisition mode.

Mutation verification must be structural and independent of display names.

---

## 18. Determinism

Run representative enabled DEP1 fixtures from two checkout roots.

Normalize only environmental path strings.

Require:

```text
stable callable identity differences: 0
cross-crate call binding differences: 0
body availability differences: 0
resource identity projection differences: 0
```

Do not normalize away:

- package identity;
- StableCrateId;
- DefPathHash;
- instance identity;
- call/return binding;
- allocation/resource IDs.

---

## 19. Performance evidence

For each enabled run collect:

```text
Cargo resolved packages
eligible runtime packages
loaded rustc crates
represented local bodies
represented dependency bodies
unavailable dependency bodies
out-of-scope bodies
unresolved calls
cross-crate exact bindings
ICFG node count
ICFG edge count
fixed-point worklist iterations
wall time
peak RSS if practical
```

Do not introduce arbitrary recursion depth limits.

If a safety bound is required, reaching it must produce an explicit incomplete-analysis state.

---

## 20. Implementation constraints

### Production files may change

Unlike P0, P1 is allowed to modify production CREMA files.

However:

- keep changes minimal and capability-gated;
- do not modify CQPL query semantics;
- do not modify the 12 canonical queries;
- do not edit RustSec/emap;
- do not add special cases for package names;
- do not ingest `std`/`core` indiscriminately;
- do not parse source to reconstruct MIR semantics;
- do not use pretty strings for binding;
- do not commit;
- do not push.

Stop for independent review after producing the P1 candidate and gates.

---

## 21. Suggested production decomposition

The exact module layout should follow the current codebase rather than forcing a redesign, but keep responsibilities separated conceptually.

### Cargo analysis context

Responsible for:

- selected package/target/features;
- target triple;
- resolved runtime PackageId graph;
- excluded host/build/proc-macro units;
- target-scoped forced-MIR build configuration.

### Compiler identity layer

Responsible for:

- `DefId` -> `DefPathHash`;
- `CrateNum` -> `StableCrateId`;
- resolved `Instance`;
- deterministic instance identity/provenance.

### Dependency body provider

Responsible for:

```text
scope check
is_mir_available
optimized_mir
body status
body provenance
```

### ICFG ingestion layer

Responsible for:

- converting each represented instance body using existing MIR semantics;
- creating canonical cross-crate node IDs;
- exact call/return binding;
- recursive worklist.

### Resource binding layer

Prefer reusing existing formal/actual/return resource propagation rather than introducing a second dependency-only abstraction.

---

## 22. First implementation sequence for the code agent

The agent should implement in this order.

### Step A — inventory and design note

Before production edits:

- record exact HEAD and branch;
- confirm clean working tree;
- run `git diff --check`;
- record pinned compiler/cargo/python;
- hash production files expected to change;
- map current Cargo invocation path;
- map current MIR body enumeration;
- map current call resolution;
- map current function/node ID generation;
- map current call/return edge generation;
- map current allocation/resource formal/actual/return propagation.

Write:

```text
repro-results/dep1-p1-<timestamp>/P1_DESIGN_MAP.md
```

Do not start by copying P0 probe code wholesale.

### Step B — feature gate + acquisition plumbing

Implement the opt-in flag and target-scoped forced MIR encoding.

At this point, disabled-mode tests must still pass before proceeding.

### Step C — stable identity + body provider

Implement structured crate/definition/instance identity and body availability/provenance.

Unit-test this before graph recursion.

### Step D — recursive body collection

Add fixed-point traversal for eligible Cargo runtime dependencies.

Do not yet claim success if bodies are disconnected from the caller.

### Step E — ICFG call/return integration

Merge dependency bodies using the same interprocedural semantics as local bodies.

### Step F — cross-crate resource identity

Add/repair formal/actual and return binding needed by D06/D07.

### Step G — enabled fixtures + adversarial verifier

Run D01-D12 and mutations.

### Step H — frozen disabled-mode neutrality

Run the full 83 × 12 gate against exact freeze `446d67db...`.

Stop for review.

---

## 23. Required P1 artifacts

Create a timestamped directory:

```text
repro-results/dep1-p1-<timestamp>/
```

Preserve at minimum:

```text
P1_DESIGN_MAP.md
DEP1_P1_REPORT.json
DEP1_P1_ENABLED_FIXTURES.json
DEP1_P1_DISABLED_NEUTRALITY.json
DEP1_P1_IDENTITY_REPORT.json
DEP1_P1_RESOURCE_IDENTITY_REPORT.json
DEP1_P1_COMPLETENESS_REPORT.json
DEP1_P1_ADVERSARIAL_REPORT.json
DEP1_P1_DETERMINISM_REPORT.json

environment.txt
git-status-before.txt
git-status-after.txt
git-diff-check.txt
source-hashes-before.json
source-hashes-after.json

complete logs
fixture outputs
verifier outputs
SHA256SUMS
```

Do not add `repro-results/` to Git unless separately authorized.

---

## 24. Required report fields

`DEP1_P1_REPORT.json` must state:

```text
schema
capability
parent_freeze
candidate_tree_or_commit_if_any
toolchain

feature_gate
target_triple
forced_mir_encoding_method

cargo_scope:
    selected_package
    selected_target
    runtime_packages
    excluded_build_packages
    excluded_proc_macro_packages

identity:
    crate_identity_mechanism
    definition_identity_mechanism
    instance_identity_mechanism
    package_crate_correlation_mechanism

body_ingestion:
    local_bodies
    dependency_bodies
    unavailable
    opaque
    unresolved
    out_of_scope

call_return:
    exact_cross_crate_bindings
    invalid_bindings
    return_binding_failures

resource_identity:
    argument_flow_pass
    return_flow_pass
    lifecycle_cross_boundary_pass

disabled_neutrality:
    targets
    query_cells
    truth_deltas
    assessment_deltas
    query_errors
    semantic_projection_diffs
    allocation_identity_diffs

enabled_fixtures:
    pass
    fail

adversarial_mutations:
    total
    rejected

determinism:
    semantic_diffs

analyzer_semantic_modifications:
    list

acceptance:
    PASS|FAIL|INCOMPLETE
```

---

## 25. P1 acceptance criteria

DEP1-P1 can be accepted only if all of the following are true:

1. exact parent freeze is `446d67db98b20043ec35d49ca042a1f7c86dc9f5`;
2. pinned nightly is used;
3. DEP1 is opt-in;
4. disabled-mode frozen corpus has zero semantic/query deltas;
5. forced MIR uses an explicit target;
6. runtime dependency scope derives from Cargo metadata, not crate names;
7. Cargo PackageId and rustc identity remain distinct and are explicitly correlated;
8. call targets use compiler semantic resolution;
9. trait calls query the resolved implementation body;
10. dependency traversal reaches a transitive fixed point;
11. generic instances are not semantically conflated;
12. persistent definition identity is not based on session-local `CrateNum`/raw `DefId`;
13. exact cross-crate call/return bindings are represented;
14. argument resource identity crosses crate boundaries;
15. return resource identity crosses crate boundaries;
16. unavailable bodies fail closed;
17. build/proc-macro/sysroot exclusions are proven;
18. FFI/ELE1 behavior is preserved;
19. adversarial mutations are rejected;
20. deterministic replay has zero semantic identity/binding/resource diffs;
21. no RustSec/emap tuning occurred;
22. no commit/push occurred before independent review.

A partial implementation must report `INCOMPLETE`, not `PASS`.

---

## 26. Hold-out evaluation rule

Only after DEP1-P1 is independently accepted and frozen may the RustSec `emap` pilot be restored and rerun.

At that point the evaluation must first establish model coverage:

```text
Keys::next body represented?
ptr::read operation represented?
Map::get body represented?
first destruction represented?
same-resource later use represented?
same-resource second destruction represented?
```

Only after these are represented is `P0-A` vs `P0-B` a meaningful detector question.

Do not change DEP1 based on the held-out result.

---

## 27. First prompt to the code agent

Give the agent:

> Read `cqpl/DEP1_DEPENDENCY_BODY_INGESTION_V1.md` and `cqpl/DEP1_P1_PRODUCTION_IMPLEMENTATION.md`. Work from exact freeze `446d67db98b20043ec35d49ca042a1f7c86dc9f5` on branch `cqpl7-dependency-body-ingestion`. Implement DEP1-P1 as an opt-in production capability following the accepted `metadata_plus_forced_encoding` acquisition architecture. Begin by producing the required design map from the existing CREMA Cargo invocation, MIR traversal, call-resolution, node-identity, call/return, and allocation/resource propagation code. Then implement the capability in the prescribed order. Use compiler-derived identities and Cargo-resolved runtime scope; do not use pretty names or package-name allow lists. Preserve disabled-mode neutrality, add the cross-crate semantic/resource fixtures and adversarial verifier, do not run RustSec/emap, do not modify CQPL semantics, do not commit, and do not push. Stop for independent review with the complete timestamped P1 evidence package.

---

## 28. Critical stop conditions

Stop and report instead of improvising if any of these occur:

- Cargo PackageId ↔ rustc crate correlation cannot be made unambiguous;
- stable instance identity cannot be constructed without text matching;
- forced MIR encoding would silently replace existing user rustflags;
- a dependency body cannot be processed under its concrete Instance substitutions;
- call/return integration would require changing CQPL semantics;
- allocation/resource identity cannot reuse or soundly extend existing interprocedural propagation;
- enabled DEP1 causes unexplained changes outside the newly represented dependency scope;
- disabled mode changes any frozen truth/assessment/query result.

These are architecture questions, not bugs to hide.

---

## 29. Documentation references

Official sources:

1. Rust Compiler Development Guide — MIR queries and passes  
   https://rustc-dev-guide.rust-lang.org/mir/passes.html

2. Rust nightly API — `rustc_middle::ty::Instance`  
   https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.Instance.html

3. Rust nightly API — `rustc_span::def_id`  
   https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/index.html

4. Rust nightly API — `DefPathHash`  
   https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.DefPathHash.html

5. Rust nightly API — `StableCrateId`  
   https://doc.rust-lang.org/nightly/nightly-rustc/rustc_span/def_id/struct.StableCrateId.html

6. Cargo Book — `cargo metadata`  
   https://doc.rust-lang.org/cargo/commands/cargo-metadata.html

7. Cargo Book — configuration / rustflags  
   https://doc.rust-lang.org/cargo/reference/config.html

8. Cargo Book — build cache and target/host split  
   https://doc.rust-lang.org/cargo/reference/build-cache.html

The exact rustc-private APIs available in the pinned `nightly-2024-11-21` toolchain are authoritative over current online nightly documentation.
