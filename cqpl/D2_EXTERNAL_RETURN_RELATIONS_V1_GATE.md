# D2 — External Return Relations V1 for Bodyless C/POSIX Library Calls

**Status:** normative implementative scientific specification  
**Gate ID:** `D2_EXTERNAL_RETURN_RELATIONS_V1`  
**Capability:** `external_return_relations_v1`  
**Baseline branch:** `cqpl6-bodyless-ffi-effect-gate`  
**Exact baseline commit:** `94a9834b7f36c1ebb63369130f6c669c50846a51`  
**Baseline semantic state:** RN1/AGE1/RBF/CR closed; D1/EFM2 closed and committed  
**Toolchain:** `nightly-2024-11-21` unless the repository itself provides a stricter already-frozen toolchain  
**Primary objective:** add proof-carrying return-identity semantics for a closed set of bodyless C/POSIX library calls without inventing fresh allocations, without conflating exact aliases with derived/interior aliases, and without changing CQPL syntax or truth semantics.

---

# 0. Scientific motivation

The current bodyless FFI implementation already models several independent semantic dimensions:

```text
allocation/deallocation
allocator family
conditional realloc
MUST-null allocation existence
per-formal read/write effects
```

What remains missing is a systematic relation between:

```text
external call result
        and
pre-existing pointer/storage identity
```

for functions whose body is unavailable.

The existing design already requires the invariant:

```text
ReturnedAlias(i) => no AllocReturn from that evidence alone
```

D2 makes this requirement concrete, versioned, proof-carrying, and independently gated.

The central distinction is:

```text
fresh owned allocation
!=
exact returned alias
!=
nullable derived/interior alias
!=
nullable borrowed external storage
```

D2 implements only the last three classes.

Fresh owned allocation remains governed by the already-existing allocation contracts for:

```text
malloc
calloc
strdup
RN1 realloc(NULL,n)
```

and conditional ordinary `realloc` remains governed by RBF/CR.

---

# 1. Baseline to preserve

D2 starts from:

```text
94a9834b7f36c1ebb63369130f6c669c50846a51
```

This baseline already contains the accepted D1 capability:

```text
external_formal_memory_effects_v2
```

with the closed read/write set:

```text
strlen
memcmp
memcpy
memmove
memset
memchr
strchr
write
```

D2 MUST NOT redefine EFM1 or EFM2.

D2 MUST NOT alter the semantics of:

```text
allocation_existence_guards_v1
reallocation_boundaries_v1/v2
conditional_reallocations_v1/v2
external_formal_memory_effects_v1/v2
external_deallocation_effects_v1
allocation contracts
allocator-family mismatch
CTL evaluation
CQPL truth lattice
normal-execution double-free assessment
```

The D1 baseline contains 65 bodyless-gate targets:

```text
57 pre-D1 targets
+
8 D1 targets b44..b51
=
65 baseline targets
```

With the current canonical 12-query matrix, the expected differential surface is:

```text
65 * 12 = 780 existing query cells
```

The D2 gate MUST compute this count from produced artifacts and MUST fail if the baseline target/query surface unexpectedly changes.

---

# 2. Scope

D2 introduces exactly one new additive capability:

```text
external_return_relations_v1
```

with payload:

```text
external_return_relations
```

The initial closed library-summary set is:

```text
memcpy
memmove
memset
memchr
strchr
getenv
```

The semantic classes are:

```text
memcpy   -> exact returned alias of formal 0
memmove  -> exact returned alias of formal 0
memset   -> exact returned alias of formal 0

memchr   -> nullable derived alias of formal 0
strchr   -> nullable derived alias of formal 0

getenv   -> nullable borrowed external storage
```

No other library function belongs to D2 V1.

---

# 3. Explicit non-goals

D2 MUST NOT implement or change:

```text
fresh allocation semantics
allocator-family inference
deallocation effects
reallocation semantics
MUST-null input analysis
nofree
nocapture
escape/capture
environment mutation invalidation
thread/interleaving semantics
general pointer arithmetic
precise byte offsets
general interprocedural alias analysis
generic C-library name heuristics
new CQPL syntax
new CTL operators
new truth values
```

In particular:

```text
malloc/calloc/strdup
```

remain fresh-owned allocation functions, not return-relation functions.

```text
realloc
```

remains conditional old-or-fresh semantics, not an ERR1 alias relation.

D2 MUST NOT use `external_return_relations_v1` to reinterpret RN1, AGE1, RBF1/RBF2, or CR1/CR2.

---

# 4. Normative return-relation taxonomy

D2 defines three relation kinds.

## 4.1 `exact_argument_alias`

Meaning:

```text
returned pointer value == source argument pointer value
```

D2 V1 functions:

```text
memcpy  -> formal 0
memmove -> formal 0
memset  -> formal 0
```

For allocation-centric reasoning:

```text
result inherits the source pointer's existing base-allocation candidates
```

but D2 MUST preserve the source pointer's pointer-provenance class.

Therefore:

```text
source is certified allocation base pointer
    => result may preserve that same base-pointer eligibility

source is derived/interior/unknown pointer
    => result MUST NOT be upgraded to a certified base pointer
```

Exact return equality does not manufacture deallocation eligibility that the source argument did not already possess.

## 4.2 `nullable_derived_alias`

Meaning:

```text
result == NULL
OR
result points within storage derived from source formal 0
```

D2 V1 functions:

```text
memchr
strchr
```

The term **derived alias** is intentional.

Do not encode this relation as “strictly interior”, because a successful result may have offset zero and therefore equal the source address.

Do not encode it as an exact alias, because the returned pointer may point at a later byte.

For allocation-centric memory-use reasoning:

```text
non-null result
    => may share the same base allocation as source formal 0
```

However:

```text
nullable_derived_alias
    !=
certified base pointer
```

and therefore the relation MUST NOT by itself authorize:

```text
free(result)
```

as a valid deallocation of the source base allocation.

This invariant is mandatory.

## 4.3 `nullable_borrowed_external`

Meaning:

```text
result == NULL
OR
result points to storage owned outside the caller's allocation obligations
```

D2 V1 function:

```text
getenv
```

The result is not:

```text
fresh caller-owned heap
alias of formal 0
malloc-family allocation
Rust global allocation
```

D2 records the external borrowed origin but MUST NOT add a caller-owned allocation record.

D2 V1 does not model invalidation caused by later:

```text
getenv
setenv
unsetenv
putenv
```

calls.

It MUST document that limitation rather than invent a liveness theorem.

---

# 5. Normative external semantics

The implementation documentation MUST pin its library semantics to stable standards/reference material.

## 5.1 Exact returned aliases

`memcpy(dst, src, n)`:

```text
returns dst
```

Normative reference:

```text
POSIX memcpy
https://pubs.opengroup.org/onlinepubs/9799919799/
or the corresponding POSIX Programmer's Manual entry
```

`memmove(dst, src, n)`:

```text
returns dst
```

Normative reference:

```text
https://pubs.opengroup.org/onlinepubs/000095399/functions/memmove.html
```

`memset(dst, c, n)`:

```text
returns dst
```

Normative reference:

```text
POSIX memset / ISO-C-aligned semantics
```

## 5.2 Nullable derived aliases

`memchr(s, c, n)`:

```text
returns pointer to located byte
or NULL if no byte is found
```

The returned non-null pointer is derived from `s`.

Normative reference:

```text
https://pubs.opengroup.org/onlinepubs/9799919799/functions/memchr.html
```

`strchr(s, c)`:

```text
returns pointer to the matching byte in the NUL-terminated string
or NULL if not found
```

The terminating NUL is part of the searched string.

The returned non-null pointer is derived from `s`.

## 5.3 Borrowed external return

`getenv(name)`:

```text
returns NULL
or a pointer to the environment variable value string
```

The result points to process-environment/static data and is not caller-owned allocation storage.

Subsequent environment-management calls may invalidate or overwrite it.

Normative reference:

```text
https://pubs.opengroup.org/onlinepubs/9699919799.2013edition/functions/getenv.html
```

## 5.4 LLVM `returned` attribute

LLVM 16 `returned` means that the function always returns that argument as its return value.

Normative reference:

```text
https://releases.llvm.org/16.0.0/docs/LangRef.html
```

D2 V1 MAY use already-existing verified LLVM `returned` evidence as corroborating exact-alias evidence if the current producer already exposes it with stable callsite/formal identity.

D2 MUST NOT add a broad new LLVM-attribute extraction subsystem merely to enlarge D2 scope.

If such evidence is not already available, D2 V1 remains a closed standard-library-summary capability.

---

# 6. Closed D2 contract

The producer and checker MUST share exactly the following six closed tuples.

| callee | arity | semantic class | relation kind | source formal | nullability | ownership | proof basis |
|---|---:|---|---|---:|---|---|---|
| `memcpy` | 3 | `memcpy_return_dst_v1` | `exact_argument_alias` | 0 | `same_as_source` | `alias_existing` | `crema_err1_closed_contract_v1` |
| `memmove` | 3 | `memmove_return_dst_v1` | `exact_argument_alias` | 0 | `same_as_source` | `alias_existing` | `crema_err1_closed_contract_v1` |
| `memset` | 3 | `memset_return_dst_v1` | `exact_argument_alias` | 0 | `same_as_source` | `alias_existing` | `crema_err1_closed_contract_v1` |
| `memchr` | 3 | `memchr_return_derived_v1` | `nullable_derived_alias` | 0 | `nullable` | `alias_existing` | `crema_err1_closed_contract_v1` |
| `strchr` | 2 | `strchr_return_derived_v1` | `nullable_derived_alias` | 0 | `nullable` | `alias_existing` | `crema_err1_closed_contract_v1` |
| `getenv` | 1 | `getenv_borrowed_environment_v1` | `nullable_borrowed_external` | none | `nullable` | `borrowed_external` | `crema_err1_closed_contract_v1` |

No seventh tuple is authorized in D2.

---

# 7. Semantic-source vocabulary

`semantic_sources` describe why the closed summary is justified.

They MUST NOT falsely claim that runtime TLI/SVF evidence was observed.

Use a closed source vocabulary such as:

```text
posix_memcpy_returns_destination_v1
posix_memmove_returns_destination_v1
posix_memset_returns_destination_v1

posix_memchr_nullable_derived_return_v1
posix_strchr_nullable_derived_return_v1

posix_getenv_nullable_borrowed_environment_v1
```

If verified LLVM `returned` evidence is genuinely present in an input artifact, an additional explicit source token MAY be used:

```text
llvm16_explicit_returned_attribute_v1
```

but only when the evidence was actually observed.

A curated-summary token is not runtime LLVM evidence.

---

# 8. Artifact contract

Introduce:

```text
capability:
    external_return_relations_v1

payload:
    external_return_relations
```

Conceptual record:

```text
ExternalReturnRelationV1 {
    node
    callee
    semantic_class
    relation_kind

    result_variable

    source_formal_index?
    source_actual_variable?

    nullability
    ownership

    basis
    semantic_sources
}
```

The exact Rust field names may adapt to existing repository conventions, but the serialized semantics MUST preserve every concept above.

## 8.1 Exact/derived source identity

For:

```text
exact_argument_alias
nullable_derived_alias
```

the record MUST contain:

```text
source_formal_index = 0
source_actual_variable = canonical Rust MIR actual at formal 0
```

## 8.2 Borrowed external return

For:

```text
nullable_borrowed_external
```

the record MUST NOT fabricate:

```text
source_formal_index
source_actual_variable
```

because the returned storage does not alias the `name` argument.

## 8.3 Result identity

`result_variable` MUST identify the real Rust MIR call destination/result place.

If the result place cannot be canonically recovered:

```text
no ERR1 relation is materialized
```

Fail closed.

Do not guess a temporary/local.

---

# 9. Producer semantics

Primary expected implementation site:

```text
crema/src/cqpl_export.rs
```

The implementation agent MUST inspect the current post-D1 architecture before editing and reuse existing MIR call/result and allocation-identity machinery.

## 9.1 Eligibility

ERR1 may be emitted only if:

```text
exact external declaration identity
AND exact symbol
AND exact ABI where represented
AND exact arity
AND no represented analyzable C body
AND canonical Rust call node
AND canonical Rust result variable
```

For source-based relations additionally require:

```text
canonical source actual at formal 0
```

## 9.2 Exact symbol matching

Forbidden:

```rust
name.contains("memcpy")
name.ends_with("strchr")
```

Required style:

```text
exact selected-crate FFI declaration
+
exact callee symbol
+
exact arity
```

## 9.3 Body-present suppression

For every D2 callee:

```text
represented_body(c)
    =>
no external_return_relations_v1 record
```

Represented-body identity remains authoritative.

## 9.4 Exact aliases

For:

```text
memcpy
memmove
memset
```

materialize:

```text
result pointer provenance = source formal 0 pointer provenance
result base-allocation candidates = source base-allocation candidates
```

Do NOT create a new allocation.

If source allocation identity is unresolved:

```text
record the return relation if pointer identity is known
but do not attach an unrelated allocation
```

## 9.5 Derived aliases

For:

```text
memchr
strchr
```

materialize a distinct **derived/base relation**, not an exact pointer equality.

Required behavior:

```text
source base allocation candidates = {a1, ..., an}

non-null result
    =>
result may derive from base allocation(s) {a1, ..., an}
```

This relation may be consumed for:

```text
read/write/UAF base-allocation association
```

but MUST NOT be consumed as:

```text
certified base pointer for deallocation
```

If the current implementation has only one undifferentiated points-to relation used both for accesses and deallocation, the agent MUST NOT simply add the derived alias to it.

Instead, introduce the smallest proof-carrying distinction necessary to preserve:

```text
access-base association
!=
base-pointer deallocation eligibility
```

This is a mandatory D2 soundness requirement.

## 9.6 Borrowed external

For:

```text
getenv
```

emit the return-relation proof record but:

```text
do not create AbstractAllocId
do not create allocator family
do not create leak obligation
do not create deallocation eligibility
do not equate result with formal 0
```

If the repository has a non-owning external-storage origin vocabulary, reuse it only if its semantics already match D2.

Do not create a pseudo malloc allocation.

## 9.7 Size-zero orthogonality

D1 EFM2 suppresses bounded memory-use effects for exact zero extents.

D2 return semantics are independent.

Therefore:

```text
memcpy(dst,src,0)
memmove(dst,src,0)
memset(dst,c,0)
```

still return `dst`.

Their exact return relation MUST still be emitted.

For:

```text
memchr(s,c,0)
```

D2 MUST NOT emit a positive derived-alias relation.

At exact zero extent there is no located byte.

D2 V1 does not require a new `known_null_return` capability.

Preferred V1 behavior:

```text
exact memchr n=0
    =>
no nullable_derived_alias record
```

unless existing, already-validated null-result machinery can represent the stronger fact without expanding D2 scope.

Dynamic/unknown/nonzero `n` retains the nullable derived relation.

---

# 10. Allocation identity and ownership invariants

These are the central D2 proof obligations.

## I1 — no fresh allocation from ERR1

For every ERR1 record:

```text
new allocation created solely by ERR1 = false
```

## I2 — no allocator-family mutation

ERR1 MUST NOT change the allocator family of an existing allocation.

## I3 — exact alias preserves source identity

For exact alias:

```text
result base-allocation set = source base-allocation set
```

at the analysis precision available at the callsite.

## I4 — exact alias preserves provenance class

Exact alias MUST NOT upgrade:

```text
derived pointer -> certified base pointer
unknown pointer -> certified base pointer
```

## I5 — derived alias shares base, not pointer equality

For derived alias:

```text
same possible base allocation
```

is allowed.

The following is forbidden:

```text
result pointer == source pointer
```

as a general fact.

## I6 — derived alias cannot discharge deallocation obligations

A `free(result)` based only on:

```text
nullable_derived_alias
```

MUST NOT be treated as a certified valid free of the source base allocation.

## I7 — borrowed external creates no caller-owned obligation

For `getenv`:

```text
allocation catalog delta attributable to ERR1 = 0
leak obligation delta attributable to ERR1 = 0
```

## I8 — source-unresolved remains unresolved

If source allocation identity is empty/ambiguous:

```text
do not invent singleton allocation identity
```

Multiple source candidates remain MAY candidates.

---

# 11. Checker validation

Expected consumer sites:

```text
cqpl/cqpl_checker/src/main.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

The agent may adapt if the current architecture has a more appropriate existing validator.

## P1 — capability/payload atomicity

Require:

```text
external_return_relations_v1 capability
IFF
non-empty external_return_relations payload
```

following the repository's existing capability convention.

## P2 — schema version

ERR1 requires:

```text
schema_version == 2
```

and the existing Rust MIR semantic identity capabilities needed to validate nodes/variables.

## P3 — real Rust call node

Every record node must:

```text
exist
be Rust
be a call terminator / call semantic node
```

## P4 — exact result scope

`result_variable` must:

```text
exist
be Rust
belong to the same Rust function scope as record.node
be the canonical result/destination of that call
```

Do not accept an arbitrary same-scope local.

## P5 — source actual validation

For exact/derived aliases:

```text
source_formal_index == 0
source_actual_variable exists
source_actual_variable belongs to same call/function scope
source_actual_variable matches formal 0 actual identity
```

## P6 — borrowed source absence

For `getenv`:

```text
source_formal_index absent
source_actual_variable absent
```

## P7 — closed six-tuple contract

Validate the complete tuple:

```text
callee
arity
semantic_class
relation_kind
source_formal_index
nullability
ownership
basis
semantic_sources
```

against the D2 closed table.

## P8 — duplicate rejection

Reject duplicate logical records:

```text
(node, callee, result_variable, relation_kind)
```

## P9 — represented-body exclusion

The gate must independently verify:

```text
represented body => no ERR1 summary
```

## P10 — no allocation contradiction

For each ERR1 call:

```text
no fresh allocation site at that call
```

unless an independent pre-existing allocation capability legitimately applies.

For the six D2 functions, no such fresh allocation is authorized.

## P11 — derived/deallocation separation

Checker/model construction MUST preserve enough provenance to ensure:

```text
derived return relation
```

does not become:

```text
base deallocation identity
```

## P12 — no truth-semantics change

D2 MUST NOT modify:

```text
CQPL grammar
CTL operators
Truth enum
three-valued connectives
query semantics
```

It enriches the model only.

---

# 12. JSON Schema requirements

Update:

```text
cqpl/schemas/annotated_icfg_v2.schema.json
```

with:

```text
external_return_relations
external_return_relation_record_v1
```

The record schema MUST be closed:

```text
additionalProperties = false
```

Required enums:

```text
callee:
    memcpy
    memmove
    memset
    memchr
    strchr
    getenv

relation_kind:
    exact_argument_alias
    nullable_derived_alias
    nullable_borrowed_external

nullability:
    same_as_source
    nullable

ownership:
    alias_existing
    borrowed_external

basis:
    crema_err1_closed_contract_v1
```

Conditional schema constraints MUST enforce:

```text
exact_argument_alias
    => source_formal_index required and == 0
    => source_actual_variable required
    => ownership == alias_existing
    => nullability == same_as_source

nullable_derived_alias
    => source_formal_index required and == 0
    => source_actual_variable required
    => ownership == alias_existing
    => nullability == nullable

nullable_borrowed_external
    => source_formal_index forbidden
    => source_actual_variable forbidden
    => ownership == borrowed_external
    => nullability == nullable
```

Payload/capability atomicity must be schema-validated where the current schema architecture supports it.

---

# 13. Capability documentation

Create:

```text
cqpl/capabilities/external_return_relations_v1.md
```

It MUST document:

1. exact baseline and scope;
2. exact six-function closed set;
3. the three relation kinds;
4. exact alias versus derived alias distinction;
5. derived alias is not deallocation eligibility;
6. borrowed external creates no allocation obligation;
7. exact zero `memchr` does not produce a derived-alias relation;
8. exact alias remains valid for zero-length `memcpy/memmove/memset`;
9. represented-body suppression;
10. exact symbol/arity admission;
11. unresolved identity remains unresolved;
12. no fresh allocation is ever created by ERR1;
13. no return-lifetime theorem for `getenv`;
14. semantic-source tokens are provenance, not fictitious runtime TLI evidence.

---

# 14. Required producer unit tests

At minimum implement the following tests.

## U1 — closed classifier

Exactly six callees accepted with exact arity:

```text
memcpy/3
memmove/3
memset/3
memchr/3
strchr/2
getenv/1
```

## U2 — wrong arity rejected

Examples:

```text
memmove/2
memchr/2
strchr/3
getenv/2
```

## U3 — exact-symbol admission

Reject semantic inference from:

```text
my_memmove
foo_strchr
getenv_wrapper
libc::memcpy
```

unless exact external declaration identity independently proves that symbol.

## U4 — represented-body suppression

For each relation category, body-present call produces:

```text
0 ERR1 records
```

## U5 — exact aliases

Verify exact source formal/result relation for:

```text
memcpy
memmove
memset
```

## U6 — derived aliases

Verify:

```text
memchr
strchr
```

produce:

```text
nullable_derived_alias
```

not:

```text
exact_argument_alias
```

## U7 — borrowed external

Verify `getenv`:

```text
relation_kind = nullable_borrowed_external
no source formal
no source actual
no allocation created
```

## U8 — zero-size orthogonality

Verify:

```text
memmove(...,0)
memcpy(...,0)
memset(...,0)
```

still produce exact return relations.

## U9 — memchr zero suppression

Verify exact:

```text
memchr(...,0)
```

produces:

```text
no derived-alias relation
```

## U10 — dynamic memchr

Dynamic/unknown `n` produces nullable derived relation.

## U11 — no allocation delta

For all six D2 functions:

```text
allocations added solely by ERR1 = 0
```

## U12 — multiple source candidates

If the source may refer to more than one allocation:

```text
result remains MAY-related to that candidate set
```

No singleton invention.

---

# 15. Required checker adversarial tests

Construct ERR1 records directly and require fail-closed rejection.

At minimum:

```text
A1  unknown callee
A2  wrong arity/callee contract
A3  wrong semantic_class
A4  wrong relation_kind
A5  wrong source formal
A6  missing source actual on exact alias
A7  source actual present on getenv
A8  wrong nullability
A9  wrong ownership
A10 wrong basis
A11 wrong semantic_sources
A12 unknown result variable
A13 result variable cross-function
A14 result variable is not call result
A15 source actual cross-function
A16 source actual does not match formal 0
A17 duplicate relation record
A18 ERR1 payload without capability
A19 capability without payload
A20 fresh allocation record fabricated for D2 call
A21 derived alias accepted as base-deallocation proof
A22 represented-body summary materialization
```

A21 is mandatory even if it requires a focused model-construction test rather than pure JSON-schema validation.

---

# 16. D2 fixture matrix

Create:

```text
cqpl/bodyless_ffi_err1_d2_fixture_manifest.json
```

Add new fixtures under:

```text
tests_and_target_repos/a-code_c_ffi_bodyless_gate/
```

Recommended IDs:

```text
b52 .. b61
```

Do not modify semantic behavior of existing b01..b51 fixtures.

## D2-F01 — `b52_memcpy_return_exact_alias_uaf`

Program shape:

```text
allocate destination base
allocate/live source
r = memcpy(dst, src, n)
free(dst)
use/dereference r
```

Required:

```text
ERR1 exact_argument_alias(formal0)
no fresh allocation at memcpy
result associated with same destination allocation
UAF result:
    unk / unk_true / true / strong_abstract_evidence
```

## D2-F02 — `b53_memmove_return_exact_alias_uaf`

Same proof shape using:

```text
memmove
```

Required UAF orientation:

```text
unk / unk_true / true / strong_abstract_evidence
```

## D2-F03 — `b54_memset_return_exact_alias_uaf`

Program:

```text
r = memset(dst, 0, n)
free(dst)
use r
```

Required:

```text
same destination allocation identity
no fresh allocation
positive UAF assessment
```

## D2-F04 — `b55_memchr_return_derived_alias_uaf`

Program shape:

```text
r = memchr(buf, needle, n)
if r != NULL:
    free(base buf)
    dereference/use r
```

Required:

```text
nullable_derived_alias(formal0)
same base allocation association for access/UAF
no exact pointer equality claim
positive UAF orientation on non-null path
```

## D2-F05 — `b56_strchr_return_derived_alias_uaf`

Program shape:

```text
r = strchr(c_string, needle)
if r != NULL:
    free(base string)
    dereference/use r
```

Required:

```text
nullable_derived_alias
same base allocation for use
positive UAF orientation
```

## D2-F06 — `b57_getenv_borrowed_no_allocation`

Program:

```text
r = getenv("PATH")
black_box(r)
```

Required:

```text
ERR1 nullable_borrowed_external
new caller-owned allocation = 0
new allocator-family entry = 0
new leak obligation = 0
no positive core allocation finding solely from getenv
```

## D2-F07 — `b58_memmove_zero_extent_return_alias`

Program:

```text
valid dst
valid src
r = memmove(dst, src, 0)
free(r) or otherwise consume r through a base-pointer-valid lifecycle
```

Required:

```text
EFM2 memory effects for call = 0
ERR1 exact return relation = 1
return relation independent of D1 zero-size suppression
no fresh allocation
```

If `free(r)` is used, the source `dst` MUST be a certified freeable base pointer so the test does not rely on undefined behavior.

## D2-F08 — `b59_memchr_zero_extent_no_derived_alias`

Program:

```text
r = memchr(buf, needle, 0)
black_box(r)
```

Required:

```text
EFM2 memory effect = 0
ERR1 derived alias relation = 0
no fresh allocation
```

D2 V1 need not prove `r == NULL`.

## D2-F09 — `b60_body_present_memmove_return_control`

Provide represented body:

```c
void *memmove(...) { ...; return dst; }
```

or an equivalent existing represented-body fixture.

Required:

```text
ERR1 records = 0
represented-body identity remains authoritative
no duplicate return relation
```

## D2-F10 — `b61_unknown_pointer_return_fail_closed`

Bodyless pointer-return function outside the closed contract:

```text
extern opaque_ptr_fn(...)
```

Required:

```text
ERR1 records = 0
fresh allocations from D2 = 0
no guessed source alias
```

---

# 17. Mandatory deallocation-safety control

D2 MUST contain at least one focused unit/integration control proving:

```text
nullable_derived_alias
```

does not automatically become a valid base-pointer free.

Preferred test form:

```text
source allocation = A
r = memchr/strchr(source,...)
record relation = nullable_derived_alias(A)

attempted deallocation identity query on r
    =>
ERR1 alone does NOT certify r as the base deallocation pointer for A
```

This test need not execute an invalid `free()` dynamically.

It may directly exercise identity/model construction.

This proof obligation is more important than maximizing end-to-end fixture count.

---

# 18. Existing targets that D2 MUST audit

D2 must explicitly inspect at least:

```text
b29_strchr_return_not_alloc
b31_memset_...
b34_memmove_returned_alias
b35_memchr_return_not_alloc

b44..b51 D1 fixtures
```

The exact `b31` directory name must be discovered from the repository rather than guessed.

Expected pre-existing behavior:

```text
return-relation proof metadata may increase
allocation catalogs must not gain fresh allocations
existing clean controls remain clean
```

For:

```text
b29
b34
b35
```

allocation catalogs MUST remain canonically equivalent to baseline except for explicitly separate ERR1 provenance structures.

No new fresh allocation is authorized.

---

# 19. Existing-corpus differential

The exact baseline is:

```text
94a9834b7f36c1ebb63369130f6c669c50846a51
```

The D2 gate MUST create an isolated exact-baseline checkout/worktree and replay all 65 baseline bodyless targets.

Candidate replay then runs:

```text
same 65 targets
+
10 D2 fixtures
=
75 candidate targets
```

Expected existing differential:

```text
baseline targets          = 65
existing query cells      = 780
existing truth deltas     = 0
existing assessment deltas= 0
query errors              = 0
```

D2 does not authorize changes to existing query truth/assessment cells.

If an existing query changes:

```text
FAIL
```

and stop for review.

Do not rewrite the oracle after seeing candidate output.

---

# 20. Preimage closure

Before editing semantic sources, create:

```text
cqpl/bodyless_ffi_err1_d2_preimage_sha256.json
```

containing:

```text
baseline_commit
head_commit
source_preimages
frozen_capability_hashes
```

The agent MUST determine the exact semantic file change set before implementation.

Expected likely files are:

```text
crema/src/cqpl_export.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

If return-identity separation genuinely requires another semantic source such as:

```text
crema/src/identity.rs
crema/src/abstract_domain.rs
```

the agent may add it only after explaining why the existing model cannot distinguish:

```text
access base relation
from
deallocation base-pointer eligibility
```

Every changed semantic source MUST be present in the preimage manifest before it is edited.

The verifier MUST independently enforce each preimage with:

```text
git show 94a9834b7f36c1ebb63369130f6c669c50846a51:<path>
    -> SHA256
    -> exact comparison
```

Fail on:

```text
missing path
extra path
duplicate key
git-show error
hash mismatch
```

Do not merely trust the JSON.

---

# 21. Frozen D1 surfaces

D2 SHOULD leave the following unchanged:

```text
cqpl/D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS_GATE.md
cqpl/capabilities/external_formal_memory_effects_v2.md
cqpl/bodyless_ffi_efm2_d1_fixture_manifest.json
cqpl/bodyless_ffi_efm2_d1_preimage_sha256.json
cqpl/scripts/run_bodyless_ffi_efm2_d1_gate.sh
cqpl/scripts/verify_bodyless_ffi_efm2_d1.py
b44..b51 source fixtures
```

The D2 gate MUST record their baseline hashes or at minimum verify that Git shows no D2 changes to these frozen D1 files.

If D2 requires changing a D1 file, stop and justify before proceeding.

---

# 22. Gate scripts

Create new scripts:

```text
cqpl/scripts/run_bodyless_ffi_err1_d2_gate.sh
cqpl/scripts/verify_bodyless_ffi_err1_d2.py
```

Do not retrofit D2 semantics into the D1 gate.

The new gate must be independently runnable from repository root.

---

# 23. Gate stages

## G0 — environment / baseline

Record:

```text
branch
HEAD
baseline commit
git status --short
git diff --check
rustc
cargo
python
```

Require baseline ancestry from:

```text
94a9834b7f36c1ebb63369130f6c669c50846a51
```

No Git tag is required.

## G1 — source preimage closure

Validate every D2 semantic preimage using exact baseline bytes from `git show`.

## G2 — complete software tests

From clean compiled state run:

```text
CQPL complete test suite
CREMA complete test suite
```

No regression accepted.

## G3 — ERR1 producer/checker tests

Run U1–U12 and A1–A22.

## G4 — exact baseline replay

Create an isolated checkout of exact baseline commit and run all 65 baseline bodyless targets.

## G5 — candidate existing replay

Run the same 65 targets on the D2 candidate.

## G6 — exact 780-cell differential

Require:

```text
truth deltas = 0
assessment deltas = 0
query errors = 0
```

## G7 — new D2 fixture matrix

Run b52..b61.

Verify semantic artifacts, not only query console output.

## G8 — proof-surface audit

For every ERR1 record verify:

```text
closed callee
closed tuple
real call node
real result local
source actual when required
no source actual for getenv
no fresh allocation at D2 call
```

Aggregate required:

```text
ERR1 closed callees = 6
invalid ERR1 records = 0
```

## G9 — relation-kind safety

Explicitly verify:

```text
exact alias propagates source base candidates
derived alias supports access-base association
derived alias does not grant base-deallocation eligibility
borrowed getenv creates no allocation/obligation
```

## G10 — D1 preservation

Require:

```text
D1 frozen source/spec/scripts unchanged
b44..b51 behavior unchanged
EFM2 capability still valid
```

## G11 — generated-artifact hygiene

Before final PASS:

```text
remove generated target/ for D2 fixtures
remove generated temporary artifacts created by the gate
restore known repository-root generated artifacts if runner mutates them
```

Fail if any remain:

```text
target/ under b52..b61
__pycache__
generated global_icfg*.json in D2 source fixtures
dirty callgraph_initial.dot.dot
dirty generated crema/ffi_functions.json
dirty generated crema/global_icfg*.json
git diff --check failure
```

Cleanup MUST also run through EXIT/trap paths where practical.

Do not delete tracked content.

---

# 24. Required gate JSON

Write:

```text
repro-results/bodyless-ffi-err1-d2-<timestamp>/gate.json
```

with at least:

```json
{
  "schema": "cqpl_external_return_relations_d2_gate_v1",
  "status": "PASS|FAIL",

  "baseline_commit": "94a9834b7f36c1ebb63369130f6c669c50846a51",
  "capability": "external_return_relations_v1",

  "preimage_validation": {
    "checked": 0,
    "mismatches": 0,
    "errors": 0
  },

  "closed_contract": {
    "callees": 6,
    "relations": 6,
    "invalid_records": 0
  },

  "relation_kinds": {
    "exact_argument_alias": 0,
    "nullable_derived_alias": 0,
    "nullable_borrowed_external": 0
  },

  "identity_safety": {
    "fresh_allocations_from_err1": 0,
    "derived_alias_base_free_certificates": 0,
    "borrowed_external_allocations": 0
  },

  "baseline": {
    "targets": 65,
    "query_cells": 780
  },

  "differential": {
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "query_errors": 0
  },

  "new_fixtures": {
    "expected": 10,
    "passed": 0,
    "failed": 0
  },

  "d1_preservation": {
    "frozen_files_modified": 0
  },

  "hygiene": {
    "fixture_target_directories": 0,
    "pycache": 0,
    "generated_global_icfg": 0,
    "dirty_callgraph_entries": 0,
    "git_diff_check_rc": 0
  }
}
```

The structure may be extended, but these facts must remain machine-readable.

---

# 25. Expected new fixture outcomes

For:

```text
b52 memcpy exact alias UAF
b53 memmove exact alias UAF
b54 memset exact alias UAF
b55 memchr derived alias UAF
b56 strchr derived alias UAF
```

targeted `use_after_free_alloc_state` should be expected to orient:

```text
truth      = unk
subresult  = unk_true
direction  = true
strength   = strong_abstract_evidence
```

provided the existing checker abstraction produces the same orientation as the accepted EFM/RN1 UAF controls.

If the implementation produces a different truth value while the proof surface is otherwise correct:

```text
do not change the oracle automatically
stop and inspect
```

For:

```text
b57 getenv borrowed
b59 memchr zero
b60 represented body
b61 unknown external pointer return
```

no new caller-owned allocation is permitted.

For `b58`, the gate must prove the orthogonality:

```text
zero-length memmove:
    EFM2 memory effect suppressed
    ERR1 exact return relation preserved
```

---

# 26. Recommended implementation/change surface

Expected:

```text
crema/src/cqpl_export.rs

cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json

cqpl/capabilities/external_return_relations_v1.md
cqpl/bodyless_ffi_err1_d2_fixture_manifest.json
cqpl/bodyless_ffi_err1_d2_preimage_sha256.json

cqpl/scripts/run_bodyless_ffi_err1_d2_gate.sh
cqpl/scripts/verify_bodyless_ffi_err1_d2.py

tests_and_target_repos/a-code_c_ffi_bodyless_gate/b52_...
...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b61_...
```

Potentially justified only if required:

```text
crema/src/identity.rs
crema/src/abstract_domain.rs
```

Default forbidden without explicit justification:

```text
cqpl/cqpl_checker/src/model_checker.rs
cqpl/cqpl_checker/src/explain.rs
CQPL parser/grammar
RN1/AGE1/RBF/CR capability documents
EFM1/EFM2 capability documents
D1 gate scripts/specification
```

---

# 27. Required development workflow

The coding agent MUST work in this order.

## Step 1 — verify clean checkpoint

Require:

```text
HEAD = 94a9834b7f36c1ebb63369130f6c669c50846a51
branch = cqpl6-bodyless-ffi-effect-gate
working tree clean
```

If HEAD differs, record the actual situation and stop before semantic edits unless the human explicitly authorizes the new baseline.

## Step 2 — inspect before editing

Read at minimum:

```text
cqpl/C_FFI_BODYLESS_LIBRARY_EFFECT_GATE.md
cqpl/D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS_GATE.md
cqpl/capabilities/external_formal_memory_effects_v2.md

crema/src/cqpl_export.rs
crema/src/identity.rs
crema/src/abstract_domain.rs

cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json

cqpl/scripts/run_bodyless_ffi_efm2_d1_gate.sh
cqpl/scripts/verify_bodyless_ffi_efm2_d1.py
cqpl/scripts/run_one_target_v6q_r1c.py
```

Inspect existing relevant fixtures:

```text
b29 strchr
b31 memset
b34 memmove
b35 memchr
b44..b51
```

Discover exact directory names rather than guessing.

## Step 3 — determine semantic change set

Before changing sources, determine which semantic files are truly necessary.

Freeze their baseline SHA-256 values in:

```text
bodyless_ffi_err1_d2_preimage_sha256.json
```

Do this before editing.

## Step 4 — red tests first

Add focused producer/checker tests and new fixture skeletons.

Where practical, demonstrate pre-implementation failure for the missing return relation.

## Step 5 — implement exact aliases first

Implement:

```text
memcpy
memmove
memset
```

with no fresh allocation.

Run focused tests.

## Step 6 — implement derived aliases

Implement:

```text
memchr
strchr
```

with a representation that preserves:

```text
same base for access
not base-free eligibility
```

Run focused adversarial tests before proceeding.

## Step 7 — implement borrowed external

Implement:

```text
getenv
```

as a non-owning, nullable external return origin.

Do not add an allocation.

## Step 8 — checker/schema

Implement closed ERR1 validation and schema atomically.

Do not weaken validation to accommodate producer output.

## Step 9 — run focused D2 tests

All unit/adversarial tests pass.

## Step 10 — run full CQPL + CREMA suites

Both complete suites pass.

## Step 11 — run full D2 gate

Run baseline replay, candidate replay, 780-cell differential, D2 matrix, D1 preservation, hygiene.

## Step 12 — inspect source diff

Record:

```text
git status --short
git diff --check
git diff --stat
semantic postimage SHA256
```

## Step 13 — no automatic commit/push

Leave validated D2 uncommitted.

Return artifacts to the human for independent audit.

---

# 28. Final PASS criteria

D2 is accepted only if all are true:

```text
[ ] exact baseline 94a9834... recorded
[ ] all semantic preimages independently verified
[ ] capability external_return_relations_v1 present
[ ] exactly six closed library tuples
[ ] exact symbol/arity admission
[ ] represented bodies suppress ERR1
[ ] memcpy result exact-aliases formal 0
[ ] memmove result exact-aliases formal 0
[ ] memset result exact-aliases formal 0
[ ] memchr result is nullable derived alias, not exact alias
[ ] strchr result is nullable derived alias, not exact alias
[ ] getenv result is nullable borrowed external
[ ] ERR1 fresh allocations = 0
[ ] ERR1 allocator-family mutations = 0
[ ] derived aliases support base association for memory use
[ ] derived aliases grant zero base-free certificates
[ ] getenv creates zero caller-owned allocations
[ ] exact zero memmove/memcpy/memset keep exact return relation
[ ] exact zero memchr creates no positive derived alias
[ ] malformed ERR1 records fail closed
[ ] duplicate ERR1 records fail closed
[ ] source/result scope identity is validated
[ ] b52..b56 targeted alias-UAF proofs pass
[ ] b57 borrowed external control passes
[ ] b58 D1/D2 zero-size orthogonality passes
[ ] b59 memchr zero control passes
[ ] b60 represented-body control passes
[ ] b61 unknown external fail-closed control passes
[ ] baseline targets = 65
[ ] existing query cells = 780
[ ] existing truth deltas = 0
[ ] existing assessment deltas = 0
[ ] query errors = 0
[ ] D1 frozen files modified = 0
[ ] CQPL full suite passes
[ ] CREMA full suite passes
[ ] git diff --check passes
[ ] generated artifacts are cleaned
```

---

# 29. Forbidden shortcuts

Reject the implementation if it:

```text
treats memchr/strchr as exact aliases
creates a new allocation for memcpy/memmove/memset/memchr/strchr/getenv
creates allocator-family metadata from ERR1
treats getenv as malloc-owned storage
allows derived aliases to certify free(base)
uses substring symbol matching
guesses result locals
guesses source actuals
merges generic pointer-return functions into the closed contract
changes EFM2 to implement D2
changes RN1/AGE1/RBF/CR
changes CQPL truth semantics
uses a dirty candidate as its own baseline
rewrites baseline oracle after seeing candidate results
leaves fixture target/ directories after PASS
commits or pushes automatically
```

---

# 30. Scientific completion claim

A successful D2 supports the claim:

> For a closed, versioned set of bodyless C/POSIX pointer-returning library declarations, CREMA distinguishes exact argument returns, nullable derived returns, and nullable borrowed external returns using proof-carrying callsite/result/source identity. Exact and derived returns reuse existing allocation bases without creating fresh allocation obligations; derived returns do not imply base-pointer deallocation eligibility; borrowed external returns create no caller-owned allocation. Existing CQPL truth semantics remain unchanged.

D2 does NOT support the stronger claims:

```text
all pointer-returning libc calls are modeled
all interior offsets are known
all environment-object lifetimes are modeled
free() validity is completely modeled
all external return attributes are consumed
general pointer provenance is solved
```

---

# 31. Prompt for the Codex coding agent

Paste this prompt from the repository root:

```text
Implement D2 exactly according to:

  cqpl/D2_EXTERNAL_RETURN_RELATIONS_V1_GATE.md

Treat that document as the normative implementation and acceptance specification.

Exact baseline:
  branch: cqpl6-bodyless-ffi-effect-gate
  commit: 94a9834b7f36c1ebb63369130f6c669c50846a51

Do not require a Git tag.

Critical semantic constraints:

1. Introduce a new additive capability:
     external_return_relations_v1
   Do not redefine EFM1 or EFM2.

2. Closed D2 library set:
     memcpy
     memmove
     memset
     memchr
     strchr
     getenv

3. Relations:
     memcpy/memmove/memset
       -> exact_argument_alias(formal0)

     memchr/strchr
       -> nullable_derived_alias(formal0)

     getenv
       -> nullable_borrowed_external

4. ERR1 must never create a fresh allocation.

5. The most important soundness invariant:
     nullable_derived_alias
       may associate accesses with the same base allocation
       BUT MUST NOT certify the returned pointer as a valid base pointer
       for deallocation.

   Do not implement memchr/strchr by blindly inserting the result into an
   undifferentiated points-to relation if that same relation is used to
   authorize frees.

6. Exact aliases preserve the source pointer's provenance. They must not
   upgrade an interior/unknown source pointer into a certified base pointer.

7. getenv:
     no caller-owned allocation
     no allocator family
     no leak obligation
     no source-formal alias
   D2 does not model later environment mutation invalidation.

8. Zero-size orthogonality:
     memcpy/memmove/memset with exact n=0 still have exact return aliases;
     memchr with exact n=0 must not get a positive derived-alias relation.

9. Exact symbol + exact arity + bodyless-only.
   No substring semantics.

10. Do not modify:
      CQPL truth semantics
      RN1/AGE1/RBF/CR semantics
      D1/EFM2 semantics
      D1 fixture behavior

Workflow:

- verify HEAD and clean tree before editing;
- inspect every file/fixture listed in section 27;
- determine the exact semantic source change set;
- freeze baseline SHA256 preimages before editing;
- add red/focused tests first;
- implement exact aliases;
- implement derived aliases with deallocation-safety separation;
- implement borrowed getenv;
- implement strict checker/schema validation;
- add b52..b61;
- create the D2 runner/verifier;
- run complete CQPL and CREMA suites;
- replay all 65 exact-baseline targets from commit 94a9834...;
- replay candidate targets;
- compare all 780 existing query cells;
- require truth deltas=0, assessment deltas=0, query errors=0;
- run all D2 fixtures;
- verify D1 frozen files are unchanged;
- clean generated artifacts and enforce hygiene;
- do not commit or push.

If current architecture cannot distinguish:
  access-base relation
from:
  base-pointer deallocation eligibility

do not weaken the invariant.
Introduce the smallest explicit provenance distinction necessary and explain it.

At completion return:
- exact files changed;
- semantic preimage and postimage SHA256;
- tests/gates with exit codes;
- D2 gate.json path and SHA256;
- baseline target/query counts;
- truth/assessment/query-error differential;
- relation counts by kind;
- fresh allocations from ERR1;
- derived-alias base-free certificate count;
- borrowed-external allocation count;
- new fixture outcomes;
- D1 preservation result;
- hygiene result;
- git status --short;
- git diff --check;
- proposed git add/commit commands.

Do not commit or push.
```

---

# 32. Human review package after Codex

After Codex reports PASS, do not commit.

Prepare an independent review package containing:

```text
complete D2 repro-results run
gate.json
D2 runner/verifier
D2 normative spec
ERR1 capability doc
fixture manifest
preimage manifest
changed semantic sources
b52..b61 sources
git diff --binary from 94a9834...
git status
git diff --check
semantic postimage SHA256
package SHA256SUMS
```

The independent audit should occur before staging the D2 commit.
