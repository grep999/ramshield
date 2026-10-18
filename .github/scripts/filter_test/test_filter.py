#!/usr/bin/env python3
"""Regression tests for the CI cfg(test) unwrap/expect filter."""

import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci_filter_testcode.py"
SPEC = importlib.util.spec_from_file_location("ci_filter_testcode", SCRIPT)
FILTER = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(FILTER)


class FindTestLinesTests(unittest.TestCase):
    def test_excludes_hits_inside_nested_cfg_test_module_only(self):
        source = """fn production() {
    value.unwrap();
}
#[cfg(test)]
mod tests {
    fn nested() {
        if true {
            fixture.expect("test");
        }
    }
}
"""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "sample.rs"
            path.write_text(source)
            test_lines = FILTER.find_test_lines(str(path))
        lines = source.splitlines()
        production_line = next(i for i, line in enumerate(lines, 1) if "value.unwrap" in line)
        fixture_line = next(i for i, line in enumerate(lines, 1) if "fixture.expect" in line)
        self.assertNotIn(production_line, test_lines)
        self.assertIn(fixture_line, test_lines)

    def test_compound_cfg_test_module_is_excluded(self):
        source = """fn production() {
    value.unwrap();
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    fn test_only() {
        fixture.expect("test");
    }
}
"""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "sample.rs"
            path.write_text(source)
            test_lines = FILTER.find_test_lines(str(path))
        lines = source.splitlines()
        production_line = next(i for i, line in enumerate(lines, 1) if "value.unwrap" in line)
        fixture_line = next(i for i, line in enumerate(lines, 1) if "fixture.expect" in line)
        self.assertNotIn(production_line, test_lines)
        self.assertIn(fixture_line, test_lines)

    def test_cfg_test_module_declaration_does_not_swallow_following_code(self):
        source = """#[cfg(test)] mod tests;
fn production() {
    value.unwrap();
}
"""
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "sample.rs"
            path.write_text(source)
            test_lines = FILTER.find_test_lines(str(path))
        production_line = next(
            i for i, line in enumerate(source.splitlines(), 1) if "value.unwrap" in line
        )
        self.assertNotIn(production_line, test_lines)

    def test_missing_source_file_is_safe(self):
        with tempfile.TemporaryDirectory() as temp:
            missing = Path(temp) / "missing.rs"
            self.assertEqual(FILTER.find_test_lines(str(missing)), set())


if __name__ == "__main__":
    unittest.main(verbosity=2)
