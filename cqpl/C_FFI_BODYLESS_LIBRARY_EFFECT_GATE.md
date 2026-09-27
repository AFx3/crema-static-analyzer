# C/FFI Bodyless Library-Effect Semantics — Implementative Scientific Gate

**Status:** design specification  
**Target baseline:** `143d3e53502a5192f48f530e9048fb99775c3ab6` plus the validation-only CTL-law branch  
**Primary objective:** improve CREMA's abstract model at Rust→C / external-library boundaries when the C body is unavailable, without weakening soundness or conflating producer evidence with heuristic name matching.

---

## 1. Scientific objective

The core analysis goals remain:

1. memory leak;
2. double free;
3. use-after-free;
4. allocator-family mismatch.

The purpose of this gate is to validate a new producer-side layer of **proof-carrying bodyless-library effects** so that a Rust call to an external C function can contribute allocation-centric facts even when no C/LLVM body is present in the loaded program.

The desired pipeline is:

```text
Rust MIR call
    |
    v
external C declaration / bodyless library call
    |
    +--> LLVM 16 explicit attributes
    |
    +--> LLVM 16 TargetLibraryInfo (TLI) evidence
    |
    +--> optional closed curated summary
    |
    v
proof-carrying external_library_effect_v1
    |
    +--> FFI actual/formal identity binding
    |
    +--> allocation identity / allocator family
    |
    +--> alloc / drop / read / write / returned-alias effects
    |
    v
existing annotated ICFG + existing CQPL queries
```

The CQPL query language should **not** be changed merely to consume these effects. The producer should enrich the same allocation/event vocabulary already used by the leak, double-free, UAF and allocator-mismatch queries.

---

## 2. Current baseline to preserve

The current implementation already has significant infrastructure that this feature must reuse rather than bypass:

- `llvm_memory_effects_v1` with LLVM-16-derived function/formal/callsite evidence;
- explicit-vs-TLI provenance;
- `external_deallocation_effects_v1`;
- `ffi_argument_identity_v1`;
- solved Andersen/SVF points-to evidence;
- allocation identity across Rust↔C boundaries when the body is present;
- special handling for standard C allocation/deallocation families;
- TLI recognition of standard functions such as `free`;
- proof-carrying allocator/deallocator contracts;
- W1 source/provenance and diagnostic-certificate validation.

Existing tests already establish examples such as:

```text
efx1_tli_free_declaration_is_positive_may_deallocation
cstring_from_raw_call_gets_read_summary_but_not_drop_at_call_site
schema_v2_attaches_c_free_contract_to_rust_allocation_drop
phase6e_c_malloc_return_reaches_scoped_rust_return_local
```

The new layer must be **additive**. It must not replace body analysis, SVF identity, typed edge flow, existing allocation contracts, or panic/unwind semantics.

---

## 3. Definition of a bodyless external call

A call is eligible for `external_library_effect_v1` iff:

1. the call is represented in the Rust/CREMA ICFG;
2. the target is an external C/ABI function or library declaration;
3. no analyzable function body for that target is present in the loaded LLVM/SVF program used for the call;
4. the call has stable callsite identity;
5. any semantic effect emitted by the feature carries an explicit evidence basis.

The feature must not duplicate effects for functions whose body is already represented.

### Required invariant

For every callsite `c`:

```text
represented_body(c) => bodyless_summary_effects(c) = empty
```

unless a later capability explicitly defines a corroboration-only mode. V1 should use the simpler non-duplicating rule.

---

## 4. Proposed artifact contract

Introduce an additive producer capability, provisionally:

```text
external_library_effects_v1
```

and an artifact payload containing records conceptually equivalent to:

```text
ExternalLibraryEffect {
    callsite_id
    callee_identity
    abi
    body_status

    effects: [
        AllocReturn { family, certainty, basis },
        ReallocArg  { formal_index, family, certainty, basis },
        FreeArg     { formal_index, family, certainty, basis },
        ReadArg     { formal_index, certainty, basis },
        WriteArg    { formal_index, certainty, basis },
        ReturnedAlias { formal_index, certainty, basis },
        CertifiedNoFree { formal_index_or_function, basis },
        CertifiedNoCapture { formal_index, basis }
    ]
}
```

This is a conceptual schema, not a mandate on exact Rust field names.

Every positive effect must have:

```text
kind
subject/formal index when applicable
certainty
basis
producer/callsite identity
```

and every family-sensitive allocation/deallocation effect must carry:

```text
allocator_family
```

when the family is proven.

Unknown family must remain explicitly unknown; it must never be replaced by a guessed family.

---

## 5. Evidence tiers and precedence

The implementation should have a closed evidence order.

### Tier E1 — explicit LLVM IR evidence

Highest priority.

Examples:

```text
allockind
alloc-family
allocptr
return noalias
allocsize
nofree
nocapture
returned
memory(...)
per-formal memory attributes
```

Basis examples:

```text
llvm16_explicit_allockind_v1
llvm16_explicit_alloc_family_v1
llvm16_explicit_nofree_v1
llvm16_explicit_memory_effect_v1
```

### Tier E2 — LLVM 16 TLI-derived evidence

Accepted only when the existing EFX1 verifier certifies that TLI recognition/inference is admissible and callsite identity/cardinality is preserved.

Basis examples:

```text
llvm16_tli_free_v1
llvm16_tli_malloc_v1
llvm16_tli_memory_effect_v1
llvm16_tli_nofree_v1
```

TLI evidence must remain distinguishable from explicit IR evidence.

### Tier E3 — closed curated library summary

Used only where LLVM/TLI gives insufficient per-formal detail.

Examples:

```text
strlen(arg0)         -> ReadArg(0)
memcmp(arg0,arg1,_)  -> ReadArg(0), ReadArg(1)
memcpy(dst,src,_)    -> WriteArg(0), ReadArg(1)
memmove(dst,src,_)   -> WriteArg(0), ReadArg(1)
memset(dst,_,_)      -> WriteArg(0)
```

A curated summary must be:

- versioned;
- symbol/ABI exact;
- target/library scoped where necessary;
- closed, not substring-based;
- represented as evidence, not silently embedded in the transfer function.

### Tier E4 — unresolved

If none of E1–E3 proves a particular effect:

```text
effect = unresolved
```

Do not invent a positive or negative event.

---

## 6. Semantic rules to implement and gate

## 6.1 Allocation returned by a bodyless function

A fresh abstract allocation may be created only with a positive allocation contract such as:

```text
allockind("alloc")
```

or an equivalent verified closed summary.

Recommended rule:

```text
allockind(alloc)
    => may_alloc_return
```

`return noalias` is useful corroboration but should **not by itself** establish allocator family.

If family is known:

```text
alloc-family = F
```

then:

```text
family(new_alloc) = F
```

The return identity must be propagated into the Rust return place through the existing FFI return bridge.

### Fail-closed rules

Do not infer a fresh allocation from:

```text
pointer return type alone
symbol name containing "alloc"
return noalias alone
unknown external pointer result
```

---

## 6.2 Deallocation

For an external deallocator:

```text
allockind("free")
+ allocptr(formal_i)
```

or an equivalent verified TLI/summary record should produce:

```text
may_free(formal_i)
```

After actual/formal identity resolution:

```text
formal_i -> {a1, ..., an}
```

materialize a may-deallocation event for the represented abstract allocation(s).

If allocator family is proven:

```text
deallocator_family = F
```

attach it to the drop/deallocation contract.

### Required use

This directly feeds:

- double-free;
- UAF;
- leak discharge reasoning;
- allocator mismatch.

---

## 6.3 `realloc`

`realloc` must not be reduced to an unconditional `free + alloc`.

LLVM 16 semantics require conditional reallocation:

```text
failure:
    result = null
    old allocation remains valid

success:
    result is a new allocation object
    old allocptr allocation is invalidated
```

even if the returned address numerically equals the old address.

The abstract transfer must therefore encode at least:

```text
result identity = old-or-fresh
old allocation  = may remain live OR may be invalidated
fresh result    = same allocator family as realloc contract
```

The gate must reject an implementation that:

```text
always frees old
always creates fresh
equates same address with same allocation identity
```

This feature is central to both double-free and UAF soundness.

---

## 6.4 Read/write effects on Rust-owned heap passed to C

This must be allocation-aware.

A generic function-level:

```text
memory(argmem: read)
```

proves that argument memory may be read, but does **not** necessarily identify which pointer formal is responsible when multiple pointer formals exist.

Therefore:

```text
function-level argmem read/write
```

may produce a call-level unresolved/candidate effect, but it must not automatically become:

```text
ReadArg(i)
```

for every pointer formal.

A concrete per-allocation `read_l(a)` / `write_l(a)` requires one of:

1. per-formal LLVM memory evidence;
2. exact TLI per-formal semantics;
3. a closed curated summary identifying the formal;
4. another producer certificate that identifies the accessed actual.

Then use:

```text
ffi_argument_identity_v1
+
SVF/points-to
```

to map the formal to abstract allocations.

### Examples

```text
strlen(p)      -> read allocation(s) reachable from p
memset(p,...)  -> write allocation(s) reachable from p
memcpy(d,s,n)  -> write(d), read(s)
```

These events feed UAF detection after a prior deallocation.

---

## 6.5 `returned` / alias-return semantics

If a function is certified to return one of its pointer arguments:

```text
returned(formal_i)
```

then the result must reuse/may-alias the same allocation identity.

It must **not** create a fresh allocation.

Gate invariant:

```text
ReturnedAlias(i) => no AllocReturn from that evidence alone
```

This prevents false leaks and false allocator-family changes.

---

## 6.6 `nofree`

`nofree` is negative evidence for deallocation, not a general liveness theorem.

Function-level LLVM `nofree` means the function does not directly/transitively deallocate a pre-existing allocation through its own execution, subject to LLVM's capture/concurrency qualification.

Therefore the gate may accept:

```text
nofree => certified absence of direct/transitive deallocation by this callee
```

but must reject:

```text
nofree => allocation definitely live after call
nofree => pointer cannot escape
nofree => another thread cannot free it
```

Stronger post-call survival reasoning requires capture/synchronization conditions.

---

## 6.7 `nocapture` and escape

`nocapture(formal_i)` is useful negative evidence:

```text
callee does not retain that particular pointer copy beyond the call
```

It applies to that formal copy, not necessarily to another aliased actual/formal.

Therefore identity/alias information must be consulted before turning `nocapture` into an allocation-level non-escape claim.

Absence of `nocapture` is **not** positive evidence of capture.

V1 may use `nocapture` only as corroborating negative evidence; it should not invent `may_capture` solely because the attribute is absent.

---

## 6.8 Allocator family

Allocator-family tracking must become structural rather than name-based whenever proof is available.

Desired model:

```text
AllocReturn(F)
FreeArg(i, F)
ReallocArg(i, F)
```

Then allocator mismatch can reason over:

```text
family(allocation) = F
family(deallocator) = G

F != G
    => mismatch evidence
```

Unknown family remains unknown.

### Forbidden inference

```text
callee name contains "malloc" => family=malloc
callee name contains "free"   => deallocator
```

unless the name is part of an exact, closed, versioned library summary or TLI classification.

---

## 7. Canonical placement and no-double-counting invariant

Each synthesized bodyless effect must appear **exactly once** in the annotated semantic model.

The implementation must choose one canonical summary program point, e.g. the existing external/DummyCall boundary or the Rust callsite summary point.

The gate does not require a particular internal node name, but requires:

```text
one semantic effect
one callsite identity
one event provenance
```

for each emitted effect.

Forbidden:

```text
Rust callsite drop + DummyCall drop for the same external free
```

unless the model explicitly distinguishes two concrete events, which a single bodyless call does not.

---

## 8. Identity requirements

No allocation-centric effect may be attached merely because the argument is pointer-typed.

For each formal effect:

```text
Effect(formal_i)
```

the producer must resolve:

```text
formal_i -> actual_i -> abstract allocation set
```

using existing certified FFI identity/points-to machinery.

If resolution is empty or unresolved:

```text
do not attach the effect to an unrelated allocation
```

and retain an unresolved external-effect record for diagnostics.

Alias cases must be handled as may semantics:

```text
arg0 -> {a}
arg1 -> {a}
```

means the same allocation may receive effects from either formal.

This is particularly important for:

- parameter-level `nofree`;
- `nocapture`;
- `memcpy`;
- functions receiving the same pointer twice.

---

## 9. Provenance requirements

Every emitted event must expose why it exists.

Minimum provenance tuple:

```text
callsite
callee
effect kind
formal index / return
abstract allocation when resolved
basis
explicit-vs-TLI-vs-summary origin
allocator family when applicable
```

W1 should anchor the event to the Rust callsite source location when no C body exists.

It must not fabricate a C source line.

Recommended source status:

```text
callsite_grounded
external_body_unavailable
```

with a separate proof basis for the effect.

---

# 10. Gate structure

Proposed scripts:

```text
cqpl/scripts/run_bodyless_ffi_effects_gate.sh
cqpl/scripts/gate_bodyless_ffi_effects.py
```

Recommended schema:

```text
cqpl_bodyless_ffi_effects_gate_v1
```

The gate should contain the following stages.

---

## G0 — Baseline and cleanliness

Requirements:

```text
expected git branch / baseline commit
git diff --check
frozen nightly toolchain
LLVM evidence schema version pinned
```

Record:

```text
rustc version
LLVM major/minor used by evidence producer
TLI target triple
```

---

## G1 — Existing full unit suite

Run:

```text
CREMA unit tests
CQPL unit tests
CQPL CLI tests
CQPL integration tests
```

No pre-existing regression may be accepted as part of the feature.

---

## G2 — Bodyless-call inventory

Before semantic changes are evaluated, produce a corpus inventory over the frozen 118 subjects.

For every reachable bodyless external call record:

```text
subject
callsite
callee
pointer formal count
body present/absent
TLI recognized?
explicit LLVM attributes?
function memory effects
formal attributes
allockind
alloc-family
allocptr
return noalias
allocsize
nofree
nocapture
returned
current CREMA semantic events
```

Required aggregate counters include:

```text
reachable_bodyless_calls
bodyless_pointer_calls
tli_recognized_calls
calls_with_explicit_efx
calls_with_allockind_alloc
calls_with_allockind_free
calls_with_allockind_realloc
calls_with_alloc_family
calls_with_argmem_read
calls_with_argmem_write
calls_with_per_formal_read
calls_with_per_formal_write
calls_with_nofree
calls_with_nocapture
calls_with_returned
currently_effectful_calls
currently_unresolved_calls
```

This inventory is a scientific baseline and must be frozen with the feature.

---

## G3 — Evidence parser/verifier tests

Test exact acceptance/rejection of E1/E2/E3 evidence.

Positive controls:

```text
explicit allockind alloc
explicit allockind free + allocptr
explicit realloc + allocptr
explicit alloc-family
explicit nofree
TLI-recognized free
TLI-recognized allocation function
memory(argmem: read)
memory(argmem: write)
returned
nocapture
```

Negative controls:

```text
invalid allocptr index
family metadata without allocation kind where required
TLI mutation that changes callsite identity/cardinality
duplicate formal indices
effect on non-pointer formal
unknown memory access encoding
summary symbol mismatch
summary ABI mismatch
```

All invalid records must fail closed.

---

## G4 — Synthetic allocation-return fixtures

Minimum bodyless fixtures:

```text
ext_alloc_no_free       -> leak evidence
ext_alloc_then_free     -> clean lifecycle
ext_alloc_double_free   -> double-free evidence
ext_alloc_then_use      -> no UAF before free
```

Requirements:

```text
fresh abstract allocation exists
Rust return local points to it
allocator family is preserved when known
alloc event appears exactly once
W1 basis identifies external effect
```

---

## G5 — Synthetic deallocation fixtures

Minimum:

```text
Rust alloc -> external free
C alloc    -> external free
external free -> second free
external free -> Rust use
```

Verify:

```text
drop event exactly once
same allocation identity
correct deallocator family
DF/UAF query receives the expected may evidence
```

---

## G6 — Reallocation fixtures

At least:

```text
realloc success abstraction
realloc null/failure abstraction
realloc then old-pointer use
realloc then old-pointer free
realloc result free
```

Required abstract invariant:

```text
old-or-fresh result
old may remain live
old may be invalidated
```

No unconditional old-free transfer is permitted.

---

## G7 — Bodyless read/write fixtures

Required closed summaries/evidence:

```text
strlen
memcmp
memcpy
memmove
memset
```

Minimum detector tests:

```text
free(p); strlen(p)      -> UAF read evidence
free(p); memset(p,...)  -> UAF write evidence

free(src); memcpy(dst,src,n) -> UAF read
free(dst); memcpy(dst,src,n) -> UAF write
```

Negative controls:

```text
memory(argmem: read) with two pointer formals but no per-formal proof
    => must NOT attach read to both allocations

memory(read) without argmem/per-formal proof
    => must NOT fabricate an argument-specific use
```

---

## G8 — Allocator mismatch fixtures

Required:

```text
family A allocation -> family A free   => no mismatch evidence
family A allocation -> family B free   => mismatch evidence
malloc family       -> Rust global dealloc
Rust global alloc   -> malloc-family free
unknown family      -> known family free
```

The final case must stay unresolved/unknown, not become a proven mismatch.

---

## G9 — `nofree` / capture negative controls

Required tests:

```text
nofree + uncaptured pointer
    => no deallocation event

nofree without nocapture/nosync
    => do not assert global post-call liveness theorem

nocapture(arg0), same allocation also passed via unannotated arg1
    => do not elevate arg0 nocapture to allocation-wide noescape
```

This gate exists specifically to prevent overinterpreting LLVM attributes.

---

## G10 — Body-present non-duplication

For corresponding C implementations with bodies:

```text
malloc-like body
free-like body
read body
write body
```

verify:

```text
body semantics are used
bodyless summary is not additionally materialized
event multiplicity remains one-per-concrete-effect
```

The gate must compare body-present and bodyless variants deliberately.

---

## G11 — Core-query outcome matrix

For every synthetic fixture run the canonical:

```text
leak_alloc_state.cqpl
double_free_alloc_state.cqpl
use_after_free_alloc_state.cqpl
allocator_mismatch_ub_v2.cqpl
```

Record both:

```text
truth
assessment
```

and explanation categories.

Do not require arbitrary `tt`; existing three-valued may semantics remain authoritative.

Expected success means the new external effect reaches the same abstraction vocabulary as equivalent represented-body behavior.

---

## G12 — Equivalence-to-represented-body controls

For a small closed family, construct pairs:

```text
A: function body available
B: body absent + certified external effect
```

Examples:

```text
malloc wrapper
free wrapper
strlen-like reader
memset-like writer
```

Compare:

```text
allocation identity relation
event kind
allocator family
truth of four canonical queries
assessment orientation
```

Exact internal node names need not match.

The scientifically important property is **observational equivalence for the four core analyses** where the summary is intended to be complete.

---

## G13 — Frozen 118-subject differential

Run the full existing 118-subject matrix before/after the feature.

Record at least:

```text
attempts = 1534
errors
truth transition matrix
assessment transition matrix
deltas by query
deltas by subject
deltas by external-effect kind
```

Unlike a pure refactor gate, **zero truth delta is not required**.

Any delta must satisfy:

1. it occurs on a subject containing a newly consumed bodyless external effect;
2. the changed result has an effect/provenance witness;
3. no unrelated query/subject changes;
4. no clean-control regression;
5. no delta is caused only by symbol substring heuristics.

Unexpected delta => gate failure.

---

## G14 — Precision metrics

Measure, but do not automatically equate "more positives" with success.

Report:

```text
bodyless effects resolved before
bodyless effects resolved after

UNKNOWN assessments before
UNKNOWN assessments after

new leak-oriented evidence
new DF-oriented evidence
new UAF-oriented evidence
new mismatch-oriented evidence

unresolved external contracts before/after
```

The desired result is:

```text
more justified semantic coverage
without unsupported truth promotion
```

---

## G15 — W1/certificate consistency

Run the diagnostic certificate gate.

Requirements:

```text
certificate_structure_valid = true
one_certificate_per_finding = true
certificate_validation_failures = []
```

For a bodyless effect, certificates must refer to:

```text
real Rust callsite
external-effect basis
allocation identity
```

and must never fabricate:

```text
C body path
C source line
typed edge inside unavailable library body
```

---

## G16 — Warning and artifact hygiene

No new framework warnings.

Generated artifacts must not enter the commit.

Freeze:

```text
git status before
git status after
git diff --check
manifest SHA256
gate JSON
```

---

# 11. Mandatory synthetic fixture matrix

| ID | Scenario | Leak | DF | UAF | Mismatch | Required semantic effect |
|---|---|---:|---:|---:|---:|---|
| B01 | bodyless alloc, no free | affected | no | no | no | AllocReturn |
| B02 | bodyless alloc → matching free | clean relative control | no | no | no | AllocReturn + FreeArg |
| B03 | bodyless alloc → free → free | no | affected | maybe | no | repeated FreeArg |
| B04 | bodyless alloc → free → strlen(ptr) | no | no | affected | no | ReadArg |
| B05 | bodyless alloc → free → memset(ptr) | no | no | affected | no | WriteArg |
| B06 | free(src) → memcpy(dst,src,n) | no | no | affected | no | ReadArg(src) |
| B07 | free(dst) → memcpy(dst,src,n) | no | no | affected | no | WriteArg(dst) |
| B08 | family A alloc → family B free | maybe | no | maybe | affected | family mismatch |
| B09 | family A alloc → family A free | clean control | no | no | no | same family |
| B10 | realloc success/failure abstraction | lifecycle | affected | affected | family-sensitive | ReallocArg |
| B11 | `returned(arg0)` | no fresh leak | no | identity-sensitive | no | ReturnedAlias |
| B12 | `nofree` only | no fake drop | no | no fake UAF | no | CertifiedNoFree |
| B13 | ambiguous `argmem:read` with two pointers | unchanged | no | no fabricated UAF | no | unresolved per-formal read |
| B14 | body-present equivalent | observational control | control | control | control | no duplicate summary |
| B15 | unknown external pointer call | unresolved | unresolved | unresolved | unresolved | fail closed |

---

# 12. Required result schema

The final gate should write a machine-readable JSON containing at least:

```json
{
  "schema": "cqpl_bodyless_ffi_effects_gate_v1",
  "status": "PASS|FAIL",
  "baseline_commit": "...",
  "toolchain": "...",
  "llvm_version": "...",

  "inventory": {
    "reachable_bodyless_calls": 0,
    "tli_recognized_calls": 0,
    "calls_with_allockind_alloc": 0,
    "calls_with_allockind_free": 0,
    "calls_with_allockind_realloc": 0,
    "calls_with_alloc_family": 0,
    "calls_with_nofree": 0,
    "calls_with_nocapture": 0,
    "calls_with_returned": 0,
    "calls_with_argmem_read": 0,
    "calls_with_argmem_write": 0
  },

  "synthetic_fixtures": {
    "passed": 0,
    "failed": 0
  },

  "identity": {
    "effect_to_actual_resolution_failures": 0,
    "duplicate_effect_materializations": 0
  },

  "core_queries": {
    "unexpected_truth_deltas": 0,
    "unexpected_assessment_deltas": 0
  },

  "corpus_differential": {
    "attempts": 1534,
    "errors": 0,
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "explained_deltas": 0,
    "unexplained_deltas": 0
  },

  "w1": {
    "certificate_validation_failures": []
  },

  "criteria": {
    "all_evidence_fail_closed": true,
    "no_name_substring_semantics": true,
    "no_body_summary_duplication": true,
    "realloc_is_conditional": true,
    "no_argmem_overassignment": true,
    "nofree_not_overinterpreted": true,
    "allocator_family_proof_carrying": true,
    "all_deltas_provenance_explained": true
  }
}
```

Counts are illustrative placeholders; the field structure is the normative part.

---

# 13. Merge criteria

The implementation is mergeable only if all of the following hold:

```text
[ ] all existing producer/checker tests pass
[ ] bodyless corpus inventory is frozen
[ ] every synthesized effect has a proof basis
[ ] no effect is inferred from an open-ended symbol-name heuristic
[ ] actual/formal/allocation identity is certified or the effect stays unresolved
[ ] allocator family is never guessed
[ ] realloc preserves success/failure uncertainty
[ ] function-level argmem effects are not over-assigned to individual formals
[ ] nofree is not promoted to unconditional post-call liveness
[ ] nocapture is not promoted across aliased formals without proof
[ ] represented bodies do not receive duplicate bodyless effects
[ ] B01–B15 synthetic matrix passes
[ ] four canonical CQPL queries are evaluated for every synthetic fixture
[ ] represented-body/bodyless observational controls pass
[ ] all 118-subject deltas are classified and provenance-backed
[ ] zero unexplained corpus delta
[ ] W1 certificates remain structurally valid
[ ] no fabricated C source path/span
[ ] no new framework warnings
[ ] working tree contains only intended source/test/spec files
```

---

# 14. Recommended implementation order

Do not implement every effect simultaneously.

## Phase A — audit only

No semantic change.

Deliver:

```text
bodyless-call inventory
EFX/TLI coverage
current unresolved-effect counts
```

This determines the real corpus opportunity.

## Phase B — allocation/deallocation family

Implement:

```text
AllocReturn
FreeArg
allocator family
ReturnedAlias
CertifiedNoFree
```

This immediately benefits:

```text
leak
double free
allocator mismatch
```

and parts of UAF.

Run full gate.

## Phase C — conditional realloc

Implement `ReallocArg` with old-or-fresh semantics.

Run full gate.

## Phase D — read/write use effects

Implement per-formal `ReadArg` / `WriteArg`, initially for a small closed library set plus explicit per-formal evidence.

This directly improves UAF.

Run full gate.

## Phase E — escape/capture refinement

Use `nocapture`, alias information and, if later justified, synchronization evidence.

This should initially improve diagnostics/precision rather than introduce aggressive truth claims.

---

# 15. Scientific interpretation of outcomes

A successful implementation should support the claim:

> For bodyless external C/library calls, CREMA consumes versioned LLVM/TLI and closed library-summary evidence to construct proof-carrying allocation, deallocation, memory-use and allocator-family effects. Effects are attached to abstract allocations only through certified FFI identity, and unresolved cases remain unresolved. The existing CQPL formulas then operate unchanged over the enriched abstract model.

It should **not** support the stronger claims:

```text
all libc semantics are modeled
all external calls are resolved
LLVM nofree proves post-call liveness in all concurrent executions
generic memory(read/write) identifies every touched pointer argument
the summary layer is a substitute for body analysis
```

---

# 16. Normative external semantics to pin

The implementation should pin its interpretation to the LLVM **16.0.0 Language Reference**, matching the evidence producer version.

Relevant LLVM concepts:

```text
allockind("alloc")
allockind("realloc")
allockind("free")
allocptr
alloc-family
allocsize
return noalias
nofree
nocapture
memory(...)
argmem
returned
```

Important semantic constraints from LLVM 16 include:

- `allockind("alloc")`: returns a new allocation or null;
- `allockind("realloc")`: success invalidates the old allocation and yields a new allocation object, failure leaves the old allocation valid;
- `allockind("free")`: frees the object designated by `allocptr`;
- `memory(argmem: read/write)` constrains accesses to pointer-argument-derived memory but does not by itself identify a unique formal;
- `nofree` does not imply no capture or unconditional post-call liveness;
- `nocapture` applies to the particular pointer copy/formal;
- return `noalias` has allocator-like semantics, but allocator family still requires separate evidence.

---

# 17. Recommended branch/checkpoint

After freezing the CTL-law validation branch, create a new branch specifically for the audit/gate:

```text
cqpl6-bodyless-ffi-effect-gate
```

The first commit on that branch should contain **only**:

```text
gate specification
inventory script
gate harness
synthetic fixture skeletons
```

with no semantic producer change.

That gives a clean scientific baseline before implementing Phase B.


---

## Phase-A empirical refinement: isolation and structured fail-closed baselines

The first direct bodyless-`malloc` fixture established an additional baseline
fact: the current producer may syntactically recognize the allocation event
while lacking an `AbstractAllocId/event_identity`, causing schema-v2 export to
fail closed. The gate must preserve this failure rather than disable the
schema-v2 invariant.

Accordingly, Phase A distinguishes:

```text
producer_pass
fail_closed_alloc_identity_missing
fail_closed_other
producer_error
```

Only explicitly manifest-declared structured fail-closed outcomes are accepted
as characterization baselines. An unclassified producer error is never a PASS.

Fixture design must also obey **single-obligation isolation**: a test for
bodyless `strlen`, `memcpy`, `memset`, `free`, or `realloc` must not depend on
direct bodyless `malloc` if that earlier feature prevents schema-v2 export.
Represented-body seed allocation or an already modeled Rust allocation may be
used to establish allocation identity, while the external effect under test
remains bodyless.

These fail-closed baseline rows are evidence of missing semantics, not evidence
that the memory-error query is correctly answered; CQPL is not run when no
valid annotated artifact exists.
