# CQPL assessment scopes

`assessment_scope` is a query-document declaration that changes **only the diagnostic assessment graph**. It does not change CQPL three-valued truth semantics.

The default is:

```cqpl
assessment_scope all_execution;
```

`all_execution` uses the complete already-projected transition relation.

The first non-default scope is:

```cqpl
requires typed_edge_flow_v1;
assessment_scope normal_execution;
```

For `normal_execution`, CQPL truth is still evaluated on the complete legacy `nodes[*].successors` relation, including unwind edges. Directional assessment traversal is instead evaluated on the typed-edge projection

```text
R_normal = { (s,t) in R | typed_edge_flow(s,t) = normal }.
```

This distinction is normative. `normal_execution` is not a state predicate and does not assert that a node is intrinsically normal. It selects a transition relation for assessment traversal.

A node with no outgoing normal edge is terminal in the assessment projection, even if it has outgoing unwind edges in the complete model. Strong-next / maximal-path treatment in the diagnostic graph follows that projected relation only; CQPL truth is unaffected.

The declaration is fail-closed: `assessment_scope normal_execution;` requires an explicit `requires typed_edge_flow_v1;`. The checker therefore never reconstructs or guesses edge flow from node names, MIR pretty-printing, or successor order.

## Initial Gate L1 use

The first consumer is the canonical allocation-state leak assessment:

```cqpl
requires allocation_state_v1;
requires typed_edge_flow_v1;
assessment_scope normal_execution;

exists_alloc a. EF (
  alloc(a) &&
  EX EG !drop(a)
)
```

The formula above is still model-checked on the complete Kripke structure. Only supporting/refuting leak findings used to orient an already-`unk` result are restricted to normal edges.

A resulting `unk_false` therefore means:

> the CQPL result is still UNKNOWN on the complete transition system, while the normal-execution assessment graph contains explicit negative evidence against the leak pattern.

It does **not** mean `ff`, memory-safety proof, or irrelevance of unwind paths. Unwind paths remain part of the truth semantics and may continue to be the reason the truth value is `unk`.
