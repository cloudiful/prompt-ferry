#!/usr/bin/env python3
"""Validate the cargo test summary: no failures, no ignored tests, real results.

libtest captures a passing test's stderr, and a service-backed skip branch
returns Ok without panicking, so a green log alone cannot prove per-test
execution. The no-silent-skip guarantee therefore rests on
`require-services.sh` (the suite's only skip condition is those two env vars
being absent); this check is the complementary summary guard and fails when the
run reported failures, ignored tests, or no test result at all. It never needs
`--nocapture` and never touches credentials.
"""

from __future__ import annotations

import argparse
import re
import sys

RESULT_RE = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True)
    parser.add_argument("--label", default="tests")
    args = parser.parse_args()

    suites = 0
    passed = 0
    failed = 0
    ignored = 0
    with open(args.log, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            match = RESULT_RE.match(line)
            if not match:
                continue
            suites += 1
            passed += int(match.group(2))
            failed += int(match.group(3))
            ignored += int(match.group(4))

    print(
        f"{args.label}: {suites} suites, {passed} passed, "
        f"{failed} failed, {ignored} ignored"
    )

    if suites == 0:
        print(f"error: no test result summary found in {args.label} log", file=sys.stderr)
        return 1
    if failed:
        print(f"error: {failed} test(s) failed in {args.label}", file=sys.stderr)
        return 1
    if ignored:
        print(
            f"error: {ignored} test(s) ignored in {args.label}; ignored tests are not allowed",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
