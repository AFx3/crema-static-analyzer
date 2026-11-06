# CStringCargo enum DF/UAF benchmark

This directory contains the canonical, updated form of the benchmark. The
`CString::new` error branch terminates with `abort()` instead of invoking
`log::error!`. This removes a reachable control dependency on the runtime
selected `dyn log::Log` implementation: that implementation is intentionally
not statically recoverable and is unrelated to the memory error under test.

The successful branch is unchanged. It still calls `CString::into_raw` and
then invokes `CString::from_raw(c_name_ptr)` twice, in the original order, on
the same pointer. The first reconstruction reclaims the CString allocation;
the second reconstruction reads already-freed C-string storage and attempts a
second reclamation. The expected double-free and use-after-free therefore
remain unchanged. No allocator mismatch or source-level leak is introduced.

Canonical `src/main.rs` SHA-256:
`583745a43c7d5c90e33413e0cd7814e53476bb43cfd56cad94b1032f4d118386`.
The previous benchmark version remains recoverable from Git history; no second
fixture or renamed target is maintained.
