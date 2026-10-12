#!/usr/bin/env bash
# Cases for scripts/ci/test-env.test.sh.
#
# Sourced by the runner, which dispatches every case in its SCENARIOS list. Each
# case builds its own sandbox, so no state leaks between them. Nothing here
# reaches a real service: `psql` and `cargo` are recording stubs and the Valkey
# probe talks to a loopback listener the harness starts.
#
# The dedicated-target and fingerprint cases live in
# scripts/ci/test-env-test-target-cases.sh, next to the policy they cover.

# --- harness self-checks ------------------------------------------------------

# Guards the negative key assertions themselves: a check that cannot fail would
# silently accept a leaked key, so a deliberately present synthetic key must be
# reported as a failure. The canary exists only inside the probe's own capture.
test_negative_key_assertion_is_sensitive() {
  assert_same 'a present key is reported as present' \
    "$(absence_probe PROMPT_FERRY_TEST_CANARY 'PROMPT_FERRY_TEST_CANARY=synthetic')" '0,1'
  assert_same 'an absent key is reported as absent' \
    "$(absence_probe PROMPT_FERRY_TEST_CANARY '')" '1,0'
}

# --- dedicated key resolution -------------------------------------------------

test_dotenv_test_keys_are_exported() {
  fresh
  run_wrapper
  assert_status 'the wrapper exits zero' 0 "$STATUS"
  assert_secret_same 'the quota test URL comes from .env' \
    "$(child_env PROMPT_FERRY_QUOTA_TEST_DATABASE_URL)" "$SECRET_PG_URL" 'wrong value'
  assert_same 'the confirmed test target comes from .env' \
    "$(child_env PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET)" "$SECRET_PG_TARGET"
  assert_same 'the valkey URL comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_VALKEY_URL)" "$SECRET_VK_URL"
  assert_same 'TMPDIR defaults to the project test directory' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp"
  assert_exists 'the test directory is created' "$SANDBOX/target/test-tmp"
}

test_process_environment_wins() {
  fresh
  AMBIENT='PROMPT_FERRY_TEST_VALKEY_URL=redis://127.0.0.1:6399/0'
  run_wrapper
  assert_same 'the process value wins over .env' \
    "$(child_env PROMPT_FERRY_TEST_VALKEY_URL)" 'redis://127.0.0.1:6399/0'
}

test_explicit_blank_process_value_is_not_replaced() {
  fresh
  AMBIENT='PROMPT_FERRY_QUOTA_TEST_DATABASE_URL='
  run_wrapper
  assert_status 'the wrapper still succeeds' 0 "$STATUS"
  assert_env_absent 'an explicitly blank process value stays unconfigured' \
    PROMPT_FERRY_QUOTA_TEST_DATABASE_URL
  assert_absent 'the dotenv value is not silently taken' \
    "$(cat "$CAPTURE/child-env")" "$SECRET_PG_URL"
}

test_only_test_keys_are_forwarded() {
  fresh
  run_wrapper
  assert_env_absent 'the application database URL is not exported' DATABASE_URL
  assert_env_absent 'the application provider key is not exported' \
    PROMPT_FERRY_WORKER__UPSTREAM_API_KEY
  assert_absent 'the application host never reaches the output' \
    "$(cat "$CAPTURE/child-env")" '10.20.30.40'
  assert_absent 'the production test database never reaches the output' \
    "$(cat "$CAPTURE/child-env")" 'prompt_ferry'
}

test_quoted_value_with_spaces_survives() {
  fresh
  mkdir -p "$SANDBOX/dir with space"
  write_env <<EOF
PROMPT_FERRY_TEST_TMPDIR="$SANDBOX/dir with space"
EOF
  run_wrapper
  assert_same 'a quoted value keeps its spaces' "$(child_env TMPDIR)" "$SANDBOX/dir with space"
  assert_same 'the test directory setting matches' \
    "$(child_env PROMPT_FERRY_TEST_TMPDIR)" "$SANDBOX/dir with space"
}

test_command_substitution_stays_inert() {
  fresh
  write_env <<'EOF'
PROMPT_FERRY_TEST_TMPDIR=$(touch canary)
PROMPT_FERRY_TEST_VALKEY_URL=`touch canary`
EOF
  run_wrapper
  assert_status 'a hostile value does not fail the wrapper' 0 "$STATUS"
  assert_missing 'a dotenv value cannot run a command' "$SANDBOX/canary"
  assert_same 'the substitution stays literal' \
    "$(child_env PROMPT_FERRY_TEST_TMPDIR)" "$SANDBOX/\$(touch canary)"
  assert_same 'backticks stay literal too' \
    "$(child_env PROMPT_FERRY_TEST_VALKEY_URL)" '`touch canary`'
}

test_explicit_env_file_override() {
  fresh
  printf 'PROMPT_FERRY_TEST_TMPDIR=%s/root-env-tmp\n' "$SANDBOX" >> "$SANDBOX/.env"
  printf 'PROMPT_FERRY_TEST_TMPDIR=%s/ci-env-tmp\n' "$SANDBOX" > "$SANDBOX/ci.env"
  AMBIENT='PROMPT_FERRY_TEST_ENV_FILE=ci.env'
  run_wrapper
  assert_same 'the override file supplies the setting' \
    "$(child_env TMPDIR)" "$SANDBOX/ci-env-tmp"
  assert_missing 'the root .env is not consulted as well' "$SANDBOX/root-env-tmp"
}

test_missing_env_file_override_fails() {
  fresh
  AMBIENT='PROMPT_FERRY_TEST_ENV_FILE=absent.env'
  run_wrapper
  assert_nonzero 'a missing override file fails' "$STATUS"
  assert_has 'the failure names the override' "$OUT" 'PROMPT_FERRY_TEST_ENV_FILE'
  assert_no_child_output 'the command never starts'
}

test_missing_default_env_file_runs_from_environment() {
  fresh
  clear_env
  AMBIENT="PROMPT_FERRY_TEST_VALKEY_URL=$SECRET_VK_URL"
  run_wrapper
  assert_status 'an absent .env is allowed for CI' 0 "$STATUS"
  assert_same 'the process environment alone is enough' \
    "$(child_env PROMPT_FERRY_TEST_VALKEY_URL)" "$SECRET_VK_URL"
  assert_same 'the default test directory still applies' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp"
}

# --- shared SQLite directory --------------------------------------------------

test_tmpdir_default_is_reused() {
  fresh
  run_wrapper
  local first
  first="$(child_env TMPDIR)"
  touch "$first/keep-me.sqlite"
  run_wrapper
  assert_same 'a repeated run resolves the same directory' "$(child_env TMPDIR)" "$first"
  assert_exists 'an existing fixture is never purged' "$first/keep-me.sqlite"
}

test_relative_tmpdir_resolves_against_repo_root() {
  fresh
  printf 'PROMPT_FERRY_TEST_TMPDIR=target/rel-tmp\n' > "$SANDBOX/.env"
  RUN_CWD="$SANDBOX/capture"
  run_wrapper
  assert_same 'a relative setting resolves against the repository root' \
    "$(child_env TMPDIR)" "$SANDBOX/target/rel-tmp"
  assert_missing 'it does not resolve against the invocation directory' \
    "$SANDBOX/capture/target/rel-tmp"
}

test_wrapper_needs_no_postgres_for_sqlite_only() {
  fresh
  write_env <<EOF
PROMPT_FERRY_TEST_VALKEY_URL=$SECRET_VK_URL
EOF
  run_wrapper
  assert_status 'a focused SQLite-only run needs no database service' 0 "$STATUS"
  assert_env_absent 'no PostgreSQL setting is invented' \
    PROMPT_FERRY_QUOTA_TEST_DATABASE_URL
  assert_env_absent 'no confirmed target is invented' \
    PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET
  assert_same 'the shared directory is still exported' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp"
}

# --- full-runner preflight ----------------------------------------------------

# Writes a .env holding only the Valkey key, so both dedicated PostgreSQL
# settings under test arrive through the process environment, where a blank
# stays meaningful.
only_valkey_env() {
  start_fake_valkey
  write_env <<EOF
PROMPT_FERRY_TEST_VALKEY_URL=$(valkey_url)
EOF
}

# Runs the full runner against an explicit dedicated URL and confirmed target.
# An empty value is passed through blank rather than dropped, so an absent
# setting stays absent instead of inheriting the standard fixture.
run_preflight() {
  local url="$1" target="$2"
  fresh
  only_valkey_env
  AMBIENT="PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=$url PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=$target"
  run_tests
}

test_preflight_requires_both_services() {
  fresh
  printf 'PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=%s\n' "$SECRET_PG_URL" > "$SANDBOX/.env"
  printf 'PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=%s\n' "$SECRET_PG_TARGET" >> "$SANDBOX/.env"
  run_tests
  assert_nonzero 'a missing valkey service fails' "$STATUS"
  assert_has 'the failure names the valkey setting' "$OUT" 'PROMPT_FERRY_TEST_VALKEY_URL'
  assert_not_run 'nothing runs without valkey' 'cargo test'

  fresh
  start_fake_valkey
  printf 'PROMPT_FERRY_TEST_VALKEY_URL=%s\n' "$(valkey_url)" > "$SANDBOX/.env"
  run_tests
  assert_nonzero 'a missing quota database fails' "$STATUS"
  assert_has 'the failure names the quota database setting' "$OUT" 'PROMPT_FERRY_QUOTA_TEST_DATABASE_URL'
  assert_not_run 'nothing runs without the quota database' 'cargo test'
}

test_full_runner_succeeds_with_stub_services() {
  fresh
  service_env
  run_tests
  assert_status 'the full runner succeeds' 0 "$STATUS"
  assert_has 'the confirmed quota database is reported' "$OUT" 'quota test schema verified'
  assert_has 'valkey is reported' "$OUT" 'service ready: valkey'
  assert_has 'the summary check runs' "$OUT" 'workspace tests: 1 suites, 3 passed'
  assert_ran_once 'cargo runs exactly once' 'cargo test'
  assert_same 'the offline sqlx mode is preserved' \
    "$(sed -n 's/^SQLX_OFFLINE=//p' "$CAPTURE/cargo-env")" 'true'
  assert_same 'cargo inherits the shared test directory' \
    "$(sed -n 's/^TMPDIR=//p' "$CAPTURE/cargo-env")" "$SANDBOX/target/test-tmp"
  assert_same 'the dotenv is loaded exactly once through the wrapper' \
    "$(sed -n 's/^PROMPT_FERRY_TEST_ENV_LOADED=//p' "$CAPTURE/cargo-env")" '1'
}

test_full_runner_propagates_cargo_failure() {
  fresh
  service_env
  set_cargo_status 101
  run_tests
  assert_status 'the cargo exit status survives' 101 "$STATUS"
  assert_ran 'the run still executed' 'cargo test'
}

test_preflight_keeps_credentials_out_of_argv_and_output() {
  fresh
  service_env
  run_tests
  assert_status 'the preflight succeeds' 0 "$STATUS"
  local argv
  argv="$(psql_argv)"
  assert_absent 'the URL is never a client argument' "$argv" "$SECRET_PG_URL"
  assert_absent 'the password is never a client argument' "$argv" "$SECRET_PG_PASSWORD"
  assert_absent 'the password is never printed' "$OUT" "$SECRET_PG_PASSWORD"
  assert_same 'the client takes the password from the environment' \
    "$(sed -n 's/^PGPASSWORD=//p' "$CAPTURE/psql-env")" "$SECRET_PG_PASSWORD"
  assert_same 'pgpass lookup is disabled' \
    "$(sed -n 's/^PGPASSFILE=//p' "$CAPTURE/psql-env")" '/dev/null'
  assert_absent 'no application setting reaches the client' \
    "$(cat "$CAPTURE/psql-env")" 'DATABASE_URL'

  set_psql_mode leak_error
  run_tests
  assert_absent 'a client message echoing the password is redacted' \
    "$OUT" "$SECRET_PG_PASSWORD"
  assert_has 'the redaction is visible' "$OUT" '[redacted]'
}

# Both dedicated services, addressed at the stub client and the loopback
# listener, so the preflights pass and the run reaches cargo.
service_env() {
  start_fake_valkey
  write_env <<EOF
PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=$SECRET_PG_URL
PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=$SECRET_PG_TARGET
PROMPT_FERRY_TEST_VALKEY_URL=$(valkey_url)
EOF
}
