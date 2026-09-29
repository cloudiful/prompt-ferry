#!/usr/bin/env bash
# Run the workspace suite with PostgreSQL and Valkey enabled and verify the
# result: the service preflight is the no-silent-skip guarantee, and the summary
# check rejects any run with failures, ignored tests, or no results at all.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

scripts/ci/require-services.sh

LOG_DIR="target/ci"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/test.log"

set +e
SQLX_OFFLINE=true cargo test --workspace --no-fail-fast -- --test-threads=1 2>&1 | tee "$LOG"
status="${PIPESTATUS[0]}"
set -e

python3 scripts/ci/check-test-summary.py --log "$LOG" --label "workspace tests" || exit 1

exit "$status"
