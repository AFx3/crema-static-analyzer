# CQPL7 / DEP1-P1-ID0 — Cargo PackageId ↔ rustc Crate Identity Correlation

**Status:** feasibility/closure protocol before DEP1-P1 production implementation  
**Capability:** `dependency_body_ingestion_v1`  
**Branch:** `cqpl7-dependency-body-ingestion`  
**Exact DEP1-P0 freeze:** `446d67db98b20043ec35d49ca042a1f7c86dc9f5`  
**Pinned toolchain:** `nightly-2024-11-21`  
**Date:** 2026-09-28

---

## 0. Motivation

DEP1-P1 correctly stopped before production edits because the current CREMA selected-target rustc callback does not yet receive a producer-certified mapping from rustc-loaded dependency crates to Cargo `PackageId`s.

This is a valid hard stop, but it is not evidence that such a mapping cannot be established.

Official interfaces expose two complementary producer-side facts:

1. Cargo `--message-format=json` emits a `compiler-artifact` record containing:
   - `package_id`;
   - `manifest_path`;
   - Cargo target identity/kind/crate types;
   - enabled features/profile;
   - exact artifact `filenames`;
   - `fresh`.

2. rustc exposes, for a loaded `CrateNum`, the source artifact from which that crate's metadata/code was loaded (`CrateSource`, via the pinned-toolchain equivalent of `used_crate_source`), with paths such as `rlib`, `rmeta`, or dylib.

The hypothesis to test is therefore an exact run-local join:

```text
Cargo compiler-artifact filename
        ==
rustc loaded CrateSource artifact path
```

This path equality is used only to correlate a loaded compiler crate to the Cargo unit/package that produced the exact artifact. It is not a persistent semantic function identity.

The persistent rustc identity remains based on compiler identity such as `StableCrateId` / `DefPathHash` / resolved `Instance`.

---

## 1. Research question

### RQ-ID0

Can every relevant non-sysroot rustc crate loaded by the selected target be correlated unambiguously to exactly one Cargo target-unit/package record by joining the exact loaded `CrateSource` artifact path with Cargo's `compiler-artifact.filenames` stream?

### Acceptance hypothesis

For eligible runtime Rust dependency crates:

```text
loaded CrateNum
  -> rustc CrateSource path
  -> exact Cargo compiler-artifact filename
  -> unique Cargo compiler-artifact record
  -> unique Cargo PackageId
```

and this remains correct for:

- direct dependencies;
- transitive dependencies;
- renamed dependencies;
- duplicate package names/versions/sources;
- build dependencies;
- proc macros;
- sysroot crates.

---

## 2. Official basis

Current official documentation is architectural guidance. Exact APIs MUST be checked against `nightly-2024-11-21`.

### Cargo JSON compiler artifacts

Cargo's JSON message stream emits one `compiler-artifact` object per compilation step. The object includes the unique Cargo `package_id`, `manifest_path`, target information, feature/profile information, and exact generated `filenames`.

Official documentation:

https://doc.rust-lang.org/cargo/reference/external-tools.html

### Cargo resolved dependency graph

`cargo metadata --format-version 1 --filter-platform <target>` exposes PackageIds and the resolved dependency graph. `resolve.nodes[].deps[].dep_kinds[]` distinguishes normal, build, and dev dependency edges.

Official documentation:

https://doc.rust-lang.org/cargo/commands/cargo-metadata.html

### rustc loaded crate source

Current rustc-private documentation exposes `used_crate_source(CrateNum)` returning a `CrateSource`, and `CrateSource` records where the loaded crate came from on the local filesystem (`rlib`, `rmeta`, dylib, etc.).

Official current-nightly documentation:

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/queries/used_crate_source/index.html

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_session/cstore/struct.CrateSource.html

The code agent MUST inspect the pinned compiler source/API and record the exact 2024 signature.

### Loaded crate enumeration

Current rustc exposes `used_crates(())` as the set of crates loaded non-speculatively. Verify the pinned equivalent.

Current-nightly reference:

https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/ty/struct.TyCtxt.html

---

## 3. Scope of this phase

DEP1-P1-ID0 is an **identity-correlation experiment only**.

Do not implement dependency-body ingestion in production CREMA yet.

Do not modify:

- CREMA ICFG semantics;
- allocation/resource semantics;
- CQPL checker semantics;
- canonical queries;
- RustSec/emap.

Production files may be inspected and hashed, but the experiment should use research-only tooling/fixtures.

Stop after the identity-correlation decision.

---

## 4. First check: pinned rustc API

Before designing the probe, inspect `nightly-2024-11-21` compiler sources and compile a minimal rustc-private test for the exact equivalents of:

```text
tcx.used_crates(())
tcx.used_crate_source(cnum)
tcx.stable_crate_id(cnum)
tcx.crate_name(cnum)
```

and the pinned `CrateSource` path fields/methods.

Record:

```text
API present
exact signature
exact return types
CrateSource fields/path behavior
```

If rustc cannot expose the exact artifact source path for loaded foreign crates on the pinned compiler, stop and report ID0 failure before attempting a weaker mapping.

Do not replace artifact identity with crate names.

---

## 5. Cargo artifact catalog

Run Cargo with:

```text
--message-format=json-render-diagnostics
```

under the same:

- manifest;
- package;
- target;
- features;
- profile;
- explicit target triple;
- forced MIR configuration

that DEP1 would use.

Parse only Cargo JSON messages whose:

```json
"reason": "compiler-artifact"
```

For each record preserve exactly:

```json
{
  "package_id": "...",
  "manifest_path": "...",
  "target": {
    "name": "...",
    "kind": [],
    "crate_types": [],
    "src_path": "..."
  },
  "profile": {},
  "features": [],
  "filenames": [],
  "executable": null,
  "fresh": true
}
```

Do not infer artifact paths by scanning `target/`.

Construct a research-only index:

```text
canonical artifact path -> one or more Cargo artifact records
```

For every path record both:

- Cargo-emitted original absolute path;
- `std::fs::canonicalize` result when successful.

Never use the basename alone.

---

## 6. Deterministic two-phase experiment

The artifact catalog must exist **before** the selected-target rustc callback needs to classify loaded crates.

Use a dedicated temporary Cargo target directory.

### Phase A — catalog build

Use the same experimental wrapper binary/path that will be used in Phase B, but in pass-through/catalog mode.

Reason: Cargo documents that `RUSTC_WORKSPACE_WRAPPER` affects workspace artifact filename hashing. Using the same wrapper path in both phases avoids changing workspace dependency artifact identities merely because instrumentation was enabled.

Run:

```text
explicit --target <target>
forced target-side MIR encoding
same feature/profile selection
same target directory
same wrapper executable path
analysis callback disabled
--message-format=json-render-diagnostics
```

Capture the complete Cargo JSON stream and construct the artifact catalog.

### Between phases

Force recompilation of **only the selected package** while retaining dependency artifacts.

Experimentally validate the pinned Cargo behavior of:

```bash
cargo clean -p <selected-package> \
  --target <target> \
  --target-dir <same-target-dir>
```

Record exactly what is removed.

If this cannot preserve dependency artifacts deterministically, stop and choose another evidence-backed mechanism.

### Phase B — correlation callback

Run the same Cargo command with:

- same target dir;
- same explicit target;
- same forced-MIR setting;
- same wrapper executable path;
- analysis callback enabled;
- artifact catalog path passed read-only to the research callback.

The selected package must be recompiled; dependency artifact paths must match the Phase A catalog.

Do not reconstruct a rustc command manually.

---

## 7. rustc-side loaded-crate inventory

Inside the selected target's real compiler session, enumerate loaded non-speculative crates with the pinned equivalent of:

```text
tcx.used_crates(())
```

For every loaded `CrateNum`, record:

```json
{
  "crate_num_diagnostic": "...",
  "crate_name_diagnostic": "...",
  "stable_crate_id": "...",
  "crate_source": {
    "rlib": null,
    "rmeta": null,
    "dylib": null,
    "other": null
  }
}
```

`CrateNum` and crate name are diagnostics only.

The correlation decision must be based on exact artifact paths.

---

## 8. Join algorithm

For each loaded rustc crate:

1. obtain every existing filesystem path from its `CrateSource`;
2. canonicalize it when possible;
3. look up exact original/canonical path in the Cargo artifact catalog;
4. deduplicate identical Cargo records;
5. classify the result.

Allowed outcomes:

```text
exact_unique
sysroot_out_of_scope
cargo_artifact_not_found
ambiguous_multiple_packages
ambiguous_multiple_units
```

### `exact_unique`

Accept only if all matched paths resolve to one coherent Cargo unit identity.

At minimum retain:

```text
PackageId
manifest_path
Cargo target name
Cargo target kind
crate_types
features
profile
artifact filenames
StableCrateId
rustc CrateSource paths
```

### `sysroot_out_of_scope`

A loaded sysroot/compiler-provided crate may have no Cargo artifact record.

Classify it explicitly through compiler/sysroot provenance, not by crate-name lists.

### No fallback

For an ordinary non-sysroot foreign crate:

```text
no exact artifact match
```

must remain:

```text
cargo_artifact_not_found
```

Do not fall back to:

- crate name;
- def-path prefix;
- source filename;
- package-name heuristics.

---

## 9. Runtime scope decision

Correlation and runtime eligibility are separate operations.

After obtaining an exact `PackageId`, consult the filtered Cargo resolve graph.

Starting from the selected package, compute the reachable normal target dependency closure using:

```text
resolve.nodes[].deps[].pkg
resolve.nodes[].deps[].dep_kinds[]
```

for the explicit target platform.

Record separately:

```text
cargo_identity = exact PackageId
runtime_scope = eligible | excluded
scope_reason
```

Build dependencies must not become runtime-eligible solely because Cargo produced an artifact.

Proc-macro target units must be excluded by structured Cargo target kind/crate type.

---

## 10. Required synthetic cases

Extend research fixtures only as needed.

### I01 — direct dependency

Prove direct foreign loaded crate -> exact PackageId.

### I02 — transitive dependency

Prove a crate only reached through another dependency maps exactly.

### I03 — renamed dependency

Use Cargo dependency rename/aliasing.

Prove the mapping does not rely on extern/crate display name.

### I04 — duplicate package versions

Construct two resolved versions of the same package name if feasible offline.

Require two distinct:

```text
PackageId
artifact path
StableCrateId
```

and zero conflation.

If an offline duplicate-version fixture is difficult, document the construction separately; do not replace it with a weaker same-name textual test.

### I05 — build dependency

Cargo artifact exists but the package is outside the selected target's normal runtime dependency closure.

Require exclusion.

### I06 — proc macro

Cargo artifact exists and target kind/crate type identifies proc macro.

Require exclusion.

### I07 — sysroot crate

A loaded `core`/`alloc`/`std`-side crate has rustc source provenance but no ordinary Cargo PackageId mapping.

Require explicit sysroot exclusion without name matching.

---

## 11. Required negative/adversarial checks

The verifier must reject at least:

1. Cargo artifact filename changed;
2. PackageId swapped between two artifact records;
3. artifact record deleted for an ordinary loaded dependency;
4. duplicate two PackageIds onto one exact artifact path;
5. StableCrateId changed in derived correlation output;
6. build dependency marked runtime eligible;
7. proc-macro unit marked runtime eligible;
8. sysroot crate assigned a fabricated Cargo PackageId.

Derived output must be recomputed from preserved raw Cargo JSON and raw rustc inventory.

Do not verify only self-consistency of the derived JSON.

---

## 12. Required output

Create:

```text
repro-results/dep1-p1-id0-<timestamp>/
```

with at minimum:

```text
DEP1_P1_ID0_REPORT.json
DEP1_P1_ID0_CORRELATION.json
DEP1_P1_ID0_RUNTIME_SCOPE.json
DEP1_P1_ID0_ADVERSARIAL.json

cargo-metadata.json
cargo-artifacts.jsonl
artifact-catalog.json

rustc-loaded-crates.json
pinned-rustc-api.txt

phase-a-command.txt
phase-b-command.txt
phase-a-cargo.log
phase-b-cargo.log

environment.txt
git-status-before.txt
git-status-after.txt
git-diff-check.txt

probe-source/
verifier-source/
SHA256SUMS
```

Generate `SHA256SUMS` with paths relative to the evidence-package root, so:

```bash
cd repro-results/dep1-p1-id0-<timestamp>
sha256sum -c SHA256SUMS
```

works directly after extraction.

---

## 13. Acceptance criteria

ID0 passes only if:

1. pinned rustc exposes exact loaded `CrateSource` artifact paths;
2. Cargo artifact stream is captured from the exact build configuration;
3. every eligible synthetic runtime dependency has an exact unique artifact-path join;
4. direct and transitive dependencies both correlate;
5. renamed dependency correlation succeeds without names;
6. duplicate version/source case is disambiguated;
7. build dependency is excluded;
8. proc macro is excluded;
9. sysroot crate is excluded without name matching;
10. derived PackageId/rustc mapping is recomputable from raw evidence;
11. all adversarial mutations are rejected;
12. no production CREMA/CQPL semantics changed;
13. no RustSec/emap execution occurred;
14. no commit/push occurred.

If these hold, the former DEP1-P1 hard stop is resolved and production P1 may resume.

---

## 14. Production implication if ID0 passes

The production correlation layer should conceptually be:

```text
Cargo metadata normal runtime closure
                 |
                 v
Cargo compiler-artifact catalog
artifact path -> PackageId / target unit
                 ^
                 |
rustc used_crate_source(CrateNum)
                 |
                 v
loaded CrateNum -> StableCrateId
```

The artifact path is a run-local join key only.

Persisted semantic body identity remains compiler-derived:

```text
StableCrateId
DefPathHash
resolved Instance identity
```

Do not persist Cargo output paths as semantic function identities.

---

## 15. Stop conditions

Stop and report rather than weaken the model if:

- pinned rustc does not expose the exact loaded artifact path;
- Cargo does not emit the corresponding artifact path;
- canonical path join is ambiguous across distinct PackageIds;
- duplicate-version correlation cannot be disambiguated;
- Phase A/Phase B cannot preserve dependency artifact identity;
- selected-package-only rebuild cannot be made deterministic;
- runtime-vs-build/proc-macro scope cannot be derived structurally.

---

## 16. Code-agent assignment

Execute **DEP1-P1-ID0 only**.

Do not resume production body ingestion until ID0 is independently reviewed and accepted.

The central experiment is:

```text
Cargo compiler-artifact.filenames
          EXACT PATH JOIN
rustc used_crate_source(CrateNum)
```

combined with Cargo metadata's resolved normal-dependency closure.

Use the pinned compiler as the authority for rustc-private APIs.
