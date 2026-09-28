# D3 — External Negative Evidence V1 for Bodyless C/LLVM Calls

**Status:** normative implementative scientific specification  
**Gate ID:** `D3_EXTERNAL_NEGATIVE_EVIDENCE_V1`  
**Capability:** `external_negative_evidence_v1`  
**Payload:** `external_negative_evidence`  
**Baseline branch:** `cqpl6-bodyless-ffi-effect-gate`  
**Exact baseline commit:** `da9555a6fa6a99f3607112be6b3d36c7acc01e84`  
**Baseline requirement:** this token MUST be replaced with the clean, committed, pushed D2/ERR1 checkpoint before Codex starts  
**Toolchain:** `nightly-2024-11-21` unless the repository itself provides a stricter frozen toolchain  
**Primary objective:** add proof-carrying negative evidence for LLVM-16 `nofree` and `nocapture` at bodyless Rust→C/external callsites, while explicitly preventing that evidence from being overinterpreted as post-call liveness, allocation-wide noescape, absence of concurrent free, or a new CQPL truth rule.

---

# 0. Scientific position

D3 is deliberately conservative.

The project already models positive semantic effects such as:

```text
allocation
deallocation
conditional realloc
formal read/write
external return relations
```

D3 adds a different kind of information:

```text
certified absence / restriction of a specific callee behavior
```

The initial D3 claim is **not**:

```text
this allocation is definitely live after the call
this allocation definitely cannot escape
this pointer can never be freed concurrently
this call makes the query false
```

The D3 claim is narrower:

```text
LLVM/TLI evidence certifies a specific negative property
at this exact external call/declaration/formal.
```

D3 V1 records and validates that evidence and exposes it for future refinement.

It MUST NOT change existing CQPL truth or assessment semantics.

---

# 1. Baseline requirements

D3 may start only after D2 / `external_return_relations_v1` has been:

```text
independently audited
committed
pushed
working tree clean
```

Before giving this document to Codex, replace:

```text
da9555a6fa6a99f3607112be6b3d36c7acc01e84
```

with:

```text
git rev-parse HEAD
```

from that clean post-D2 checkpoint.

At D3 start, Codex MUST require:

```text
branch = cqpl6-bodyless-ffi-effect-gate
HEAD   = exact rendered D3 baseline
working tree clean except this normative D3 specification
```

D3 MUST preserve:

```text
RN1 / AGE1 / RBF / CR
D1 / EFM2
D2 / ERR1
CQPL parser
CTL semantics
three-valued truth lattice
normal-execution double-free semantics
allocation ownership semantics
return provenance semantics
```

If the accepted D2 corpus contains 75 bodyless-gate targets and each target still has the canonical 12-query matrix, the expected pre-D3 differential surface is:

```text
75 * 12 = 900 existing query cells
```

The gate MUST derive these counts from the exact baseline rather than blindly hardcode them.

---

# 2. Normative LLVM 16 semantics

D3 is pinned to LLVM **16.0.0** semantics.

Normative reference:

```text
https://releases.llvm.org/16.0.0/docs/LangRef.html
```

## 2.1 Function attribute `nofree`

LLVM 16 function-level `nofree` states, in substance:

```text
the function does not directly or transitively invoke a memory
deallocator on an allocation that existed before the call.
```

Important consequences and limitations:

```text
nofree(function)
    DOES NOT mean:
        the function never frees any memory

nofree(function)
    DOES NOT mean:
        every pre-existing pointer is definitely live after the call

nofree(function)
    DOES NOT mean:
        the function cannot capture a pointer

nofree(function)
    DOES NOT mean:
        another thread cannot free captured storage
```

LLVM explicitly allows a `nofree` function to free storage it allocated during the call.

In environments where captured pointers can be communicated to another thread, function-level `nofree` alone is insufficient for a general post-call liveness theorem.

LLVM notes that additional conditions such as:

```text
uncaptured pointer
or
nosync
```

are relevant to stronger dereferenceability reasoning.

D3 V1 does not implement that stronger theorem.

## 2.2 Parameter attribute `nofree`

LLVM 16 also supports pointer-argument `nofree` semantics:

```text
the callee does not free that pointer argument.
```

This is formal-specific evidence.

It MUST NOT be lifted automatically to:

```text
the entire allocation cannot be freed through another alias/formal
```

unless independent alias and effect proof closes all relevant alternatives.

D3 V1 does not perform that allocation-wide lift.

## 2.3 Parameter attribute `nocapture`

LLVM 16 `nocapture` states:

```text
the callee does not capture that particular copy of the pointer
passed through that argument.
```

The particular-copy qualification is mandatory.

If the same pointer value is passed through:

```text
arg0 = nocapture
arg1 = not nocapture
```

then the callee may validly capture through `arg1`.

Therefore:

```text
nocapture(formal_i)
    !=
allocation-wide noescape
```

Absence of `nocapture` is not positive evidence of capture.

## 2.4 `nosync`

LLVM `nosync` is relevant to stronger thread-communication reasoning.

D3 V1 MAY inventory `nosync` as corroborating context if already available in the existing LLVM evidence surface.

D3 V1 MUST NOT introduce a new `CertifiedNoSync` semantic capability and MUST NOT use `nosync` to promote existing truth/assessment results.

---

# 3. Capability design

Introduce exactly one additive capability:

```text
external_negative_evidence_v1
```

with payload:

```text
external_negative_evidence
```

D3 V1 supports exactly three negative-evidence kinds:

```text
no_free_function
no_free_formal
no_capture_formal
```

No fourth semantic kind belongs to D3 V1.

---

# 4. Evidence sources

D3 MUST NOT infer negative evidence from function names.

Forbidden:

```text
"strlen never frees because I know strlen"
"function name contains read"
"libc routine usually does not capture"
```

D3 V1 may consume the following proof sources.

## 4.1 E1 — explicit LLVM 16 attributes

Preferred.

Accepted:

```text
explicit function nofree
explicit pointer-formal nofree
explicit pointer-formal nocapture
```

Suggested bases:

```text
llvm16_explicit_function_nofree_v1
llvm16_explicit_formal_nofree_v1
llvm16_explicit_formal_nocapture_v1
```

The producer MUST preserve whether evidence came from:

```text
callee declaration/function attribute
or
callsite/formal attribute
```

when the current evidence representation exposes that distinction.

## 4.2 E2 — verified LLVM/TLI `nofree`

Function-level `nofree` MAY also be consumed from TLI only if the repository's already-existing LLVM/TLI evidence verifier establishes:

```text
TLI recognition is admissible
exact callee identity is preserved
callsite identity is preserved
attribute/effect cardinality is preserved
```

Suggested basis:

```text
llvm16_tli_verified_function_nofree_v1
```

D3 MUST NOT create a new independent TLI recognizer when an existing EFX/TLI proof surface already exists.

## 4.3 No curated negative summaries in D3 V1

D3 V1 intentionally forbids:

```text
closed hand-written "this libc function is nofree"
closed hand-written "this formal is nocapture"
```

unless that information is already represented as verified LLVM/TLI evidence.

Reason:

```text
positive read/write/return summaries
```

and:

```text
negative absence guarantees
```

have different proof risk.

Negative guarantees are therefore restricted to compiler/evidence-backed sources in V1.

## 4.4 E4 — unresolved

If the required negative property is not proven:

```text
emit no positive negative-evidence record
```

Do not encode:

```text
not nofree => may_free
not nocapture => capture
```

Absence remains unresolved.

---

# 5. Artifact contract

Conceptual record:

```text
ExternalNegativeEvidenceV1 {
    node
    callee

    evidence_kind

    formal_index?
    actual_variable?

    evidence_source
    basis

    producer_evidence_identity?
}
```

The exact Rust field names may follow repository conventions.

Serialized semantics MUST preserve the following distinctions.

## 5.1 `no_free_function`

Required:

```text
evidence_kind = no_free_function
formal_index absent
actual_variable absent
```

Meaning:

```text
the call's callee is certified nofree at function level
under the cited LLVM/TLI evidence.
```

## 5.2 `no_free_formal`

Required:

```text
evidence_kind = no_free_formal
formal_index = i
actual_variable = canonical Rust actual bound to formal i
```

Meaning:

```text
the callee does not free the pointer copy supplied through formal i.
```

## 5.3 `no_capture_formal`

Required:

```text
evidence_kind = no_capture_formal
formal_index = i
actual_variable = canonical Rust actual bound to formal i
```

Meaning:

```text
the callee does not capture that particular formal pointer copy.
```

---

# 6. Eligibility

An ENE1 record may be materialized only when all required identity conditions hold:

```text
real Rust/CREMA call node
exact external callee identity
external declaration/bodyless call
no represented analyzable body
stable callsite identity
verified LLVM/TLI negative evidence
```

For formal-specific evidence additionally require:

```text
valid formal index
canonical Rust actual for that exact formal
same Rust function scope
```

If formal→actual identity is unresolved:

```text
do not attach formal-specific negative evidence to another variable
```

Fail closed.

---

# 7. Body-present suppression

D3 is specifically a bodyless external evidence capability.

Required invariant:

```text
represented_body(call)
    =>
external_negative_evidence_v1 records for that call = 0
```

This does not mean LLVM attributes on represented functions are semantically false.

It means D3's **bodyless summary capability** does not duplicate represented-body analysis.

---

# 8. Semantic consumption rule: evidence only in D3 V1

This is the most important architectural constraint.

D3 V1 MUST NOT synthesize any ordinary semantic event:

```text
no drop
no read
no write
no allocation
no reallocation
no return alias
no escape event
```

D3 V1 MUST NOT directly change:

```text
allocation lifecycle state
points_to
access_bases
deallocation eligibility
allocator family
leak obligation state
```

D3 V1 stores certified negative evidence.

The following existing query outputs therefore remain unchanged for all pre-D3 targets:

```text
truth
assessment.subresult
assessment.direction
assessment.strength
```

The capability may be surfaced in diagnostics/proof metadata, but such metadata MUST NOT itself alter the current assessment classification in D3 V1.

A future gate may consume conjunctions such as:

```text
nofree + nocapture
nofree + nosync
alias-closed nocapture set
```

to justify stronger refinements.

That future promotion is explicitly out of scope here.

---

# 9. Mandatory anti-overinterpretation invariants

## N1 — nofree is not post-call liveness

Forbidden:

```text
no_free_function(call)
    =>
all pre-existing allocations definitely live after call
```

## N2 — nofree does not prohibit freeing newly allocated storage

Do not encode:

```text
no_free_function
    =>
callee frees nothing at all
```

## N3 — nofree does not imply nocapture

Forbidden:

```text
no_free_function
    =>
no_capture_formal(i)
```

## N4 — nocapture is formal-copy specific

Forbidden:

```text
no_capture_formal(i)
    =>
allocation-wide noescape
```

## N5 — aliased formals remain independent

If:

```text
actual(arg0) = p
actual(arg1) = p

formal0 has nocapture
formal1 lacks nocapture
```

then:

```text
formal0 negative evidence remains valid
allocation-wide noescape remains unproven
```

## N6 — absence is not positive opposite evidence

Forbidden:

```text
missing nofree => may_free
missing nocapture => capture
```

## N7 — nofree cannot suppress independent positive deallocation proof

If another already-validated capability proves:

```text
FreeArg(i)
or
ReallocArg(i)
```

D3 must not simply erase that positive evidence.

Instead, contradictory evidence at the same call must:

```text
fail closed
```

or remain explicitly unresolved according to the existing validator architecture.

It MUST NOT silently prefer negative evidence.

## N8 — no_free_formal is not allocation-wide

If the same allocation is reachable through another formal without nofree proof:

```text
no_free_formal(i)
```

does not prove:

```text
allocation cannot be freed by callee
```

## N9 — no truth/assessment promotion in D3

Any pre-existing truth or assessment delta caused solely by ENE1 is a gate failure.

---

# 10. Cross-capability consistency

D3 checker validation MUST detect at least the following contradictions when both facts refer to the same real call/formal and existing capability identity is sufficiently precise.

## 10.1 Function `nofree` versus positive pre-existing deallocation

Reject or fail closed when:

```text
no_free_function(call)
AND
validated bodyless positive FreeArg/ReallocArg
for a pre-existing allocation at the same call
```

Do not silently choose one.

## 10.2 Formal `nofree` versus positive free/realloc of same formal

Reject or fail closed when:

```text
no_free_formal(call, i)
AND
validated FreeArg/ReallocArg(call, i)
```

## 10.3 `nocapture` cross-capability scope

D3 V1 is NOT required to implement a general contradiction theorem between:

```text
nocapture(formal_i)
```

and every possible returned/derived pointer relation.

If the current repository already exposes a precise LLVM capture-consistency verifier, reuse it.

Otherwise record the limitation rather than inventing an incomplete cross-capability rule.

---

# 11. Schema requirements

Update:

```text
cqpl/schemas/annotated_icfg_v2.schema.json
```

Add:

```text
external_negative_evidence_v1
external_negative_evidence
```

Record schema must be closed:

```text
additionalProperties = false
```

Required enum:

```text
evidence_kind:
    no_free_function
    no_free_formal
    no_capture_formal
```

Recommended closed evidence-source vocabulary:

```text
llvm16_explicit_ir
llvm16_verified_tli
```

Allowed kind/source combinations:

```text
no_free_function:
    llvm16_explicit_ir
    OR llvm16_verified_tli

no_free_formal:
    llvm16_explicit_ir only

no_capture_formal:
    llvm16_explicit_ir only
```

Closed basis vocabulary:

```text
llvm16_explicit_function_nofree_v1
llvm16_tli_verified_function_nofree_v1
llvm16_explicit_formal_nofree_v1
llvm16_explicit_formal_nocapture_v1
```

Conditional schema constraints:

```text
no_free_function:
    formal_index forbidden
    actual_variable forbidden

no_free_formal:
    formal_index required
    actual_variable required

no_capture_formal:
    formal_index required
    actual_variable required
```

Capability/payload atomicity must follow existing schema conventions.

---

# 12. Checker proof obligations

Expected consumer sites:

```text
cqpl/cqpl_checker/src/main.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

Adapt only if current architecture provides a more canonical validator.

## P1 — real call node

Record `node` must:

```text
exist
be Rust-side
correspond to a call terminator / call semantic point
```

## P2 — bodyless call

Record must not certify a represented-body call.

## P3 — exact evidence kind

Only the three D3 kinds are accepted.

## P4 — exact source/basis pairing

Checker validates:

```text
kind
evidence_source
basis
```

as a closed tuple.

## P5 — formal range

For formal evidence:

```text
formal_index < exact call arity
```

## P6 — exact actual binding

For formal evidence:

```text
actual_variable
```

must be the actual argument at exactly `formal_index`.

Same-scope suffix matching alone is insufficient if exact call-argument identity is available.

## P7 — no actual for function-level nofree

Function-level evidence with an `actual_variable` is malformed.

## P8 — duplicate rejection

Reject duplicate logical records:

```text
(node, callee, evidence_kind, formal_index?)
```

## P9 — evidence provenance

If an existing LLVM/TLI capability exposes a stable record identity, ENE1 should carry/reference it and the checker should verify the link.

If no stable ID currently exists, the record must still carry enough closed provenance to distinguish:

```text
explicit IR
verified TLI
```

## P10 — no semantic transfer

Checker/model construction must not translate ENE1 into:

```text
drop/read/write/alloc/free/escape
```

events in D3 V1.

## P11 — no query logic changes

No changes to:

```text
parser
model_checker truth operators
CTL semantic laws
truth enum
query formulas
assessment direction rules
```

are authorized.

Test constructors may be extended with empty ENE1 fields only.

---

# 13. Producer implementation requirements

The implementation agent MUST inspect and reuse the current LLVM/TLI evidence pipeline.

Do not build a parallel parser if current code already contains:

```text
llvm_memory_effects_v1
EFX/TLI evidence
formal/callsite identity
```

Likely relevant files include:

```text
crema/src/cqpl_export.rs
crema/src/identity.rs
```

and any already-existing LLVM evidence representation discovered by inspection.

## 13.1 Inventory before implementation

Before editing semantic code, produce a machine-readable inventory of:

```text
bodyless callsites with explicit function nofree
bodyless callsites with verified TLI nofree
bodyless callsites with formal nofree
bodyless callsites with formal nocapture

represented-body equivalents
evidence records lacking stable actual/formal identity
```

This inventory determines actual corpus coverage.

Do not assume standard-library functions have an attribute merely because LLVM commonly models them.

## 13.2 No symbol heuristics

Function names may be used only as identifiers after LLVM/TLI evidence exists.

Forbidden:

```text
if callee == strlen => nofree
```

in D3.

## 13.3 Exact formal mapping

For formal evidence:

```text
LLVM formal i
    ->
real call actual i
    ->
canonical Rust MIR variable
```

must be proven.

## 13.4 No allocation lift

The producer MUST NOT transform `nocapture` into:

```text
allocation.noescape = true
```

or equivalent.

## 13.5 No liveness lift

The producer MUST NOT transform `nofree` into:

```text
allocation.state = live
```

after the call.

---

# 14. Unit tests — producer

At minimum:

```text
U1 explicit function nofree -> one no_free_function record
U2 verified TLI function nofree -> one record, distinct basis
U3 explicit formal nofree -> exact formal/actual record
U4 explicit nocapture -> exact formal/actual record
U5 wrong/unresolved formal identity -> no formal record
U6 represented body -> no ENE1 record
U7 no attribute -> no ENE1 record
U8 same pointer through two formals, one nocapture -> one formal record only
U9 missing nocapture does not create capture evidence
U10 missing nofree does not create free evidence
U11 nofree creates no lifecycle-state mutation
U12 nocapture creates no allocation-wide escape-state mutation
```

If the baseline corpus has no verified TLI nofree example, U2 may use the current verifier's synthetic/unit evidence path.

Do not fabricate a TLI result in end-to-end production code.

---

# 15. Unit/adversarial tests — checker

At minimum:

```text
A1 unknown evidence_kind rejected
A2 wrong basis rejected
A3 wrong source/basis pairing rejected
A4 formal nofree missing formal_index rejected
A5 formal nocapture missing actual_variable rejected
A6 function nofree carrying actual_variable rejected
A7 out-of-range formal rejected
A8 wrong actual for formal rejected
A9 cross-function actual rejected
A10 duplicate record rejected
A11 payload without capability rejected
A12 capability without payload rejected
A13 represented-body ENE1 rejected/gate-failed
A14 positive FreeArg + no_free_formal same formal contradiction fails closed
A15 positive ReallocArg + no_free_formal same formal contradiction fails closed
A16 function nofree + incompatible positive pre-existing deallocation fails closed
A17 ENE1 does not create ordinary semantic event
A18 ENE1 does not alter truth evaluator
```

---

# 16. D3 fixture strategy

Create:

```text
cqpl/bodyless_ffi_ene1_d3_fixture_manifest.json
```

Recommended IDs:

```text
b62 .. b69
```

The exact external function used for evidence-bearing fixtures MUST be chosen from the **observed LLVM/TLI inventory**, not guessed.

The fixture manifest MUST record:

```text
fixture
callee
expected evidence kind
expected evidence source
formal index if any
reason the evidence is actually available
```

## D3-F01 — b62 function nofree positive

Use an observed bodyless call with verified function-level nofree.

Required:

```text
one no_free_function record
no new drop event
no allocation-state strengthening
```

## D3-F02 — b63 formal nocapture positive

Use an observed explicit formal nocapture.

Required:

```text
one no_capture_formal(i)
exact actual binding
no allocation-wide noescape fact
```

## D3-F03 — b64 formal nofree positive

Use explicit pointer-formal nofree if available.

Required:

```text
one no_free_formal(i)
no lift to other aliases/formals
```

If no end-to-end declaration in the real pipeline exposes parameter `nofree`, keep the semantic coverage in unit tests and use this fixture slot for an additional verified function-no-free case. The gate JSON must report that formal-nofree end-to-end coverage is unavailable rather than fabricate it.

## D3-F04 — b65 aliased-formal nocapture negative control

Construct or select a call where:

```text
same pointer allocation may reach formal i and formal j
formal i has nocapture evidence
formal j lacks equivalent proof
```

Required:

```text
formal i ENE1 present
allocation-wide noescape absent
formal j negative evidence absent
```

The test must avoid C undefined behavior.

## D3-F05 — b66 nofree-only no-liveness control

Required:

```text
function nofree evidence present
no post-call "definitely live" state introduced
no query truth/assessment promotion
```

Do not attempt to simulate concurrency merely to satisfy this test.

Test the absence of the forbidden semantic lift directly in the abstract model/artifact.

## D3-F06 — b67 represented-body suppression

Represented equivalent function/body.

Required:

```text
ENE1 bodyless records = 0
```

## D3-F07 — b68 no-attribute unresolved control

Unknown/bodyless external call with no certified negative attribute.

Required:

```text
ENE1 records = 0
no inferred free
no inferred capture
```

## D3-F08 — b69 conjunction control

Where actual evidence permits, exercise:

```text
function nofree + formal nocapture
```

Required D3 V1 behavior:

```text
both proof records preserved
no truth change
no assessment change
no post-call liveness promotion
```

This fixture proves D3 stores conjunctive evidence without prematurely consuming it.

---

# 17. Existing corpus differential

After D2 is committed, the exact rendered baseline contains the accepted b01..b61 bodyless corpus.

Expected baseline target count:

```text
75
```

Expected canonical existing query cells:

```text
900
```

The gate MUST derive both from the isolated baseline run.

Candidate expected target count with b62..b69:

```text
83
```

D3 authorization:

```text
existing truth deltas      = 0
existing assessment deltas = 0
query errors               = 0
```

Because D3 V1 is evidence-only, any existing truth or assessment delta is unauthorized.

For new D3 fixtures, the core query outputs are observational controls.

The gate tests ENE1 proof surface, not arbitrary query positivity.

---

# 18. D1/D2 preservation

D3 MUST freeze and verify no semantic modifications to accepted D1 and D2 sources/specifications unless a test-constructor-only update is strictly required.

At minimum freeze:

```text
D1 normative spec
EFM2 capability doc
D1 gate scripts
b44..b51 fixture sources

D2 normative spec
ERR1 capability doc
D2 gate scripts
b52..b61 fixture sources
```

If checker test constructors require:

```text
external_negative_evidence: vec![]
```

those changes must be:

```text
byte-audited
recorded in a dedicated test-constructor preimage file
proven logic-neutral
```

following the D2 precedent.

---

# 19. Preimage closure

Before semantic editing create:

```text
cqpl/bodyless_ffi_ene1_d3_preimage_sha256.json
```

containing:

```text
baseline_commit
head_commit
semantic_source_preimages
frozen_capability_hashes
```

Every semantic source to be modified MUST be registered before modification.

The D3 verifier MUST independently validate baseline bytes:

```text
git show <D3_BASELINE>:<path>
    -> SHA256
    -> exact comparison
```

Fail on:

```text
missing expected path
extra semantic path not frozen
duplicate JSON key
git-show failure
hash mismatch
```

If additional test-constructor-only files are needed, use a second dedicated manifest analogous to D2.

---

# 20. Expected source-change surface

Likely semantic files:

```text
crema/src/cqpl_export.rs

cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

Potentially justified if current LLVM evidence representation requires it:

```text
crema/src/identity.rs
another existing LLVM/EFX evidence source file discovered during inspection
```

Normally forbidden without justification:

```text
cqpl/cqpl_checker/src/model_checker.rs
cqpl/cqpl_checker/src/explain.rs
CQPL grammar/parser
RN1/AGE1/RBF/CR capability docs
EFM2 semantic code
ERR1 semantic code
```

Empty-field additions in test constructors are allowed if separately frozen and audited.

---

# 21. D3 capability documentation

Create:

```text
cqpl/capabilities/external_negative_evidence_v1.md
```

It MUST explicitly state:

```text
nofree != post-call liveness
nofree != nocapture
nofree may coexist with freeing memory allocated during the call
nocapture applies only to the particular formal pointer copy
nocapture != allocation-wide noescape
absence of nocapture != capture
absence of nofree != free
D3 V1 changes no CQPL truth or assessment semantics
represented bodies suppress ENE1 bodyless summaries
```

It MUST pin LLVM 16 as normative semantics.

---

# 22. Gate scripts

Create:

```text
cqpl/scripts/run_bodyless_ffi_ene1_d3_gate.sh
cqpl/scripts/verify_bodyless_ffi_ene1_d3.py
```

Do not retrofit D3 into D1 or D2 gates.

---

# 23. Gate stages

## G0 — exact baseline / environment

Record:

```text
exact rendered D3 baseline
branch
HEAD
git status
rustc
cargo
python
```

Create isolated exact-baseline checkout.

## G1 — preimage closure

Verify every semantic preimage against exact baseline Git bytes.

## G2 — LLVM/TLI negative-evidence inventory

Write machine-readable inventory:

```text
explicit_function_nofree_calls
tli_verified_function_nofree_calls
explicit_formal_nofree_calls
explicit_formal_nocapture_calls
unresolved_formal_identity
represented_body_candidates
```

No semantic inference from names.

## G3 — complete software tests

Run full:

```text
CQPL
CREMA
```

No failures.

## G4 — focused ENE1 tests

Run U1–U12 and A1–A18.

## G5 — exact baseline replay

Run all baseline b01..b61 targets.

Expected derived count:

```text
75 targets
```

## G6 — candidate existing replay

Run same 75 targets.

## G7 — exact differential

Expected:

```text
query cells = 900
truth deltas = 0
assessment deltas = 0
query errors = 0
```

Counts must be derived, then compared to expected values.

## G8 — new D3 fixtures

Run b62..b69.

Inspect annotated artifacts, not only console PASS strings.

## G9 — anti-overinterpretation audit

Require machine-verifiable:

```text
post_call_liveness_promotions = 0
allocation_wide_noescape_promotions = 0
capture_inferences_from_missing_nocapture = 0
free_inferences_from_missing_nofree = 0
ordinary_semantic_events_created_by_ene1 = 0
```

## G10 — alias-formal isolation

Verify the b65-style control:

```text
one nocapture formal
same allocation reachable through unproven formal
=>
allocation-wide noescape remains unproven
```

## G11 — contradiction gate

Verify malformed/conflicting evidence cannot silently override positive:

```text
FreeArg
ReallocArg
```

proof.

## G12 — D1/D2 frozen surfaces

Require:

```text
unexpected frozen D1 modifications = 0
unexpected frozen D2 modifications = 0
```

## G13 — hygiene

Before PASS require:

```text
D3 fixture target/ dirs = 0
__pycache__ = 0
generated fixture global_icfg*.json = 0
dirty generated root ICFG files = 0
dirty callgraph_initial.dot.dot = 0
git diff --check = 0
```

Use cleanup/trap behavior analogous to hardened D1/D2 gates.

Never delete tracked source files.

---

# 24. Required gate JSON

Write:

```text
repro-results/bodyless-ffi-ene1-d3-<timestamp>/gate.json
```

with at least:

```json
{
  "schema": "cqpl_external_negative_evidence_d3_gate_v1",
  "status": "PASS|FAIL",
  "baseline_commit": "da9555a6fa6a99f3607112be6b3d36c7acc01e84",
  "capability": "external_negative_evidence_v1",

  "inventory": {
    "explicit_function_nofree_calls": 0,
    "tli_verified_function_nofree_calls": 0,
    "explicit_formal_nofree_calls": 0,
    "explicit_formal_nocapture_calls": 0,
    "unresolved_formal_identity": 0
  },

  "evidence_records": {
    "no_free_function": 0,
    "no_free_formal": 0,
    "no_capture_formal": 0,
    "invalid": 0
  },

  "anti_overinterpretation": {
    "post_call_liveness_promotions": 0,
    "allocation_wide_noescape_promotions": 0,
    "capture_inferences_from_absence": 0,
    "free_inferences_from_absence": 0,
    "ordinary_events_created": 0
  },

  "preimage_validation": {
    "checked": 0,
    "mismatches": 0,
    "errors": 0
  },

  "baseline": {
    "targets": 75,
    "query_cells": 900
  },

  "differential": {
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "query_errors": 0
  },

  "new_fixtures": {
    "expected": 8,
    "passed": 0,
    "failed": 0
  },

  "preservation": {
    "d1_unexpected_modifications": 0,
    "d2_unexpected_modifications": 0
  },

  "hygiene": {
    "fixture_target_directories": 0,
    "pycache": 0,
    "generated_global_icfg": 0,
    "dirty_generated_entries": 0,
    "git_diff_check_rc": 0
  }
}
```

The verifier may add more fields.

It MUST NOT omit these scientific claims.

---

# 25. D3 PASS criteria

D3 is accepted iff:

```text
[ ] exact post-D2 baseline rendered into this specification
[ ] exact baseline working tree is clean before implementation
[ ] semantic preimages frozen before edits
[ ] preimage verifier checks real git-show bytes
[ ] LLVM 16 semantics are the normative source
[ ] no_free_function implemented
[ ] no_free_formal implemented where evidence exists
[ ] no_capture_formal implemented
[ ] explicit IR and verified TLI nofree remain distinguishable
[ ] no symbol-name negative summaries
[ ] exact formal/actual identity validated
[ ] body-present calls get no ENE1 bodyless record
[ ] nofree creates zero post-call liveness promotions
[ ] nocapture creates zero allocation-wide noescape promotions
[ ] missing nocapture creates zero capture claims
[ ] missing nofree creates zero free claims
[ ] ENE1 creates zero ordinary semantic events
[ ] conflicting positive/negative deallocation proof fails closed
[ ] aliased-formal nocapture control passes
[ ] D1 frozen semantics unchanged
[ ] D2 frozen semantics unchanged
[ ] baseline targets = 75
[ ] existing query cells = 900
[ ] truth deltas = 0
[ ] assessment deltas = 0
[ ] query errors = 0
[ ] b62..b69 gate passes
[ ] full CQPL tests pass
[ ] full CREMA tests pass
[ ] generated artifacts cleaned
[ ] git diff --check passes
```

If exact corpus counts differ because the accepted D2 checkpoint legitimately differs from the anticipated 75×12 surface:

```text
do not silently update expected counts
report the discrepancy and stop for human review
```

---

# 26. Forbidden shortcuts

Reject D3 if it does any of the following:

```text
maps nofree directly to allocation.state=live
maps nocapture directly to allocation.noescape=true
uses libc symbol names as negative evidence
treats missing nofree as may_free
treats missing nocapture as captured
lifts one formal's nocapture across aliased unannotated formals
silently discards positive FreeArg/ReallocArg because of negative evidence
creates drop/read/write/allocation events from ENE1
changes query truth rules
changes assessment orientation rules
changes D1 EFM2 semantics
changes D2 ERR1 semantics
uses candidate output as baseline oracle
rewrites baseline after seeing candidate results
leaves build artifacts under b62..b69
commits or pushes automatically
```

---

# 27. Scientific completion claim

A successful D3 supports the claim:

> For bodyless external calls with verified LLVM-16/TLI negative evidence, CREMA can export and CQPL can validate proof-carrying function-level `nofree`, formal-level `nofree`, and formal-level `nocapture` facts tied to exact call/formal/actual identity. D3 preserves the distinction between per-copy capture evidence and allocation-wide escape properties, does not promote `nofree` to unconditional post-call liveness, does not infer positive behavior from missing attributes, and leaves existing CQPL truth and assessment semantics unchanged.

D3 does NOT support:

```text
global post-call liveness
allocation-wide noescape
thread-safe survival
complete escape analysis
complete capture analysis
all libc negative semantics
generic absence-of-effect inference
```

---

# 28. Required Codex workflow

Codex MUST work in this order.

## Step 1

Verify:

```text
HEAD == rendered D3 baseline
branch == cqpl6-bodyless-ffi-effect-gate
tree clean except normative D3 spec
```

## Step 2

Read:

```text
this D3 specification
original C_FFI_BODYLESS_LIBRARY_EFFECT_GATE
D1 specification/capability/gate
D2 specification/capability/gate
current LLVM/TLI producer evidence code
cqpl_export.rs
identity.rs
kripke.rs
main.rs
annotated_icfg_v2.schema.json
```

## Step 3

Inventory current negative LLVM/TLI evidence before semantic edits.

## Step 4

Determine exact semantic change set.

Freeze all preimage SHA-256 values before edits.

## Step 5

Add focused red/unit/adversarial tests.

## Step 6

Implement proof records only.

Do not add liveness/noescape transfer semantics.

## Step 7

Implement strict checker/schema validation and contradiction handling.

## Step 8

Add b62..b69 using evidence actually observed by the inventory.

Do not invent LLVM attributes to make a fixture pass.

## Step 9

Run focused tests.

## Step 10

Run complete CQPL and CREMA suites.

## Step 11

Run exact baseline/candidate differential.

## Step 12

Run full D3 gate and hygiene.

## Step 13

Return artifacts for independent review.

Do not commit or push.

---

# 29. Prompt for Codex

After this document has been rendered with the exact post-D2 baseline commit, paste:

```text
Implement D3 exactly according to:

  cqpl/D3_EXTERNAL_NEGATIVE_EVIDENCE_V1_GATE.md

Treat that document as the normative implementation and acceptance specification.

Critical semantic rule:
D3 V1 is proof-carrying negative evidence only.
It MUST NOT change existing CQPL truth or assessment semantics.

Implement:
  external_negative_evidence_v1

with exactly:
  no_free_function
  no_free_formal
  no_capture_formal

Evidence:
- explicit LLVM 16 attributes;
- verified existing TLI function-nofree evidence where already supported;
- NO hand-written libc negative summaries.

Do NOT infer:
- post-call liveness from nofree;
- allocation-wide noescape from nocapture;
- capture from missing nocapture;
- free from missing nofree.

A nocapture fact is for the particular formal pointer copy only.
If the same allocation also reaches another unannotated formal, no
allocation-wide noescape fact may be produced.

Function-level nofree does not mean the callee frees no memory at all and
does not by itself prevent another thread from freeing captured storage.

Do not let negative evidence silently override existing positive
FreeArg/ReallocArg evidence. Contradictions must fail closed.

Required outcome for the exact pre-D3 corpus:
  existing truth deltas = 0
  existing assessment deltas = 0
  query errors = 0

Preserve D1/EFM2 and D2/ERR1 exactly.

Before editing:
- verify exact HEAD from the rendered specification;
- inventory current explicit/TLI nofree/nocapture evidence;
- determine semantic source change set;
- freeze preimage hashes.

Then implement producer + checker + schema + capability doc + b62..b69 +
independent D3 gate.

Run complete CQPL and CREMA suites and the full baseline/candidate gate.

Do not commit or push.

At completion report:
- exact baseline SHA;
- exact files changed;
- semantic preimage/postimage SHA256;
- negative-evidence inventory counts;
- record counts by evidence kind/source;
- focused and full tests with exit codes;
- gate.json path and SHA256;
- baseline target/query-cell counts;
- truth/assessment/query-error differential;
- anti-overinterpretation counts;
- contradiction-test outcomes;
- b62..b69 outcomes;
- D1/D2 preservation counts;
- hygiene counts;
- git status --short;
- git diff --check;
- proposed staging/commit commands.
```

---

# 30. Human independent review package

After Codex returns PASS, do not commit.

Create a package containing:

```text
complete D3 repro-results run
gate.json
D3 normative specification
ENE1 capability document
inventory
fixture manifest
preimage manifests
all changed semantic source baseline/candidate bytes
D3 runner/verifier
b62..b69 source fixtures
full tracked patch from exact D3 baseline
git status
git diff --check
semantic postimage SHA256
package SHA256SUMS
```

Independent audit occurs before staging.
