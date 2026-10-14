#!/usr/bin/env bash
# Focused checks for the test-only dotenv entrypoint — the runner.
#
# The sandbox harness lives in scripts/ci/test-env-test-lib.sh, the assertion
# vocabulary in scripts/ci/test-env-test-assertions.sh, and the cases in
# scripts/ci/test-env-test-cases.sh plus
# scripts/ci/test-env-test-target-cases.sh for the dedicated test target. All
# are resolved next to this file, so the suite runs from any directory. Every
# case is sandboxed: no real PostgreSQL, Valkey, cargo, or psql is used, and no
# repository `.env` is read.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Fail closed. `set -e` is deliberately not used: the cases assert on child
# commands that are expected to exit non-zero. So a missing or broken helper is
# caught explicitly, and a listed case that never got defined is caught before
# any case runs, rather than being skipped into a green tally.
load_helper() {
  local helper="$1"
  if [ ! -r "$helper" ]; then
    printf 'test-env.test.sh: cannot read required helper: %s\n' "$helper" >&2
    exit 1
  fi
  if ! source "$helper"; then
    printf 'test-env.test.sh: failed to load required helper: %s\n' "$helper" >&2
    exit 1
  fi
}

load_helper "$SCRIPT_DIR/test-env-test-lib.sh"
load_helper "$SCRIPT_DIR/test-env-test-assertions.sh"
load_helper "$SCRIPT_DIR/test-env-test-cases.sh"
load_helper "$SCRIPT_DIR/test-env-test-target-cases.sh"

SCENARIOS=(
  test_negative_key_assertion_is_sensitive
  test_dotenv_test_keys_are_exported
  test_process_environment_wins
  test_explicit_blank_process_value_is_not_replaced
  test_only_test_keys_are_forwarded
  test_partition_opt_in_flag_is_exported
  test_partition_opt_in_process_wins
  test_partition_opt_in_blank_stays_unconfigured
  test_partition_opt_in_defaults_to_absent
  test_s3_test_keys_are_exported
  test_s3_keys_are_unset_when_missing
  test_quoted_value_with_spaces_survives
  test_command_substitution_stays_inert
  test_explicit_env_file_override
  test_missing_env_file_override_fails
  test_missing_default_env_file_runs_from_environment
  test_run_directories_are_fresh_but_the_root_is_reused
  test_run_directory_holds_sidecars_then_is_removed
  test_failing_command_keeps_its_exit_and_cleans_up
  test_sccache_env_wrapper_is_emptied_not_removed
  test_global_config_sccache_is_overridden_empty
  test_ordinary_compiler_wrapper_is_preserved
  test_relative_tmpdir_resolves_against_repo_root
  test_wrapper_needs_no_postgres_for_sqlite_only
  test_confirmed_target_is_required_before_any_connection
  test_only_the_confirmed_target_is_accepted
  test_another_database_is_refused
  test_the_production_service_is_refused
  test_malformed_urls_and_targets_are_refused
  test_a_refusal_never_echoes_the_url_or_password
  test_the_target_parser_is_a_pure_importable_module
  test_missing_partition_coverage_fails_before_cargo
  test_partition_opt_in_creates_only_the_missing_child
  test_partition_opt_in_flag_must_be_strict
  test_a_missing_parser_module_fails_closed
  test_preflight_rejects_unusable_postgres_databases
  test_the_connected_database_is_checked_against_the_confirmed_one
  test_preflight_requires_both_services
  test_full_runner_requires_the_s3_target
  test_partial_s3_target_fails_before_any_io
  test_mismatched_s3_confirmed_target_is_refused
  test_unsafe_http_s3_target_is_refused
  test_missing_s3_parser_module_fails_closed
  test_full_runner_succeeds_with_stub_services
  test_full_runner_propagates_cargo_failure
  test_preflight_keeps_credentials_out_of_argv_and_output
)

test_main() {
  local script
  for script in "$WITH_TEST_ENV_SH" "$TEST_ENV_SH" "$RUN_TESTS_SH" \
    "$REQUIRE_SERVICES_SH" "$REQUIRE_TEST_POSTGRES_PY" "$TEST_PG_TARGET_PY" \
    "$TEST_S3_TARGET_PY"; do
    if [ ! -f "$script" ]; then
      printf 'FAIL missing %s\n' "$script"
      exit 1
    fi
  done

  local scenario missing=()
  for scenario in "${SCENARIOS[@]}"; do
    if ! declare -F "$scenario" > /dev/null; then
      missing+=("$scenario")
    fi
  done
  if [ "${#missing[@]}" -gt 0 ]; then
    printf 'test-env.test.sh: listed test functions are not defined: %s\n' "${missing[*]}" >&2
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
