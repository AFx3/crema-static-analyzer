#!/usr/bin/env python3
import importlib.util
import tempfile
import subprocess
import sys
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("runner", HERE / "run_target_repo_cqpl.py")
runner = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(runner)


class HarnessTests(unittest.TestCase):
    def test_literal_target_keys_match_frozen_protocol(self):
        self.assertEqual(
            runner.target_key("a-code_full_rust/a-double_free_full_rust_literals/boxed_i32"),
            "boxed_i32__df",
        )
        self.assertEqual(
            runner.target_key("a-code_full_rust/a-memory_leaks_full_rust_literals/boxed_u8"),
            "boxed_u8__ml",
        )
        self.assertEqual(
            runner.target_key("a-code_full_rust/a-use_after_free_full_rust_literals/boxed_bool"),
            "boxed_bool__uaf",
        )

    def test_minimal_cargo_root_discovery_avoids_workspace_member_duplication(self):
        with tempfile.TemporaryDirectory() as td:
            tests = Path(td)
            root = tests / "category" / "workspace"
            member = root / "member"
            member.mkdir(parents=True)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (member / "Cargo.toml").write_text("[package]\nname='member'\nversion='0.1.0'\n", encoding="utf-8")
            roots = runner.discover_minimal_cargo_roots(tests)
            self.assertEqual(roots, [root.resolve()])

    def test_differential_relation_is_directionally_neutral(self):
        self.assertEqual(
            runner.relation(["UAF"], "use_after_free", "unk"),
            "legacy-positive/cqpl-nonrefuting",
        )
        self.assertEqual(
            runner.relation(["UAF"], "use_after_free", "ff"),
            "REVIEW:legacy-positive/cqpl-refuted",
        )
        self.assertEqual(
            runner.relation([], "use_after_free", "unk"),
            "cqpl-only-nonrefuting",
        )
        self.assertEqual(
            runner.relation(None, "use_after_free", "ff"),
            "no-legacy-reference",
        )

    def test_frozen_scope_excludes_only_declared_late_or_protocol_targets(self):
        with tempfile.TemporaryDirectory() as td:
            tests = Path(td)
            rels = [
                "a-code_full_rust/clean_into_from_raw",
                "a-code_full_rust/drop_raw_ptr_no_free",
                "no_errors_projects/openapi-client-gen",
                "a-code_c_to_rust_alloc/c_malloc_rust_free_clean",
            ]
            roots = []
            for rel in rels:
                p = tests / rel
                p.mkdir(parents=True)
                (p / "Cargo.toml").write_text("[package]\nname='x'\nversion='0.1.0'\n", encoding="utf-8")
                roots.append(p.resolve())
            frozen = [rel for rel, _ in runner.select_targets(roots, tests, "frozen92")]
            self.assertEqual(frozen, ["a-code_full_rust/clean_into_from_raw"])


    def test_runner_timeout_is_enforced(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            runner.run(
                [sys.executable, "-c", "import time; time.sleep(2)"],
                timeout=0.05,
            )

    def test_ub_ffi_is_explicitly_unmodeled_by_current_query_set(self):
        self.assertIn("UB_FFI", runner.UNMODELED_LEGACY_CLASSES)
        self.assertNotIn("UB_FFI", runner.LEGACY_CLASS_FOR_QUERY.values())

    def test_lock_free_entry_override_matches_current_icfg_identifier(self):
        reference = HERE.parent / "reference" / "entry_overrides.json"
        import json
        entries = json.loads(reference.read_text(encoding="utf-8"))
        self.assertEqual(
            entries["found vulns/lock-free"],
            "rust::LockFreeStack::<T>::push::bb0",
        )


if __name__ == "__main__":
    unittest.main()
