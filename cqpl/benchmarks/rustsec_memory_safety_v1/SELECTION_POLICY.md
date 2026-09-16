# B1.1 selection policy

## Inclusion

A candidate is selected only if, as of 2026-09-16:

1. RustSec has an advisory page for a crates.io package.
2. The advisory describes memory unsafety or undefined behavior relevant to
   CREMA's memory-safety scope.
3. RustSec publishes an explicit patched release boundary.
4. The advisory identifies an affected function or a sufficiently precise
   operation path.
5. The candidate adds causal diversity or directly stresses a known CREMA
   abstraction boundary.

## Exclusion

B1.1 excludes:

- unmaintained-only advisories;
- malicious-package advisories;
- memory-safety advisories with no fixed release;
- advisories whose only evidence is an unverified third-party report;
- cases selected solely because they are easy for the current 12 queries.

## Bias control

The candidate set is not optimized for CREMA success.

It deliberately includes cases expected to require future domains such as:

```text
panic_unwind_lifecycle_v1
pointer_validity_v1
memory_initialization_v1
allocation_layout_v1
```

These are hypotheses, not implemented capabilities.

## Admission boundary

`status=candidate` is not benchmark admission.

B1.2 must replace advisory-level version ranges with exact materialized
vulnerable/fixed revisions and source hashes before any case can become
`status=admitted`.
