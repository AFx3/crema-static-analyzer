# RustSec memory-safety benchmark v1 — Gate B1.1

Status: **candidate selection only**.

B1.1 selects a provenance-complete advisory-level candidate set. It does not
yet materialize vulnerable/fixed source trees and therefore does not admit any
case into precision/recall statistics.

## Scientific purpose

The selected set is intentionally heterogeneous:

- panic/unwind double-free and use-after-free;
- raw ownership lifecycle errors;
- invalid deallocation / layout errors;
- pointer-validity errors;
- an FFI-adjacent CString lifetime error.

The purpose is to learn which failures are:

1. representable in the current abstract state;
2. expressible by the frozen 12 CQPL queries;
3. blocked by missing library semantics;
4. blocked by missing abstract domains.

## B1.1 invariants

```text
candidate cases = 14
admitted cases  = 0
subjects rows   = 0
accuracy_ready  = NO
```

The 14 cases are generated deterministically from `selection_evidence.json`.

Every selected advisory has:

- an official RustSec advisory page;
- an explicit patched release boundary;
- at least one affected function / operation;
- secondary upstream provenance when RustSec publishes it.

## Deliberate non-claims

B1.1 does not claim:

- source-pair materialization;
- compilability under the CREMA pinned toolchain;
- trigger reproducibility;
- representability;
- CQPL expressibility;
- true positive / false negative status;
- precision or recall.

Those belong to B1.2/B1.3.

## Next gate after B1.1

B1.2 materializes exact vulnerable/fixed revisions, hashes the source trees,
records toolchain/features/entrypoints, and admits only pairs whose provenance
and build identity are complete.

## B1.3 / A3 panic-unwind differential

The A3 experimental runner analyzes the admitted vulnerable/fixed reproducer
harnesses and enables CREMA `panic_unwind_lifecycle_v1`. Under A3, CREMA imports
reachable dependency MIR when rustc makes it available. The runner requires the
`aligned_box::...::realloc_with_default` body to appear in both ICFGs before it
interprets any CQPL differential.

```bash
python3 cqpl/benchmarks/rustsec_memory_safety_v1/run_b1_3_panic_unwind_case.py \
  --root "$PWD" \
  --case-id rustsec_2026_0282_aligned_box_realloc_panic
```

Interpretation order is strict: runtime oracle -> distinct symbolic inputs ->
capability gate -> CQPL differential.
