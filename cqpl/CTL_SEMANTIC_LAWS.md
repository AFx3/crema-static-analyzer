# CQPL three-valued CTL semantic laws

Status: validation specification for the total whole-program CQPL truth model.

This document fixes the algebraic assumptions, the CTL equations implemented by
`cqpl_checker`, the laws that are proved from those assumptions, and the bounded
exhaustive experiments used to cross-check the Rust implementation.

`--intra` is deliberately outside this claim: it retains the legacy partial /
maximal-finite-path compatibility semantics and is used as a negative control.

## 1. Truth algebra

CQPL uses the finite chain

\[
\mathbb B_3 = \{\mathsf{ff} < \mathsf{unk} < \mathsf{tt}\}.
\]

For `x,y in B3`:

\[
x \wedge y = \min(x,y), \qquad
x \vee y = \max(x,y),
\]

and negation is

\[
\neg\mathsf{ff}=\mathsf{tt},\quad
\neg\mathsf{unk}=\mathsf{unk},\quad
\neg\mathsf{tt}=\mathsf{ff}.
\]

Negation is an involutive order anti-isomorphism. Therefore De Morgan holds:

\[
\neg(x\vee y)=\neg x\wedge\neg y,\qquad
\neg(x\wedge y)=\neg x\vee\neg y.
\]

CQPL is **not Boolean**. In particular,

\[
\mathsf{unk}\vee\neg\mathsf{unk}=\mathsf{unk},\qquad
\mathsf{unk}\wedge\neg\mathsf{unk}=\mathsf{unk}.
\]

No proof below may silently use excluded middle or non-contradiction.

## 2. Total Kripke truth model

Let

\[
K=(S,R,V)
\]

be the whole-program CQPL truth model after quiescent completion totalization.
The totalization invariant is

\[
\forall s\in S.\;\exists t\in S.\;(s,t)\in R.
\]

The producer ICFG and its typed/provenance edges remain separate from this
semantic completion relation.

For a valuation `f : S -> B3`, define the predecessor transformers

\[
Pre_E(f)(s)=\bigvee_{(s,t)\in R} f(t),
\]

\[
Pre_A(f)(s)=\bigwedge_{(s,t)\in R} f(t).
\]

Because `R` is total, both aggregates are over non-empty finite successor sets.
By finite De Morgan,

\[
\neg Pre_E(f)=Pre_A(\neg f),
\qquad
\neg Pre_A(f)=Pre_E(\neg f).
\]

This is the first place where totalization is semantically material for the
current implementation: the legacy partial compatibility mode used a special
strong-next deadlock case and does not satisfy this duality.

## 3. Fixed-point semantics implemented by CQPL

All equations are over the finite complete lattice `B3^S`.

\[
EX\,p = Pre_E(p), \qquad AX\,p = Pre_A(p).
\]

\[
EF\,p = \mu Z.\;p\vee Pre_E(Z),
\]

\[
AF\,p = \mu Z.\;p\vee Pre_A(Z),
\]

\[
EG\,p = \nu Z.\;p\wedge Pre_E(Z),
\]

\[
AG\,p = \nu Z.\;p\wedge Pre_A(Z).
\]

\[
E[p\ U\ q]
 = \mu Z.\;q\vee(p\wedge Pre_E(Z)),
\]

\[
A[p\ U\ q]
 = \mu Z.\;q\vee(p\wedge Pre_A(Z)).
\]

The Rust implementation computes these fixed points by finite iteration from
`ff` for `mu` and from `tt` for `nu`.

## 4. Formally justified laws

### 4.1 Next duality

From predecessor duality:

\[
AX\,p = \neg EX\,\neg p.
\]

### 4.2 Least/greatest fixed-point duality

Let `N(f)=not f` pointwise. `N` is an order anti-isomorphism of `B3^S`.
For every monotone `F`, define

\[
F^N = N\circ F\circ N.
\]

On a finite complete lattice,

\[
N(\mu F)=\nu F^N,
\qquad
N(\nu F)=\mu F^N.
\]

Applying this theorem with predecessor duality gives

\[
AG\,p = \neg EF\,\neg p,
\]

\[
AF\,p = \neg EG\,\neg p,
\]

and equivalently

\[
EG\,p = \neg AF\,\neg p,
\qquad
EF\,p = \neg AG\,\neg p.
\]

These proofs use De Morgan and the anti-isomorphism property, not excluded
middle.

### 4.3 Fixed-point unfolding

Every computed least/greatest fixed point is a fixed point of its defining
functional, hence

\[
EF\,p = p\vee EX(EF\,p),
\]

\[
AF\,p = p\vee AX(AF\,p),
\]

\[
EG\,p = p\wedge EX(EG\,p),
\]

\[
AG\,p = p\wedge AX(AG\,p),
\]

\[
E[p\ U\ q]
 = q\vee(p\wedge EX(E[p\ U\ q])),
\]

\[
A[p\ U\ q]
 = q\vee(p\wedge AX(A[p\ U\ q])).
\]

### 4.4 Join/meet preservation used by the checker

Finite max/min aggregation gives

\[
EX(p\vee q)=EX\,p\vee EX\,q,
\]

\[
AX(p\wedge q)=AX\,p\wedge AX\,q.
\]

The corresponding reachability/safety closures preserve these operations:

\[
EF(p\vee q)=EF\,p\vee EF\,q,
\]

\[
AG(p\wedge q)=AG\,p\wedge AG\,q.
\]

### 4.5 Universal-until reduction

The standard CTL reduction

\[
A[p\ U\ q]
 =
 \neg E[\neg q\ U\ (\neg p\wedge\neg q)]
 \wedge
 \neg EG\,\neg q
\]

is included as an explicit validation obligation. It is exhaustively validated
by the independent finite-model oracle for all total models up to three states
and by the actual Rust checker for all generated models up to two producer
states. The current document treats this equality as **experimentally
validated plus standard fixed-point reduction**, rather than claiming a new
standalone mechanized proof for the three-valued setting.

## 5. Laws that are deliberately *not* claimed

The gate contains counterexamples for tempting but invalid identities.
For example, in general

\[
EG(p\vee q) \ne EG\,p\vee EG\,q,
\]

because one infinite path can alternate which disjunct supplies the truth.
Likewise

\[
EX(p\wedge q) \ne EX\,p\wedge EX\,q,
\]

because the existential witnesses for `p` and `q` may be different successors.

The gate must find such counterexamples; otherwise the experimental harness is
considered suspect.

## 6. Experimental validation design

### 6.1 Actual Rust checker

`cqpl/cqpl_checker/tests/ctl_semantic_laws.rs` tests the public checker API.
It does not call private fixed-point helpers.

Two program variables encode arbitrary ternary atomic valuations without a
special test primitive:

* `read_l(x)` supplies `tt`;
* `alloc(x)` over abstract `TOP` supplies `unk` when the read label is absent;
* absence of both supplies `ff`.

The test enumerates **all producer successor relations with 1 or 2 states**,
including producer deadlocks. Deadlocks are therefore exercised through the
real `ModelChecker::new` quiescent-totalization path.

For every relation it enumerates all ternary valuations of `p` and `q` and
every original state as the CQPL entry. The resulting number of checked entry
configurations is

\[
2\cdot 3^2\cdot1
+
4^2\cdot3^4\cdot2
=
2610.
\]

All sixteen positive laws are checked at each entry configuration.

The same integration test also contains negative controls for:

* failure of excluded middle at `unk`;
* invalid `EG` distribution over disjunction;
* invalid `EX` distribution over conjunction;
* failure of `AX/EX` duality under the deliberately partial legacy `--intra`
  deadlock semantics.

### 6.2 Independent Python oracle

`cqpl/scripts/gate_ctl_semantic_laws.py` does **not** import, execute, or parse
`cqpl_checker`. It independently implements the mathematical equations above.

It exhaustively enumerates every total relation and every ternary valuation of
`p,q` up to three states:

| states | total relations | valuations / atom | relation+`p,q` cases |
|---:|---:|---:|---:|
| 1 | 1 | 3 | 9 |
| 2 | 9 | 9 | 729 |
| 3 | 343 | 27 | 250047 |

Total binary cases per law:

\[
250785.
\]

With sixteen positive laws, the gate performs

\[
4,012,560
\]

law checks, in addition to the truth-algebra obligations and required negative
counterexamples.

## 7. Interpretation and limits

A PASS establishes three distinct facts:

1. the algebraic proof obligations listed in Sections 1--4 are internally
   consistent with the intended finite-lattice semantics;
2. an independent executable oracle finds no counterexample within the complete
   state-space bound `|S| <= 3`;
3. the actual Rust checker agrees with all listed laws on an exhaustive family
   of small producer graphs that includes deadlock totalization.

It is **not** a formal verification of the Rust implementation for arbitrary
model size. The bounded oracle is experimental evidence; the algebraic
arguments establish the semantic laws of the specified equations; agreement
between the two and the Rust integration test is the implementation-validation
bridge.

No claim in this document applies to the legacy partial `--intra` semantics.
