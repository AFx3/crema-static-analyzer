# Next steps after v6S-r1

Do not design the v6S-r2 leak formula before the r1 disposition census exists.

## Gate 1 — freeze v6S-r1 observational evidence

Require zero mismatches on the 1344 historical results and preserve the raw-pointer no-op invariants.

## Gate 2 — inspect the 105 leak unknowns

Use the generated report to count overlapping event presence and mutually exclusive complete signatures, for example:

```text
box_into_raw only
box_into_raw + box_from_raw + may_deallocate
return_escape
box_leak
mem_forget_owned_box
mixed/path-dependent
no disposition evidence
```

These are MAY observations, not final classifications.

## Gate 3 — design v6S-r2 MUST/obligation semantics

Only after the census, add a separate domain rather than reinterpreting historical `alloc_l` / `drop_l`.

Candidate algebra:

```text
MAY at join  = union
MUST at join = intersection
```

A future query may then reason about an allocation obligation that is definitely still open at a relevant normal exit.  The intended calibration examples are:

- `boxed_bool__ml`: desirable `tt` only if creation and open obligation are certified;
- `clean_into_from_raw`: desirable `ff` only if cleanup/discharge is certified;
- ambiguous branches/external calls: remain `unk`.

## Gate 4 — external summaries

Only after the r1 census shows how much unresolved escape is due to calls without bodies, introduce a separate producer-certified external-effect summary layer for libc/std/core APIs.  Do not infer ownership transfer from names alone.
