#!/usr/bin/env python3
"""Adversarial regression tests for ci_filter_testcode.py."""
from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
FILTER = REPO / ".github" / "scripts" / "ci_filter_testcode.py"


class FilterTestCodeTests(unittest.TestCase):
    def run_filter(self, source: str) -> list[str]:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = root / "fixture.rs"
            fixture.write_text(source, encoding="utf-8")
            lines = source.splitlines()
            hits = [
                f"fixture.rs:{number}:{line}"
                for number, line in enumerate(lines, 1)
                if ".unwrap(" in line or ".expect(" in line
            ]
            result = subprocess.run(
                [sys.executable, str(FILTER), str(root)],
                input="\n".join(hits) + ("\n" if hits else ""),
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            return result.stdout.splitlines()

    def test_test_module_is_filtered_but_production_hit_remains(self) -> None:
        violations = self.run_filter(
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    fn test_only() { value.unwrap(); }\n"
            "}\n"
            "fn production() { value.expect(\"required\"); }\n"
        )
        self.assertEqual(len(violations), 1)
        self.assertIn("fn production()", violations[0])

    def test_out_of_line_test_module_does_not_hide_production(self) -> None:
        violations = self.run_filter(
            "#[cfg(test)] mod tests;\n"
            "fn production() { value.unwrap(); }\n"
        )
        self.assertEqual(len(violations), 1)
        self.assertIn("fn production()", violations[0])

    def test_compound_cfg_test_module_is_filtered(self) -> None:
        violations = self.run_filter(
            '#[cfg(all(test, target_os = "linux"))]\n'
            "mod tests {\n"
            "    fn test_only() { value.unwrap(); }\n"
            "}\n"
            "fn production() { value.expect(\"required\"); }\n"
        )
        self.assertEqual(len(violations), 1)
        self.assertIn("fn production()", violations[0])

    def test_no_hits_is_clean(self) -> None:
        self.assertEqual(self.run_filter("fn production() {}\n"), [])


if __name__ == "__main__":
    unittest.main()
