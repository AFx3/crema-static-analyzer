# CQPL7 / DEP1-P1-R1 — Resume Production Implementation After ID0

**Status:** production continuation protocol  
**Capability:** `dependency_body_ingestion_v1`  
**Branch:** `cqpl7-dependency-body-ingestion`  
**Semantic neutrality baseline:** `446d67db98b20043ec35d49ca042a1f7c86dc9f5`  
**Pinned toolchain:** `nightly-2024-11-21`  
**Date:** 2026-09-28

---

## 0. Accepted prerequisite

DEP1-P1-ID0 is accepted.

The accepted run is:

```text
repro-results/dep1-p1-id0-20260928T173000Z
```

Archive SHA256:

```text
1383475255e3688246f4bb57ec815d6370a243a84f44bb5213d50b259f5643ff
```

ID0 established on the pinned compiler:

```text
tcx.used_crates(())
tcx.used_crate_source(CrateNum)
tcx.stable_crate_id(CrateNum)
```

with `CrateSource` exposing concrete artifact paths.

It also established an exact producer-side correlation:

```text
rustc loaded CrateNum
    -> CrateSource exact artifact path
    -> Cargo compiler-artifact.filenames exact path
    -> unique Cargo artifact record
    -> unique Cargo PackageId
```

The accepted synthetic result contained:

```text
loaded rustc crates:              24
exact Cargo correlations:          5
explicit sysroot exclusions:      19
unresolved/ambiguous:              0
adversarial mutations rejected:  8/8
```

The exact-path correlation passed for:

- direct dependency;
- transitive dependency;
- renamed dependency;
- duplicate package versions;
- proc macro identity/exclusion;
- build dependency exclusion;
- sysroot exclusion.

The artifact path is a run-local provenance join key only. It is not a persistent semantic callable identity.

---

## 1. Production consequence

The former P1 hard stop:

```text
Cargo PackageId <-> rustc loaded crate identity unresolved
```

is CLOSED.

Production DEP1 may now resume, but must implement the accepted ID0 relation rather than substitute a weaker heuristic.

The production identity stack is:

```text
Cargo PackageId
      ^
      | exact compiler artifact path join
      |
rustc CrateSource(CrateNum)
      |
      v
StableCrateId
      |
      v
DefPathHash
      |
      v
resolved Instance
```

Keep each layer distinct.

---

## 2. Baselines

### Semantic neutrality baseline

All disabled-mode semantic comparison remains against:

```text
446d67db98b20043ec35d49ca042a1f7c86dc9f5
```

A later protocol-only commit does not replace this semantic baseline.

### Toolchain

Use exactly:

```text
nightly-2024-11-21
rustc 1.84.0-nightly (3fee0f12e 2024-11-20)
cargo 1.84.0-nightly (66221abde 2024-11-19)
```

---

## 3. Required production acquisition flow

DEP1-enabled Cargo analysis shall conceptually perform:

### Phase A — dependency artifact catalog

Run the selected Cargo build configuration with:

- exact selected package/target/features/profile;
- explicit target triple;
- target-side MIR encoding enabled;
- CREMA wrapper executable path identical to Phase B;
- analysis callback disabled for the selected target;
- `--message-format=json-render-diagnostics`.

Parse Cargo `compiler-artifact` messages and build:

```text
exact original/canonical artifact path
    -> Cargo artifact unit
    -> Cargo PackageId
```

Do not scan `target/`.

### Intermediate selected-package invalidation

Force recompilation of the selected package while preserving dependency artifacts.

The accepted ID0 implementation used the pinned Cargo equivalent of:

```text
cargo clean -p <selected-package>
    --target <selected-target>
    --target-dir <same-target-dir>
```

Production code must validate the selected-package identity it passes to Cargo.

If this cannot be done unambiguously for the selected plan, fail explicitly.

### Phase B — real analysis callback

Run the same Cargo plan with:

- same target dir;
- same explicit target;
- same feature/profile selection;
- same wrapper executable path;
- same dependency MIR encoding configuration;
- selected-target CREMA analysis enabled;
- read-only Phase A artifact catalog available to the callback.

For loaded crates, correlate `CrateSource` paths to the catalog.

No name fallback is permitted.

---

## 4. Rustflags requirement remains unresolved until production code proves it

ID0 proved target/host scoping for the test environment.

It did NOT by itself prove safe composition with arbitrary existing project/user rustflags.

Before production DEP1 is accepted, implement and test a method that preserves the subject's existing effective rustflags while adding the pinned equivalent of:

```text
-Zalways-encode-mir=yes
```

for target-side compilations only.

Required controls include at least:

- no preexisting rustflags;
- preexisting `RUSTFLAGS`;
- preexisting `CARGO_ENCODED_RUSTFLAGS`;
- Cargo `build.rustflags`;
- target-specific Cargo rustflags.

The verifier must demonstrate that preexisting flags remain effective.

If Cargo precedence prevents safe composition in a configuration, return a structured DEP1 configuration error rather than silently replacing flags.

Do not assume that setting a new `RUSTFLAGS` environment variable is safe.

---

## 5. Cargo runtime scope

Use:

```text
cargo metadata --format-version 1 --filter-platform <selected-target>
```

with the selected feature/target context.

Compute the reachable normal runtime closure from the selected package through structured:

```text
resolve.nodes[].deps[].pkg
resolve.nodes[].deps[].dep_kinds[]
```

Correlation and eligibility remain separate:

```text
loaded rustc crate
    -> exact PackageId correlation

PackageId
    -> Cargo graph eligibility
```

A correlated crate can still be ineligible, e.g. proc macro.

Exclude:

- build dependencies;
- proc-macro body ingestion;
- host-only units;
- sysroot/compiler crates;
- FFI/bodyless libraries already governed by existing semantics.

---

## 6. Production correlation object

Introduce an internal structured record equivalent to:

```text
CargoRustcCrateCorrelation {
    package_id
    manifest_path
    cargo_target
    cargo_target_kind
    cargo_crate_types
    features/profile provenance

    crate_num              // session-local, internal only
    stable_crate_id
    crate_source_paths
    matched_artifact_path

    runtime_eligible
    scope_reason
}
```

Do not make `crate_num`, crate display name, or artifact path a persisted semantic callable ID.

Export provenance fields as needed for verification.

---

## 7. Callable identity

### Definition identity

Use the pinned compiler equivalent of `DefPathHash` for persisted definition identity.

The verifier must prove:

```text
same definition, replay A/B -> same identity
duplicate package versions -> distinct definition namespace
```

Do not persist raw `DefId` or `CrateNum` as stable identity.

### Instance identity

Traversal itself must use real rustc `Instance<'tcx>` semantics.

For persisted concrete instance identity, inspect pinned compiler facilities first.

The ID must distinguish at least:

- `generic::<u32>`;
- `generic::<u64>`;
- concrete trait implementation instances.

Do not use `Debug`/pretty strings as semantic lookup keys.

If no deterministic compiler-derived instance identity can be implemented soundly, STOP for review.

---

## 8. Body provider

For every semantically resolved dependency callee:

```text
resolved_instance = Instance::try_resolve(...)

query_def_id = resolved_instance.def_id()
```

Then:

```text
scope correlation succeeds?
runtime eligible?
is_mir_available(query_def_id)?
```

Only if all pass:

```text
tcx.optimized_mir(query_def_id)
```

Body status must be structured:

```text
represented_body
body_unavailable
out_of_scope
unresolved_scope_identity
unresolved_instance
virtual_or_dynamic_unresolved
compiler_query_failed
resource_limit_reached
```

Do not query the original trait declaration when the resolved instance points at a concrete impl.

---

## 9. Fixed-point traversal

Extend the existing compiler-semantic call traversal rather than introducing a string-based second call graph.

Worklist semantic key:

```text
real rustc Instance
```

For each represented instance:

1. ingest body under concrete instance context;
2. inspect MIR `Call` terminators structurally;
3. resolve `FnDef + GenericArgs` through rustc;
4. classify scope/body status;
5. enqueue exact eligible represented callees;
6. continue to fixed point.

Do not recurse into every loaded/sysroot crate.

---

## 10. ICFG integration

Dependency body nodes must enter the existing Rust ICFG machinery.

For each exact represented cross-crate call require:

```text
caller call
    -> existing dummyCall/call edge mechanism
    -> dependency callee entry

dependency normal return
    -> existing dummyRet/return mechanism
    -> exact caller continuation
```

Where existing semantics represent unwind, preserve that path too.

Do not create a weaker dependency-only edge model.

Disabled mode must preserve existing node IDs exactly.

Enabled-mode cross-crate bodies require collision-free identity based on stable definition + instance identity, with pretty names diagnostic only.

---

## 11. Resource identity

The production continuation is not accepted until resource identity survives crate boundaries.

Required fixtures:

### Return-flow

```text
dep:
    allocate A
    return A

caller:
    receive A
    later use/drop/deallocate A
```

Prove the same allocation identity crosses the return binding.

### Argument-flow

```text
caller:
    own/allocate A
    dep.consume(A)

dep:
    formal receives A
    use/drop/deallocate A
```

Prove actual/formal identity equality.

### Lifecycle crossing

At least one structural path:

```text
caller alloc A
    -> dependency destroys A
    -> caller later use/destroy A
```

Do not introduce CQPL special cases.

---

## 12. Required production fixtures

Implement or preserve:

```text
D01 direct ordinary dependency body
D02 inline dependency
D03 two concrete generic instances
D04 concrete trait implementation
D05 app -> dep -> dep2
D06 dependency allocation returned to caller
D07 caller allocation consumed/destroyed by dependency
D08 duplicate package names/versions
D09 body unavailable control
D10 build/proc-macro exclusion
D11 sysroot exclusion
D12 FFI/ELE1 preservation
```

D08 must exercise the ID0 production correlation mechanism, not a special fixture-only mapping.

---

## 13. Required validation order

Run gates in this order.

### Gate A — production unit tests

Run CREMA/CQPL tests after each architectural stage.

### Gate B — disabled-mode neutrality

Compare against exact semantic baseline:

```text
446d67db98b20043ec35d49ca042a1f7c86dc9f5
```

Require:

```text
83 targets
12 canonical queries
996 cells

truth deltas = 0
assessment deltas = 0
query errors = 0
semantic projection diffs = 0
allocation identity diffs = 0

D1 preservation failures = 0
D2 preservation failures = 0
D3 preservation failures = 0
D4-P0 preservation failures = 0
D4/ELE1 preservation failures = 0
INFRA1 preserved
```

### Gate C — enabled synthetic fixtures

Require D01-D12 structural checks.

### Gate D — adversarial

Mutate at minimum:

- Cargo artifact path;
- PackageId;
- StableCrateId;
- DefPathHash;
- concrete instance identity;
- trait resolved impl;
- body entry;
- call continuation;
- transitive dep2 binding;
- runtime eligibility;
- argument resource binding;
- return resource binding;
- allocation identity;
- body availability/provenance.

### Gate E — deterministic replay

Two checkout roots.

Normalize environmental path strings only.

Require zero semantic differences for:

- Cargo/rustc correlation;
- stable definition identity;
- instance identity;
- call/return binding;
- resource identity.

---

## 14. Evidence package

Continue using:

```text
repro-results/dep1-p1-<new timestamp>/
```

or create a fresh P1 production run directory.

Preserve:

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

DEP1_P1_CARGO_RUSTC_CORRELATION.json
DEP1_P1_RUSTFLAGS_COMPOSITION.json

raw Cargo compiler-artifact streams
Cargo metadata
structured correlation outputs
complete logs
source hashes
SHA256SUMS
```

`SHA256SUMS` must be package-root-relative.

Do not add `repro-results/` to Git.

---

## 15. Hold-out rule

Do not restore or run the RustSec/emap subject during production P1 development.

Only after independent DEP1-P1 acceptance may the held-out subject be rerun.

---

## 16. Stop conditions

STOP rather than weaken semantics if:

- exact Cargo artifact ↔ CrateSource join is missing/ambiguous for an eligible ordinary dependency;
- selected-package Phase A/B invalidation cannot preserve dependency artifact identity;
- safe rustflags composition cannot be proved;
- deterministic concrete instance identity cannot be established;
- generic instance substitutions cannot be preserved;
- exact call/return integration cannot reuse sound existing interprocedural semantics;
- resource identity cannot cross call/return crate boundaries;
- disabled mode changes frozen semantics;
- enabled mode changes unrelated bodyless FFI semantics.

Report `INCOMPLETE`.

---

## 17. Agent execution instruction

Resume DEP1-P1 production implementation.

Use:

- `cqpl/DEP1_DEPENDENCY_BODY_INGESTION_V1.md`;
- `cqpl/DEP1_P1_PRODUCTION_IMPLEMENTATION.md`;
- `cqpl/DEP1_P1_ID0_PACKAGE_CRATE_CORRELATION.md`;
- this continuation protocol.

Treat ID0's exact artifact-path join as accepted prerequisite evidence.

Do not redo acquisition architecture research unless production behavior contradicts the accepted evidence.

Do not commit or push.

Stop for independent review with the full P1 production candidate and evidence package.
