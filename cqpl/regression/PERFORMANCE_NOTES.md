# Performance hardening notes

## Observed `square` regression

During the frozen-92 CQPL replication, target `no_errors_projects/square`
produced its AnnotatedIcfg successfully.  The exported graph contained:

- 7,171 nodes
- 10,610 edges
- 766 program variables
- zero non-empty `post.cells`

The Leak and DF queries completed with `ff`; the UAF query did not complete
within roughly 15 minutes and was interrupted manually.

Because every post-state is empty, `alloc(x)=ff` at every node for every
program variable.  Therefore the official UAF formula

    exists x. EF (
      alloc(x) &&
      EX EF (
        drop_l(x) &&
        EX E[(!alloc_l(x)) U use_l(x)]
      )
    )

is semantically `ff` without evaluating its temporal suffix.

The pre-v2 evaluator nevertheless evaluated both operands of `&&` eagerly,
thereby running nested fixed points for hundreds of variables over a graph of
thousands of nodes.

## v2 change

The evaluator now performs only lattice-law-preserving global short-circuits:

- if the left valuation of `A && B` is `ff` at every node, return it without
  evaluating `B`;
- if the left valuation of `A || B` is `tt` at every node, return it without
  evaluating `B`;
- existential/universal quantifier accumulation terminates only when its
  complete valuation has respectively reached all-`tt` / all-`ff`.

These optimizations do not change CQPL truth values.

The corpus runner also enforces a configurable per-query wall-clock timeout
(default 120 seconds) and records the timeout as an infrastructure failure,
so a single pathological target cannot silently stall the complete experiment.

The timeout is a robustness guard, not a semantic approximation.
