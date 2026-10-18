#!/usr/bin/env python3
"""
ci_filter_testcode.py — Filter rg output to drop lines inside test-only cfg blocks.

Usage:
    rg ... src/ | python3 ci_filter_testcode.py <src_dir>

Reads the rg output (path:line:content) from stdin.
For each unique source file in the hits, reads the file, finds all cfg attributes
whose condition includes `test`, drops hits inside those test-only blocks.
"""

import os
import re
import sys
from collections import defaultdict


def find_test_lines(src_path: str) -> set[int]:
    """Return the set of line numbers inside cfg attributes containing test."""
    if not os.path.isfile(src_path):
        return set()
    with open(src_path) as f:
        content = f.read()

    test_lines: set[int] = set()
    # Match both #[cfg(test)] and forms such as #[cfg(all(test, target_os = "linux"))].
    cfg_positions = [
        m.start()
        for m in re.finditer(r"#\[cfg\([^]]*\btest\b[^]]*\)\]", content)
    ]

    for pos in cfg_positions:
        brace_start = content.find("{", pos)
        if brace_start == -1:
            continue
        # #[cfg(test)] mod tests; / #[cfg(test)] use x; declare no block here.
        semi = content.find(";", pos)
        if semi != -1 and semi < brace_start:
            continue
        depth = 0
        line_no = content[:brace_start].count("\n") + 1
        for ch in content[brace_start:]:
            if ch == "\n":
                line_no += 1
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    test_lines.add(line_no)
                    break
                if depth < 0:
                    depth = 0
            test_lines.add(line_no)
    return test_lines


def main():
    if len(sys.argv) < 2:
        print("Usage: ci_filter_testcode.py <src_dir>", file=sys.stderr)
        sys.exit(2)
    src_dir = sys.argv[1]

    hits_by_file: dict[str, list[tuple[int, str]]] = defaultdict(list)
    all_lines = sys.stdin.read().splitlines()

    for line in all_lines:
        line = line.rstrip("\n")
        if not line:
            continue
        parts = line.split(":", 2)
        if len(parts) < 3:
            print(line)
            continue
        fpath, lno_str, _content = parts
        try:
            lno = int(lno_str)
        except ValueError:
            print(line)
            continue
        hits_by_file[fpath].append((lno, line))

    violations = []
    for fpath, hits in hits_by_file.items():
        abs_path = os.path.join(src_dir, fpath)
        if not os.path.isfile(abs_path):
            abs_path = os.path.join(src_dir, os.path.basename(fpath))
        test_lines = find_test_lines(abs_path)
        for lno, line in hits:
            if lno not in test_lines:
                violations.append(line)

    for violation in violations:
        print(violation)


if __name__ == "__main__":
    main()
