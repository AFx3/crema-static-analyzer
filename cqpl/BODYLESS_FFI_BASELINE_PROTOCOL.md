# Bodyless C/FFI baseline protocol

This checkpoint is intentionally **validation-only**. It adds no producer semantics.

## Purpose

Establish a frozen empirical baseline before implementing
`external_library_effects_v1`.

The benchmark separates:

- **strict controls**: cases already expected to be structurally supported by
  the current CREMA/EFX1 pipeline;
- **characterization cases**: cases designed to expose missing bodyless
  semantics. Their current CQPL result is recorded, not declared correct;
- **represented-body controls**: equivalent or analogous calls with a C body,
  used later for observational-equivalence experiments.

No characterization result may be promoted into a future expected result merely
because it is the current output.

## Why these targets are outside the historical 118-subject corpus

The directory is:

`tests_and_target_repos/a-code_c_ffi_bodyless_gate/`

The gate invokes it explicitly. Do not add these targets to the frozen Full13
manifest until the baseline and the first semantic implementation phase have
both been reviewed. This prevents the historical 118-subject baseline from
silently changing.

## Bodyless criterion used by these fixtures

Except for the represented-body control, fixture crates contain **no authored C
source**. They declare system C functions with `extern "C"`. Therefore the
analyzed project contributes no C implementation body for those symbols.

This is stronger and more reproducible than guessing bodylessness from node
names in an exported ICFG.

## Four canonical queries

Every fixture is evaluated with:

- `leak_alloc_state.cqpl`
- `double_free_alloc_state.cqpl`
- `use_after_free_alloc_state.cqpl`
- `allocator_mismatch_ub_v2.cqpl`

The baseline records both the truth and diagnostic subresult reported by the
checker. Phase A does not assert that characterization-case results are
semantically complete.

## Academic fixture matrix

| ID | Target | Class | Intended future semantic obligation |
|---|---|---|---|
| B01 | b01_malloc_leak | characterization | bodyless allocation return |
| B02 | b02_malloc_free_clean | strict-control | allocation + matching free |
| B03 | b03_malloc_double_free | strict/control-characterization | repeated bodyless free |
| B04 | b04_free_then_strlen_uaf_read | characterization | bodyless per-formal read |
| B05 | b05_free_then_memset_uaf_write | characterization | bodyless per-formal write |
| B06 | b06_free_src_then_memcpy_uaf_read | characterization | memcpy source read |
| B07 | b07_free_dst_then_memcpy_uaf_write | characterization | memcpy destination write |
| B08a | b08a_rust_box_then_c_free_mismatch | strict-control | Rust-vs-C allocator family |
| B08b | b08b_c_malloc_then_rust_dealloc_mismatch | characterization | C-vs-Rust family |
| B09 | b09_malloc_free_family_match | strict-control | matching C family |
| B10a | b10a_realloc_branch_clean | characterization | conditional realloc success/failure |
| B10b | b10b_realloc_then_old_use | characterization | old pointer invalidated only on success |
| B10c | b10c_realloc_then_old_free | characterization | repeated free only on realloc success |
| B11 | b11_memcpy_returned_alias | characterization | returned-alias, not fresh allocation |
| B12 | b12_strlen_nofree_control | characterization/negative-control | nofree must not synthesize drop |
| B13 | b13_memcmp_two_pointer_argmem | negative-control | no blanket per-formal read |
| B14a | b14a_bodyless_strlen | characterization | bodyless read |
| B14b | b14b_body_present_reader | represented-body-control | represented read event |
| B15 | b15_external_write_pointer_unknown | characterization | unresolved/external pointer use |

## Phase-A pass criteria

The baseline gate passes if:

1. existing CREMA and CQPL test suites pass;
2. all fixture crates build with the frozen toolchain;
3. CREMA can export every fixture;
4. all four canonical CQPL queries execute for every fixture;
5. all fixture results and EFX1 evidence are frozen in machine-readable JSON;
6. the gate does not assert unsupported bodyless-call counts using open-ended
   graph-name heuristics.

Phase A is therefore a *measurement checkpoint*, not a claim that bodyless
semantics are already complete.


## Phase-A correction after first empirical run

The first empirical run exposed a valid schema-v2 fail-closed boundary:

```text
reachable modeled alloc event ... has no AbstractAllocId/event_identity
```

for a direct bodyless `malloc` declaration.

This is now treated as an explicit **baseline gap**, not as a gate infrastructure
failure and not as acceptable final semantics.

Two fixtures intentionally retain direct bodyless `malloc`:

- `B01` — allocation-return/leak characterization;
- `B08b` — C-allocation to Rust-deallocator mismatch characterization.

Their Phase-A expected producer status is:

```text
fail_closed_alloc_identity_missing
```

All other fixtures are isolated so they do not depend on direct bodyless
`malloc`:

- read/write tests use Rust allocations whose identity already exists;
- bodyless `free` tests use either a Rust allocation (mismatch control) or a
  represented C seed allocator;
- bodyless `realloc` tests use a represented C seed allocation;
- returned-alias and nofree tests use Rust allocations.

A represented seed allocator is permitted because the experimental variable is
the *target external effect* (`free` or `realloc`), not the origin allocation.
Its C body establishes the allocation family/identity without supplying a body
for the external effect under test.

Therefore the Phase-A gate is successful only if every fixture matches its
declared producer-status expectation and every successfully exported fixture
runs all four canonical CQPL queries.
