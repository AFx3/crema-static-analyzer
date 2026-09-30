# DEP1-P2-R1: matched return semantics

## Status

The current CQPL checker is a finite Kripke graph model checker. The actual transition relation is `AnnotatedNode.successors`; it has no Rust call stack or call/return-pair field. A shared return node with ordinary successors to multiple `dummyRet` nodes therefore admits cross-return paths. Edge labels do not constrain CTL transitions.

## Required invariant

`ANNOTATED_ICFG_SINGLE_SOURCE_OF_TRUTH`: CREMA produces one authoritative annotated transition structure and CQPL consumes it. CQPL must not resolve callees or reconstruct a separate graph. Every transition seen by CQPL must map to that authoritative annotated ICFG.

## Identity separation

- `CanonicalBodyIdentity` is the accepted concrete rustc `Instance` identity.
- `CanonicalCodeNodeIdentity` is `(CanonicalBodyIdentity, MIR BasicBlock)`.
- If necessary for finite acyclic call graphs, an `ExecutionStateIdentity` may pair a canonical code node with its full call-context identity. Each expanded node must retain a link to its canonical body/code identity. Context expansion is not additional MIR acquisition.

A fixed `k` call string, truncation, or context merge is not exact. For recursive reachable calls, an implementation must either use a sound stack-aware transition semantics or fail closed as `recursive_call_context_unsupported`, making semantic completeness false. It must never silently drop or merge return contexts.

## Architecture decision

The source audit and executable control establish that current CQPL uses plain successor adjacency. Thus Architecture B would require a new stack-aware/pushdown transition model checker. No such model is present. Architecture A is viable only if CREMA emits context-explicit states in the same authoritative global and annotated ICFG, with recursion handled explicitly as above. The current P2 producer does not yet do this; the repeated-call cross-return witness remains a blocker.

No production graph or CQPL transition semantics are changed by this document.
