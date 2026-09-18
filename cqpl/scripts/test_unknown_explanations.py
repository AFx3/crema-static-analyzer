#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import io
import json
import stat
import tempfile
import unittest
from pathlib import Path

from unknown_explanations import UnknownExplanationError, explain_unknown


FAKE = r'''#!/usr/bin/env python3
import json, pathlib, sys
args=sys.argv[1:]
path=pathlib.Path(args[args.index("--explain-json")+1])
mode=pathlib.Path(sys.argv[0]).name
plain="tt" if "mismatch" in mode else "unk"
report_result="ff" if "reportmismatch" in mode else "unk"
reasons=[] if "noreason" in mode else ["MAY_ALLOCATION"]
specific=False if "nonspecific" in mode else True
assessment={
  "schema":"cqpl_result_assessment_v1",
  "result":"unk",
  "subresult":"unk_true",
  "direction":"true",
  "strength":"strong_abstract_evidence",
  "basis":["finding:test"],
  "caveats":["MAY evidence is never promoted to MUST"],
}
report={
  "result":report_result,
  "assessment":assessment,
  "reason_frontier":reasons,
  "witnesses":[{"truth":"unk"}],
  "supporting_findings":[{"kind":"normal_return_open_manual_obligation","strength":"strong_abstract_evidence"}],
  "diagnostics":{"unknown_has_reason_frontier":bool(reasons),"unknown_has_specific_origin":specific},
}
path.write_text(json.dumps(report)+"\n")
if "--explain-unk-verbose" in args:
    print("VERBOSE_UNKNOWN_REPORT", file=sys.stderr)
print(json.dumps({"result":plain,"assessment":assessment}))
'''


class UnknownExplanationTests(unittest.TestCase):
    def make_checker(self, root: Path, name: str) -> Path:
        p=root/name
        p.write_text(FAKE)
        p.chmod(p.stat().st_mode | stat.S_IXUSR)
        return p

    def test_non_unknown_does_not_run_explainer(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            result=explain_unknown(
                checker=root/"missing", artifact=root/"a.json", query=root/"q.cqpl",
                result="ff", explanation=root/"q.explain.json",
            )
            self.assertIsNone(result)
            self.assertFalse((root/"q.explain.json").exists())

    def test_unknown_generates_validated_report(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            checker=self.make_checker(root,"checker")
            q=root/"leak.cqpl"; q.write_text("q")
            art=root/"a.json"; art.write_text("{}")
            explain=root/"leak.explain.json"
            record=explain_unknown(
                checker=checker, artifact=art, query=q, result="unk", explanation=explain,
            )
            self.assertEqual(record["result"],"unk")
            self.assertEqual(record["subresult"],"unk_true")
            self.assertEqual(record["direction"],"true")
            self.assertEqual(record["strength"],"strong_abstract_evidence")
            self.assertEqual(record["reason_frontier"],["MAY_ALLOCATION"])
            self.assertEqual(record["supporting_findings"],1)
            self.assertTrue(explain.is_file())

    def test_verbose_unknown_forwards_checker_report_to_stderr(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            checker=self.make_checker(root,"checker")
            q=root/"leak.cqpl"; q.write_text("q")
            art=root/"a.json"; art.write_text("{}")
            explain=root/"leak.explain.json"
            stderr=io.StringIO()
            with contextlib.redirect_stderr(stderr):
                record=explain_unknown(
                    checker=checker, artifact=art, query=q, result="unk",
                    explanation=explain, verbose=True,
                )
            self.assertEqual(record["result"],"unk")
            self.assertIn("VERBOSE_UNKNOWN_REPORT", stderr.getvalue())

    def test_unknown_truth_mismatch_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            checker=self.make_checker(root,"checker-mismatch")
            q=root/"q.cqpl"; q.write_text("q")
            art=root/"a.json"; art.write_text("{}")
            with self.assertRaises(UnknownExplanationError):
                explain_unknown(
                    checker=checker, artifact=art, query=q, result="unk",
                    explanation=root/"q.explain.json",
                )

    def test_unknown_without_specific_frontier_fails_closed(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            checker=self.make_checker(root,"checker-nonspecific")
            q=root/"q.cqpl"; q.write_text("q")
            art=root/"a.json"; art.write_text("{}")
            with self.assertRaises(UnknownExplanationError):
                explain_unknown(
                    checker=checker, artifact=art, query=q, result="unk",
                    explanation=root/"q.explain.json",
                )


if __name__ == "__main__":
    unittest.main()
