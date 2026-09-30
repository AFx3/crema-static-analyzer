# DEP1-P2 context-explicit execution ICFG v1

## Three-graph contract

- **G** is CREMA's authoritative program execution ICFG.
- **A** is the annotated program graph exported from G. Annotation adds facts; it does not alter program identities or successors.
- **T(A)** is CQPL's truth model after its checker-side semantic transformations.

For program states, G and A must have identical identities and successor transitions. Annotation may add facts only. For original program states `u` and `v`, `edge_T(u,v)` must equal `edge_A(u,v)`. CQPL completion may add `terminal t -> q_t` and `q_t -> q_t`; each `q_t` is a checker-semantic quiescence state, not a CREMA program ICFG node.

## Identity

- `CanonicalBodyIdentity = concrete rustc Instance identity`.
- `CanonicalCodeNodeIdentity = (Concrete Instance, MIR BasicBlock)`.
- `ExecutionContext = exact ordered vector of active call_key values`.
- `ExecutionStateIdentity = (CanonicalCodeNodeIdentity, ExecutionContext)`.

The root context is empty. Context expansion represents execution states, not duplicate acquisition of MIR. Every expanded state refers to one canonical body and code node. Pretty names, filesystem paths, and session-local CrateNum are diagnostic only.

## Matched execution

For an acyclic represented-Rust call graph, entering call key `K` changes context from `C` to `C ++ [K]`; a normal return pops exactly `K` and reaches that call's MIR `return_target` under `C`. Unwind cleanup uses the same exact context. Diverging calls have no normal return. CFG loops do not grow the static context. Reachable recursive SCCs must fail closed as `recursive_call_context_unsupported`; no truncation or context merging is allowed.

## CQPL quiescence compatibility issue

CQPL's existing `CqplTruthModel::from_projected` also inserts `__cqpl_realloc_success__` states for the established `conditional_reallocations_v1` semantics before terminal quiescence. Those are neither G/A program states nor quiescence states. This existing behavior is preserved. Independent review must reconcile it with a strict claim that `T(A) \\ A` contains only quiescence nodes before CXQ1 can claim the requested conservative-extension contract. No CQPL semantic change is made here.
