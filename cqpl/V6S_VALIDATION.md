# v6S-r1 validation protocol

## r1a compiler-safety hotfix

The first full v6S-r1 attempt (`20260915T110915Z`) is **not an experimental result**. It was interrupted after the first FFI subjects failed during CREMA export with a pinned-rustc ICE caused by querying `TyCtxt::item_name` on an `Impl` parent `DefId`. r1a fixes only this producer-side compiler API misuse: before reading a parent name, CREMA now proves `DefKind::Mod`. The intended allocation-disposition semantics, the 12 frozen queries, and all MAY-only contracts are unchanged.

The r1a full run must use a fresh output directory. The failed r1 directory must be retained only as debugging provenance and must not be counted toward acceptance.

v6S-r1 changes the CREMA→CQPL artifact boundary, so unlike v6R it **must regenerate the 112 graphs**. Re-running only the checker on the frozen v6R graphs is insufficient.

## Acceptance contract

Acceptance requires all of the following:

1. pinned rustc `1.84.0-nightly (3fee0f12e 2024-11-20)`;
2. CREMA tests pass;
3. CQPL checker tests pass;
4. fresh run of 109 corpus subjects + 3 pinned registry crates;
5. every fresh schema-v2 graph declares `allocation_disposition_v1` and serializes `allocation_disposition` on every node;
6. all 12 frozen query files remain byte-identical to v6R-r1;
7. the fresh 112×12 truth matrix is cell-for-cell identical to the v6R/v6Q frozen matrix (`1344/1344`, zero mismatch);
8. `boxed_bool__ml` contains `box_into_raw` provenance and no `box_from_raw` provenance;
9. `clean_into_from_raw` contains both `box_into_raw` and `box_from_raw` provenance;
10. `drop_raw_ptr_no_free` contains `raw_pointer_drop_noop` and the corresponding node has no allocation `drop` label for the same allocation, no `may_deallocate` record caused by that node, and no `FREED` `allocation_post` cell for that allocation;
11. the disposition report covers all 105 baseline `leak_alloc=unk` subjects without interpreting missing MAY records as MUST-negative evidence.

A reduction in `unk` is **not** an r1 acceptance criterion. Old results are required to remain unchanged.

## Why the raw-pointer gate is normative

The Rust Reference states that copying or dropping a raw pointer does not affect the lifecycle of any other value:

<https://doc.rust-lang.org/reference/types/pointer.html>

`std::mem::drop` is an ordinary generic function and effectively does nothing for `Copy` types; raw pointers are `Copy`:

<https://doc.rust-lang.org/std/mem/fn.drop.html>

Therefore `mem::drop(raw)` must not be interpreted as destruction or deallocation of the pointee. This is distinct from `ptr::drop_in_place(raw)`, which executes the pointee destructor but still does not, by itself, prove allocator deallocation:

<https://doc.rust-lang.org/std/ptr/fn.drop_in_place.html>

## Canonical installed-tree command

```bash
ROOT=/home/af/Documenti/a-phd
NIGHTLY=nightly-2024-11-21
BASE="$ROOT/repro-results/cqpl-v6q-r1c-final112"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT="$ROOT/repro-results/cqpl-v6s-r1a-validation-$STAMP"
LOG="$OUT.console.log"

CREMA_PHD_ROOT="$ROOT" \
CREMA_RUST_TOOLCHAIN="$NIGHTLY" \
V6S_BASE="$BASE" \
V6S_OUT="$OUT" \
LOG="$LOG" \
bash -o pipefail -c 'bash "$CREMA_PHD_ROOT/cqpl/scripts/validate_v6s_r1_candidate.sh" 2>&1 | tee "$LOG"'
```

For a reproducible evidence directory, set `V6S_OUT` explicitly before invocation.

## Expected final markers

A successful run prints at least:

```text
V6S_R1_STATIC_VERIFY: PASS
V6S_R1_ARTIFACT_BOUNDARY: PASS subjects=112 disposition_records=<N>
V6S_R1_RESULT_IDENTITY: PASS subjects=112 queries=12 attempts=1344 mismatches=0
V6S_R1_DISPOSITION_ANALYSIS: PASS
V6S_R1_FIXTURE_GATES: PASS
V6S_R1_VALIDATION: PASS
```

The exact number `<N>` of disposition records is an empirical output, not a predeclared success target.

## Generated scientific evidence

The validator writes, below `V6S_OUT`:

- `fresh112/`: newly generated 109+3 graphs and the 12-query matrix;
- `result-identity.json`: cell-for-cell comparison with the frozen v6R matrix;
- `allocation-disposition-summary.json`: disposition census for all 112 subjects and separately the 105 baseline leak unknowns.

The script writes fresh evidence below `repro-results/` and never treats that directory as source to be committed.
