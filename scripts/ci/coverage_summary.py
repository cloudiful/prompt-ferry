#!/usr/bin/env python3
"""Render workspace + redaction-chain coverage visibility from an lcov tracefile.

Whole-repo coverage is informational only (issue #569 keeps repo-wide numbers as
visibility, not a gate); the redaction chain gets its own baseline report and a
list of zero-hit lines that feeds Phase 1. Nothing here fails the build.
"""

from __future__ import annotations

import argparse
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lcov_lib import file_line_totals, in_scope, parse_lcov, parse_scope, pct  # noqa: E402


def build_report(lcov_path: str, root: str, scope_file: str) -> dict:
    patterns = parse_scope(scope_file)
    files = parse_lcov(lcov_path, root)

    workspace_total = 0
    workspace_covered = 0
    workspace_fnf = 0
    workspace_fnh = 0
    redaction_total = 0
    redaction_covered = 0
    redaction_fnf = 0
    redaction_fnh = 0
    redaction_files: list[tuple[str, int, int, int, int]] = []
    blind_spots: list[str] = []
    workspace_uncovered: list[tuple[int, str, int, int]] = []

    for rel, record in files.items():
        total, covered = file_line_totals(record)
        workspace_total += total
        workspace_covered += covered
        workspace_fnf += record["fnf"]
        workspace_fnh += record["fnh"]
        if total and covered < total:
            workspace_uncovered.append((total - covered, rel, covered, total))
        if not in_scope(rel, patterns):
            continue
        redaction_total += total
        redaction_covered += covered
        redaction_fnf += record["fnf"]
        redaction_fnh += record["fnh"]
        redaction_files.append((rel, covered, total, record["fnh"], record["fnf"]))
        for lineno, hits in sorted(record["lines"].items()):
            if hits == 0:
                blind_spots.append(f"{rel}:{lineno}")

    redaction_files.sort(key=lambda row: (pct(row[1], row[2]), row[0]))
    workspace_uncovered.sort(reverse=True)

    return {
        "workspace_total": workspace_total,
        "workspace_covered": workspace_covered,
        "workspace_fnf": workspace_fnf,
        "workspace_fnh": workspace_fnh,
        "redaction_total": redaction_total,
        "redaction_covered": redaction_covered,
        "redaction_fnf": redaction_fnf,
        "redaction_fnh": redaction_fnh,
        "redaction_files": redaction_files,
        "blind_spots": blind_spots,
        "workspace_uncovered": workspace_uncovered,
        "file_count": len(files),
    }


def render_markdown(report: dict) -> str:
    out: list[str] = []
    wt = report["workspace_total"]
    wc = report["workspace_covered"]
    rt = report["redaction_total"]
    rc = report["redaction_covered"]
    out.append("## Coverage (#569 Phase 0)")
    out.append("")
    out.append(
        f"- Workspace (visibility only): **{pct(wc, wt):.2f}%** "
        f"({wc}/{wt} lines, {report['file_count']} files; "
        f"functions {pct(report['workspace_fnh'], report['workspace_fnf']):.1f}%)"
    )
    if rt:
        out.append(
            f"- Redaction chain baseline: **{pct(rc, rt):.2f}%** ({rc}/{rt} lines; "
            f"functions {pct(report['redaction_fnh'], report['redaction_fnf']):.1f}%)"
        )
    else:
        out.append("- Redaction chain baseline: no instrumented lines in scope")
    out.append(f"- Redaction zero-hit lines: {len(report['blind_spots'])}")
    out.append("")
    out.append("| Redaction file | Lines | Functions |")
    out.append("| --- | ---: | ---: |")
    for rel, covered, total, fnh, fnf in report["redaction_files"]:
        out.append(
            f"| `{rel}` | {pct(covered, total):.1f}% ({covered}/{total}) "
            f"| {pct(fnh, fnf):.1f}% ({fnh}/{fnf}) |"
        )
    out.append("")
    out.append("### Top uncovered workspace files")
    out.append("")
    out.append("| File | Uncovered | Coverage |")
    out.append("| --- | ---: | ---: |")
    for uncovered, rel, covered, total in report["workspace_uncovered"][:15]:
        out.append(f"| `{rel}` | {uncovered} | {pct(covered, total):.1f}% |")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lcov", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--scope-file", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    report = build_report(args.lcov, args.root, args.scope_file)
    os.makedirs(args.out_dir, exist_ok=True)

    markdown = render_markdown(report)
    print(markdown, end="")

    with open(os.path.join(args.out_dir, "coverage-summary.md"), "w", encoding="utf-8") as handle:
        handle.write(markdown)

    with open(
        os.path.join(args.out_dir, "redaction-baseline.txt"), "w", encoding="utf-8"
    ) as handle:
        handle.write("Redaction-chain line coverage baseline (#569 Phase 0)\n")
        handle.write(
            f"lines: {report['redaction_covered']}/{report['redaction_total']}  "
            f"functions: {report['redaction_fnh']}/{report['redaction_fnf']}\n\n"
        )
        for rel, covered, total, fnh, fnf in report["redaction_files"]:
            handle.write(
                f"lines {pct(covered, total):6.1f}% ({covered:5}/{total:<5})  "
                f"fn {pct(fnh, fnf):6.1f}% ({fnh:4}/{fnf:<4})  {rel}\n"
            )

    with open(os.path.join(args.out_dir, "blind-spots.txt"), "w", encoding="utf-8") as handle:
        handle.write(
            "Redaction-chain zero-hit lines (Phase 1 input)\n"
            f"count: {len(report['blind_spots'])}\n\n"
        )
        handle.write("\n".join(report["blind_spots"]) + "\n")

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as handle:
            handle.write(markdown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
