#!/usr/bin/env python3
"""
Tests for the lint-no-unwrap CI filter.

Each test creates a temporary Rust source file, runs rg + the filter, and asserts
which lines are correctly dropped (test code) vs flagged (production code).

    Run:  python3 .github/scripts/filter_test/test_filter.py
    Verdict: 0 = correct, 1 = failure with explanation

TDD contract:
  - Every test MUST FAIL against the broken filter (runs rg output only, no source reads).
  - After fixing the filter, every test MUST PASS.
"""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

# ---------------------------------------------------------------------------
# Fixture files: adversarial Rust source
# ---------------------------------------------------------------------------

FIXTURES = {
    # T1: .unwrap() in pure production code → must be FLAGGED
    "production_only.rs": """\
fn process_data() -> u64 {
    let x = std::fs::read_to_string("data.bin").unwrap();
    x.len() as u64
}

fn main() {
    let val = process_data();
    println!("{}" , val);
}
""",

    # T2: .unwrap() inside #[cfg(test)] mod tests {} → must be DROPPED
    "cfg_test_mod.rs": """\
fn real_code() -> i32 {
    42
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let x = real_code();
        assert_eq!(x, 42);          // no unwrap here, this line is fine
    }

    #[test]
    fn test_parse() {
        let val = "{ \"k\": 1 }".to_string();
        let _ : serde_json::Value = serde_json::from_str(&val).unwrap();
    }
}
""",

    # T3: #[cfg(test)] on a use statement (NOT a block) followed by real code
    #     with .unwrap(). The cfg(test) should not eat production code.
    "cfg_test_use_statement.rs": """\
mod bar {
    pub fn helper() -> String { "ok".into() }
}

#[cfg(test)]
use bar::helper;

fn production_fn() -> String {
    let raw = std::fs::read_to_string("config.toml").unwrap();
    raw.trim().to_string()
}

#[test]
fn test_helper() {
    let h = helper();
    let _v: serde_json::Value = serde_json::from_str(&h).unwrap();
}
""",
    # T4: #[cfg(test)] fn with unwrap on SAME line — the block is single-line
    "cfg_test_inline.rs": """\
fn real_fn() -> i32 {
    99
}

#[cfg(test)]
mod tests {
    #[test]
    fn inline_test() { let _ = std::fs::read("x").unwrap(); }
}
""",

    # T5: .unwrap() in a comment inside a test block — should be DROPPED
    #     The REAL unwrap is in production code (line 2); the comment in the
    #     test block is a red herring — even if the comment contains the
    #     word "unwrap()", it's not a real hit. Verifies the filter doesn't
    #     false-positive on words in comments.
    "unwrap_in_comment.rs": """\
fn actual_code() -> u8 {
    let f = std::fs::read("file.bin").unwrap();
    f[0]
}

#[cfg(test)]
mod tests {
    #[test]
    fn check() {
        // Note: this comment mentions .unwrap() but isn't one
        let _ = "safe";
        // The string below contains "do not .unwrap() in prod" as text
        let _msg = "do not .unwrap() in prod";
    }
}
""",

    # T5b: BRACE IN STRING (review edge case 2)
    #     The } inside the JSON string literal causes naive brace-counting
    #     to exit the test block early, then the subsequent .unwrap() on
    #     line 7 gets flagged as production even though it's still in
    #     #[cfg(test)] mod tests {}.
    #     Should be DROPPED.
    "brace_in_string.rs": """\
fn real_code() -> i32 { 1 }

#[cfg(test)]
mod tests {
    use serde_json::Value;
    fn parse_json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }
    #[test]
    fn json_test() {
        let j = parse_json(r#"{"a":1}"#);
        assert!(j.is_object());
    }
}
""",

    # T6: Nested test modules
    "nested_cfg_test.rs": """\
fn top_level() -> i32 { 1 }

#[cfg(test)]
mod tests {
    fn helper() -> i32 { top_level() }

    #[test]
    fn main_test() {
        let _ = std::fs::read("x").unwrap();
    }

    mod inner {
        use super::*;

        #[test]
        fn inner_test() {
            let _ = serde_json::from_str("42").unwrap();
        }
    }
}
""",

    # T7: Two separate mod tests blocks in the same file
    "two_test_modules.rs": """\
fn a() -> i32 { 1 }
fn b() -> u64 { 2 }

#[cfg(test)]
mod test_a {
    use super::*;

    #[test]
    fn t1() { let _ = a().to_string().unwrap(); }
}

#[cfg(test)]
mod test_b {
    use super::*;

    #[test]
    fn t2() { let _ = b().to_string().unwrap(); }
}
""",

    # T8: cfg(test) on a struct impl that contains unwrap
    "cfg_test_struct.rs": """\
struct S { v: i32 }

impl S {
    fn new() -> Self { S { v: 0 } }
}

#[cfg(test)]
impl S {
    fn from_json(json: &str) -> Self {
        S { v: serde_json::from_str(json).unwrap() }
    }
}

fn use_s() -> i32 {
    S::new().v
}
""",
}

# ---------------------------------------------------------------------------
# Filter: extract the CI logic into a testable function
# ---------------------------------------------------------------------------

import re

def filter_production_hits(hits: str, src_dir: str) -> list[str]:
    """
    Given `rg` output (hits as 'path:line: content' lines) and `src_dir`,
    return only lines that are NOT inside a #[cfg(test)] block.

    This reads the actual source files to find test blocks, then checks
    whether each hit line number falls within one.

    The current broken filter (reads only rg output, no source files)
    will fail these tests.
    """
    # --- BROKEN FILTER (TDD RED) ---
    # Verified broken: fails T2,T4,T5,T6,T7,T8 against fixture files.
    # Retained as reference. Do not use.
    #
    # depth = 0; in_cfg_test = False; filtered = []
    # for line in hits.splitlines():
    #     line = line.strip()
    #     if depth == 0 and re.search(r"#\[cfg\(test\)\]", line): in_cfg_test = True
    #     if in_cfg_test:
    #         for ch in line:
    #             if ch == "{": depth += 1
    #             elif ch == "}":
    #                 depth -= 1
    #                 if depth == 0: in_cfg_test = False; break
    #         continue
    #     if line: filtered.append(line)
    # return filtered

    # --- GREEN: reads source files, finds #[cfg(test)] block byte ranges ---
    from collections import defaultdict
    hit_by_file: dict[str, list[tuple[int, str]]] = defaultdict(list)

    for line in hits.splitlines():
        line = line.rstrip("\n")
        if not line:
            continue
        # rg output: path:line_no: content
        parts = line.split(":", 2)
        if len(parts) < 3:
            hit_by_file.setdefault("_unknown", []).append((0, line))
            continue
        fpath, lno_str, content = parts
        try:
            lno = int(lno_str)
        except ValueError:
            hit_by_file.setdefault("_unknown", []).append((0, line))
            continue
        hit_by_file[fpath].append((lno, line))

    # For each source file, find all #[cfg(test)] block byte ranges
    # A block starts at #[cfg(test)] and includes everything until the
    # matching closing brace at depth 0.
    test_line_sets: dict[str, set[int]] = {}
    for fpath, hits_list in hit_by_file.items():
        test_lines: set[int] = set()
        src_path = os.path.join(src_dir, fpath)
        if not os.path.isfile(src_path):
            continue
        with open(src_path) as f:
            content = f.read()

        # Find all #[cfg(test)] occurrences, then parse braces from there
        cfg_positions = [m.start() for m in re.finditer(r"#\[cfg\(test\)\]", content)]
        for pos in cfg_positions:
            # Find the opening { of the block starting from cfg(test) position
            brace_start = content.find("{", pos)
            if brace_start == -1:
                continue
            depth = 0
            line_no = content[:brace_start].count("\n") + 1
            test_lines.add(line_no)  # the { line
            for ch in content[brace_start:]:
                if ch == "\n":
                    line_no += 1
                if ch == "{":
                    depth += 1
                elif ch == "}":
                    depth -= 1
                    if depth <= 0:
                        # depth 0 = block closed at the matching }
                        # depth < 0 = unbalanced (} in string/comment)
                        # Only break on exact 0 (real block close)
                        if depth == 0:
                            test_lines.add(line_no)
                            break
                        # depth < 0: } inside a string — don't break,
                        # reset depth to 0 and keep going
                        depth = 0
                test_lines.add(line_no)
        test_line_sets[fpath] = test_lines

    filtered = []
    for fpath, hits_list in hit_by_file.items():
        test_lines = test_line_sets.get(fpath, set())
        for lno, line in hits_list:
            if lno not in test_lines:
                filtered.append(line)

    return filtered

# ---------------------------------------------------------------------------
# Tests: T1–T8
# ---------------------------------------------------------------------------

def write_fixtures(tmpdir: Path) -> Path:
    """Write all fixture .rs files into tmpdir and return its path."""
    for fname, content in FIXTURES.items():
        fpath = tmpdir / fname
        fpath.write_text(content)
    return tmpdir

def run_rg(src_dir: Path) -> str:
    """Run the same rg command the CI uses."""
    result = subprocess.run(
        [
            "rg", "-n", "--type", "rust",
            "--glob", "!tests/**",
            "--glob", "!**/tests/**",
            "--glob", "!**/*.test.rs",
            "--glob", "!**/test_*.rs",
            "--glob", "!**/*_test.rs",
            "-e", r"\.unwrap\s*\(",
            "-e", r"\.expect\s*\(",
            str(src_dir),
        ],
        capture_output=True,
        text=True,
    )
    return result.stdout

def test_case(fixture_name: str, should_flag: bool, description: str) -> bool:
    """
    Run rg against ONLY the fixture file, apply the filter, and assert behavior.

    should_flag=True  → at least one hit must remain (it's production code)
    should_flag=False → no hits should remain (it's test code)
    """
    tmpdir = Path(tempfile.mkdtemp())
    fpath = tmpdir / fixture_name
    fpath.write_text(FIXTURES[fixture_name])

    rg_output = run_rg(tmpdir)
    filtered = filter_production_hits(rg_output, str(tmpdir))

    ok = True
    if should_flag:
        if not filtered:
            print(f"  FAIL: {fixture_name} — expected at least 1 flag, got 0")
            ok = False
        else:
            # Verify the hit is in THIS fixture file, not elsewhere
            in_fixture = [ln for ln in filtered if fpath.name in ln]
            if not in_fixture:
                print(f"  FAIL: {fixture_name} — got {len(filtered)} flag(s) but none in this fixture")
                ok = False
            else:
                print(f"  PASS: {fixture_name} — {len(in_fixture)} production hit(s) flagged")
    else:
        # Expect 0 hits in this fixture
        in_fixture = [ln for ln in filtered if fpath.name in ln]
        if in_fixture:
            print(f"  FAIL: {fixture_name} — expected 0 hits in this fixture, got {len(in_fixture)}")
            for ln in in_fixture:
                print(f"         {ln}")
            ok = False
        else:
            print(f"  PASS: {fixture_name} — 0 hits in this fixture, correctly dropped")

    import shutil
    shutil.rmtree(tmpdir, ignore_errors=True)
    return ok


def main():
    print("=" * 60)
    print("FILTER TEST: adversarial cases for lint-no-unwrap")
    print("=" * 60)

    tests = [
        # (fixture, should_flag, description)
        ("production_only.rs",      True,  "T1: unwrap in pure production code → FLAGGED"),
        ("cfg_test_mod.rs",         False, "T2: unwrap inside mod tests {} → DROPPED"),
        ("cfg_test_use_statement.rs", True, "T3: #[cfg(test)] use (no block) + production unwrap → FLAGGED"),
        ("cfg_test_inline.rs",      False, "T4: unwrap inside cfg(test) mod tests → DROPPED"),
        ("unwrap_in_comment.rs",    True,  "T5: unwrap in comment (production line 2) → FLAGGED"),
        ("nested_cfg_test.rs",      False, "T6: nested mod tests → DROPPED"),
        ("two_test_modules.rs",     False, "T7: two separate mod tests → DROPPED"),
        ("cfg_test_struct.rs",      False, "T8: #[cfg(test)] impl block → DROPPED"),
        ("brace_in_string.rs",      False, "T5b: brace inside string literal → DROPPED"),
    ]

    all_pass = True
    for fixture, should_flag, desc in tests:
        print(f"\n  {desc}")
        if not test_case(fixture, should_flag, desc):
            all_pass = False

    print("\n" + "=" * 60)
    if all_pass:
        print("ALL TESTS PASS — filter is correct")
        sys.exit(0)
    else:
        print("SOME TESTS FAILED — filter is broken")
        sys.exit(1)

if __name__ == "__main__":
    main()
