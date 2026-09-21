# CQPL total transition semantics (cqpl4)

## Scope

This feature aligns whole-program CQPL truth evaluation with the theoretical
assumption that the Kripke transition relation is total.  It does **not**
change CREMA abstract interpretation, the annotated ICFG artifact, or the
intraprocedural (`--intra`) semantics.

## Producer model and truth model

Let the already entry-projected producer model be

\[
K=(B,R,L,\Pi^\#_{pre},\Pi^\#_{post},\ldots).
\]

`R` is the producer ICFG successor relation.  CREMA computes the abstract fixed
point and all node annotations before CQPL performs the construction below.
Hence no synthetic edge participates in abstract interpretation.

Let

\[
T=\{t\in B\mid Succ_R(t)=\varnothing\}
\]

be the deadlocks of the projected producer model.  For every `t in T`, introduce
a fresh CQPL-only completion state `hat(t)`.  Define

\[
B^+=B\cup\{\hat t\mid t\in T\}
\]

and

\[
R^+=R
 \cup\{(t,\hat t)\mid t\in T\}
 \cup\{(\hat t,\hat t)\mid t\in T\}.
\]

Therefore

\[
\forall b\in B^+.\ \exists b'.\ (b,b')\in R^+.
\]

`R+` is the relation used by whole-program CQPL truth evaluation and formula
witnesses.

## Quiescent completion states

A completion state represents stuttering *after* the final producer block has
executed.  It must not replay actions performed by that block.  For each
terminal `t`:

* `Pi_pre(hat(t)) = Pi_post(t)`;
* `Pi_post(hat(t)) = Pi_post(t)`;
* allocation-state post annotations are copied from `t`;
* panic-lifecycle state/coverage is copied from `t` because `repeat_drop` is a
  state-like MAY predicate;
* state-like points-to identity metadata is copied;
* event labels, allocation-event labels, structural MIR labels,
  allocation-disposition events, and event-identity metadata are empty;
* `hat(t)` has the single successor `hat(t)`.

This distinction is required because a MIR basic block contains zero or more
statements followed by a terminator.  A block whose terminator is `Return` can
therefore also contain reads, writes, or assignments.  Self-looping the real
basic block would replay those actions and could create spurious temporal event
patterns such as a false double-free or use-after-free.

## State/path semantics

The CQPL state and path semantic clauses are unchanged.  In particular the
standard definitions of `X`, `U`, `F`, `G`, `E`, and `A` are interpreted over
`R+`.  Because `R+` is total, every path is infinite and all path positions
indexed by natural numbers are defined.  No whole-program deadlock exception is
required for strong next or globally.

The synthetic completion construction changes the model, not the logic.

A consequence is deliberate and should not be confused with the legacy
maximal-finite-path semantics: if an event proposition holds in every producer
block of a finite terminating path, that finite prefix does **not** witness
`EG event`. After the final producer block, the path continues in the
quiescent completion state where event predicates are false. Conversely, a
real producer cycle on which the event holds can witness `EG event`. This is
exactly the infinite-path interpretation used by the theoretical `G` clause.

## Separation from diagnostic evidence

Producer diagnostics, assessment findings, source provenance, and
`typed_edge_flow_v1` certificates continue to use `(B,R)` exactly as exported
by CREMA.  Synthetic completion edges exist only in the CQPL truth model and are
not typed as normal/unwind producer edges.

The implementation enforces this separation with a distinct `CqplTruthModel`
type that intentionally contains no typed-edge/source-provenance evidence.

## MIR endpoint interpretation

The totalization applies mechanically to every deadlock of the projected CQPL
producer model; it does not require calling every deadlock a "normal program
exit".  The audited corpus contains normal returns, unwind-resume/terminate
endpoints, explicit unreachable endpoints, synthetic terminate nodes, and MIR
calls with no normal return target.  These operational categories remain
separate; completion is only the temporal convention that extends a maximal
finite producer path into an infinite quiescent path.

In rustc MIR, `Call.target = None` means that the call has no normal
continuation (it necessarily diverges); this does not imply that all such calls
are immediate process termination.

## Intraprocedural mode

`--intra` is intentionally out of scope for cqpl4.  Its hard interprocedural
cut can create scope-boundary deadlocks that are not program completion.  Until
that projection receives a separate formal treatment, the CLI selects the
legacy maximal-finite-path semantics for `--intra` and uses totalized semantics
only for the standard whole-program/reachable mode.

## Validation obligations

The feature is accepted only after:

1. CQPL unit tests establish totality, quiescence, frozen state, no event replay,
   CTL-next behavior, and preservation of legacy `--intra` behavior;
2. a differential 118-subject x 13-query run classifies every truth/assessment
   delta against the frozen `e89277b` artifact set;
3. a fresh full13 run regenerates CREMA artifacts and passes the existing L1,
   W1, double-free, UAF-unwind, and certificate gates.
