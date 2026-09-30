# DEP1-P2 ICFG Stitching Contract v1

## Scope

P2 adds reachable dependency MIR control flow to CREMA's existing global ICFG. It establishes represented dependency MIR nodes, exact call-to-entry control flow, matched normal-return control flow, and matched unwind control flow. It does not establish cross-crate resource identity, actual-to-formal resource bindings, return-resource bindings, allocation-site remapping, or any resource-analysis semantics.

The implementation must preserve COV1 semantic ownership. Only a call whose COV1 `semantic_coverage_path` is `dep1_rust_mir` may enter a DEP1 represented Rust body. CREMA FFI body coverage remains in the existing CREMA FFI path; CQPL/ELE1 external-library coverage remains in the existing external model; uncovered and fail-closed DEP1 statuses do not get fabricated Rust bodies.

## Identities

- `BodyIdentity` is the accepted concrete rustc `Instance` identity produced by the pinned compiler's native stable identity mechanism.
- `NodeIdentity` is `(BodyIdentity, MIR BasicBlock index)`.
- `CallSiteIdentity` reuses the accepted COV1 `call_key` exactly.
- `CallReturnPairIdentity` is that same `CallSiteIdentity`.

Crate and function display names are diagnostic only. `CrateNum` is session-local diagnostic information only. No second canonical-identity layer is permitted. A generic `Instance` retains its substitution context even where rustc supplies a shared generic/source MIR body; the output must not claim that shared MIR is separately monomorphized.

## Materialization

For each reachable DEP1 body with status `represented_body`, materialize each MIR basic block exactly once per concrete `BodyIdentity`. Each node is identified by `(BodyIdentity, BasicBlock index)`. Distinct concrete instances, including `generic::<u32>` and `generic::<u64>`, have distinct node namespaces even when they use the same source definition/MIR.

No dependency MIR node is created for `body_unavailable`, `out_of_scope`, `unresolved_scope_identity`, `unresolved_instance`, `virtual_or_dynamic_unresolved`, `crema_ffi_body`, `cqpl_external_library_model`, or `uncovered`.

## Call/return control flow

A represented dependency call is stitched only through the exact chain:

`COV1 call_key -> exact DEP1 call binding -> resolved concrete Instance -> materialized body -> MIR START_BLOCK`.

Every represented callsite has exactly one call-entry relation. Ordinary intra-procedural Call-to-return edges must not permit bypass of a represented dependency body. Any retained summary edge must have a distinct machine-readable relation kind and explicit documented semantics.

Normal return uses the actual MIR Call `return_target`. It is never inferred from block order. Every callee normal `Return` relation must be matched to its originating callsite's continuation. A shared body called at multiple sites must not permit a return from callsite A to continuation B. The implementation must use a proven callsite-matched convention or an explicit context-aware relation whose consumers enforce the pairing; plain graph fan-out from shared return blocks to every caller continuation is invalid.

Diverging calls and bodies without a reachable normal `Return` do not receive synthetic normal-return relations.

Unwind is distinct from normal return. Where the existing pinned MIR/panic model represents a cleanup target, unwind exits return only to the exact originating callsite's cleanup continuation. Normal returns cannot flow to unwind continuations and unwind exits cannot flow to normal continuations. Existing abstractions for pinned unwind actions are preserved; P2 does not redesign panic semantics.

## Fixed point and ownership

The accepted P1 semantic worklist discovers concrete rustc `Instance`s to a fixed point. P2 materializes each represented instance once, terminates cycles by semantic Instance identity, and stitches internal dependency calls only from their exact represented caller body. Transitive bodies such as `app -> dep -> dep2` enter the graph because the call in represented `dep` MIR resolves to `dep2`.

COV1 `call_key` and producer references remain provenance on every new call/entry/return relation. P2 changes control-flow reachability only; resource identity remains out of scope.
