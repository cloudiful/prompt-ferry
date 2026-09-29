#!/usr/bin/env python3
"""Compare redaction criterion benchmarks against the committed baseline.

`#569` Phase 3 keeps a measured Criterion baseline in the repository
(`benches/redaction_bench_baseline.json`) and gates only the bounded `quick`
subset. The full 10k/100k/500k x rules/secrets matrix is recorded but never
gated, so mutation testing and full-load runs stay manual.

Usage:
    # after `cargo bench --bench redaction_pipeline --bench redaction_restore`
    scripts/ci/bench_compare.py --save

    # after `cargo bench ... -- quick`
    scripts/ci/bench_compare.py --check

Exit status: `--check` is non-zero when a gated benchmark regressed past its
factor or produced no measurement at all.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

DEFAULT_BASELINE = Path("benches/redaction_bench_baseline.json")
DEFAULT_CRITERION_DIR = Path("target/criterion")
DEFAULT_GATE_SEGMENT = "quick"
DEFAULT_MAX_FACTOR = 2.5

QUICK_NOTE = (
    "Only the 'quick' subset is gated with `cargo bench ... -- quick`; the full "
    "matrix is recorded for the growth curve but is never a hard gate."
)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        choices=("save", "check"),
        default="check",
        help="save rewrites the baseline; check compares against it (default).",
    )
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--criterion-dir", type=Path, default=DEFAULT_CRITERION_DIR)
    parser.add_argument(
        "--max-factor",
        type=float,
        default=DEFAULT_MAX_FACTOR,
        help=f"Default regression factor for gated entries (default: {DEFAULT_MAX_FACTOR}).",
    )
    return parser.parse_args(argv)


def read_estimates(criterion_dir: Path) -> dict[str, float]:
    """Map each `<group>/<function>/<value>` benchmark to its median estimate (ns)."""
    estimates: dict[str, float] = {}
    if not criterion_dir.is_dir():
        return estimates
    for estimates_file in criterion_dir.glob("**/new/estimates.json"):
        # <criterion_dir>/<group>/<function>/<value>/new/estimates.json
        benchmark_id = estimates_file.parent.parent.relative_to(criterion_dir).as_posix()
        data = json.loads(estimates_file.read_text())
        median = data.get("median", {}).get("point_estimate")
        if median is not None:
            estimates[benchmark_id] = float(median)
    return estimates


def is_gated(benchmark_id: str) -> bool:
    """The gate covers the `quick` tier only. Criterion sanitizes the tier into
    the function segment (`quick_ctx_10k`), so match both forms."""
    segments = benchmark_id.split("/")
    if len(segments) < 2:
        return False
    segment = segments[1]
    return segment == DEFAULT_GATE_SEGMENT or segment.startswith(f"{DEFAULT_GATE_SEGMENT}_")


def ms(ns: float | None) -> str:
    return "n/a" if ns is None else f"{ns / 1e6:.3f} ms"


def write_summary(lines: list[str]) -> None:
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not summary_path:
        return
    with open(summary_path, "a", encoding="utf-8") as handle:
        handle.write("\n".join(lines) + "\n")


def save(args: argparse.Namespace) -> int:
    estimates = read_estimates(args.criterion_dir)
    if not estimates:
        print(f"error: no criterion estimates under {args.criterion_dir}", file=sys.stderr)
        return 1
    benchmarks = {
        benchmark_id: {
            "median_ns": round(median, 1),
            "gate": is_gated(benchmark_id),
            "max_regression_factor": args.max_factor,
        }
        for benchmark_id, median in sorted(estimates.items())
    }
    document = {
        "version": 1,
        "generated_by": "scripts/ci/bench_compare.py --save",
        "notes": QUICK_NOTE,
        "benchmarks": benchmarks,
    }
    args.baseline.parent.mkdir(parents=True, exist_ok=True)
    args.baseline.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
    gated = sum(1 for entry in benchmarks.values() if entry["gate"])
    print(f"saved {len(benchmarks)} benchmarks ({gated} gated) to {args.baseline}")
    write_summary(
        [
            "### Redaction benchmark baseline",
            "",
            f"- saved: {len(benchmarks)} benchmarks ({gated} gated)",
            f"- file: `{args.baseline}`",
        ]
    )
    return 0


def check(args: argparse.Namespace) -> int:
    if not args.baseline.is_file():
        print(f"error: baseline not found at {args.baseline}", file=sys.stderr)
        return 1
    baseline = json.loads(args.baseline.read_text())["benchmarks"]
    estimates = read_estimates(args.criterion_dir)
    failures: list[str] = []
    rows: list[str] = []
    for benchmark_id, entry in sorted(baseline.items()):
        measured = estimates.get(benchmark_id)
        base = float(entry.get("median_ns") or 0.0)
        gated = bool(entry.get("gate"))
        if not gated:
            if measured is not None:
                ratio = measured / base if base else float("inf")
                rows.append(
                    f"| {benchmark_id} | {ms(base)} | {ms(measured)} | ungated | {ratio:.2f}x |"
                )
            continue
        if measured is None:
            failures.append(f"{benchmark_id}: gated benchmark produced no measurement")
            rows.append(f"| {benchmark_id} | {ms(base)} | n/a | **MISSING** | n/a |")
            continue
        limit = float(entry.get("max_regression_factor") or args.max_factor)
        factor = measured / base if base else float("inf")
        status = "PASS" if factor <= limit else "FAIL"
        if status == "FAIL":
            failures.append(
                f"{benchmark_id}: {ms(measured)} vs baseline {ms(base)} "
                f"= {factor:.2f}x > {limit:.2f}x"
            )
        rows.append(f"| {benchmark_id} | {ms(base)} | {ms(measured)} | {status} | {factor:.2f}x |")

    print("| benchmark | baseline | measured | status | factor |")
    print("| --- | --- | --- | --- | --- |")
    print("\n".join(rows))
    gated = sum(1 for entry in baseline.values() if entry.get("gate"))
    print(f"\ngated {gated} benchmark(s); {len(failures)} failure(s)")
    write_summary(
        [
            "### Redaction benchmark comparison",
            "",
            f"- gated: {gated}; measured: {len(estimates)}; failures: {len(failures)}",
            "",
            "| benchmark | baseline | measured | status | factor |",
            "| --- | --- | --- | --- | --- |",
            *rows,
        ]
    )
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    return save(args) if args.mode == "save" else check(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
