#!/usr/bin/env bash
# Run the workspace suite with the dedicated test services enabled and verify
# the result: the service preflights are the no-silent-skip guarantee, and the
# summary check rejects any run with failures, ignored tests, or no results at
# all.
#
# The run loads only the dedicated test settings from the project `.env` through
# scripts/ci/with-test-env.sh — the quota PostgreSQL URL, the Valkey URL, and the
# absolute SQLite temporary directory — and never reads the application
# DATABASE_URL. It therefore needs both services reachable and the quota test
# database migrated; provisioning either one is an operator step, and the
# preflights run before cargo so an absent service fails the run instead of
# leaving the service-backed tests skipped.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# Self-wrapping guard: run-tests.sh re-enters itself through the wrapper exactly
# once. The wrapper exports PROMPT_FERRY_TEST_ENV_LOADED, so the second pass
# never reloads the dotenv file and never recurses.
if [ "${PROMPT_FERRY_TEST_ENV_LOADED:-}" != "1" ]; then
  exec bash "$ROOT/scripts/ci/with-test-env.sh" bash "$ROOT/scripts/ci/run-tests.sh" "$@"
fi

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
