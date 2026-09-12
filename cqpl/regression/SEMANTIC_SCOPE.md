# CQPL regression semantic scope (v2)

## Modeled memory-error formulas

The current formal CQPL layer evaluates exactly three memory-error families:

- Leak
- Double Free (DF)
- Use After Free (UAF)

The regression runner therefore executes exactly the corresponding three query
files for every target.

`UB_FFI` from the legacy CREMA detector is **not silently mapped** to one of
these three properties.  The current CQPL formal language has no allocator-
family/provenance predicate able to distinguish, for example, C `malloc`
origin from a Rust allocator origin and no formal label predicate for an
allocator-family/ownership-contract mismatch.

Adding an `UB_FFI` CQPL query before extending the formal syntax, abstract
Kripke annotation, and atomic-predicate semantics would therefore create a
query whose meaning is not justified by the current theory.

The runner records `UB_FFI` as an explicitly unmodeled legacy class.

## Why there is no `no_errors.cqpl`

A formula can be written syntactically as the negation of a finite disjunction
of error formulas.  For example, at the meta level,

    !(Leak || DF || UAF)

would be the complement of those three formulas.

This is **not** equivalent to proving that the program is memory-safe:

1. it covers only the error families included in the disjunction;
2. in the three-valued semantics, `!unk = unk`;
3. a sound over-approximation may therefore leave the complement unknown;
4. the present formal result establishes the required no-false-negative
   property for positive atomic may predicates, not yet an unrestricted
   formula-level theorem for arbitrary CQPL/CTL formulas.

For this reason the regression runner emits the derived status

- `all-modeled-queries-refuted`, or
- `at-least-one-modeled-query-nonrefuting`

instead of a misleading `NO_ERRORS` verdict.

`all-modeled-queries-refuted` means only that the currently modeled Leak/DF/UAF
formulas all evaluate to `ff` on the exported abstract Kripke.

## UB_FFI extension path

A principled future CQPL extension should first formalize enough information to
express allocator/ownership-family contracts.  One possible design is to add
an implementation/formal provenance component and corresponding MAY
predicates, together with syntactic labels for relevant deallocation/ownership
APIs.  The concrete predicate, abstraction, three-valued atomic semantics, and
soundness statement must be defined before using such a query as an oracle.
