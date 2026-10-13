#!/usr/bin/env bash
# Run one command with the dedicated test settings from the project `.env`.
#
# Usage: bash scripts/ci/with-test-env.sh <command> [args...]
#
# This is the documented entrypoint for focused test commands: it loads only
# PROMPT_FERRY_QUOTA_TEST_DATABASE_URL, PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET,
# PROMPT_FERRY_TEST_VALKEY_URL, the eight PROMPT_FERRY_TEST_S3_* settings, and
# PROMPT_FERRY_TEST_TMPDIR (plus an absolute TMPDIR for the SQLite fixtures)
# and then execs the command with the original
# arguments, exit status, and signals. The command is never evaluated, so a test
# invocation cannot be reinterpreted as shell input, and no application setting
# or secret from the `.env` reaches the child.
#
# The final exec goes through scripts/ci/test_run_tmp.py, which owns one fresh
# run directory under the exported test root for this invocation, points
# TMPDIR/PROMPT_FERRY_TEST_TMPDIR at it, forwards signals to the owned process
# group, preserves the exit status, and removes exactly that directory
# afterwards — so both full and focused runs are covered exactly once.
#
# The confirmed target travels with the quota URL because neither is usable
# alone: the preflight and the Rust test helper refuse any database the two do
# not agree on.
#
# A direct `cargo test` does not load `.env` and so sees only variables already
# exported in the environment; without them the service-backed tests return early
# and report themselves as skipped, and one SQL regression helper fails outright.
# Use this wrapper (or scripts/ci/run-tests.sh for the full integration run) so
# the settings are present. SQLite-only commands work through this wrapper
# without any database service, and must not claim PostgreSQL coverage.

if [ -z "${BASH_VERSION:-}" ]; then
  printf 'with-test-env.sh must run under bash\n' >&2
  exit 1
fi

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

usage() {
  printf 'Usage: bash scripts/ci/with-test-env.sh <command> [args...]\n' >&2
}

if [ "$#" -lt 1 ]; then
  usage
  exit 2
fi

HELPER="$SCRIPT_DIR/test-env.sh"
[ -f "$HELPER" ] || {
  printf 'with-test-env.sh: missing required helper: %s\n' "$HELPER" >&2
  exit 1
}

# shellcheck source=test-env.sh
source "$HELPER"
cd "$ROOT"
test_env_load

exec python3 "$SCRIPT_DIR/test_run_tmp.py" -- "$@"
