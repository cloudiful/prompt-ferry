#!/usr/bin/env bash
# Run the retained backend unit suite and verify the result.
#
# The suite is unit-only: every test is pure or in-memory, so this runner reads
# no `.env`, exports no test settings, and connects to no external service. The
# summary check rejects failures, `#[ignore]`d tests, or a run that produced no
# results at all. Elapsed wall time is printed so a disproportionate slowdown is
# visible without a separate profiling step.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

LOG_DIR="target/ci"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/test.log"

started="$SECONDS"
set +e
SQLX_OFFLINE=true cargo test --workspace --lib --no-fail-fast 2>&1 | tee "$LOG"
status="${PIPESTATUS[0]}"
set -e
elapsed="$((SECONDS - started))"

python3 scripts/ci/check-test-summary.py --log "$LOG" --label "unit tests" || exit 1

printf 'unit tests finished in %ss\n' "$elapsed"

exit "$status"
