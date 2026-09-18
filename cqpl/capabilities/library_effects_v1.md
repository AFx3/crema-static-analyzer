# Historical note

The `468` UNKNOWN count below is the acceptance count of the historical v6U-A2 migration experiment. It is **not** the current FINAL112 truth closure.

Current frozen FINAL112 after the approved precision deltas is:

```text
ff=705  unk=413  tt=226
```

R2 provenance/explainability changes do not alter those counts. See `../ANALYSIS_PIPELINE.md`.

# library_effects_v1 — A2 migration-only candidate

A2 moves only the existing producer-certified `allocation_disposition_v1`
projection table into `rust_std_v1.json`.

Historical v6U-A2 migration summaries:

- rust_box_into_raw_v1
- rust_box_from_raw_v1
- rust_box_leak_v1
- rust_mem_forget_owned_box_v1
- rust_mem_drop_raw_pointer_v1

B1.1 adds two producer-summary entries to the same registry format:

- rust_cstring_into_raw_v1
- rust_cstring_from_raw_v1

Those two entries project only into the additive `allocation_disposition_v2`
capability; they do not mutate the frozen seven-kind
`allocation_disposition_v1` vocabulary.  All remain `certainty = may_abstract`.
No MUST semantics and no libc summaries are introduced.

Official CString contract: <https://doc.rust-lang.org/std/ffi/struct.CString.html>.

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

Within that historical migration experiment, 468 was the expected UNKNOWN closure for its 112 x 12 query
matrix. It is not the total number of *.explain.json files in the complete
evidence tree.
