#!/usr/bin/env python3
"""Enforce the incremental coverage gate on changed redaction-chain lines.

The whole-repo number stays informational (#569); only lines a change adds to
the redaction-chain scope must be covered, and at a configurable minimum. The
scope file lists include globs plus `!`-prefixed excludes for test-only paths
that cargo-llvm-cov omits from LCOV by default. For a file that IS in the
coverage report, the denominator is its changed executable lines (comments and
blanks never count); a changed in-scope production file MISSING from the report
was never instrumented, so its changed lines count as uncovered and the gate
fails instead of passing vacuously.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lcov_lib import in_scope, parse_lcov, parse_scope, pct  # noqa: E402

HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def split_scope(patterns: list[str]) -> tuple[list[str], list[str]]:
    """Split scope lines into include globs and `!`-prefixed exclude globs."""

    includes: list[str] = []
    excludes: list[str] = []
    for pattern in patterns:
        if pattern.startswith("!"):
            excludes.append(pattern[1:])
        else:
            includes.append(pattern)
    return includes, excludes


def in_production_scope(rel: str, includes: list[str], excludes: list[str]) -> bool:
    return in_scope(rel, includes) and not in_scope(rel, excludes)


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], capture_output=True, text=True, check=True
    ).stdout


def changed_lines(base: str, head: str) -> dict[str, set[int]]:
    """Return new-file line numbers added or modified between base and head."""

    diff = git("diff", "--unified=0", "--no-color", "--no-ext-diff", base, head)
    result: dict[str, set[int]] = {}
    current: str | None = None
    for line in diff.splitlines():
        if line.startswith("+++ "):
            target = line[4:].strip()
            if target == "/dev/null":
                current = None
            else:
                current = target[2:] if target.startswith("b/") else target
                result.setdefault(current, set())
        elif line.startswith("@@") and current is not None:
            match = HUNK_RE.match(line)
            if not match:
                continue
            start = int(match.group(1))
            count = int(match.group(2) or "1")
            if count > 0:
                result[current].update(range(start, start + count))
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lcov", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--scope-file", required=True)
    parser.add_argument("--base", required=True, help="Merge-base commit to diff against")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--min", type=float, default=90.0)
    parser.add_argument("--out", required=True, help="Markdown report path")
    args = parser.parse_args()

    includes, excludes = split_scope(parse_scope(args.scope_file))
    files = parse_lcov(args.lcov, args.root)
    changed = changed_lines(args.base, args.head)

    total = 0
    covered = 0
    non_executable = 0
    absent_files: list[str] = []
    excluded_files: list[tuple[str, int]] = []
    rows: list[tuple[str, int, int]] = []
    for rel in sorted(changed):
        lines = changed[rel]
        if not lines:
            continue
        if not in_scope(rel, includes):
            continue
        if not in_production_scope(rel, includes, excludes):
            # Test-only paths cargo-llvm-cov omits from LCOV by default.
            excluded_files.append((rel, len(lines)))
            continue
        record = files.get(rel)
        if record is None:
            # A changed redaction-chain production file missing from the coverage
            # tracefile was never instrumented: count every changed line as
            # uncovered and fail, never pass vacuously.
            absent_files.append(rel)
            total += len(lines)
            rows.append((rel, 0, len(lines)))
            continue
        file_total = 0
        file_covered = 0
        for lineno in sorted(lines):
            if lineno not in record["lines"]:
                # The file is instrumented, so a line without a report entry is
                # non-executable (comment/blank) and does not count.
                non_executable += 1
                continue
            file_total += 1
            if record["lines"][lineno] > 0:
                file_covered += 1
        total += file_total
        covered += file_covered
        if file_total:
            rows.append((rel, file_covered, file_total))

    percentage = pct(covered, total)
    ok = not absent_files and (total == 0 or percentage >= args.min)

    out: list[str] = []
    out.append("### Redaction diff coverage gate")
    out.append("")
    out.append(f"- Base: `{args.base}` -> `{args.head}`")
    out.append(f"- Scope: `{args.scope_file}`")
    out.append(f"- Threshold: {args.min:.0f}%")
    if total == 0:
        out.append(
            "- Changed redaction-chain lines: 0 -> gate passes "
            f"({non_executable} changed non-executable line(s))"
        )
    else:
        out.append(
            f"- Changed redaction-chain lines: **{percentage:.1f}%** ({covered}/{total})"
        )
    if absent_files:
        out.append(
            "- Changed files missing from the coverage report (counted uncovered): "
            + ", ".join(f"`{rel}`" for rel in absent_files)
        )
    if excluded_files:
        total_excluded = sum(count for _, count in excluded_files)
        out.append(
            f"- Excluded test-only paths (cargo-llvm-cov default omission): "
            f"{total_excluded} changed line(s) in "
            + ", ".join(f"`{rel}`" for rel, _ in excluded_files)
        )
    out.append("")
    if rows:
        out.append("| File | Changed lines | Covered |")
        out.append("| --- | ---: | ---: |")
        for rel, file_covered, file_total in rows:
            out.append(f"| `{rel}` | {file_total} | {file_covered} |")
        out.append("")
    out.append("PASS" if ok else "FAIL")
    report = "\n".join(out) + "\n"

    print(report, end="")
    with open(args.out, "w", encoding="utf-8") as handle:
        handle.write(report)
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as handle:
            handle.write(report)

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
