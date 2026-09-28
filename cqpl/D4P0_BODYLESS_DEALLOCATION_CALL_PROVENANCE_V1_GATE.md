# D4-P0 — Bodyless Deallocation Call Provenance V1 (Corrected)

**Status:** normative prerequisite subgate for D4  
**Revision:** corrected after pre-edit inventory audit  
**Gate ID:** `D4P0_BODYLESS_DEALLOCATION_CALL_PROVENANCE_V1`  
**Capability:** `external_deallocation_call_provenance_v1`  
**Payload:** `external_deallocation_call_provenance`  
**Exact baseline commit:** `8ebf4a9d7b5718d5632e946813710e45d4d9fe85`  
**Branch:** `cqpl6-bodyless-ffi-effect-gate`  
**Role:** close the call-local provenance gap discovered by the D4 preflight before ELE1 consolidation  
**Semantic status:** proof/provenance serialization only; **zero new semantic events and zero query-result changes**

---

# 0. Correction rationale

The first D4-P0 draft incorrectly equated:

```text
all direct bodyless free callsites
```

with:

```text
direct bodyless free callsites lacking allocation-specific drop labels
```

The pre-edit structured inventory established the correct partition:

```text
all structured Rust MIR calls to declared bodyless free = 44

  with allocation-specific drop label                  = 28
  without allocation-specific drop label, CR1          = 8
  without allocation-specific drop label, no CR1       = 8
```

Therefore:

```text
44 = 28 + 8 + 8
```

The previously cited `16` is only:

```text
8 + 8 = calls lacking allocation-specific drop identity
```

and is not the total admission set.

This corrected specification requires D4-P0 provenance for **all 44**
eligible direct bodyless `free` calls.

Scientific reason:

```text
D4 needs one uniform, stable, Rust-call-local deallocation-family source
for every eligible direct bodyless free call.
```

A selective 16-record protocol would leave two authorities:

```text
28 calls using allocation/drop-derived provenance
16 calls using D4-P0 provenance
```

which is exactly the heterogeneity D4 is intended to eliminate.

---

# 1. Scientific objective

Introduce a call-local provenance record representing the already-existing
MIR classification that a specific bodyless external Rust call is a direct
C-family `free` call.

The desired pipeline is:

```text
real Rust MIR Call terminator
    |
    +--> exact external declaration identity
    +--> exact callee symbol
    +--> exact call arity
    +--> exact argument position
    +--> canonical Rust actual when representable
    +--> represented-body exclusion
    |
    v
external_deallocation_call_provenance_v1
    |
    v
uniform D4 ELE1 deallocation-family source
```

D4-P0 MUST NOT add or modify any existing semantic event/state.

---

# 2. Non-goals

D4-P0 MUST NOT:

```text
add a new deallocation event
change when free is considered a drop
change deallocation identity resolution
change allocation liveness
change allocator-family semantics
change CR1/RBF/RN1
change EFM2
change ERR1
change ENE1
add a new libc deallocator
infer deallocation from a name substring
change CQPL syntax
change CTL semantics
change truth/assessment rules
```

D4-P0 is not a generalized deallocator framework.

V1 covers only the exact direct bodyless `free` call contract needed by D4.

---

# 3. Closed V1 contract

D4-P0 supports exactly one semantic tuple:

```text
callee              = free
arity               = 1
formal_index         = 0
family               = c_malloc
operation            = free
language             = c
body_status          = bodyless
certainty            = may_effect
basis                = rust_mir_exact_external_free_call_v1
```

No second callee belongs to V1.

D4-P0 does NOT classify:

```text
realloc
std::alloc::dealloc
Box drop
Vec drop
CString drop
mem::drop
user deallocator wrappers
functions whose names merely contain "free"
```

---

# 4. Exact admission rule

A D4-P0 record may be emitted only when all conditions hold:

```text
1. node is a real Rust MIR Call terminator;

2. the call target resolves through the existing Rust FFI/external
   declaration inventory;

3. the exact external symbol is `free`;

4. call arity is exactly 1;

5. `free` is present as the exact selected-crate external FFI declaration;

6. no analyzable represented C body for `free` is present in the loaded
   LLVM/SVF program for that call;

7. formal index 0 is the deallocation subject;

8. the call's actual operand at index 0 is serialized using the same
   canonical MIR argument-binding machinery already used by the producer.
```

Forbidden positive evidence:

```text
details.contains("free")
substring matching
pretty-printed call text alone
drop label alone
allocation label alone
external_deallocation_effect DummyCall record alone
```

A call satisfying the exact structured admission rule receives the provenance
record whether or not an allocation identity was successfully resolved.

---

# 5. Record contract

Conceptually:

```text
ExternalDeallocationCallProvenanceV1 {
    node
    callee
    arity

    formal_index
    actual_variable?

    family
    operation
    language

    body_status
    certainty
    basis
}
```

## 5.1 Node

Canonical Rust call node:

```text
rust::<function>::bb<N>
```

and a real `term:call` node.

## 5.2 Callee

Exactly:

```text
free
```

## 5.3 Arity / formal

Exactly:

```text
arity        = 1
formal_index = 0
```

## 5.4 Actual

When representable as canonical Rust program variable:

```text
actual_variable = canonical Rust variable
```

Otherwise:

```text
actual_variable = null
```

Do not invent a temporary.

## 5.5 Contract fields

Exactly:

```text
family      = c_malloc
operation   = free
language    = c
body_status = bodyless
certainty   = may_effect
basis       = rust_mir_exact_external_free_call_v1
```

---

# 6. Provenance is not allocation resolution

The record proves:

```text
this exact Rust MIR call is the bodyless external C-family free(formal0)
contract
```

It does NOT prove:

```text
a non-null object exists
a unique allocation is resolved
an allocation-specific drop event exists
```

Therefore all three categories below receive D4-P0 provenance:

```text
28 resolved calls with allocation-specific drop labels
8 unresolved/gap calls with CR1 corroboration
8 unresolved/gap calls without CR1 corroboration
```

and:

```text
free(NULL)
```

may also receive a call-provenance record while having no allocation-specific
drop effect.

---

# 7. Zero-semantic-change requirement

For every D4-P0 record:

```text
new ordinary semantic events caused by D4-P0 = 0
```

D4-P0 MUST NOT itself create or modify:

```text
drop
drop_l
allocation labels
allocation disposition
points_to
access_bases
allocator family
conditional realloc state
lifecycle state
```

The existing producer remains authoritative for semantics.

---

# 8. Relationship to existing proof protocols

## 8.1 `external_deallocation_effects_v1`

Do not replace it.

It describes external/C-frontier deallocation evidence.

D4-P0 describes exact Rust MIR call-local direct-free provenance.

Both may coexist.

## 8.2 CR1

CR1 remains a separate corroborating relation.

For the 8 callsites referenced by CR1:

```text
D4-P0:
    proves the exact direct free call contract

CR1:
    proves the relation between a conditional-realloc result and the
    later deallocation call
```

No ordinary event may be duplicated.

## 8.3 Allocation-specific drop labels

For the 28 resolved calls:

```text
D4-P0 record
+
existing drop/allocation label
```

are not duplicate semantic effects.

The former is provenance metadata; the latter is existing semantic state.

---

# 9. Frozen baseline inventory

Before semantic editing create:

```text
cqpl/bodyless_ffi_dcp1_d4p0_inventory.json
```

It MUST contain exactly 44 eligible structured Rust callsites.

For each:

```text
target
node
callee
formal_index
actual_variable if representable
allocation_specific_drop_label_present
cr1_corroboration_present
```

Required partition:

```text
all eligible direct bodyless free calls        = 44

with allocation-specific drop label            = 28

without allocation-specific drop label:
    CR1 corroborated                            = 8
    no CR1 corroboration                        = 8
```

Required arithmetic:

```text
44 = 28 + 8 + 8
```

If independently reconstructed counts differ:

```text
STOP
report exact discrepancy
do not silently update the oracle
```

---

# 10. Checker validation

Expected surfaces:

```text
cqpl/cqpl_checker/src/main.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

Require:

```text
capability/payload atomicity
closed tuple
real Rust term:call node
same-scope actual when present
arity = 1
formal_index = 0
bodyless represented-body exclusion
duplicate rejection
no fabricated allocation identity
```

The checker MUST NOT require an allocation-specific drop label.

---

# 11. Producer requirements

Expected primary source:

```text
crema/src/cqpl_export.rs
```

Reuse existing structured MIR/FFI identity.

Do not create a second text parser.

The positive D4-P0 path MUST NOT use:

```text
is_c_free_call_text(...)
details.contains(...)
callee substring checks
drop-label inference
```

as its proof basis.

A central helper is recommended, conceptually:

```text
bodyless_direct_c_free_call_contract(...)
```

returning a positive contract only from structured identity.

---

# 12. Required corpus controls

No b70+ fixtures.

Use existing b01..b69.

Mandatory controls:

```text
C1 resolved malloc/free call
    -> D4-P0 record present
    -> existing drop semantics unchanged

C2 b23_free_null_control
    -> D4-P0 record present
    -> no fabricated allocation-specific drop

C3 b20j_second_realloc_chain_partial
    -> D4-P0 records at identity-gap direct frees
    -> no new allocation labels

C4 CR1-corroborated direct free
    -> one D4-P0 record
    -> CR1 unchanged
    -> no duplicate ordinary event

C5 represented-body control
    -> zero D4-P0 bodyless records

C6 non-free external call
    -> zero D4-P0 records
```

---

# 13. Producer tests

At minimum:

```text
P1 exact external free/1 accepted
P2 wrong arity rejected
P3 my_free rejected
P4 free_wrapper rejected
P5 pretty call text alone rejected
P6 FFI inventory missing free rejected
P7 represented free body rejected
P8 canonical actual serialized when available
P9 non-local actual remains null
P10 provenance creates no semantic event
P11 CR1-corroborated free gets one record
P12 resolved drop-labelled free also gets one record
P13 free(NULL) contract accepted without allocation identity
P14 unrelated deallocator APIs rejected
```

---

# 14. Checker/schema adversarial tests

At minimum:

```text
A1 unknown callee rejected
A2 wrong arity rejected
A3 wrong formal_index rejected
A4 wrong family rejected
A5 wrong operation rejected
A6 wrong language rejected
A7 wrong certainty rejected
A8 wrong basis rejected
A9 unknown node rejected
A10 non-Rust node rejected
A11 non-call node rejected
A12 cross-function actual rejected
A13 unknown actual rejected
A14 duplicate record rejected
A15 payload without capability rejected
A16 capability without payload rejected
A17 represented-body record rejected
A18 additional property rejected
```

---

# 15. Preimage closure

Before editing create:

```text
cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json
```

Expected semantic files:

```text
crema/src/cqpl_export.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

If another semantic source is genuinely required:

```text
STOP
explain why
freeze its baseline hash before editing
```

Validate exact baseline bytes with:

```text
git show 8ebf4a9d7b5718d5632e946813710e45d4d9fe85:<path>
```

---

# 16. Capability documentation

Create:

```text
cqpl/capabilities/external_deallocation_call_provenance_v1.md
```

It MUST state:

```text
proof sidecar only
closed callee set = {free}
exact arity = 1
formal 0
structured MIR/FFI identity
no call-text heuristics
bodyless-only
44-call uniform coverage in frozen baseline
allocation identity not required
free(NULL) permitted
no semantic event creation
CR1 is corroborating
D4 consumes D4-P0 as deallocation-family source
```

---

# 17. Gate scripts

Create:

```text
cqpl/scripts/run_bodyless_ffi_dcp1_d4p0_gate.sh
cqpl/scripts/verify_bodyless_ffi_dcp1_d4p0.py
```

---

# 18. Full differential

Exact baseline:

```text
8ebf4a9d7b5718d5632e946813710e45d4d9fe85
```

Replay:

```text
83 baseline targets
83 candidate targets
996 existing query cells
```

Require:

```text
truth deltas       = 0
assessment deltas  = 0
query errors       = 0
```

After canonical removal only of:

```text
external_deallocation_call_provenance_v1 capability
external_deallocation_call_provenance payload
```

require:

```text
semantic_projection_differences = 0
```

Projection must include all pre-existing semantic fields/protocols.

---

# 19. Gate JSON

Required minimum:

```json
{
  "schema": "cqpl_bodyless_deallocation_call_provenance_d4p0_gate_v1",
  "status": "PASS|FAIL",
  "baseline_commit": "8ebf4a9d7b5718d5632e946813710e45d4d9fe85",
  "capability": "external_deallocation_call_provenance_v1",

  "inventory": {
    "direct_bodyless_free_calls": 44,
    "with_allocation_specific_drop": 28,
    "without_allocation_specific_drop_cr1": 8,
    "without_allocation_specific_drop_no_cr1": 8
  },

  "records": {
    "expected": 44,
    "produced": 0,
    "invalid": 0,
    "duplicates": 0
  },

  "semantic_invariance": {
    "ordinary_events_created": 0,
    "allocation_labels_added": 0,
    "allocation_dispositions_added": 0,
    "semantic_projection_differences": 0
  },

  "baseline": {
    "targets": 83,
    "query_cells": 996
  },

  "differential": {
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "query_errors": 0
  },

  "preimage_validation": {
    "checked": 0,
    "mismatches": 0,
    "errors": 0
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

---

# 20. Gate stages

## G0
Exact baseline / clean branch.

## G1
Freeze and verify the exact 44-call inventory and 28/8/8 partition.

## G2
Preimage closure.

## G3
Focused producer/checker/schema tests.

## G4
Complete CQPL and CREMA tests.

## G5
Isolated exact-baseline replay: 83 targets.

## G6
Candidate replay: 83 targets.

## G7
Record closure:

```text
produced = 44
invalid = 0
duplicates = 0
```

Every eligible call has exactly one record.
No ineligible call has a record.

## G8
Semantic projection invariance = 0.

## G9
996-cell differential = 0/0/0.

## G10
D1/D2/D3 preservation.

## G11
Hygiene.

---

# 21. Hard stop conditions

Stop if:

```text
baseline differs

eligible direct bodyless free calls != 44

partition != 28 / 8 / 8

exact free classification cannot be obtained without text heuristics

a record requires inventing an allocation identity

semantic projection changes

truth/assessment changes

D1/D2/D3 semantic behavior changes
```

---

# 22. Completion workflow

After D4-P0 PASS:

```text
STOP
produce review package
do not continue D4 proper
do not commit
do not push
```

After independent audit:

```text
commit D4-P0
push D4-P0
```

Then the new HEAD becomes the exact D4 baseline.

The existing D4 consolidation specification must then be rerendered with that
new post-D4-P0 commit before D4 starts.

---

# 23. Codex prompt

```text
Implement the corrected D4 prerequisite subgate exactly according to:

  cqpl/D4P0_BODYLESS_DEALLOCATION_CALL_PROVENANCE_V1_GATE.md

Exact baseline:

  8ebf4a9d7b5718d5632e946813710e45d4d9fe85

Important correction from the first D4-P0 draft:

The structured baseline inventory proves that there are 44 eligible direct
bodyless Rust MIR calls to declared `free`, not 16.

Required partition:

  all eligible direct bodyless free calls                = 44
  with allocation-specific drop label                    = 28
  without allocation-specific drop, CR1 corroborated     = 8
  without allocation-specific drop, no CR1               = 8

  44 = 28 + 8 + 8

The previous number 16 referred only to the identity-gap subset:

  8 + 8 = 16 calls without allocation-specific drop labels.

D4-P0 must produce ONE provenance record for ALL 44 eligible calls so that D4
has one uniform call-local source for the deallocation family.

Implement only:

  external_deallocation_call_provenance_v1

Closed contract:

  callee       = free
  arity        = 1
  formal_index = 0
  family       = c_malloc
  operation    = free
  language     = c
  body_status  = bodyless
  certainty    = may_effect
  basis        = rust_mir_exact_external_free_call_v1

Critical rules:

- exact structured Rust MIR + FFI declaration identity only;
- no pretty-text or substring heuristic;
- no new semantic event/state;
- no allocation identity required;
- free(NULL) may receive provenance;
- represented bodies receive no record;
- CR1 remains separate corroboration;
- resolved drop-labelled calls ALSO receive D4-P0 provenance.

Before editing:

1. independently reconstruct and freeze all 44 callsites;
2. verify the 28/8/8 partition;
3. determine the minimal semantic source set;
4. freeze exact baseline preimage SHA256 values;
5. validate them using git show from the exact baseline.

Create:

  cqpl/capabilities/external_deallocation_call_provenance_v1.md
  cqpl/bodyless_ffi_dcp1_d4p0_inventory.json
  cqpl/bodyless_ffi_dcp1_d4p0_preimage_sha256.json
  cqpl/scripts/run_bodyless_ffi_dcp1_d4p0_gate.sh
  cqpl/scripts/verify_bodyless_ffi_dcp1_d4p0.py

Use the existing b01..b69 corpus.
Do not add b70+.

Required final results:

  produced provenance records     = 44
  invalid records                 = 0
  duplicate records               = 0

  ordinary semantic events added  = 0
  allocation labels added         = 0
  allocation dispositions added  = 0
  semantic projection differences = 0

  baseline targets                = 83
  existing query cells            = 996
  truth deltas                    = 0
  assessment deltas               = 0
  query errors                    = 0

Run focused tests, full CQPL/CREMA suites, exact isolated baseline replay,
candidate replay, semantic projection comparison, complete 996-cell
differential, D1/D2/D3 preservation, and hygiene.

At completion produce the source-review package.

Do not commit.
Do not push.
Do not continue D4 proper after PASS.
```
