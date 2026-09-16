# library_effects_v1 — A2 migration-only candidate

A2 moves only the existing producer-certified `allocation_disposition_v1`
projection table into `rust_std_v1.json`.

Active migration summaries:

- rust_box_into_raw_v1
- rust_box_from_raw_v1
- rust_box_leak_v1
- rust_mem_forget_owned_box_v1
- rust_mem_drop_raw_pointer_v1

All remain `certainty = may_abstract`. No MUST semantics and no libc summaries
are introduced.

Acceptance requires 112 subjects, 1344 attempts, 392 disposition records,
468 UNKNOWN explanation sidecars, and zero mismatches in all three surfaces:
query truth/rc, disposition records, and explanation sidecars.

## A2 equivalence accounting

The frozen v6T -> v6U-A2 equivalence gate distinguishes query-matrix
UNKNOWN explanations from corpus evidence copies.

Validated counts:

    query-matrix UNKNOWN results        = 468
    query-matrix explanation sidecars  = 468
    corpus explanation sidecars        = 227
    total explanation sidecars         = 695

Acceptance requires:

    truth/rc mismatches                 = 0
    allocation_disposition mismatches  = 0
    query-matrix explanation mismatches = 0
    corpus explanation mismatches       = 0
    total explanation mismatches        = 0

The value 468 is the normative UNKNOWN closure for the 112 x 12 query
matrix. It is not the total number of *.explain.json files in the complete
evidence tree.
