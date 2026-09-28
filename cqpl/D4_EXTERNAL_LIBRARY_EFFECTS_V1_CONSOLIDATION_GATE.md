# D4 — Consolidated External Library Effects V1

**Status:** normative final implementative scientific specification  
**Gate ID:** `D4_EXTERNAL_LIBRARY_EFFECTS_V1_CONSOLIDATION`  
**Capability:** `external_library_effects_v1`  
**Protocol acronym:** `ELE1`  
**Baseline branch:** `cqpl6-bodyless-ffi-effect-gate`  
**Exact baseline commit:** `8c7a9b3cb732e8e1760c64889476688559790396`  
**Baseline requirement:** replace this token with the clean, committed, pushed D3/ENE1 checkpoint before Codex starts  
**Toolchain:** `nightly-2024-11-21` unless the repository itself provides a stricter already-frozen toolchain  
**Primary objective:** close the bodyless-FFI implementation by consolidating all already-proven external-call effects under one canonical Rust MIR call binding and one cross-capability proof envelope, while preserving every accepted D1/D2/D3/RN1 semantic result and introducing **no new CQPL truth semantics**.

---

# 0. Role of D4

D4 is the **last implementative gate** before freezing the analyzer for empirical evaluation.

D4 is not a new bug class.

D4 does not add a new positive or negative semantic transfer.

It closes the protocol-level architecture that was originally envisioned as:

```text
Rust MIR external call
    |
    +--> exact callsite/formal/result identity
    |
    +--> proof-carrying external effect families
    |
    v
consolidated external_library_effects_v1
```

The original bodyless-library design already identified the conceptual effect vocabulary:

```text
AllocReturn
ReallocArg
FreeArg
ReadArg
WriteArg
ReturnedAlias
CertifiedNoFree
CertifiedNoCapture
```

The implementation reached those semantics incrementally through:

```text
RN1 / AGE1 / RBF / CR
D1 / EFM2
D2 / ERR1
D3 / ENE1
```

D4 now provides:

```text
one canonical call binding
+
one consolidated proof envelope
+
one cross-capability consistency validator
```

without replacing the already accepted semantic records.

---

# 1. Baseline and freeze requirement

D4 starts only after D3 / `external_negative_evidence_v1` has been:

```text
independently audited
committed
pushed
working tree clean
```

Before giving this document to Codex, replace:

```text
8c7a9b3cb732e8e1760c64889476688559790396
```

with:

```text
git rev-parse HEAD
```

from the clean post-D3 checkpoint.

At D4 start require:

```text
branch = cqpl6-bodyless-ffi-effect-gate
HEAD   = exact rendered D4 baseline
working tree clean except this D4 specification
```

Expected post-D3 corpus:

```text
b01 .. b69
```

Expected target count:

```text
83
```

Expected canonical query surface:

```text
83 * 12 = 996 existing query cells
```

The gate MUST derive the target count and query count from the exact baseline and MUST stop for human review if they differ.

---

# 2. Scientific non-goal: no new semantics

D4 MUST NOT change the meaning of:

```text
allocation existence
allocation identity
allocator family
drop/deallocation
conditional realloc
MUST-null reasoning
read/write events
return aliases
derived access bases
borrowed external storage
nofree
nocapture
leak
double-free
use-after-free
allocator mismatch
CQPL parsing
CTL evaluation
truth lattice
assessment direction/strength
normal-execution scope
```

D4 MUST NOT introduce:

```text
new truth values
new CQPL operators
new query formulas
new liveness theorem
new escape theorem
new pointer-offset theorem
new libc summary semantics
```

If implementation of D4 would require any of those:

```text
STOP
report the architectural dependency
do not enlarge D4 scope
```

---

# 3. D4 closes two residual architectural gaps

## 3.1 Canonical external-call binding

D1, D2 and D3 were implemented incrementally and therefore contain partially duplicated callsite/formal/result identity machinery.

The D3 independent audit identified a remaining hardening opportunity:

```text
the checker validates feature-local call bindings,
while some source-origin checks still rely on gate-side verification.
```

D4 introduces one canonical binding certificate:

```text
external_call_bindings
```

under capability:

```text
external_library_effects_v1
```

All consolidated bodyless effect records MUST resolve through that binding.

## 3.2 Consolidated cross-capability closure

Before D4, each capability validates its own records.

D4 additionally verifies global invariants such as:

```text
one bodyless Rust call
one canonical binding
all effect records agree on callee/arity/formal/result
no effect exists without its source proof
no duplicate effect materialization
positive and negative evidence do not silently contradict
represented bodies receive no bodyless envelope
```

This is protocol closure, not semantic strengthening.

---

# 4. Canonical binding contract

Introduce top-level payload:

```text
external_call_bindings
```

Each binding is conceptually:

```text
ExternalCallBindingV1 {
    binding_id
    node
    rust_function_scope

    callee
    arity
    abi?

    arguments: [RustActual?]
    result_variable?

    body_status
    basis
}
```

Exact field names may follow repository conventions.

Serialized semantics MUST preserve every concept above except `abi`, which may be omitted only if the current Rust MIR/FFI producer does not already expose an exact ABI identity.

## 4.1 `binding_id`

Use a deterministic callsite-local identity.

Preferred:

```text
binding_id = "ele1:" + node
```

because the canonical Rust call node is already globally unique in the annotated ICFG.

Do not introduce a cryptographic digest merely to create an opaque identifier.

The checker MUST require:

```text
binding_id == "ele1:" + node
```

or an equivalently simple deterministic encoding explicitly documented by the implementation.

## 4.2 Required binding basis

Use a closed basis:

```text
rustc_mir_external_call_binding_v1
```

The basis means:

```text
callee/arity/arguments/result were exported from the real Rust MIR call
```

It is not a cryptographic authenticity claim.

## 4.3 Arguments

`arguments` MUST preserve call arity.

Each position is:

```text
canonical Rust actual variable
or
null when no canonical variable identity exists
```

Formal-specific effects are allowed only where the corresponding argument identity is non-null and independently valid.

## 4.4 Result

`result_variable` is:

```text
canonical Rust MIR call destination
```

or absent/null when the call has no representable pointer/result identity relevant to current semantics.

Do not guess temporaries.

## 4.5 Body status

D4 V1 accepts only:

```text
body_status = bodyless
```

Represented-body calls MUST NOT receive an ELE1 bodyless binding/envelope.

---

# 5. Standalone checker closure and trust boundary

The D4 checker MUST validate a binding using only the annotated artifact as far as structurally possible:

```text
node exists
node is Rust
node is term:call
scope exists
all non-null argument variables exist
all argument variables belong to the same Rust function scope
result variable exists when present
result belongs to the same Rust function scope
arity matches arguments length
binding_id is deterministic
bodyless status is valid
```

It MUST also cross-check all feature-local identities against the canonical binding.

The implementation MUST be scientifically explicit about the trust boundary:

```text
the standalone checker proves internal artifact consistency;
the producer/gate proves that the binding originated from real Rust MIR.
```

D4 MUST NOT claim cryptographic tamper resistance.

The D4 gate MUST independently validate producer-side binding origin using the existing MIR/export test surface or a dedicated producer fixture.

---

# 6. Consolidated external library effect envelope

Introduce top-level payload:

```text
external_library_effects
```

with exactly one envelope per **effectful bodyless call**.

Conceptual record:

```text
ExternalLibraryEffectsV1 {
    binding_id
    effect_families
    effect_counts
    basis
}
```

Required basis:

```text
crema_external_library_effects_v1
```

## 6.1 Effect families

Closed vocabulary:

```text
allocation_return
reallocation
deallocation
formal_memory
return_relation
negative_evidence
```

No seventh effect family belongs to ELE1 V1.

## 6.2 Effect counts

For each family store an exact non-negative integer count.

Conceptually:

```json
{
  "allocation_return": 0,
  "reallocation": 0,
  "deallocation": 0,
  "formal_memory": 0,
  "return_relation": 0,
  "negative_evidence": 0
}
```

`effect_families` MUST equal exactly the set of families whose count is greater than zero.

An envelope with all-zero counts is forbidden.

Unknown/unresolved external calls therefore have:

```text
no ELE1 envelope
```

Absence is not negative evidence.

---

# 7. Source-of-truth rule

ELE1 is a **cross-capability proof envelope**.

It MUST NOT become a second semantic transfer layer.

The existing accepted records/events remain authoritative.

Conceptually:

```text
existing proof-carrying capability/event
        |
        +--> analysis semantics
        |
        +--> counted/referenced by ELE1
```

Forbidden:

```text
ELE1 envelope
    -> independently synthesize read/write/drop/alloc/realloc/alias/liveness
```

This prevents double materialization.

---

# 8. Effect-family mapping

Before implementing, Codex MUST inventory the exact current proof records used by the post-D3 baseline.

No heuristic reconstruction is allowed.

## 8.1 `formal_memory`

Source:

```text
external_formal_memory_effects_v2
```

Legacy EFM1 remains accepted by the checker but D4 candidate production should use the accepted current EFM2 protocol where applicable.

Count:

```text
one count unit per validated formal-memory record
```

Read and write records count separately.

## 8.2 `return_relation`

Source:

```text
external_return_relations_v1
```

Count:

```text
one count unit per validated ERR1 relation
```

## 8.3 `negative_evidence`

Source:

```text
external_negative_evidence_v1
```

Count:

```text
one count unit per validated ENE1 record
```

## 8.4 `deallocation`

Source:

```text
external_deallocation_effects_v1
```

or the exact current proof-carrying bodyless deallocation record discovered during baseline inspection.

Do not count arbitrary node drop labels unless their external-call provenance is already proven.

## 8.5 `allocation_return`

Source:

```text
existing positive proof-carrying bodyless allocation-return contract
```

including accepted malloc/calloc/strdup-style contracts and RN1 malloc-like allocation only when the current model exposes a stable positive external-call proof.

Do not infer from:

```text
pointer return type
allocation-like name
return noalias alone
```

## 8.6 `reallocation`

Source:

```text
existing RBF/CR/RN1 proof-carrying external realloc contract
```

only where the baseline architecture provides a stable call-local proof record.

Do not infer reallocation merely from callee spelling.

## 8.7 Missing stable source record

If baseline inspection finds that a lifecycle family is semantically present but lacks a stable machine-checkable proof record suitable for ELE1 counting:

```text
STOP BEFORE IMPLEMENTATION
```

Report:

```text
family
current semantic representation
why exact call-local attribution cannot be proven
minimal required representation change
```

Do not manufacture a family count from ambiguous labels.

This is a hard scientific constraint.

---

# 9. Effectful-call closure

Define:

```text
E = union of bodyless call nodes referenced by
    supported validated effect families
```

D4 candidate MUST satisfy:

```text
for every node in E:
    exactly one external_call_binding
    exactly one external_library_effects envelope

for every ELE1 envelope:
    its binding exists
    its node belongs to E
```

No envelope for an unresolved effectless call.

---

# 10. Feature-local records and `binding_id`

D4 candidate producer SHOULD attach:

```text
binding_id
```

to current feature-local records where this can be done without semantic ambiguity.

Expected candidates:

```text
EFM2 records
ERR1 records
ENE1 records
external deallocation records if schema permits
```

Because accepted legacy artifact schemas exist, the checker MUST support two modes.

## 10.1 Legacy mode

When:

```text
external_library_effects_v1 capability absent
```

accept the already-validated D1/D2/D3 legacy formats unchanged.

Do not require `binding_id`.

## 10.2 Consolidated mode

When:

```text
external_library_effects_v1 capability present
```

require every supported bodyless effect record to resolve to exactly one canonical `binding_id`.

No mixed ambiguous mode.

---

# 11. D2 legacy return-call binding migration

D2 introduced a feature-local return-call binding.

D4 MUST inspect the exact current field/payload, expected conceptually as:

```text
external_return_call_bindings
```

The final architecture should have one authority.

Preferred candidate behavior:

```text
ELE1 present
    =>
canonical external_call_bindings is authoritative
    =>
legacy ERR1-specific binding payload is not emitted
```

Checker behavior:

```text
legacy artifact without ELE1:
    accept old ERR1-specific binding

ELE1 artifact:
    require canonical external_call_bindings
    reject simultaneous second authoritative ERR1 call-binding payload
```

If removing candidate emission of the legacy D2 binding would break an externally frozen artifact contract beyond the repository's own checker/schema:

```text
STOP and report
```

Do not silently maintain two authorities.

---

# 12. Cross-capability identity invariants

For every consolidated call:

## C1 — callee agreement

All records must agree with binding:

```text
record.callee == binding.callee
```

## C2 — node agreement

All records must resolve to:

```text
record.node == binding.node
```

where the source protocol has a node field.

## C3 — formal agreement

For a formal-specific effect on formal `i`:

```text
0 <= i < binding.arity
binding.arguments[i] is non-null
record.actual == binding.arguments[i]
```

when the source record carries an actual variable.

## C4 — result agreement

For result-based effects:

```text
ERR1.result_variable == binding.result_variable
```

and equivalent for allocation/reallocation return identity where current proof records expose the result.

## C5 — function scope

All canonical variables belong to:

```text
binding.rust_function_scope
```

## C6 — bodyless agreement

Every source capability and binding must agree:

```text
bodyless
```

## C7 — arity

Any source protocol carrying arity must exactly match binding arity.

---

# 13. Cross-capability semantic invariants

D4 MUST preserve and jointly validate the already accepted semantic distinctions.

## S1 — zero extent orthogonality

For exact zero:

```text
memcpy/memmove/memset:
    EFM2 bounded memory events may be absent
    ERR1 exact return relation may remain present
```

ELE1 counts actual source records.

It must not require all families for a known callee.

For exact:

```text
memchr(...,0)
```

both the EFM2 bounded read and positive derived ERR1 relation remain absent according to D1/D2 semantics.

## S2 — derived alias safety

For `memchr` / `strchr`:

```text
return_relation may associate access base
```

but MUST NOT create or count a deallocation proof that does not independently exist.

D4 must preserve:

```text
access_bases
!=
base free eligibility
```

## S3 — borrowed external safety

For `getenv` ERR1:

```text
return_relation may exist
allocation_return count = 0
allocator-family mutation = 0
```

## S4 — nofree contradiction

Existing D3 rules remain:

```text
no_free_formal(i) + FreeArg(i)       => fail closed
no_free_formal(i) + ReallocArg(i)    => fail closed
precise no_free_function + incompatible pre-existing positive deallocation
                                      => fail closed
```

## S5 — nocapture scope

```text
no_capture_formal(i)
```

does not add:

```text
allocation-wide noescape
```

ELE1 must not change this.

## S6 — realloc remains conditional

ELE1 must not flatten:

```text
old-or-fresh
```

into:

```text
free + alloc
```

Count/provenance only.

## S7 — RN1 NULL-source semantics remain distinct

`realloc(NULL,n)` successful-result allocation semantics remain the accepted RN1 semantics.

ELE1 does not turn it into a generic exact alias or ordinary realloc source relation.

## S8 — allocator family remains source-proven

ELE1 cannot invent or rewrite allocator family.

---

# 14. No-double-counting invariant

The original bodyless-effect architecture requires:

```text
one semantic effect
one callsite identity
one event provenance
```

D4 MUST verify this globally.

For every call/family:

```text
ELE1 count == exact number of validated underlying source records
```

ELE1 itself contributes:

```text
0 ordinary semantic events
```

Required gate counters:

```text
duplicate_call_bindings = 0
duplicate_effect_records = 0
envelope_count_mismatches = 0
orphan_underlying_effects = 0
orphan_envelope_effects = 0
ordinary_events_created_by_ele1 = 0
```

---

# 15. Schema contract

Update:

```text
cqpl/schemas/annotated_icfg_v2.schema.json
```

with definitions for:

```text
external_call_binding_v1
external_library_effects_v1
```

Both must use:

```text
additionalProperties = false
```

## 15.1 Binding schema

Required:

```text
binding_id
node
rust_function_scope
callee
arity
arguments
body_status
basis
```

Optional/nullable:

```text
result_variable
abi
```

Constraints:

```text
arity >= 0
len(arguments) == arity
body_status == "bodyless"
basis == "rustc_mir_external_call_binding_v1"
```

The `len(arguments)==arity` relation may require checker enforcement if JSON Schema cannot express it directly.

## 15.2 Envelope schema

Required:

```text
binding_id
effect_families
effect_counts
basis
```

Closed family names:

```text
allocation_return
reallocation
deallocation
formal_memory
return_relation
negative_evidence
```

Basis:

```text
crema_external_library_effects_v1
```

All counts:

```text
integer >= 0
```

At least one count must be positive.

`effect_families` must contain unique entries.

Exact set/count agreement is checker-enforced if inconvenient in pure JSON Schema.

---

# 16. Capability/payload atomicity

Require:

```text
external_library_effects_v1 capability
IFF
non-empty external_call_bindings
AND
non-empty external_library_effects
```

Additionally:

```text
set(envelope.binding_id)
==
set(bindings required by effectful-call closure)
```

No half-enabled protocol.

---

# 17. Legacy compatibility obligations

The post-D4 checker MUST still accept frozen artifacts representing:

```text
pre-D1 / EFM1
D1 / EFM2
D2 / ERR1 legacy binding
D3 / ENE1
```

when ELE1 capability is absent.

The gate MUST replay exact frozen baseline artifacts through the post-D4 checker.

Required:

```text
legacy schema validation failures = 0
legacy checker failures           = 0
```

No accepted capability may be made unreadable merely because D4 exists.

---

# 18. D4 uses the existing corpus — no new semantic fixtures

D4 SHOULD NOT add b70+ semantic programs.

Reason:

```text
D4 is consolidation/protocol closure,
not a new semantic feature.
```

The accepted b01..b69 corpus already contains the necessary combinations.

Create instead:

```text
cqpl/bodyless_ffi_ele1_d4_consolidation_manifest.json
```

listing exact existing targets used as proof controls.

Required control classes include at least:

## K1 — allocation return

Select existing bodyless allocation fixtures proving:

```text
allocation_return family
```

including an RN1 NULL-source allocation control if its call-local proof is stably representable.

## K2 — reallocation

Select existing ordinary realloc fixtures proving:

```text
reallocation family
```

without flattening conditional semantics.

## K3 — deallocation

Select existing external-free fixtures proving:

```text
deallocation family
```

## K4 — EFM2 only / memory

Examples from existing corpus:

```text
strlen-like read
write-like call
```

## K5 — EFM2 + ERR1 exact alias

Required control:

```text
b52 memcpy exact alias UAF
```

or the exact corresponding existing fixture.

Expected envelope:

```text
formal_memory > 0
return_relation > 0
```

## K6 — EFM2 + ERR1 derived alias

Required:

```text
b55 memchr derived alias UAF
```

Expected:

```text
formal_memory > 0
return_relation > 0
deallocation count does not arise from derived alias
```

## K7 — EFM2/ENE1 combination

Use an existing D3 function-nofree read control where both proof families genuinely occur, e.g. the exact accepted D3 `strlen` control if still present.

Expected:

```text
formal_memory > 0
negative_evidence > 0
```

## K8 — explicit nocapture conformance

Use accepted D3 nocapture fixture.

Expected:

```text
negative_evidence > 0
```

and no noescape semantic promotion.

## K9 — zero-size orthogonality

Use:

```text
b58 zero-size memmove return alias
b59 zero-size memchr
```

## K10 — represented body

Use:

```text
b60 represented memmove
```

Expected:

```text
ELE1 bodyless binding/envelope = 0
```

## K11 — unknown external

Use:

```text
b61 unknown external
```

Expected:

```text
ELE1 envelope = 0
```

The manifest must discover/record exact target names; do not guess silently.

---

# 19. Candidate artifact differential

D4 is intended to change only:

```text
consolidation metadata
shared call binding representation
optional binding_id linkage
legacy ERR1 binding migration
```

It must not change semantic model state.

For each existing target compare baseline/candidate after canonical removal of D4-only metadata.

Define a canonical semantic projection that removes only:

```text
capability external_library_effects_v1
external_call_bindings
external_library_effects
D4-only binding_id fields
legacy-vs-canonical call-binding representation differences explicitly
authorized by section 11
```

The projected baseline/candidate artifacts MUST be equal for semantic fields, including at least:

```text
allocations
allocation labels
allocation disposition
node labels
pre/post memory state
identity
event identity
allocation_post
allocator family metadata
external formal-memory semantics
external return semantics
external negative evidence semantics
external deallocation semantics
RN1/RBF/CR semantics
```

Required:

```text
semantic_projection_differences = 0
```

---

# 20. Existing-query differential

Expected baseline:

```text
targets = 83
query cells = 996
```

D4 authorizes:

```text
truth deltas      = 0
assessment deltas = 0
query errors      = 0
```

Any non-zero delta:

```text
FAIL
```

Do not update the oracle.

---

# 21. Standalone binding adversarial tests

At minimum:

```text
B1  unknown binding node rejected
B2  non-Rust binding node rejected
B3  non-call binding node rejected
B4  wrong function scope rejected
B5  arguments length != arity rejected
B6  argument variable undeclared rejected
B7  argument variable cross-function rejected
B8  result variable undeclared rejected
B9  result variable cross-function rejected
B10 wrong deterministic binding_id rejected
B11 represented-body binding rejected
B12 duplicate binding_id rejected
B13 duplicate node binding rejected
B14 envelope references unknown binding rejected
B15 binding without required effectful envelope rejected
B16 all-zero envelope rejected
B17 effect_families/count disagreement rejected
```

---

# 22. Cross-capability adversarial tests

At minimum:

```text
X1  EFM2 formal index outside binding arity rejected
X2  EFM2 actual != canonical argument rejected
X3  ERR1 result != canonical result rejected
X4  ERR1 source actual != canonical formal0 rejected
X5  ENE1 formal actual != canonical argument rejected
X6  deallocation formal != canonical actual rejected
X7  underlying effect missing from envelope count rejected
X8  envelope claims effect without underlying proof rejected
X9  duplicate underlying effect count rejected
X10 represented-body source record + ELE1 rejected
X11 derived alias artificially counted as deallocation rejected
X12 borrowed getenv artificially counted as allocation_return rejected
X13 nofree conflicting with positive free/realloc remains fail-closed
X14 unknown call with fabricated envelope rejected
X15 legacy and canonical ERR1 call-binding authorities simultaneously active rejected
```

If allocation/reallocation source protocols expose stronger stable keys, add analogous mismatch attacks.

---

# 23. Producer tests

At minimum:

```text
P1 one canonical binding per effectful bodyless call
P2 binding arity/arguments derived from MIR call
P3 result binding derived from MIR destination
P4 exact formal identity shared across EFM2/ENE1/deallocation
P5 ERR1 uses canonical shared result/source binding
P6 one envelope per effectful call
P7 envelope counts exactly match underlying records
P8 unresolved external call gets no envelope
P9 represented body gets no bodyless binding/envelope
P10 ELE1 creates zero ordinary semantic events
P11 zero-size EFM2/ERR1 orthogonality preserved
P12 legacy ERR1-specific binding not emitted in consolidated mode
```

---

# 24. D4 preimage closure

Before semantic editing create:

```text
cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json
```

containing:

```text
baseline_commit
semantic_source_preimages
frozen_capability_hashes
frozen_gate_hashes
```

Likely semantic files include:

```text
crema/src/cqpl_export.rs

cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

Potentially:

```text
crema/src/identity.rs
```

only if canonical binding resolution genuinely belongs there.

Any additional semantic source MUST be added to the preimage manifest before editing.

The verifier MUST validate exact bytes with:

```text
git show 8c7a9b3cb732e8e1760c64889476688559790396:<path>
```

after the placeholder has been rendered.

Fail on:

```text
missing entry
extra semantic source
duplicate JSON key
git-show failure
SHA mismatch
```

---

# 25. Frozen implementation surfaces

D4 MUST freeze semantics of:

```text
RN1 / AGE1 / RBF / CR
D1 / EFM2
D2 / ERR1
D3 / ENE1
```

Expected frozen source/spec/test surfaces include:

```text
D1 spec/capability/gate
b44..b51

D2 spec/capability/gate
b52..b61

D3 spec/capability/gate
b62..b69
```

D4 MAY modify shared producer/checker/schema files because consolidation necessarily touches them.

The gate therefore distinguishes:

```text
semantic behavior preservation
```

from:

```text
byte-identical shared implementation files
```

D1/D2/D3 dedicated specs, fixture sources and dedicated gates should remain byte-identical.

---

# 26. Dedicated D4 artifacts

Create:

```text
cqpl/D4_EXTERNAL_LIBRARY_EFFECTS_V1_CONSOLIDATION_GATE.md
cqpl/capabilities/external_library_effects_v1.md

cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json
cqpl/bodyless_ffi_ele1_d4_consolidation_manifest.json

cqpl/scripts/run_bodyless_ffi_ele1_d4_gate.sh
cqpl/scripts/verify_bodyless_ffi_ele1_d4.py
```

No b70+ fixtures are required unless baseline inspection proves an essential consolidation combination is genuinely absent from b01..b69.

If a new fixture becomes necessary:

```text
STOP
explain the missing proof obligation
obtain human approval before adding it
```

---

# 27. ELE1 capability documentation

`external_library_effects_v1.md` MUST document:

```text
ELE1 is consolidation metadata, not a second transfer layer

one canonical binding per effectful bodyless call

one envelope per effectful bodyless call

six closed effect families

legacy artifact compatibility

D4 candidate canonical binding authority

represented-body suppression

no symbol-name heuristics

underlying proof records remain semantic source of truth

no new allocation/free/read/write/realloc/return/liveness semantics

checker internal-consistency trust boundary

gate-side MIR-origin validation

no cryptographic tamper-resistance claim
```

It must reference the already frozen D1/D2/D3 semantic capability documents rather than duplicating/redefining their semantics.

---

# 28. D4 gate stages

Create:

```text
cqpl/scripts/run_bodyless_ffi_ele1_d4_gate.sh
cqpl/scripts/verify_bodyless_ffi_ele1_d4.py
```

The gate stages are normative.

## G0 — environment / exact baseline

Record:

```text
branch
HEAD
exact D4 baseline
git status
git diff --check
rustc
cargo
python
```

Require exact rendered baseline ancestry.

## G1 — preimage closure

Independently validate every semantic preimage using exact baseline Git bytes.

## G2 — baseline protocol inventory

Before semantic edits/runs freeze counts of:

```text
effectful bodyless calls
allocation-return source records
reallocation source records
external deallocation records
EFM2 records
ERR1 records
ENE1 records
legacy ERR1 call bindings
represented-body controls
unknown external controls
```

## G3 — focused producer/checker tests

Run P1–P12, B1–B17 and X1–X15.

## G4 — complete software tests

Run full:

```text
CQPL
CREMA
```

No regressions.

## G5 — isolated exact-baseline replay

Replay all exact post-D3 bodyless targets.

Expected:

```text
83
```

derived from baseline tree.

## G6 — candidate replay

Replay same 83 targets.

No additional semantic targets expected.

## G7 — schema + legacy compatibility

Validate:

```text
all baseline artifacts under post-D4 schema/checker
all candidate artifacts
frozen EFM1/EFM2 artifacts
frozen ERR1 legacy artifacts
frozen ENE1 artifacts
```

## G8 — canonical call-binding verification

For every candidate ELE1 binding verify:

```text
real Rust call node
real call semantics
scope
callee
arity
arguments
result
bodyless
binding_id
```

Gate-side producer/MIR-origin tests must verify the binding was generated from the actual MIR call rather than inferred from feature records.

## G9 — envelope closure

Require:

```text
one envelope per effectful call
one binding per effectful call
exact family counts
zero orphan effects
zero fabricated effects
zero duplicates
```

## G10 — semantic projection invariance

Compare baseline/candidate canonical semantic projections.

Require:

```text
semantic_projection_differences = 0
```

## G11 — exact 996-cell differential

Require:

```text
truth deltas      = 0
assessment deltas = 0
query errors      = 0
```

## G12 — consolidation control matrix

Execute the existing-target controls from the D4 consolidation manifest.

## G13 — cross-capability contradiction suite

Run the required X tests including nofree/free/realloc contradictions.

## G14 — dedicated D1/D2/D3 preservation

Require zero unexpected modifications to dedicated:

```text
specs
capability docs
gate scripts
fixture sources
```

## G15 — no-double-counting

Require:

```text
ordinary_events_created_by_ele1 = 0
duplicate_effect_materializations = 0
```

## G16 — hygiene

Require:

```text
fixture target dirs after gate = 0
__pycache__ = 0
generated fixture global_icfg*.json = 0
dirty generated root ICFG files = 0
dirty callgraph_initial.dot.dot = 0
git diff --check = 0
```

Use hardened EXIT/trap cleanup patterns from D1–D3.

Never delete tracked content.

---

# 29. Required D4 gate JSON

Write:

```text
repro-results/bodyless-ffi-ele1-d4-<timestamp>/gate.json
```

with at least:

```json
{
  "schema": "cqpl_external_library_effects_d4_gate_v1",
  "status": "PASS|FAIL",
  "baseline_commit": "8c7a9b3cb732e8e1760c64889476688559790396",
  "capability": "external_library_effects_v1",

  "preimage_validation": {
    "checked": 0,
    "mismatches": 0,
    "errors": 0
  },

  "baseline": {
    "targets": 83,
    "query_cells": 996
  },

  "bindings": {
    "effectful_calls": 0,
    "canonical_bindings": 0,
    "duplicate_nodes": 0,
    "invalid_bindings": 0,
    "mir_origin_failures": 0
  },

  "effect_family_records": {
    "allocation_return": 0,
    "reallocation": 0,
    "deallocation": 0,
    "formal_memory": 0,
    "return_relation": 0,
    "negative_evidence": 0
  },

  "envelopes": {
    "count": 0,
    "orphan_underlying_effects": 0,
    "orphan_envelope_effects": 0,
    "count_mismatches": 0,
    "duplicate_effect_materializations": 0
  },

  "semantic_invariance": {
    "semantic_projection_differences": 0,
    "ordinary_events_created_by_ele1": 0
  },

  "legacy_compatibility": {
    "efm1_failures": 0,
    "efm2_failures": 0,
    "err1_legacy_failures": 0,
    "ene1_failures": 0
  },

  "differential": {
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "query_errors": 0
  },

  "contradictions": {
    "tests_passed": 0,
    "tests_failed": 0
  },

  "preservation": {
    "d1_unexpected_changes": 0,
    "d2_unexpected_changes": 0,
    "d3_unexpected_changes": 0
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

The verifier may add fields but MUST retain all of these scientific claims.

---

# 30. Required final source-review package

After a PASS, Codex MUST NOT commit.

Produce:

```text
repro-results/bodyless-ffi-ele1-d4-review-<timestamp>.tar.gz
```

containing:

```text
D4 gate.json
full run status
baseline/candidate semantic source bytes
preimage manifest
postimage SHA256
tracked binary patch
changed-files.txt
git status
git diff --check
D4 spec
ELE1 capability doc
consolidation manifest
runner/verifier
schema
focused test logs
full CQPL/CREMA logs
legacy compatibility evidence
semantic projection comparison
```

Include `SHA256SUMS`.

---

# 31. D4 PASS criteria

D4 is accepted iff all are true:

```text
[ ] exact post-D3 baseline rendered and verified
[ ] baseline target count = 83
[ ] baseline query cells = 996
[ ] all preimages validate against git-show baseline bytes

[ ] external_library_effects_v1 exists
[ ] canonical external_call_bindings exist
[ ] one canonical binding per effectful bodyless call
[ ] one envelope per effectful bodyless call

[ ] binding node/scope/callee/arity/arguments/result validate
[ ] binding producer origin is tested against real MIR
[ ] represented-body calls get no ELE1 binding/envelope

[ ] six effect families only
[ ] exact underlying-family counts
[ ] orphan underlying effects = 0
[ ] orphan envelope effects = 0
[ ] envelope count mismatches = 0
[ ] duplicate materializations = 0

[ ] ELE1 creates zero ordinary semantic events
[ ] semantic projection differences = 0

[ ] derived aliases remain non-deallocation provenance
[ ] borrowed getenv creates no allocation
[ ] nofree contradictions remain fail-closed
[ ] nocapture remains formal-copy-specific
[ ] realloc remains conditional
[ ] RN1 remains distinct

[ ] EFM1 legacy accepted
[ ] EFM2 legacy accepted
[ ] ERR1 legacy accepted
[ ] ENE1 legacy accepted

[ ] truth deltas = 0
[ ] assessment deltas = 0
[ ] query errors = 0

[ ] D1 dedicated surfaces preserved
[ ] D2 dedicated surfaces preserved
[ ] D3 dedicated surfaces preserved

[ ] complete CQPL suite passes
[ ] complete CREMA suite passes

[ ] generated artifacts cleaned
[ ] git diff --check passes
[ ] review package produced
[ ] no commit
[ ] no push
```

---

# 32. Hard stop conditions

Stop and request human review if any of these occur:

```text
baseline != rendered D4 commit
baseline targets != 83
baseline canonical query cells != 996

a lifecycle effect family cannot be attributed to a stable proof record
without heuristic reconstruction

D4 requires new query semantics

D4 requires changing D1/D2/D3 semantic meaning

semantic projection differs

existing truth/assessment changes

legacy accepted artifacts become invalid

two authoritative call-binding protocols remain active in consolidated mode

represented-body calls receive bodyless ELE1 envelopes
```

Do not "fix" a hard stop by weakening the checker.

---

# 33. Scientific completion claim

A successful D4 supports the claim:

> CREMA's bodyless Rust→C external-call model exposes a consolidated proof-carrying library-effect protocol. Every effectful bodyless call has one canonical Rust MIR call binding and one cross-capability envelope covering validated allocation-return, reallocation, deallocation, formal-memory, return-relation, and negative-evidence families. The envelope is non-semantic metadata: existing allocation and temporal semantics remain authoritative, legacy artifacts remain accepted, represented bodies are not duplicated, and cross-capability inconsistencies fail closed.

After D4, the implementation claim is frozen.

The project MUST NOT claim:

```text
complete libc semantics
complete C behavior
complete pointer provenance
cryptographic artifact authenticity
complete concurrency reasoning
complete escape analysis
all external calls are resolved
```

---

# 34. Post-D4 implementation freeze

After independent human audit and commit/push of D4:

```text
DO NOT create D5
```

unless empirical evaluation exposes a correctness blocker.

The next repository phase is:

```text
EVALUATION_PROTOCOL_V1
```

covering:

```text
known-vulnerability ground truth
RustSec supported-subset methodology
vulnerable/fixed pairs
real-world crate sampling
FFI-enriched cohort
precision/recall
manual triage
responsible disclosure
ablation
baseline tools
runtime/scalability
threats to validity
```

New semantic ideas discovered during evaluation should be recorded as:

```text
future work
or
post-freeze correctness fix
```

not automatically incorporated into the evaluation version.

---

# 35. Required Codex workflow

Codex MUST execute in this order.

## Step 1 — recover exact clean baseline

Require:

```text
HEAD = rendered D4 baseline
branch = cqpl6-bodyless-ffi-effect-gate
tree clean except D4 spec
```

## Step 2 — read accepted architecture

Read at minimum:

```text
D4 spec
original C_FFI_BODYLESS_LIBRARY_EFFECT_GATE
D1 spec + EFM2 capability
D2 spec + ERR1 capability
D3 spec + ENE1 capability

current cqpl_export.rs
current identity.rs
current checker kripke/main
current schema
D1/D2/D3 gate scripts
```

## Step 3 — inventory first

Before any semantic edit, map every currently accepted bodyless effect family to its exact proof record and callsite identity.

Produce a short report.

If any required family cannot be mapped without heuristic inference:

```text
STOP
```

## Step 4 — determine semantic change set

Freeze all preimages before edits.

## Step 5 — tests first

Add binding/envelope/legacy/adversarial tests.

## Step 6 — implement canonical binding

Do not implement envelope until binding tests pass.

## Step 7 — migrate consolidated ERR1 binding

Preserve legacy-reader compatibility.

## Step 8 — implement envelope

Counts/closure only.

No semantic transfer.

## Step 9 — cross-capability validation

Implement C/S/no-double-counting invariants.

## Step 10 — schema/capability documentation

Fail closed.

## Step 11 — focused tests

All pass.

## Step 12 — complete CQPL/CREMA suites

All pass.

## Step 13 — exact baseline replay

83 targets expected.

## Step 14 — candidate replay

Same 83 targets.

## Step 15 — semantic projection and 996-cell differential

All zero.

## Step 16 — legacy compatibility

All frozen accepted artifacts pass.

## Step 17 — preservation + hygiene

All pass.

## Step 18 — fresh immutable final D4 gate

Do not edit scripts during the run.

## Step 19 — source-review package

Produce it automatically.

## Step 20

Do not commit or push.

---

# 36. Prompt for Codex

After rendering the exact post-D3 baseline into this file, paste:

```text
Implement the final implementative gate D4 exactly according to:

  cqpl/D4_EXTERNAL_LIBRARY_EFFECTS_V1_CONSOLIDATION_GATE.md

Treat that document as the normative specification.

D4 is protocol consolidation, NOT a new semantic feature.

Implement:

  external_library_effects_v1

with:

  one canonical external_call_binding per effectful bodyless Rust call
  one external_library_effects envelope per effectful bodyless Rust call

Closed effect families:

  allocation_return
  reallocation
  deallocation
  formal_memory
  return_relation
  negative_evidence

Critical rule:

  existing proof records/events remain the semantic source of truth.

ELE1 MUST NOT synthesize any new:

  allocation
  drop
  read
  write
  realloc
  return alias
  liveness
  escape/noescape
  allocator-family
  identity

semantics.

Before editing:

1. verify exact rendered post-D3 baseline;
2. inventory every existing bodyless effect family and exact proof record;
3. map each effect to stable callsite identity;
4. STOP if allocation/reallocation/deallocation cannot be attributed without
   heuristics;
5. determine exact semantic source change set;
6. freeze preimages from exact baseline bytes.

The canonical binding must contain:

  binding_id
  node
  Rust function scope
  callee
  arity
  ordered arguments
  result when applicable
  bodyless status
  rustc_mir_external_call_binding_v1 basis

Feature-local D1/D2/D3 records in consolidated mode must resolve through the
canonical binding.

Preserve legacy-reader compatibility when ELE1 is absent.

In consolidated ELE1 mode, do not retain two authoritative ERR1 call-binding
protocols.

The envelope is count/closure metadata only.

For every effectful call:

  exactly one binding
  exactly one envelope
  exact underlying family counts
  zero orphan effects
  zero fabricated effects
  zero duplicates

Preserve all accepted semantic distinctions:

  EFM2 zero-size rules
  ERR1 exact/derived/borrowed distinctions
  access_bases != free eligibility
  ENE1 nofree/nocapture non-overinterpretation
  conditional realloc
  RN1 NULL-source semantics
  allocator-family proof origin

Use the existing b01..b69 corpus.
Do NOT add b70+ unless a required consolidation proof obligation is genuinely
absent; if so, STOP and ask before adding a new fixture.

Expected exact baseline after D3:

  83 targets
  996 existing query cells

Derive both counts and fail if they differ.

Required outcome:

  truth deltas = 0
  assessment deltas = 0
  query errors = 0
  semantic_projection_differences = 0
  ordinary_events_created_by_ele1 = 0
  orphan_underlying_effects = 0
  orphan_envelope_effects = 0
  duplicate_effect_materializations = 0

Run:
- focused producer tests;
- focused checker/adversarial tests;
- complete CQPL suite;
- complete CREMA suite;
- exact isolated baseline replay;
- candidate replay;
- semantic projection comparison;
- exact query differential;
- legacy EFM1/EFM2/ERR1/ENE1 compatibility;
- D1/D2/D3 preservation;
- final hygiene.

Produce:
  cqpl/capabilities/external_library_effects_v1.md
  cqpl/bodyless_ffi_ele1_d4_preimage_sha256.json
  cqpl/bodyless_ffi_ele1_d4_consolidation_manifest.json
  cqpl/scripts/run_bodyless_ffi_ele1_d4_gate.sh
  cqpl/scripts/verify_bodyless_ffi_ele1_d4.py

At completion produce a complete source-review tar automatically.

Report:
- exact baseline SHA;
- exact changed files;
- semantic preimage/postimage SHA256;
- baseline protocol inventory;
- canonical binding counts;
- effect-family counts;
- envelope closure counts;
- focused/full test exit codes;
- baseline target/query counts;
- truth/assessment/query-error differential;
- semantic projection differences;
- legacy compatibility counts;
- contradiction/adversarial outcomes;
- D1/D2/D3 preservation;
- hygiene;
- gate.json path and SHA256;
- review package path and SHA256;
- git status --short;
- git diff --check;
- proposed staging/commit commands.

Do not commit.
Do not push.
```

---

# 37. Human review after Codex

Independent review must verify at minimum:

```text
source genealogy
shared binding correctness
MIR-origin proof
legacy ERR1 migration
envelope exact counts
cross-capability contradictions
semantic projection equivalence
996-cell differential
legacy checker compatibility
no duplicate semantic transfer
D1/D2/D3 freeze
hygiene
```

Only after that audit may D4 be committed and pushed.

That commit becomes the immutable implementation version used by the journal evaluation.
