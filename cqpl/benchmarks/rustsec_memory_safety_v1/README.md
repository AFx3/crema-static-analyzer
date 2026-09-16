# RustSec memory-safety benchmark v1

Gate B1 status: **scaffold only**.

This benchmark is intended to establish external vulnerable/fixed ground truth.

B1 contains no admitted cases and therefore makes no precision/recall claim.

## Required separation

For every future admitted case record independently:

```text
representability
query expressibility
detection
```

A real bug that is not representable is not silently conflated with a checker
failure.

## Pair invariant

Vulnerable and fixed revisions must use the same, documented analysis
configuration whenever technically possible.

## Initial target size

Gate B1.1 should admit 10–20 provenance-complete vulnerable/fixed pairs before
any ecosystem-scale scan is attempted.
