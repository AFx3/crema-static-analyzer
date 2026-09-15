#!/usr/bin/env python3
"""Fail-closed automatic explainability for CQPL Unknown results.

This module does not participate in CQPL truth evaluation.  Callers first run a
query normally and pass its frozen three-valued result here.  Only when that
result is ``unk`` do we re-run the same checker/query with ``--explain-json``
and require a valid, specific explanation whose reported truth is still
``unk``.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any


class UnknownExplanationError(RuntimeError):
    """An ``unk`` query could not be paired with a valid explanation report."""


def explain_unknown(
    *,
    checker: Path,
    artifact: Path,
    query: Path,
    result: str,
    explanation: Path,
    max_witnesses: int = 8,
    cwd: Path | None = None,
    timeout: float | None = None,
    verbose: bool = False,
) -> dict[str, Any] | None:
    """Generate and validate an explanation sidecar iff ``result == 'unk'``.

    The semantic query has already been evaluated by the caller.  This function
    is observational: it must reproduce the same ``unk`` result and may not
    promote/demote truth.  Validation mirrors the v6R explainability gates:
    every unknown needs a non-empty reason frontier and a specific uncertainty
    origin.
    """
    if result != "unk":
        return None
    if max_witnesses < 1:
        raise UnknownExplanationError("max_witnesses must be >= 1")

    explanation.parent.mkdir(parents=True, exist_ok=True)
    explanation.unlink(missing_ok=True)
    cmd = [
        str(checker),
        str(artifact),
        str(query),
        "--json",
        "--explain-json",
        str(explanation),
        "--explain-max-witnesses",
        str(max_witnesses),
    ]
    if verbose:
        cmd.append("--explain-unk-verbose")
    try:
        cp = subprocess.run(
            cmd,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as e:
        explanation.unlink(missing_ok=True)
        raise UnknownExplanationError(
            f"explanation timed out after {timeout}s for query {query.name}"
        ) from e

    if verbose and cp.stderr:
        sys.stderr.write(cp.stderr)
        sys.stderr.flush()

    if cp.returncode != 0:
        explanation.unlink(missing_ok=True)
        detail = " ".join(x.strip() for x in cp.stderr.splitlines() if x.strip())[:500]
        raise UnknownExplanationError(
            f"explanation checker failed rc={cp.returncode} for {query.name}: {detail}"
        )
    if not explanation.is_file():
        raise UnknownExplanationError(
            f"checker returned zero but explanation sidecar is missing for {query.name}"
        )

    try:
        plain = json.loads(cp.stdout)
        report = json.loads(explanation.read_text(encoding="utf-8"))
    except Exception as e:
        explanation.unlink(missing_ok=True)
        raise UnknownExplanationError(
            f"invalid explanation JSON for {query.name}: {type(e).__name__}: {e}"
        ) from e

    if plain.get("result") != "unk":
        raise UnknownExplanationError(
            f"explanation re-run changed query truth for {query.name}: {plain.get('result')!r}"
        )
    if report.get("result") != "unk":
        raise UnknownExplanationError(
            f"explanation report/result mismatch for {query.name}: {report.get('result')!r}"
        )

    reasons = sorted({str(x) for x in report.get("reason_frontier", [])})
    diagnostics = report.get("diagnostics", {})
    if not reasons or not diagnostics.get("unknown_has_reason_frontier", False):
        raise UnknownExplanationError(
            f"unknown query {query.name} has no validated reason frontier"
        )
    if not diagnostics.get("unknown_has_specific_origin", False):
        raise UnknownExplanationError(
            f"unknown query {query.name} has no specific uncertainty origin"
        )

    findings = report.get("supporting_findings", [])
    if not isinstance(findings, list):
        raise UnknownExplanationError(
            f"supporting_findings must be an array in {explanation}"
        )

    return {
        "query": query.stem,
        "result": "unk",
        "explanation": str(explanation),
        "reason_frontier": reasons,
        "witnesses": len(report.get("witnesses", [])),
        "supporting_findings": len(findings),
        "supporting_finding_kinds": sorted({
            str(f.get("kind")) for f in findings if isinstance(f, dict) and f.get("kind")
        }),
        "supporting_finding_strengths": sorted({
            str(f.get("strength")) for f in findings if isinstance(f, dict) and f.get("strength")
        }),
    }
