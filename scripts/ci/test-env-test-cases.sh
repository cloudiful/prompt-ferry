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
  assert_has 'TMPDIR is a fresh run directory under the test root' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp/run-"
  assert_same 'the fixture directory setting names the same run directory' \
    "$(child_env PROMPT_FERRY_TEST_TMPDIR)" "$(child_env TMPDIR)"
  assert_exists 'the test root is created' "$SANDBOX/target/test-tmp"
  assert_missing 'the run directory is removed afterwards' "$(child_env TMPDIR)"
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

# --- partition backfill opt-in plumbing ---------------------------------------
# The opt-in travels with the other dedicated test keys: a dotenv entry is
# exported, the process environment wins, and a blank stays unconfigured so
# the preflight default stays read-only.

test_partition_opt_in_flag_is_exported() {
  fresh
  printf 'PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS=true\n' >> "$SANDBOX/.env"
  run_wrapper
  assert_status 'the wrapper exits zero' 0 "$STATUS"
  assert_same 'the partition opt-in comes from .env' \
    "$(child_env PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS)" 'true'
}

test_partition_opt_in_process_wins() {
  fresh
  printf 'PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS=true\n' >> "$SANDBOX/.env"
  AMBIENT='PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS=false'
  run_wrapper
  assert_same 'the process value wins for the opt-in' \
    "$(child_env PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS)" 'false'

  fresh
  AMBIENT='PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS=true'
  run_wrapper
  assert_same 'an explicit opt-in travels without a dotenv entry' \
    "$(child_env PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS)" 'true'
}

test_partition_opt_in_blank_stays_unconfigured() {
  fresh
  printf 'PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS=true\n' >> "$SANDBOX/.env"
  AMBIENT='PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS='
  run_wrapper
  assert_status 'the wrapper still succeeds' 0 "$STATUS"
  assert_env_absent 'an explicitly blank opt-in stays unconfigured' \
    PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS
  assert_absent 'the dotenv opt-in is not silently taken' \
    "$(cat "$CAPTURE/child-env")" 'AUTO_CREATE_PARTITIONS=true'
}

test_partition_opt_in_defaults_to_absent() {
  fresh
  run_wrapper
  assert_env_absent 'the opt-in is absent unless configured' \
    PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS
}

test_s3_test_keys_are_exported() {
  fresh
  run_wrapper
  assert_status 'the wrapper exits zero' 0 "$STATUS"
  assert_same 'the S3 endpoint comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_ENDPOINT)" "$SECRET_S3_ENDPOINT"
  assert_same 'the S3 bucket comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_BUCKET)" "$SECRET_S3_BUCKET"
  assert_same 'the S3 region comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_REGION)" "$SECRET_S3_REGION"
  assert_same 'the S3 HTTP opt-in comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_ALLOW_HTTP)" "$SECRET_S3_ALLOW_HTTP"
  assert_same 'the S3 path-style flag comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_PATH_STYLE)" "$SECRET_S3_PATH_STYLE"
  assert_same 'the S3 confirmed target comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET)" "$SECRET_S3_CONFIRMED_TARGET"
  assert_secret_same 'the S3 access key comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_ACCESS_KEY)" "$SECRET_S3_ACCESS" 'wrong value'
  assert_secret_same 'the S3 secret key comes from .env' \
    "$(child_env PROMPT_FERRY_TEST_S3_SECRET_KEY)" "$SECRET_S3_SECRET" 'wrong value'
}

test_s3_keys_are_unset_when_missing() {
  fresh
  write_env <<EOF
PROMPT_FERRY_TEST_VALKEY_URL=$SECRET_VK_URL
EOF
  run_wrapper
  assert_status 'the wrapper still succeeds without S3' 0 "$STATUS"
  local key
  for key in PROMPT_FERRY_TEST_S3_ENDPOINT PROMPT_FERRY_TEST_S3_BUCKET \
    PROMPT_FERRY_TEST_S3_REGION PROMPT_FERRY_TEST_S3_ACCESS_KEY \
    PROMPT_FERRY_TEST_S3_SECRET_KEY PROMPT_FERRY_TEST_S3_ALLOW_HTTP \
    PROMPT_FERRY_TEST_S3_PATH_STYLE PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET; do
    assert_env_absent "an unconfigured S3 setting stays unconfigured: $key" "$key"
  done
}

test_quoted_value_with_spaces_survives() {
  fresh
  mkdir -p "$SANDBOX/dir with space"
  write_env <<EOF
PROMPT_FERRY_TEST_TMPDIR="$SANDBOX/dir with space"
EOF
  run_wrapper
  assert_has 'a quoted value keeps its spaces' "$(child_env TMPDIR)" "$SANDBOX/dir with space/run-"
  assert_same 'the test directory setting matches the run directory' \
    "$(child_env PROMPT_FERRY_TEST_TMPDIR)" "$(child_env TMPDIR)"
  assert_missing 'the run directory is removed afterwards' "$(child_env TMPDIR)"
  assert_exists 'the spaced root itself remains' "$SANDBOX/dir with space"
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
  assert_has 'the substitution stays literal' \
    "$(child_env PROMPT_FERRY_TEST_TMPDIR)" "$SANDBOX/\$(touch canary)/run-"
  assert_same 'backticks stay literal too' \
    "$(child_env PROMPT_FERRY_TEST_VALKEY_URL)" '`touch canary`'
}

test_explicit_env_file_override() {
  fresh
  printf 'PROMPT_FERRY_TEST_TMPDIR=%s/root-env-tmp\n' "$SANDBOX" >> "$SANDBOX/.env"
  printf 'PROMPT_FERRY_TEST_TMPDIR=%s/ci-env-tmp\n' "$SANDBOX" > "$SANDBOX/ci.env"
  AMBIENT='PROMPT_FERRY_TEST_ENV_FILE=ci.env'
  run_wrapper
  assert_has 'the override file supplies the test root' \
    "$(child_env TMPDIR)" "$SANDBOX/ci-env-tmp/run-"
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
  assert_has 'the default test root still applies' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp/run-"
}

# --- per-run test directory -------------------------------------------------

test_run_directories_are_fresh_but_the_root_is_reused() {
  fresh
  run_wrapper
  local first second
  first="$(child_env TMPDIR)"
  assert_has 'the first run owns a directory under the root' "$first" "$SANDBOX/target/test-tmp/run-"
  assert_missing 'the first run directory is removed' "$first"
  touch "$SANDBOX/target/test-tmp/keep-me.sqlite"
  run_wrapper
  second="$(child_env TMPDIR)"
  assert_different 'a repeated run gets a fresh directory' "$first" "$second"
  assert_has 'the second run directory is under the same root' "$second" "$SANDBOX/target/test-tmp/run-"
  assert_missing 'the second run directory is removed' "$second"
  assert_exists 'an existing fixture is never purged' "$SANDBOX/target/test-tmp/keep-me.sqlite"
  assert_exists 'the shared root itself remains' "$SANDBOX/target/test-tmp"
}

test_run_directory_holds_sidecars_then_is_removed() {
  fresh
  CHILD_CMD='write-sidecars'
  run_wrapper
  assert_status 'the sidecar writer succeeds' 0 "$STATUS"
  assert_no_run_dirs 'the run directory is removed with its sidecars' "$SANDBOX/target/test-tmp"
  assert_exists 'the shared root itself remains' "$SANDBOX/target/test-tmp"
}

test_failing_command_keeps_its_exit_and_cleans_up() {
  fresh
  CHILD_CMD='exit-seven'
  run_wrapper
  assert_status 'the command exit status survives' 7 "$STATUS"
  assert_no_run_dirs 'the run directory is removed after failure' "$SANDBOX/target/test-tmp"
  assert_exists 'the shared root itself remains' "$SANDBOX/target/test-tmp"
}

test_sccache_env_wrapper_is_emptied_not_removed() {
  fresh
  AMBIENT='RUSTC_WRAPPER=/usr/bin/sccache RUSTC_WORKSPACE_WRAPPER=sccache'
  run_wrapper
  assert_status 'the wrapper still succeeds' 0 "$STATUS"
  # Emptied, not removed: deleting the key would fall back to a
  # file-configured wrapper, while empty overrides Cargo config.
  assert_same 'an sccache RUSTC_WRAPPER is emptied in the owned child' \
    "$(child_env RUSTC_WRAPPER)" ''
  assert_same 'an sccache workspace wrapper is emptied in the owned child' \
    "$(child_env RUSTC_WORKSPACE_WRAPPER)" ''
  assert_has 'the emptied key is present, not removed' \
    "$(cat "$CAPTURE/child-env")" 'RUSTC_WRAPPER='
}

test_global_config_sccache_is_overridden_empty() {
  fresh
  printf '[build]\nrustc-wrapper = "sccache"\n' > "$SANDBOX/cargo-home/config.toml"
  run_wrapper
  assert_status 'the wrapper still succeeds' 0 "$STATUS"
  assert_same 'a global-config sccache is overridden empty in the owned child' \
    "$(child_env RUSTC_WRAPPER)" ''
}

test_ordinary_compiler_wrapper_is_preserved() {
  fresh
  AMBIENT='RUSTC_WRAPPER=/opt/custom-wrapper'
  run_wrapper
  assert_status 'the wrapper still succeeds' 0 "$STATUS"
  assert_same 'a non-sccache wrapper is preserved' "$(child_env RUSTC_WRAPPER)" '/opt/custom-wrapper'
}

test_relative_tmpdir_resolves_against_repo_root() {
  fresh
  printf 'PROMPT_FERRY_TEST_TMPDIR=target/rel-tmp\n' > "$SANDBOX/.env"
  RUN_CWD="$SANDBOX/capture"
  run_wrapper
  assert_has 'a relative setting resolves against the repository root' \
    "$(child_env TMPDIR)" "$SANDBOX/target/rel-tmp/run-"
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
  assert_has 'a run directory is still exported' \
    "$(child_env TMPDIR)" "$SANDBOX/target/test-tmp/run-"
}

# --- full-runner preflight ----------------------------------------------------

# Writes a .env holding the Valkey key plus the dedicated S3 settings, so both
# dedicated PostgreSQL settings under test arrive through the process
# environment, where a blank stays meaningful, while the S3 target stays
# valid so PG accept/refuse cases keep testing only PostgreSQL.
only_valkey_env() {
  start_fake_valkey
  write_env <<EOF
PROMPT_FERRY_TEST_VALKEY_URL=$(valkey_url)
EOF
  s3_env
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
  assert_has 'the S3 target is reported' "$OUT" 's3 test target verified'
  assert_has 'the summary check runs' "$OUT" 'workspace tests: 1 suites, 3 passed'
  assert_ran_once 'cargo runs exactly once' 'cargo test'
  assert_same 'the offline sqlx mode is preserved' \
    "$(sed -n 's/^SQLX_OFFLINE=//p' "$CAPTURE/cargo-env")" 'true'
  assert_has 'cargo inherits a run directory under the shared test root' \
    "$(sed -n 's/^TMPDIR=//p' "$CAPTURE/cargo-env")" "$SANDBOX/target/test-tmp/run-"
  assert_same 'the dotenv is loaded exactly once through the wrapper' \
    "$(sed -n 's/^PROMPT_FERRY_TEST_ENV_LOADED=//p' "$CAPTURE/cargo-env")" '1'
  assert_missing 'the full-run directory is removed afterwards' \
    "$(sed -n 's/^TMPDIR=//p' "$CAPTURE/cargo-env")"
}

test_full_runner_propagates_cargo_failure() {
  fresh
  service_env
  set_cargo_status 101
  run_tests
  assert_status 'the cargo exit status survives' 101 "$STATUS"
  assert_ran 'the run still executed' 'cargo test'
  assert_missing 'the run directory is removed after failure' \
    "$(sed -n 's/^TMPDIR=//p' "$CAPTURE/cargo-env")"
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

# All dedicated services, addressed at the stub client and the loopback
# listener, plus the dedicated S3 target, so the preflights pass and the run
# reaches cargo.
service_env() {
  start_fake_valkey
  write_env <<EOF
PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=$SECRET_PG_URL
PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=$SECRET_PG_TARGET
PROMPT_FERRY_TEST_VALKEY_URL=$(valkey_url)
EOF
  s3_env
}
