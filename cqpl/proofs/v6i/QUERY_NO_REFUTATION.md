# CQPL schema-v2 no-refutation theorem for the three official allocation queries

## Scope

This result is deliberately **not** a soundness theorem for arbitrary CQPL.
It applies only to the exact schema-v2 allocation-centric queries shipped in
`cqpl/queries_v2/{double_free_alloc,leak_alloc,use_after_free_alloc}.cqpl`.

Let `B = {ff < unk < tt}` and write `NR(phi,b)` for
`[[phi]]#(b) != ff`.

For an abstract allocation `A`, the schema-v2 event atom `p_l(A)` with
`p in {alloc,drop,read,write,use}` is produced only with
`certainty = may_abstract`.  Therefore:

- `[[p_l(A)]]# in {ff, unk}`;
- `[[!p_l(A)]]# in {unk, tt}`;
- in particular, a negative allocation-event guard can never itself evaluate
  to `ff`.

## Assumptions discharged by the analysis boundary

The query theorem is conditional on the following analysis obligations.  These
are the exact bridge from the concrete program semantics to the already-defined
CQPL model-checking semantics.

**A1 — Path embedding.** Every modeled concrete witness trace has a path in the
combined Rust+C ICFG with the same block order.  For the strong `EX` operators
used by the official queries, consecutive witness events occur at distinct ICFG
nodes in the indicated order.

**A2 — Allocation-event adequacy.** If a modeled concrete event `p` occurs on
concrete allocation `a` in block `b`, and `A` represents `a`, then the v2
producer emits enough `event_identity` evidence to make
`[[p_l(A)]]#(b) = unk`, never `ff`.

**A3 — Identity coherence.** The allocation, deallocation and use events in one
concrete bug witness are represented by one common `AbstractAllocId A`.  `A`
is an abstract site+bounded-context identity; no concrete uniqueness is assumed.

**A4 — Quantifier-domain inclusion.** The `A` from A2/A3 occurs in the schema-v2
`allocations` domain, so `exists_alloc` ranges over it.

**A5 — Maximal leak suffix.** A concrete leak witness has a post-allocation
successor and a maximal concrete suffix on which that concrete allocation is
not deallocated.  This matches the strong `EX EG` shape of the current leak
query.  For successful `malloc`/allocation calls this is the normal-return
successor.

A1 and A5 are representation/ICFG obligations. A2–A4 are allocation-identity
boundary obligations.  The v6I executable gate checks all of them on a real
C-malloc -> Rust -> C-free -> Rust-use regression target; the mathematical
result below still requires their general proof for the supported language
fragment.

## Temporal lifting lemmas

For the implemented Kleene chain `ff < unk < tt` and existential CTL
operators:

1. if `NR(phi,b')` for some successor `b'` of `b`, then `NR(EX phi,b)`;
2. if a finite ICFG path from `b` reaches `c` with `NR(phi,c)`, then
   `NR(EF phi,b)`;
3. if a finite path `b=b0,...,bn` has `NR(psi,bn)` and
   `NR(phi,bi)` for every `i<n`, then `NR(E[phi U psi],b)`;
4. if a maximal path from `b` has `NR(phi,bi)` at every position, then
   `NR(EG phi,b)` (the implementation uses weak continuation at terminal nodes);
5. the meet of two non-`ff` values is non-`ff`, and an existential join remains
   non-`ff` if one candidate is non-`ff`.

These lemmas follow directly from the fixpoint equations implemented by the
checker.

## Theorem DF — complete double-free query no-refutation

Let a concrete modeled execution contain an allocation event for concrete
allocation `a`, followed later by two deallocation events of that same `a`.
Assume A1–A4 and let `A` be the common abstract identity.  Then the official
schema-v2 query

```cqpl
exists_alloc a. EF (
  alloc_l(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U drop_l(a)]
  )
)
```

evaluates at the program entry to `unk`, hence not to `ff`.

**Proof.** By A2, `alloc_l(A)` is `unk` at the allocation node and each concrete
deallocation gives `drop_l(A)=unk` at its node.  Positive allocation-event atoms
are never `tt`, so `!alloc_l(A)` is always `unk` or `tt`, hence never `ff`.
Using the concrete path supplied by A1, the second drop lifts backwards through
`E[!alloc_l(A) U drop_l(A)]`, then through the preceding `EX`, then the first
`drop_l(A) && ...`, then `EF`, then the `EX` after the allocation node.  Meeting
with `alloc_l(A)=unk` remains non-`ff`; the outer `EF` lifts the witness to the
entry.  By A4 the existential allocation quantifier includes `A`, so its join is
non-`ff`.  Finally, every candidate body contains a positive `alloc_l`, which is
never `tt`; therefore the complete query cannot evaluate to `tt`.  The only
remaining value is `unk`. QED.

## Theorem UAF — complete use-after-free query no-refutation

Under A1–A4, if a concrete modeled execution allocates `a`, later deallocates
that same `a`, and later performs a modeled read/write/use of `a`, then the
official query

```cqpl
exists_alloc a. EF (
  alloc_l(a) &&
  EX EF (
    drop_l(a) &&
    EX E[(!alloc_l(a)) U use_l(a)]
  )
)
```

evaluates to `unk` at entry.

**Proof.** Identical to the DF proof, replacing the final drop witness by the
concrete use witness.  A2 yields `use_l(A)=unk`; the negative allocation guard
cannot be `ff`; the existential temporal operators lift the concrete ICFG path;
A4 supplies the quantified witness.  The positive `alloc_l` conjunct excludes
`tt`, hence the result is exactly `unk`. QED.

## Theorem Leak — complete leak query no-refutation

Under A1–A5, if a concrete modeled execution allocates `a` and has a maximal
post-allocation execution suffix on which that allocation is never deallocated,
then the official query

```cqpl
exists_alloc a. EF (alloc_l(a) && EX EG !drop_l(a))
```

evaluates to `unk` at entry.

**Proof.** A2 gives `alloc_l(A)=unk` at the allocation node.  Because positive
allocation-event atoms are only `ff` or `unk`, `!drop_l(A)` is always `tt` or
`unk`; in particular it is non-`ff` at every node of the concrete maximal leak
suffix.  By the `EG` lifting lemma and A1/A5, `EG !drop_l(A)` is non-`ff` at the
post-allocation successor; `EX` lifts it to the allocation node.  Meeting with
`alloc_l(A)=unk` yields `unk`, and `EF` plus `exists_alloc` lift the witness to
entry.  Again the positive allocation atom prevents a `tt` result, so the
result is exactly `unk`. QED.

## Consequence and limitation

For the modeled, block-observable fragment satisfying A1–A5:

`ConcreteDF => CQPL_DF = unk`,
`ConcreteUAF => CQPL_UAF = unk`, and
`ConcreteLeak => CQPL_Leak = unk`.

This is a **no-refutation/no-false-negative property**, not a bug-certification
property: `unk` may also occur without a concrete bug because the analysis is a
MAY over-approximation.  The theorem does not cover unmodeled UB classes,
arbitrary CQPL formulas, or concrete event multiplicities/order that the ICFG
representation itself does not preserve.
