# CQPL8 FPF port changelog

Prepared against `port/rq1-ml-fn-to-cqpl8` at
`10b243b0e6b8ffc7e9a8da7ad5880aa45a5869b4`. No build, test, replay, Python
script, benchmark, commit, merge, or cherry-pick was executed while preparing
this port.

## P0 — closure allocation identity

Source: the validated closure correction in
`/mnt/wsx-storage/af/crema-validated-integration-work/crema/src/identity.rs`.

- Direct `mir_call` boundaries with ABI `rustc_closure_body_v1` now accept the
  certified closure environment/reference relation.
- The one synthetic constant aggregate argument emitted for a direct zero-arg
  closure is removed before actual/formal cardinality checking.
- Indirect calls, unknown ABI shapes, extra arguments, and missing typed
  boundary evidence remain fail-closed.
- Allocation identity is propagated through the closure environment instead of
  being lost before the two `Box::from_raw` drops in `closure_df`.

Regression tests: `p3_r1_direct_closure_call_propagates_captured_allocation_identity`
and `p3_r1_uncertified_closure_boundary_remains_fail_closed`.

## P1 — destructor event export (FPF-001)

Source: validated commit `7a051db` (`Refine destructor memory event export`).
The change was integrated into the CQPL8 exporter rather than replacing the
file, preserving CQPL8 heap-transfer and the already ported ML logic.

- Typed `OwnedBoxDataPointerCopyForDeref` and certified `Drop::drop` receiver
  reborrows are recognized as drop-glue pointer extraction mechanics.
- Those mechanics do not become user UAF read events.
- `ptr::drop_in_place` is no longer exported as a write merely because its
  callee name contains a raw-pointer primitive.
- Ordinary dereferences and real `ptr::write` operations remain exported.

Regression tests cover drop-in-place, Box drop glue, destructor receiver
reborrow, ordinary dereference and a real raw-pointer write positive control.

## P2 — certified constant MIR branches (FPF-002)

Source: validated commit `22bc6e7` (`Prune producer-proved constant MIR
branches`).

- `SwitchInt` records carry optional producer certificates:
  `resolved_constant_target` and `constant_resolution_basis`.
- Only closed same-block scalar constants and a deliberately small set of
  scalar unary/binary operations are evaluated from typed MIR.
- Projections, unknown operands, unsupported writes or operations fail closed
  and preserve every successor.
- The ICFG exporter emits one successor only when the typed certificate exists.

Regression tests cover certified false, certified true and dynamic fan-out.

## P3 — ML correction preserved

The existing CQPL8 changes from `0ab9d0b` and `10b243b` were not overwritten.
The checker tests for represented C `malloc`/`calloc`, reachable discharge,
realloc overwrite evidence and zero/unknown-size conservatism remain present.
The manual smoke queue includes the three recovered ML cases, `b39`, the clean
`b20c` control, and the real DF in `b20b`.

## Residual risks requiring execution

- Rustc nightly API compatibility of the typed scalar evaluator.
- Semantic equivalence of CQPL8 heap-transfer annotations after P1/P2.
- Exact witness preservation for all RQ1, Table 5 and frozen RQ2 findings.
- The historical non-exportable `cstringcargo_enum_df_only_rust` remains an
  expected separately reported failure unless the manual campaign proves
  otherwise.

None of these gates is reported as passing in this preparation.

## Export recovery for DefId-only legacy callbacks

The frozen `cstringcargo_enum_df_only_rust` log showed a successful subject
build followed by a CREMA panic at `crema/src/icfg.rs` while an instance-aware
DEP1 reuse index was active. Legacy callback discovery provided only a `DefId`,
which cannot select a generic `Instance` soundly. The old code aborted the
entire crate.

The new behavior keeps the identity check: it refuses that optional body
acquisition, emits `A3_EXTERNAL_CALLBACK_MIR_SKIPPED_FAIL_CLOSED`, and leaves
the higher-order callback body unresolved. It neither invents an Instance nor
converts the failure into a query result. Regression tests cover both the DEP1
rejection and the unchanged pre-DEP1 worklist behavior.

The follow-up log identified that the unresolved callback is specifically the
bodyless enum constructor `log::MaybeStaticStr::Static`, passed to the
producer-certified `Option::map` calls in `RecordBuilder::file_static` and
`RecordBuilder::module_path_static`. Such a constructor has no callback MIR
body to acquire and no concrete callback `Instance` certificate. The full
profile now enables CREMA's existing narrow
`--a3-bodyless-enum-constructor-callback-v1` capability. The producer admits
the summary only when rustc reports `DefKind::Ctor`; ordinary missing closure
or function bodies remain unresolved and schema-v2 remains fail-closed.
