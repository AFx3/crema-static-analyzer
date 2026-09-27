# D1 — Bodyless C FFI Formal Memory-Effect Coverage Closure

**Status:** implementative scientific specification  
**Gate ID:** `D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS`  
**Baseline branch:** `cqpl6-bodyless-ffi-effect-gate`  
**Baseline commit:** `39886ed` (the pushed RN1 + HOTFIX1 + HOTFIX2 checkpoint; the implementation agent MUST resolve and record the full SHA before editing)  
**Toolchain:** `nightly-2024-11-21` unless the repository itself declares a stricter already-frozen toolchain  
**Primary objective:** complete proof-carrying, per-formal `read` / `write` semantics for a closed subset of declaration-only/bodyless C standard-library calls, without changing CQPL syntax, CTL semantics, allocation/reallocation semantics, or the meaning of previously frozen EFM1 artifacts.

---

## 0. Executive decision: D1 is a coverage gate, but the capability MUST be versioned as EFM2

The existing repository defines:

```text
external_formal_memory_effects_v1
```

as a **closed/frozen v1 contract**.

Therefore D1 MUST NOT silently add new tuples to the semantic meaning of `external_formal_memory_effects_v1`.

That would make previously frozen artifacts non-reproducible: two binaries could both claim capability `external_formal_memory_effects_v1` while accepting different closed tuples.

D1 therefore introduces:

```text
external_formal_memory_effects_v2
```

abbreviated **EFM2**.

D1 is still conceptually the "EFM1 coverage closure" gate because it closes the missing coverage discovered after EFM1, but the artifact capability is version-bumped.

### Required compatibility rule

The checker MUST accept:

```text
legacy EFM1 artifacts
```

with their original closed tuple set, original basis vocabulary, original payload shape, and original semantics.

The new producer MUST emit:

```text
external_formal_memory_effects_v2
```

for newly generated artifacts after D1.

The producer MUST NOT emit both EFM1 and EFM2 for the same `external_formal_memory_effects` payload.

---

# 1. Current baseline that D1 must preserve

At baseline, the relevant producer code is centered in:

```text
crema/src/cqpl_export.rs
```

and the checker-side validation is centered in:

```text
cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

The existing capability documentation is:

```text
cqpl/capabilities/external_formal_memory_effects_v1.md
```

The current EFM1 closed library set is:

```text
strlen
memset
memcpy
memcmp
write
```

with exact formal roles:

```text
strlen(p)
    ReadArg(0)

memset(dst, c, n)
    WriteArg(0), extent=n

memcpy(dst, src, n)
    WriteArg(0), extent=n
    ReadArg(1),  extent=n

memcmp(a, b, n)
    ReadArg(0), extent=n
    ReadArg(1), extent=n

write(fd, buf, n)
    ReadArg(1), extent=n
```

Existing EFM1 invariants that MUST remain true:

1. exact selected-crate external declaration name;
2. exact call arity;
3. represented C body suppresses the bodyless summary;
4. exact `const 0_usize` suppresses bounded memory-use events;
5. unknown/dynamic/nonzero extent remains MAY;
6. formal role and Rust actual identity are distinct;
7. no synthetic SVF variable is equated with a Rust MIR local;
8. EFM creates no allocation identity and no returned alias;
9. the resulting node event is the already-existing CQPL `read(v)` / `write(v)` vocabulary;
10. allocation-centric `read(A)` / `write(A)` arises only through existing CREMA allocation identity;
11. checker validation is fail-closed and validates a closed proof tuple;
12. bodyless summary events must not duplicate represented-body events.

D1 MUST preserve all of the above.

---

# 2. Scope

D1 implements only **memory-use effects**:

```text
ReadArg(formal_i, extent)
WriteArg(formal_i, extent)
```

for the following closed library set:

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

The actual coverage delta from baseline is:

```text
+ memmove
+ memchr
+ strchr
```

The existing five functions are re-expressed under EFM2 so the new producer emits one coherent versioned contract.

---

# 3. Explicit non-goals

D1 MUST NOT implement any of the following:

```text
ReturnedAlias
interior-pointer return identity
borrowed-return identity
AllocReturn
FreeArg
ReallocArg
nofree reasoning
nocapture reasoning
escape/capture reasoning
new allocator-family rules
new AGE1 rules
new RBF1/RBF2 rules
new CR1/CR2 rules
new CTL operators
new CQPL syntax
new truth lattice values
```

In particular:

```text
memmove(...) return value
memset(...)  return value
memcpy(...)  return value
memchr(...)  return value
strchr(...)  return value
```

MUST NOT be modeled as an alias in D1.

Those return relations belong to the later `external_return_relations_v1` gate.

D1 is complete only if it avoids accidentally solving D2.

---

# 4. Scientific model

For a bodyless external callsite `c`, let:

```text
callee(c)          = exact external symbol
args(c)            = ordered Rust MIR actual arguments
represented(c)     = true iff an analyzable C/LLVM body for that callee is represented
ffi_declared(c)    = true iff the selected-crate FFI inventory admits the exact external symbol
arity(c)           = number of MIR call arguments
```

A per-formal memory effect may be materialized only when a closed EFM2 tuple exists:

```text
Contract(callee, arity, formal_i, access, extent)
```

and:

```text
ffi_declared(c)
AND !represented(c)
AND exact_symbol_match(c)
AND exact_arity_match(c)
AND actual(formal_i) is a canonical Rust MIR local
```

Then:

```text
ReadArg(i, extent)
    =>
emit MAY read(actual_i) on the real Rust MIR call node

WriteArg(i, extent)
    =>
emit MAY write(actual_i) on the real Rust MIR call node
```

No other pointer formal receives an event.

## 4.1 No blanket `argmem` expansion

The following inference is forbidden:

```text
function-level memory(argmem: read)
AND two pointer formals
    =>
ReadArg(0) + ReadArg(1)
```

unless independent per-formal evidence or a closed curated EFM2 tuple establishes those positions.

Generic function-level `argmem` evidence is not a per-formal proof.

## 4.2 MAY interpretation

EFM2 emits memory-use evidence at the existing MAY abstraction level.

It MUST NOT convert:

```text
read/write MAY
```

into a MUST dereference claim.

This gate may improve UAF evidence and assessment but does not alter the three-valued CQPL semantics.

---

# 5. EFM2 explicit extent model

EFM2 MUST make the extent semantics explicit.

The v1 field:

```text
size_argument_index: Option<usize>
```

is retained only for legacy EFM1 deserialization/validation.

New EFM2 records MUST use:

```text
extent_kind
extent_argument_index
```

with the following closed vocabulary:

```text
extent_kind = "bytes_from_formal"
extent_kind = "c_string_until_nul"
```

Rules:

```text
bytes_from_formal
    => extent_argument_index MUST be Some(k)

c_string_until_nul
    => extent_argument_index MUST be None
```

No third extent kind is authorized in D1.

Suggested Rust representation:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMemoryExtentKind {
    BytesFromFormal,
    CStringUntilNul,
}
```

The exact internal type may differ if the repository style strongly favors another representation, but the serialized semantics MUST be equivalent.

## 5.1 Zero extent

For:

```text
extent_kind = bytes_from_formal
```

if the corresponding MIR actual is exactly:

```text
const 0_usize
```

the producer MUST emit:

```text
no read/write node event
no EFM2 record
```

For:

```text
dynamic size
unknown size
nonzero constant size
```

the MAY event remains active.

No symbolic range analysis is introduced in D1.

## 5.2 NUL-terminated extent

For:

```text
strlen
strchr
```

the extent is:

```text
c_string_until_nul
```

D1 does not attempt to compute the concrete string length.

The effect remains a MAY read on the base argument allocation.

---

# 6. Closed EFM2 semantic contract

The producer and checker MUST share the following exact closed tuples.

| callee | arity | semantic class | formal | access | extent | proof basis |
|---|---:|---|---:|---|---|---|
| `strlen` | 1 | `strlen_read_c_string_v1` | 0 | read | `c_string_until_nul` | `crema_efm2_closed_contract_v1` |
| `memset` | 3 | `memset_v1` | 0 | write | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memcpy` | 3 | `memcpy_v1` | 0 | write | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memcpy` | 3 | `memcpy_v1` | 1 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memcmp` | 3 | `memcmp_v1` | 0 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memcmp` | 3 | `memcmp_v1` | 1 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `write` | 3 | `posix_write_v1` | 1 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memmove` | 3 | `memmove_v1` | 0 | write | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memmove` | 3 | `memmove_v1` | 1 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `memchr` | 3 | `memchr_bounded_read_v1` | 0 | read | `bytes_from_formal(2)` | `crema_efm2_closed_contract_v1` |
| `strchr` | 2 | `strchr_read_c_string_v1` | 0 | read | `c_string_until_nul` | `crema_efm2_closed_contract_v1` |

No other tuple belongs to D1.

---

# 7. Frozen semantic-source vocabulary

`semantic_sources` are provenance tokens describing the external semantics from which the closed contract was derived.

They are NOT claims that TLI/SVF executed on the current bodyless call.

Existing source tokens may be retained where already frozen.

For the three new functions, introduce the following closed tokens:

## 7.1 `memmove`

```text
posix_memmove_n_byte_copy_semantics_v1
llvm16_memmove_formal_semantics_v1
llvm16_tli_memmove_recognition_v1
```

Interpretation:

```text
arg0 = destination => write
arg1 = source      => read
arg2 = byte extent
```

The LLVM token refers to LLVM-16 memmove source/destination/length semantics as corroborating documentation, not runtime evidence.

## 7.2 `memchr`

```text
posix_memchr_bounded_read_semantics_v1
llvm16_tli_memchr_recognition_v1
```

Interpretation:

```text
arg0 = searched memory => read
arg2 = maximum byte extent
```

The returned interior pointer is explicitly out of scope for D1.

## 7.3 `strchr`

```text
posix_strchr_c_string_read_semantics_v1
```

Interpretation:

```text
arg0 = NUL-terminated searched string => read
```

The nullable/interior returned pointer is explicitly out of scope for D1.

---

# 8. Normative external references for D1

The implementation documentation should freeze the following normative references.

## POSIX / ISO-C-aligned behavior

`memmove`:

```text
https://pubs.opengroup.org/onlinepubs/9799919799/functions/memmove.html
```

Semantic use:

```text
copy n bytes from source object to destination object
```

therefore:

```text
ReadArg(1, bytes_from_formal(2))
WriteArg(0, bytes_from_formal(2))
```

`memchr`:

```text
https://pubs.opengroup.org/onlinepubs/9799919799/functions/memchr.html
```

Semantic use:

```text
search initial n bytes of object
```

therefore:

```text
ReadArg(0, bytes_from_formal(2))
```

`strchr`:

```text
POSIX / ISO C <string.h> strchr semantics
```

Semantic use:

```text
search NUL-terminated string
```

therefore:

```text
ReadArg(0, c_string_until_nul)
```

## LLVM 16

Freeze against LLVM 16, matching the existing evidence-producer version:

```text
https://releases.llvm.org/16.0.0/docs/LangRef.html
https://llvm.org/doxygen/classllvm_1_1TargetLibraryInfo.html
```

The standard-library recognition/prototype layer is corroborating evidence only.

D1 MUST NOT claim that runtime TLI evidence was observed unless the actual artifact contains such evidence.

---

# 9. Producer requirements

Primary implementation site:

```text
crema/src/cqpl_export.rs
```

The agent MUST first inspect current HEAD and reuse existing abstractions rather than create a parallel exporter.

## 9.1 Contract classifier

Refactor or extend the existing:

```text
external_function_memory_contract(...)
```

so that the new producer contract is versioned.

Preferred shape:

```text
external_function_memory_contract_v2(...)
```

or an equivalent internal version field.

Do not replace exact equality with substring matching.

Forbidden:

```rust
function_called.contains("memmove")
```

Required style:

```rust
raw.trim() == "memmove"
```

plus the selected-crate FFI admission check and exact arity.

## 9.2 Body-present suppression

For every admitted function:

```text
represented_c_functions.contains(callee)
    =>
no EFM2 contract
```

This MUST apply to:

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

not merely the newly added functions.

## 9.3 Actual-variable binding

For every active rule:

```text
formal_index
    ↓
MIR argument at that exact position
    ↓
canonical_mir_local
    ↓
ProgramVarId::rust(function, actual)
```

If either canonical representation cannot be recovered:

```text
fail closed by omitting that EFM2 effect
```

Do not guess another local.

## 9.4 Allocation mapping

EFM2 MUST NOT directly synthesize:

```text
AbstractAllocId
```

The ordinary node read/write label is emitted on the actual MIR variable.

Existing allocation identity then maps that variable to zero, one, or multiple abstract allocations.

Multiple possible allocation targets remain MAY.

---

# 10. Artifact contract

The payload key remains:

```text
external_formal_memory_effects
```

to avoid duplicating semantically equivalent arrays.

Exactly one capability version may govern it:

```text
external_formal_memory_effects_v1
XOR
external_formal_memory_effects_v2
```

## 10.1 Legacy EFM1 record

Legacy records remain accepted exactly as before:

```text
node
callee
semantic_class
formal_index
access
event_variable
actual_variable
size_argument_index?
basis = crema_efm1_closed_contract_v1
semantic_sources
```

## 10.2 EFM2 record

EFM2 records contain:

```text
node
callee
semantic_class
formal_index
access
event_variable
actual_variable
extent_kind
extent_argument_index?
basis = crema_efm2_closed_contract_v1
semantic_sources
```

The new producer SHOULD omit `size_argument_index` for EFM2 records.

A checker MUST reject EFM2 records that simultaneously try to encode the same extent through incompatible legacy and v2 fields.

---

# 11. Consumer/checker proof obligations

Primary sites:

```text
cqpl/cqpl_checker/src/main.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/schemas/annotated_icfg_v2.schema.json
```

## P1 — capability/payload atomicity

For EFM2:

```text
capability present <=> non-empty external_formal_memory_effects payload present
```

subject to the existing artifact convention.

## P2 — version exclusivity

Reject:

```text
external_formal_memory_effects_v1
AND
external_formal_memory_effects_v2
```

in the same artifact.

## P3 — required base capability

EFM2 requires:

```text
schema_version == 2
mir_semantics_v2
```

and therefore the existing MIR semantic-label vocabulary.

## P4 — real Rust call node

Record node must:

```text
exist
start with rust::
carry term:call
```

## P5 — same-scope Rust actual

`actual_variable` must:

```text
exist in the variable catalog
be Rust
belong to the same Rust function scope as record.node
```

## P6 — event/actual consistency

`event_variable` and `actual_variable` must identify the same MIR local.

## P7 — matching ordinary event

The referenced node MUST already contain exactly the corresponding ordinary:

```text
read(event_variable)
```

or:

```text
write(event_variable)
```

## P8 — closed tuple

The checker MUST validate the complete tuple:

```text
callee
semantic_class
formal_index
access
extent_kind
extent_argument_index
basis
semantic_sources
```

against the closed EFM2 table.

No partially valid record is accepted.

## P9 — duplicate rejection

Reject duplicate:

```text
(node, callee, formal_index, access)
```

records.

## P10 — no generic argmem overassignment

No checker or producer fallback may infer per-formal EFM2 records from generic function-level argmem alone.

## P11 — bodyless-only provenance

The producer is responsible for not generating EFM2 for represented bodies.

The gate MUST test this independently.

## P12 — no truth-rule change

No EFM2 implementation code may change:

```text
CQPL parser
CTL evaluator
Truth enum
three-valued connectives
exists_alloc semantics
read/write query semantics
```

D1 changes only the model presented to existing queries.

---

# 12. JSON schema requirements

Update:

```text
cqpl/schemas/annotated_icfg_v2.schema.json
```

without invalidating EFM1 artifacts.

Required schema behavior:

```text
EFM1 capability
    => legacy EFM1 record schema

EFM2 capability
    => EFM2 record schema

both capabilities
    => invalid

payload without either capability
    => invalid

capability without payload
    => invalid
```

EFM2 closed enums MUST include only:

```text
strlen
memset
memcpy
memcmp
write
memmove
memchr
strchr
```

and only the semantic classes/tokens listed in this specification.

Do not make `callee` an arbitrary string for EFM2.

---

# 13. Documentation requirements

Create:

```text
cqpl/capabilities/external_formal_memory_effects_v2.md
```

Do not overwrite or redefine:

```text
cqpl/capabilities/external_formal_memory_effects_v1.md
```

The v2 document MUST state:

1. EFM2 is a strict versioned successor of EFM1;
2. EFM1 remains valid for old artifacts;
3. EFM2 has explicit extent semantics;
4. EFM2 adds `memmove`, `memchr`, `strchr`;
5. EFM2 creates no aliases/allocations;
6. EFM2 does not claim generic argmem is per-formal evidence;
7. represented bodies suppress bodyless summaries;
8. dynamic extent remains MAY;
9. exact zero bounded extent suppresses the event;
10. `semantic_sources` are documentation/evidence provenance, not claims of observed runtime TLI/SVF execution.

---

# 14. Producer unit tests

At minimum add tests proving all of the following.

## U1 — full exact classifier

The exact eight-function closed set is accepted with the required arity.

Verify exact formal roles for every function.

## U2 — wrong arity rejection

Examples:

```text
memmove / 2 args => none
memchr  / 2 args => none
strchr  / 3 args => none
```

## U3 — undeclared symbol rejection

If the exact symbol is absent from the selected-crate FFI inventory:

```text
no EFM2 contract
```

## U4 — qualified/name-substring rejection

Examples that MUST NOT be accepted solely by name matching:

```text
libc::memmove
my_memmove
memmove_wrapper
foo_strchr
```

unless some future explicit declaration-identity layer proves they are the actual library declaration.

## U5 — represented body suppression

For each new function:

```text
represented_c_functions = {callee}
    =>
no EFM2 contract
```

At least one test should cover all eight functions in a loop/table.

## U6 — correct node labels

Expected:

```text
memmove(dst,src,8)
    write(dst)
    read(src)

memchr(buf,c,8)
    read(buf)

strchr(buf,c)
    read(buf)
```

No extra read/write labels.

## U7 — zero extent

Exact:

```text
memmove(dst,src,0)
memchr(buf,c,0)
```

must produce no EFM2 memory event/record.

Also preserve zero behavior for existing:

```text
memcpy
memcmp
memset
write
```

## U8 — unknown/dynamic extent

Dynamic size must still produce MAY effects.

## U9 — record extent encoding

Assert exact EFM2 serialized semantics:

```text
extent_kind
extent_argument_index
basis
semantic_sources
```

## U10 — formal/actual separation

At least one two-pointer call (`memmove`) must prove:

```text
formal 0 -> destination actual
formal 1 -> source actual
```

without equating formal identity with Rust variable identity.

---

# 15. Checker adversarial tests

Add unit tests that construct EFM2 records directly and require fail-closed rejection.

At minimum:

```text
A1 wrong callee
A2 wrong semantic_class
A3 wrong formal_index
A4 wrong access
A5 wrong extent_kind
A6 wrong extent_argument_index
A7 wrong basis
A8 wrong semantic_sources
A9 event_variable / actual_variable mismatch
A10 missing matching node read/write event
A11 cross-function actual_variable
A12 duplicate record
A13 unknown node
A14 non-call node
A15 both EFM1 and EFM2 capabilities present
A16 EFM2 payload with only EFM1 legacy extent encoding
A17 EFM1 legacy artifact still accepted unchanged
```

The acceptance test for A17 is mandatory.

---

# 16. End-to-end D1 fixture matrix

Create a new D1 fixture manifest rather than silently changing the semantic meaning of the original B01–B15 manifest:

```text
cqpl/bodyless_ffi_efm2_d1_fixture_manifest.json
```

Use new target directories under:

```text
tests_and_target_repos/a-code_c_ffi_bodyless_gate/
```

Recommended numbering begins after the current RN1 corpus:

```text
b44+
```

## D1-F01 — memmove freed source

Suggested target:

```text
b44_memmove_freed_src_uaf_read
```

Program shape:

```text
src = Box::into_raw(...)
dst = live Box
drop(Box::from_raw(src))
memmove(dst, src, 8)
```

Required proof surface:

```text
EFM2 record:
    callee=memmove
    formal_index=1
    access=read
    extent=bytes_from_formal(2)

matching node read(src)
same source allocation identity
```

Required targeted query outcome:

```text
use_after_free_alloc_state:
    truth      = unk
    assessment = unk_true
    direction  = true
    strength   = strong_abstract_evidence
```

No allocator-mismatch evidence may be introduced by EFM2.

## D1-F02 — memmove freed destination

Suggested target:

```text
b45_memmove_freed_dst_uaf_write
```

Required EFM2 effect:

```text
formal_index=0
access=write
extent=bytes_from_formal(2)
```

Required targeted UAF assessment:

```text
unk / unk_true / true / strong_abstract_evidence
```

## D1-F03 — memchr freed buffer

Suggested target:

```text
b46_memchr_freed_buffer_uaf_read
```

Required effect:

```text
ReadArg(0, bytes_from_formal(2))
```

Required targeted UAF assessment:

```text
unk / unk_true / true / strong_abstract_evidence
```

## D1-F04 — strchr freed string

Suggested target:

```text
b47_strchr_freed_string_uaf_read
```

Use an object that was a valid NUL-terminated byte sequence before deallocation.

Required effect:

```text
ReadArg(0, c_string_until_nul)
```

Required targeted UAF assessment:

```text
unk / unk_true / true / strong_abstract_evidence
```

## D1-F05 — memmove exact-zero control

Suggested target:

```text
b48_memmove_zero_extent_no_memory_event
```

Use live valid objects and:

```text
memmove(dst, src, 0)
```

Required:

```text
no EFM2 read/write record
no EFM2-added read/write event
```

Do not use a dangling pointer merely to test the zero rule.

## D1-F06 — memchr exact-zero control

Suggested target:

```text
b49_memchr_zero_extent_no_memory_event
```

Required:

```text
no EFM2 read/write record
no EFM2-added read event
```

## D1-F07 — represented-body non-duplication

Suggested target:

```text
b50_body_present_memmove_control
```

Provide a represented C body for a function named exactly `memmove` or use an equivalent test harness that guarantees the callee body is present in the loaded C/LLVM ICFG.

Required:

```text
no EFM2 record for represented memmove
represented-body events remain the only source
event multiplicity is not duplicated
```

## D1-F08 — dynamic extent

Suggested target:

```text
b51_memmove_dynamic_extent_may_effect
```

Required:

```text
dynamic n
=> read/write EFM2 effects remain present
```

No proof of positive/nonzero size is claimed.

---

# 17. Existing fixtures that D1 MUST audit

D1 must not rely only on new tests.

Audit existing EFM1 positives:

```text
b24_memcmp_freed_left_uaf
b25_memcmp_freed_right_uaf
b26_write_after_free_uaf
```

They MUST preserve their UAF behavior.

Audit existing currently uncovered calls:

```text
b29_strchr_return_not_alloc
b34_memmove_returned_alias
b35_memchr_return_not_alloc
```

After D1 these should gain appropriate EFM2 memory-use records/events, but D1 MUST NOT create any returned allocation/alias identity.

Expected query-neutrality for these live-memory controls:

```text
no new leak truth
no new double-free truth
no new UAF truth
no new allocator-mismatch truth
```

The exact full query matrix must be compared against the baseline checkpoint.

---

# 18. Existing-corpus differential

Baseline:

```text
git commit 39886ed...
```

The gate MUST compare every pre-D1 bodyless fixture that exists at the baseline against the D1 candidate.

For pre-existing targets:

```text
unexpected truth delta      = 0
unexpected assessment delta = 0
checker rc != 0             = 0
```

D1 is a model-enrichment feature whose newly covered existing calls are live/clean controls; therefore no pre-existing query result is authorized to change in this gate.

If an existing target changes:

```text
FAIL
```

unless the developer stops and creates a new explicitly reviewed oracle amendment.

The agent MUST NOT silently relax this condition.

---

# 19. Gate implementation

Create:

```text
cqpl/scripts/run_bodyless_ffi_efm2_d1_gate.sh
cqpl/scripts/verify_bodyless_ffi_efm2_d1.py
```

Optionally add a small helper if required, but avoid a parallel framework.

The gate must be independently rerunnable from the repository root.

## G0 — baseline / repository hygiene

Record:

```text
git branch
git HEAD
git status --short
git diff --check
rustc
cargo
python
```

Require that the resolved baseline ancestry includes the pushed D1 baseline checkpoint:

```text
39886ed
```

Do not require a Git tag.

## G1 — full software tests

Run from clean compiled state:

```text
cargo +nightly-2024-11-21 test --manifest-path cqpl/cqpl_checker/Cargo.toml
cargo +nightly-2024-11-21 test --manifest-path crema/Cargo.toml
```

All current checker sub-suites must pass.

## G2 — EFM1 backward compatibility

Use checker unit tests and/or frozen test artifact to prove:

```text
legacy external_formal_memory_effects_v1 artifact accepted unchanged
```

## G3 — EFM2 producer/checker unit obligations

All U1–U10 and A1–A17 must pass.

## G4 — focused existing controls

Run and audit:

```text
b24
b25
b26
b29
b34
b35
```

Verify proof records as well as query output.

## G5 — new D1 fixtures

Run:

```text
b44 ... b51
```

or the final equivalent names chosen by the implementation.

The verifier must inspect:

```text
annotated_icfg_v2.json
query-results.tsv
use_after_free_alloc_state.explain.json
```

not only console PASS strings.

## G6 — body-present non-duplication

Require:

```text
represented body
=> zero EFM2 records for that call
```

and verify there is no duplicated allocation-centric read/write label caused by summary + body.

## G7 — zero/dynamic extent

Require exact-zero suppression and dynamic MAY retention.

## G8 — baseline differential

Compare all pre-existing bodyless target truth and assessment cells against a baseline derived from commit `39886ed...`.

Acceptable implementations:

1. a frozen baseline oracle generated from that exact commit and committed with explicit provenance; or
2. an isolated detached worktree replay of the exact baseline commit.

Forbidden:

```text
candidate compared against itself
current dirty tree used as baseline
baseline silently regenerated after candidate changes
```

## G9 — source/artifact hygiene

Require:

```text
git diff --check = clean
no __pycache__
no target/
no generated global_icfg*.json
no callgraph_initial.dot.dot
no run output committed as source
```

Intentional small oracle/manifest files are allowed if documented and checksum-frozen.

---

# 20. Required machine-readable result

The verifier must write a JSON result, for example:

```text
repro-results/bodyless-ffi-efm2-d1-<timestamp>/gate.json
```

with at least:

```json
{
  "schema": "cqpl_bodyless_ffi_efm2_d1_gate_v1",
  "status": "PASS|FAIL",
  "baseline_commit": "...",
  "candidate_commit_or_worktree": "...",
  "toolchain": "nightly-2024-11-21",
  "capability": "external_formal_memory_effects_v2",

  "legacy_efm1": {
    "accepted": true
  },

  "closed_contract": {
    "callees": [
      "strlen",
      "memcmp",
      "memcpy",
      "memmove",
      "memset",
      "memchr",
      "strchr",
      "write"
    ]
  },

  "proof_obligations": {
    "exact_symbol": true,
    "exact_arity": true,
    "per_formal_only": true,
    "explicit_extent": true,
    "zero_extent_suppressed": true,
    "dynamic_extent_remains_may": true,
    "represented_body_not_duplicated": true,
    "formal_actual_separation": true,
    "generic_argmem_not_overassigned": true,
    "no_return_relation_added": true
  },

  "unit_tests": {
    "cqpl_rc": 0,
    "crema_rc": 0
  },

  "new_fixtures": {
    "passed": 0,
    "failed": 0
  },

  "existing_corpus": {
    "truth_deltas": 0,
    "assessment_deltas": 0,
    "query_errors": 0
  }
}
```

The exact JSON organization may be extended but these scientific facts must remain machine-readable.

---

# 21. Final PASS criteria

D1 PASS iff all of the following are true:

```text
[ ] baseline commit recorded and traceable to 39886ed
[ ] EFM1 legacy artifact remains accepted
[ ] producer emits EFM2, not silently redefined EFM1
[ ] EFM1 and EFM2 are mutually exclusive in one artifact
[ ] exact eight-function EFM2 set is closed
[ ] memmove arg0=write, arg1=read, extent=arg2
[ ] memchr arg0=read, extent=arg2
[ ] strchr arg0=read, extent=c_string_until_nul
[ ] formal index is explicit for every effect
[ ] extent semantics are explicit for every effect
[ ] proof basis is explicit for every effect
[ ] exact zero suppresses bounded effects
[ ] dynamic/unknown extent remains MAY
[ ] generic argmem never expands blindly to all pointer formals
[ ] Rust actual identity is preserved
[ ] no allocation/alias identity is synthesized by EFM2
[ ] represented bodies receive no EFM2 summary
[ ] duplicate proof records are rejected
[ ] malformed proof tuples fail closed
[ ] b44–b47 targeted UAF assessments are unk_true strong evidence
[ ] b48–b49 zero controls emit no EFM2 memory event
[ ] represented-body control has zero duplicate EFM2 records
[ ] b24/b25/b26 behavior is preserved
[ ] b29/b34/b35 gain only intended proof/event metadata
[ ] pre-existing bodyless truth delta = 0
[ ] pre-existing bodyless assessment delta = 0
[ ] CQPL full tests pass
[ ] CREMA full tests pass
[ ] git diff --check passes
[ ] generated artifacts are not accidentally staged
```

No quota of newly oriented queries is required beyond the explicit D1 UAF fixtures.

Correctness and fail-closed isolation take priority over coverage.

---

# 22. Forbidden shortcuts

The implementation MUST be rejected if it does any of the following:

```text
add memmove/memchr/strchr directly to EFM1 without a capability version bump
use substring symbol matching
assign generic argmem(read/write) to all pointer arguments
infer a returned alias in D1
create fresh allocation identity for memchr/strchr/memmove
treat missing evidence as a positive read/write fact
duplicate events when a C body is represented
turn a dynamic extent into MUST-positive extent
treat unknown extent as zero
change CQPL truth rules
change RN1/AGE1/RBF/CR semantics
weaken checker validation merely to accept producer output
modify baseline oracle after seeing candidate results
skip full existing-corpus differential
```

---

# 23. Expected file-change surface

The implementation should normally touch only a subset of:

```text
crema/src/cqpl_export.rs

cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json

cqpl/capabilities/external_formal_memory_effects_v2.md

cqpl/scripts/run_bodyless_ffi_efm2_d1_gate.sh
cqpl/scripts/verify_bodyless_ffi_efm2_d1.py

cqpl/bodyless_ffi_efm2_d1_fixture_manifest.json

tests_and_target_repos/a-code_c_ffi_bodyless_gate/b44_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b45_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b46_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b47_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b48_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b49_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b50_...
tests_and_target_repos/a-code_c_ffi_bodyless_gate/b51_...
```

If the agent needs to modify:

```text
identity.rs
abstract_domain.rs
model_checker.rs
explain.rs
RN1/AGE1/RBF/CR capability documents
```

it must stop and justify why D1 cannot be implemented through the existing read/write event vocabulary.

The default expectation is that those files remain untouched.

---

# 24. Required agent workflow

The coding agent MUST execute the task in this order.

## Step 1 — inspect, do not edit

Read:

```text
cqpl/capabilities/external_formal_memory_effects_v1.md
cqpl/C_FFI_BODYLESS_LIBRARY_EFFECT_GATE.md
crema/src/cqpl_export.rs
cqpl/cqpl_checker/src/kripke.rs
cqpl/cqpl_checker/src/main.rs
cqpl/schemas/annotated_icfg_v2.schema.json
cqpl/bodyless_ffi_fixture_manifest.json
cqpl/scripts/run_bodyless_ffi_phase_a_gate.sh
cqpl/scripts/run_bodyless_ffi_phase_b_minimal_gate.sh
cqpl/scripts/run_one_target_v6q_r1c.py
```

Also inspect existing fixtures:

```text
b24_memcmp_freed_left_uaf
b25_memcmp_freed_right_uaf
b26_write_after_free_uaf
b29_strchr_return_not_alloc
b34_memmove_returned_alias
b35_memchr_return_not_alloc
```

## Step 2 — record preimage

Record:

```text
git HEAD
git status --short
SHA256 of every source file that will be changed
```

Do not edit a dirty generated artifact.

## Step 3 — write/adjust tests first

Add:

```text
producer closed-contract tests
checker negative tests
legacy EFM1 compatibility test
new fixture sources
gate verifier expectations
```

The new tests should fail for the expected missing D1 behavior before implementation where practical.

## Step 4 — implement smallest producer delta

Add EFM2 and the three missing semantic families.

Do not implement return relations.

## Step 5 — implement checker/schema validation

Keep v1 and v2 validation explicitly separated.

Do not implement an open-ended "accept any known function" path.

## Step 6 — run focused tests

Run new producer/checker tests before the full gate.

## Step 7 — run full software tests

CQPL + CREMA all green.

## Step 8 — run D1 gate

The gate must produce the machine-readable result and retain evidence.

## Step 9 — inspect git diff manually

The agent must summarize:

```text
changed files
why each file changed
proof obligations discharged
truth/assessment differential counts
remaining limitations
```

## Step 10 — do not commit/push automatically

Unless explicitly instructed by the human, the agent MUST leave the validated changes in the working tree and provide the exact suggested `git add` / `git commit` commands.

---

# 25. Definition of scientific completion

D1 does NOT claim:

```text
all libc calls are modeled
all pointer accesses are known
all memory effects are complete
all UAF UNKNOWNs are solved
```

D1 supports the narrower claim:

> For a closed, versioned set of bodyless C/POSIX library declarations, CREMA can emit proof-carrying per-formal read/write effects with explicit extent semantics. Effects are attached to real Rust MIR actuals, represented-body duplication is excluded, ambiguous generic argmem evidence is not over-assigned, malformed proof records fail closed, and existing CQPL semantics operate unchanged over the enriched model.

This is the claim the gate must validate.

---

# 26. Prompt to give to the Codex coding agent

Paste the following prompt from the repository root after Codex authentication works:

```text
Implement D1 exactly according to:

  cqpl/D1_EFM2_BODYLESS_FORMAL_MEMORY_EFFECTS_GATE.md

Treat that file as the normative implementation and acceptance specification.

Baseline:
- branch: cqpl6-bodyless-ffi-effect-gate
- pushed checkpoint begins with commit 39886ed
- do not require a Git tag
- verify and record the full baseline/HEAD SHA before editing

Critical architectural constraint:
- DO NOT redefine external_formal_memory_effects_v1.
- Introduce external_formal_memory_effects_v2 as the versioned successor.
- Preserve legacy EFM1 artifact acceptance unchanged.
- EFM2 must cover exactly:
  strlen, memcmp, memcpy, memmove, memset, memchr, strchr, write.
- D1 implements per-formal memory read/write effects only.
- DO NOT implement return aliases, interior-pointer identity, allocation effects,
  nofree, nocapture, escape semantics, or any RN1/AGE1/RBF/CR changes.

Work to completion:
1. inspect all files and fixtures listed in section 24 before editing;
2. record source preimage hashes;
3. add focused producer/checker tests and D1 fixtures;
4. implement the smallest proof-carrying producer change;
5. implement strict EFM1/EFM2 checker + JSON-schema validation;
6. implement run_bodyless_ffi_efm2_d1_gate.sh and verify_bodyless_ffi_efm2_d1.py;
7. run focused tests;
8. run complete CQPL and CREMA test suites;
9. run the full D1 gate, including the differential against the 39886ed baseline;
10. inspect all resulting artifacts and report exact PASS/FAIL evidence.

Fail closed. Do not weaken a validator simply to make producer output pass.
Do not use substring-based libc semantics.
Do not treat generic function-level argmem as per-formal proof.
Do not modify CQPL truth semantics.
Do not commit or push.

If an implementation detail in the repository differs from the specification,
adapt to the current code architecture while preserving every scientific
invariant and acceptance criterion in the specification.

At the end return:
- files changed;
- tests/gates run with exit codes;
- D1 gate JSON path;
- exact existing truth delta count;
- exact existing assessment delta count;
- new fixture outcomes;
- remaining limitations;
- proposed git add/commit commands.
```

---

# 27. Human review after Codex finishes

Before accepting Codex's work, the human reviewer should independently check:

```text
git diff --check
git status --short
git diff --stat
git diff -- cqpl/capabilities/external_formal_memory_effects_v1.md
```

The last command should ideally show:

```text
no semantic change to the frozen v1 document
```

Then inspect:

```text
external_formal_memory_effects_v2.md
cqpl_export.rs contract table
kripke.rs closed validator table
annotated_icfg_v2.schema.json
D1 fixture manifest
D1 gate JSON
```

Only after that review should the D1 changes be committed.
