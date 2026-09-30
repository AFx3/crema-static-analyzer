# DEP1-P2 Context-Explicit Execution ICFG V2

## Scope

This contract separates CREMA program control flow from CQPL's existing checker-local graph semantics. It does not change CQPL transition semantics or add call-stack behavior to the checker.

## Graph stages

- `G`: CREMA's authoritative program execution ICFG. Rust represented calls use exact callsite/context push and matched return pop transitions.
- `A`: CREMA's annotated program ICFG. Its program-state identities and successor relation must be exactly those of `G`; annotations may add facts only.
- `S(A)`: the entry-projected CQPL checker model after only the already-registered checker semantic elaborations. In particular, `conditional_reallocations_v1` retains its certified success refinement state and edge rewrite.
- `K = T(S(A))`: the CQPL truth model after the existing terminal/deadlock completion transformation.

CQPL model checking traverses `K`. The contract does not claim `A == S(A)`, `A == K`, or CTL equivalence between these graphs. CTL `X` may observe checker semantic refinement states and completion states.

## Conservation obligations

`program_control_skeleton(G) == program_control_skeleton(A)` requires exact producer node and successor equality. `S(A)` must retain provenance for every checker-added refinement and its producer edge. Contracting only registered refinements must recover the entry-scoped program-control skeleton. `T` must retain every `S(A)` transition and add only the existing certified completion transitions for deadlocks.

Entry projection and `--intra` are explicit scope operations. `--intra` remains the existing same-function induced projection and is not evidence for whole-program DEP1 control flow.

## Identity

- `CanonicalBodyIdentity`: concrete rustc `Instance` identity.
- `CanonicalCodeNodeIdentity`: `(CanonicalBodyIdentity, MIR BasicBlock)`.
- `ExecutionContext`: exact ordered vector of active static COV1 `call_key` values; root context is `[]`.
- `ExecutionStateIdentity`: `(CanonicalCodeNodeIdentity, ExecutionContext)`.

Context-expanded states refer to one acquired canonical MIR body. Display strings, package names, filesystem paths, and session-local `CrateNum` are not persistent identities.

## Recursion

This finite explicit-context version supports only reachable acyclic represented-Rust call graphs. A reachable recursive SCC must fail closed with `recursive_call_context_unsupported`, set `p2_control_flow_complete=false`, and prevent a complete model from being presented to CQPL. No bounded stack, context merge, or recursive summary is permitted.

## Ordering

Required producer order is MIR acquisition, canonical body graph, context-explicit `G`, identity analysis, abstract-domain analysis, panic-lifecycle analysis, then annotated export `A`. CQPL then constructs `S(A)` and `K` using its existing registered transformations. Context cloning only during export is forbidden.

## Status

This document defines the target contract. It does not assert that the current candidate implements context expansion or recursion fail-closed behavior.
