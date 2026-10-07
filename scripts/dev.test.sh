#!/usr/bin/env bash
# Focused checks for scripts/dev.sh — the runner.
#
# The sandbox harness and assertion vocabulary live in scripts/dev-test-lib.sh;
# the cases live in scripts/dev-test-cases.sh. Both are resolved next to this
# file, so the suite runs from any directory. The repository .env is never read
# and the database URL is a synthetic fixture that is never printed.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Fail closed. `set -e` is deliberately not used: the cases assert on child
# commands that are expected to exit non-zero. So a missing or broken helper is
# caught explicitly, and a listed case that never got defined is caught before
# any case runs, rather than being skipped into a green tally.
load_helper() {
  local helper="$1"
  if [ ! -r "$helper" ]; then
    printf 'dev.test.sh: cannot read required helper: %s\n' "$helper" >&2
    exit 1
  fi
  if ! source "$helper"; then
    printf 'dev.test.sh: failed to load required helper: %s\n' "$helper" >&2
    exit 1
  fi
}

load_helper "$SCRIPT_DIR/dev-test-lib.sh"
load_helper "$SCRIPT_DIR/dev-test-cases.sh"

SCENARIOS=(
  test_help
  test_default_command_is_help
  test_invalid_modes
  test_missing_supervisor
  test_missing_env_helper
  test_prerequisites
  test_database_url_required
  test_database_url_forwarded_but_hidden
  test_dotenv_parsing
  test_extra_dotenv_keys_forwarded
  test_defaults
  test_precedence
  test_backend_delegation
  test_full_delegation
  test_external_relay_summary
  test_help_without_tools
  test_runner_fails_closed
)

test_main() {
  if [ ! -f "$DEV_SH" ]; then
    printf 'FAIL missing %s\n' "$DEV_SH"
    exit 1
  fi

  local scenario missing=()
  for scenario in "${SCENARIOS[@]}"; do
    if ! declare -F "$scenario" > /dev/null; then
      missing+=("$scenario")
    fi
  done
  if [ "${#missing[@]}" -gt 0 ]; then
    printf 'dev.test.sh: listed test functions are not defined: %s\n' "${missing[*]}" >&2
    exit 1
  fi

  for scenario in "${SCENARIOS[@]}"; do
    "$scenario"
  done

  printf '\n%s passed, %s failed\n' "$PASS" "$FAIL"
  if [ "$FAIL" -gt 0 ]; then
    exit 1
  fi
}

test_main "$@"
