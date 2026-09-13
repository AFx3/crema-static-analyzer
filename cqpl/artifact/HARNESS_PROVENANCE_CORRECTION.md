# r8 corpus harness provenance correction

The already-recorded r8 corpus evidence is not modified retroactively.

The script used for that run printed this Stage-1 banner:

```text
=== Stage 1: v6L schema-v2 discovery over active 109 (runner r7) ===
```

This was a stale human-readable label only. The same run records the protocol as
`CREMA-CQPL-v6L-full-corpus-discovery-r8`, uses the r8 source boundary, and the
r7-to-r8 differential observed the single expected semantic change for
`c_malloc_rust_string_from_raw_parts_ub`.

Future corpus execution through `cqpl/run_all.sh` prints `runner r8`.
Keeping the original evidence immutable is intentional.
