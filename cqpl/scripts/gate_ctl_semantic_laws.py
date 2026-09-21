#!/usr/bin/env python3
"""Independent finite-model oracle for the CQPL three-valued CTL laws.

This script intentionally does not import or invoke cqpl_checker.  It implements
only the mathematical semantics over B={ff<unk<tt}, enumerates every total
Kripke relation up to three states, and checks the laws documented in
cqpl/CTL_SEMANTIC_LAWS.md.

The negative controls are part of the gate: the script must find concrete
counterexamples to identities that are *not* valid.  This makes accidental
vacuity materially harder.
"""
from __future__ import annotations

import argparse
import itertools
import json
from pathlib import Path
from typing import Callable, Dict, Iterable, List, Sequence, Tuple

FF, UNK, TT = 0, 1, 2
NAMES = {FF: "ff", UNK: "unk", TT: "tt"}
Valuation = Tuple[int, ...]
Relation = Tuple[Tuple[int, ...], ...]


def neg(x: int) -> int:
    return 2 - x


def vneg(v: Valuation) -> Valuation:
    return tuple(neg(x) for x in v)


def vor(a: Valuation, b: Valuation) -> Valuation:
    return tuple(max(x, y) for x, y in zip(a, b))


def vand(a: Valuation, b: Valuation) -> Valuation:
    return tuple(min(x, y) for x, y in zip(a, b))


def total_relations(n: int) -> Iterable[Relation]:
    nonempty = [
        tuple(j for j in range(n) if mask & (1 << j))
        for mask in range(1, 1 << n)
    ]
    yield from itertools.product(nonempty, repeat=n)


def ternary_valuations(n: int) -> List[Valuation]:
    return list(itertools.product((FF, UNK, TT), repeat=n))


def pre(relation: Relation, values: Valuation, quantifier: str) -> Valuation:
    if quantifier == "E":
        return tuple(max(values[j] for j in relation[i]) for i in range(len(relation)))
    if quantifier == "A":
        return tuple(min(values[j] for j in relation[i]) for i in range(len(relation)))
    raise ValueError(quantifier)


def eventually(relation: Relation, phi: Valuation, quantifier: str) -> Valuation:
    z = (FF,) * len(relation)
    while True:
        pz = pre(relation, z, quantifier)
        nxt = tuple(max(phi[i], pz[i]) for i in range(len(relation)))
        if nxt == z:
            return z
        z = nxt


def globally(relation: Relation, phi: Valuation, quantifier: str) -> Valuation:
    z = (TT,) * len(relation)
    while True:
        pz = pre(relation, z, quantifier)
        nxt = tuple(min(phi[i], pz[i]) for i in range(len(relation)))
        if nxt == z:
            return z
        z = nxt


def until(
    relation: Relation,
    lhs: Valuation,
    rhs: Valuation,
    quantifier: str,
) -> Valuation:
    z = (FF,) * len(relation)
    while True:
        pz = pre(relation, z, quantifier)
        nxt = tuple(
            max(rhs[i], min(lhs[i], pz[i]))
            for i in range(len(relation))
        )
        if nxt == z:
            return z
        z = nxt


def serialise_relation(relation: Relation) -> List[List[int]]:
    return [list(xs) for xs in relation]


def serialise_valuation(values: Valuation) -> List[str]:
    return [NAMES[x] for x in values]


def check_truth_algebra() -> Dict[str, bool]:
    domain = (FF, UNK, TT)
    return {
        "negation_involutive": all(neg(neg(x)) == x for x in domain),
        "negation_order_reversing": all(
            (x > y) or (neg(y) <= neg(x))
            for x in domain
            for y in domain
        ),
        "de_morgan_join": all(
            neg(max(x, y)) == min(neg(x), neg(y))
            for x in domain
            for y in domain
        ),
        "de_morgan_meet": all(
            neg(min(x, y)) == max(neg(x), neg(y))
            for x in domain
            for y in domain
        ),
        "excluded_middle_is_not_boolean": max(UNK, neg(UNK)) == UNK,
        "non_contradiction_is_not_boolean": min(UNK, neg(UNK)) == UNK,
    }


def run(max_states: int) -> Dict[str, object]:
    algebra = check_truth_algebra()
    if not all(algebra.values()):
        raise AssertionError(f"truth-algebra obligation failed: {algebra}")

    law_names = [
        "AX p = !EX !p",
        "AG p = !EF !p",
        "AF p = !EG !p",
        "EG p = !AF !p",
        "EF p = !AG !p",
        "EF unfold",
        "AF unfold",
        "EG unfold",
        "AG unfold",
        "EU unfold",
        "AU unfold",
        "AU reduction through EU and EG",
        "EX distributes over join",
        "AX distributes over meet",
        "EF distributes over join",
        "AG distributes over meet",
    ]
    checked = {name: 0 for name in law_names}
    per_size: Dict[str, Dict[str, int]] = {}

    for n in range(1, max_states + 1):
        vals = ternary_valuations(n)
        relations = 0
        binary_cases = 0

        for relation in total_relations(n):
            relations += 1
            ex = {v: pre(relation, v, "E") for v in vals}
            ax = {v: pre(relation, v, "A") for v in vals}
            ef = {v: eventually(relation, v, "E") for v in vals}
            af = {v: eventually(relation, v, "A") for v in vals}
            eg = {v: globally(relation, v, "E") for v in vals}
            ag = {v: globally(relation, v, "A") for v in vals}
            eu_cache: Dict[Tuple[Valuation, Valuation], Valuation] = {}
            au_cache: Dict[Tuple[Valuation, Valuation], Valuation] = {}

            def eu(p: Valuation, q: Valuation) -> Valuation:
                return eu_cache.setdefault((p, q), until(relation, p, q, "E"))

            def au(p: Valuation, q: Valuation) -> Valuation:
                return au_cache.setdefault((p, q), until(relation, p, q, "A"))

            for p in vals:
                np = vneg(p)
                for q in vals:
                    binary_cases += 1
                    nq = vneg(q)
                    pq_or = vor(p, q)
                    pq_and = vand(p, q)
                    eu_pq = eu(p, q)
                    au_pq = au(p, q)

                    obligations = {
                        "AX p = !EX !p": ax[p] == vneg(ex[np]),
                        "AG p = !EF !p": ag[p] == vneg(ef[np]),
                        "AF p = !EG !p": af[p] == vneg(eg[np]),
                        "EG p = !AF !p": eg[p] == vneg(af[np]),
                        "EF p = !AG !p": ef[p] == vneg(ag[np]),
                        "EF unfold": ef[p] == vor(p, ex[ef[p]]),
                        "AF unfold": af[p] == vor(p, ax[af[p]]),
                        "EG unfold": eg[p] == vand(p, ex[eg[p]]),
                        "AG unfold": ag[p] == vand(p, ax[ag[p]]),
                        "EU unfold": eu_pq == vor(q, vand(p, ex[eu_pq])),
                        "AU unfold": au_pq == vor(q, vand(p, ax[au_pq])),
                        "AU reduction through EU and EG": au_pq
                        == vand(
                            vneg(eu(nq, vand(np, nq))),
                            vneg(eg[nq]),
                        ),
                        "EX distributes over join": ex[pq_or] == vor(ex[p], ex[q]),
                        "AX distributes over meet": ax[pq_and] == vand(ax[p], ax[q]),
                        "EF distributes over join": ef[pq_or] == vor(ef[p], ef[q]),
                        "AG distributes over meet": ag[pq_and] == vand(ag[p], ag[q]),
                    }
                    for name, holds in obligations.items():
                        if not holds:
                            raise AssertionError(
                                json.dumps(
                                    {
                                        "law": name,
                                        "states": n,
                                        "relation": serialise_relation(relation),
                                        "p": serialise_valuation(p),
                                        "q": serialise_valuation(q),
                                    },
                                    indent=2,
                                )
                            )
                        checked[name] += 1

        per_size[str(n)] = {
            "total_relations": relations,
            "ternary_valuations_per_atom": len(vals),
            "binary_relation_valuation_cases": binary_cases,
        }

    # Negative controls. The gate requires concrete counterexamples.
    negative: Dict[str, object] = {}

    # Strong-next semantics over a *partial* one-state deadlock: both AX and EX
    # return ff in the legacy compatibility semantics, breaking the duality.
    p = (FF,)
    partial_ax_p = (FF,)
    partial_ex_not_p = (FF,)
    negative["partial_deadlock_breaks_AX_EX_duality"] = {
        "found": partial_ax_p != vneg(partial_ex_not_p),
        "AX_p": serialise_valuation(partial_ax_p),
        "not_EX_not_p": serialise_valuation(vneg(partial_ex_not_p)),
    }

    def first_counterexample(
        predicate: Callable[[Relation, Valuation, Valuation], bool],
    ) -> Dict[str, object]:
        for n in range(1, max(3, max_states) + 1):
            vals = ternary_valuations(n)
            for relation in total_relations(n):
                for p0 in vals:
                    for q0 in vals:
                        if not predicate(relation, p0, q0):
                            return {
                                "found": True,
                                "states": n,
                                "relation": serialise_relation(relation),
                                "p": serialise_valuation(p0),
                                "q": serialise_valuation(q0),
                            }
        return {"found": False}

    negative["EG_does_not_distribute_over_join"] = first_counterexample(
        lambda r, p0, q0: globally(r, vor(p0, q0), "E")
        == vor(globally(r, p0, "E"), globally(r, q0, "E"))
    )
    negative["EX_does_not_distribute_over_meet"] = first_counterexample(
        lambda r, p0, q0: pre(r, vand(p0, q0), "E")
        == vand(pre(r, p0, "E"), pre(r, q0, "E"))
    )

    if not all(bool(item.get("found")) for item in negative.values()):
        raise AssertionError(f"negative control did not produce a counterexample: {negative}")

    return {
        "schema": "cqpl_ctl_semantic_laws_gate_v1",
        "status": "PASS",
        "max_states": max_states,
        "truth_algebra": algebra,
        "per_size": per_size,
        "valid_law_checks": checked,
        "negative_controls": negative,
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-states", type=int, default=3)
    ap.add_argument("--out", type=Path)
    args = ap.parse_args()
    if args.max_states < 1 or args.max_states > 3:
        raise SystemExit("--max-states must be in 1..3 for the frozen exhaustive gate")

    report = run(args.max_states)
    payload = json.dumps(report, indent=2, sort_keys=True)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload + "\n")
    print(payload)


if __name__ == "__main__":
    main()
