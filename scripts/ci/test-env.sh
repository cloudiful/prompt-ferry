#!/usr/bin/env bash
# Dedicated test environment for the test entrypoints, sourced by
# scripts/ci/with-test-env.sh and scripts/ci/run-tests.sh.
#
# The project `.env` is read as data through the existing parser in
# scripts/dev-env.sh and is never executed. Only the dedicated test keys
# leave this helper, so DATABASE_URL, provider keys, and every other application
# setting stay out of the test process environment.
#
# Precedence: a value already present in the process environment wins over the
# dotenv value, and an explicitly blank process value is authoritative, so a
# caller can mask a `.env` entry instead of silently inheriting it. A blank
# dotenv value is not a setting and falls back to the documented default.
#
# The SQLite temporary directory never comes from the ambient TMPDIR: it comes
# from PROMPT_FERRY_TEST_TMPDIR or the <repo>/target/test-tmp default, it is
# created when missing, and it is never purged, because a concurrent run may
# still own files and WAL sidecars inside it.

if [ -z "${BASH_VERSION:-}" ] || [ "${BASH_VERSINFO[0]:-0}" -lt 4 ]; then
  printf 'test-env.sh must run under bash 4 or newer (associative arrays)\n' >&2
  exit 1
fi

TEST_ENV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_ENV_ROOT="$(cd "$TEST_ENV_DIR/../.." && pwd)"

# The only keys this helper is allowed to export. The confirmed target travels
# with the quota URL because neither is usable alone: the preflight and the Rust
# test helper refuse any database the two do not agree on.
TEST_ENV_DB_KEYS=(
  PROMPT_FERRY_QUOTA_TEST_DATABASE_URL
  PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET
  PROMPT_FERRY_TEST_VALKEY_URL
)
TEST_ENV_TMPDIR_KEY=PROMPT_FERRY_TEST_TMPDIR

test_env_die() {
  printf 'test-env: %s\n' "$1" >&2
  exit 1
}

# The repository parser, never the .env file itself: it only defines functions
# and the associative array, so exporting stays an explicit per-key decision.
# It is sourced at file scope because its `declare -A DOTENV` would otherwise be
# local to a wrapper function and invisible to the loader.
TEST_ENV_PARSER="$TEST_ENV_ROOT/scripts/dev-env.sh"
[ -f "$TEST_ENV_PARSER" ] || {
  printf 'test-env.sh: missing dotenv parser: %s\n' "$TEST_ENV_PARSER" >&2
  exit 1
}
# shellcheck source=../dev-env.sh
source "$TEST_ENV_PARSER"
unset TEST_ENV_PARSER

# Resolves the dotenv file into TEST_ENV_DOTENV_FILE. An explicit
# PROMPT_FERRY_TEST_ENV_FILE (process environment only, never read from the
# file it selects) must exist; the default <repo>/.env may be absent so CI can
# run from the process environment alone.
test_env_resolve_dotenv_file() {
  local override="${PROMPT_FERRY_TEST_ENV_FILE:-}"
  TEST_ENV_DOTENV_FILE="$TEST_ENV_ROOT/.env"

  [ -z "$override" ] && return 0

  case "$override" in
    /*) TEST_ENV_DOTENV_FILE="$override" ;;
    *) TEST_ENV_DOTENV_FILE="$TEST_ENV_ROOT/$override" ;;
  esac

  [ -f "$TEST_ENV_DOTENV_FILE" ] ||
    test_env_die "PROMPT_FERRY_TEST_ENV_FILE does not name a readable file: $TEST_ENV_DOTENV_FILE"
}

# Prints the effective value of one dedicated key: the process environment
# first, including an explicit blank, then a non-blank dotenv entry, then
# nothing, which lets the caller apply its own default.
test_env_value() {
  local key="$1" from_file

  if [ "${!key+x}" = "x" ]; then
    printf '%s' "${!key}"
    return 0
  fi

  from_file="${DOTENV[$key]-}"
  if [ -n "$from_file" ] && ! is_blank "$from_file"; then
    printf '%s' "$from_file"
  fi
}

# Exports one dedicated key, or unsets it when it resolves to nothing so an
# unconfigured service stays visibly unconfigured for the preflight and for the
# Rust early-return skip branch.
test_env_export() {
  local key="$1" value
  value="$(test_env_value "$key")"
  if [ -n "$value" ] && ! is_blank "$value"; then
    export "$key=$value"
  else
    unset "$key"
  fi
}

# Exports the absolute, existing SQLite temporary directory. Relative settings
# resolve against the repository root, never the invocation directory.
test_env_export_tmpdir() {
  local value dir
  value="$(test_env_value "$TEST_ENV_TMPDIR_KEY")"

  if [ -z "$value" ] || is_blank "$value"; then
    dir="$TEST_ENV_ROOT/target/test-tmp"
  else
    case "$value" in
      /*) dir="$value" ;;
      *) dir="$TEST_ENV_ROOT/$value" ;;
    esac
  fi

  mkdir -p "$dir" 2> /dev/null || test_env_die "cannot create the test temporary directory: $dir"
  [ -d "$dir" ] || test_env_die "test temporary directory is not a directory: $dir"
  [ -w "$dir" ] || test_env_die "test temporary directory is not writable: $dir"

  export "$TEST_ENV_TMPDIR_KEY=$dir"
  export TMPDIR="$dir"
}

# Loads the dedicated test settings once. PROMPT_FERRY_TEST_ENV_LOADED is the
# self-wrapping guard: a runner that re-enters itself through
# scripts/ci/with-test-env.sh neither reloads the dotenv file nor recurses.
test_env_load() {
  if [ "${PROMPT_FERRY_TEST_ENV_LOADED:-}" = "1" ]; then
    return 0
  fi

  test_env_resolve_dotenv_file
  load_dotenv "$TEST_ENV_DOTENV_FILE"

  local key
  for key in "${TEST_ENV_DB_KEYS[@]}"; do
    test_env_export "$key"
  done
  test_env_export_tmpdir

  export PROMPT_FERRY_TEST_ENV_LOADED=1
}
