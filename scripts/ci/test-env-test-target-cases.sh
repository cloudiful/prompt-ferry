#!/usr/bin/env bash
# Dedicated-target and confirmed-fingerprint cases for scripts/ci/test-env.test.sh.
#
# Sourced by the runner, which dispatches every case in its SCENARIOS list. The
# policy under test is the one both the shell preflight and
# tests/support/test_pg_target.rs enforce: a test database is the one the
# dedicated URL addresses *and* an operator confirmed as host:port/database.
# Nothing here reaches a real service — the preflight runs against the recording
# `psql` stub — so a production-shaped URL is refused without being connected to.

# --- the confirmed target is required ----------------------------------------

# Runs the full runner with an explicit URL/target pair supplied through the
# process environment, where a blank stays meaningful. Implemented by the shared
# `run_preflight` helper in scripts/ci/test-env-test-cases.sh.

test_confirmed_target_is_required_before_any_connection() {
  local entry label url target fragment
  for entry in \
    "the confirmed target|postgres://quota_tester:pg-fixture-secret@pg-test.example.test:5433/postgres||is required whenever" \
    "the quota URL||pg-test.example.test:5433/postgres|is required for the quota integration tests"; do
    IFS='|' read -r label url target fragment <<< "$entry"
    run_preflight "$url" "$target"
    assert_nonzero "$label is refused" "$STATUS"
    assert_has "$label reports why" "$OUT" "$fragment"
    assert_not_run "$label stops before cargo" 'cargo test'
    assert_not_run "$label never opens a connection" 'psql'
  done
}

# --- only the confirmed target is accepted -----------------------------------

test_only_the_confirmed_target_is_accepted() {
  local accepted entry label url target
  for entry in \
    "the confirmed remote target|$SECRET_PG_URL|$SECRET_PG_TARGET" \
    "host case and the default port|postgresql://quota_tester@PG-Test.Example.Test/postgres|pg-test.example.test:5432/postgres" \
    "a percent-encoded host|postgres://quota_tester@pg-test.example%2Etest:5433/postgres|pg-test.example.test:5433/postgres"; do
    IFS='|' read -r label url target <<< "$entry"
    run_preflight "$url" "$target"
    assert_status "$label is accepted" 0 "$STATUS"
    assert_has "$label reaches the schema check" "$OUT" 'quota test schema verified'
    assert_ran_once "$label connects once" 'SELECT 1'
  done
}

test_another_database_is_refused() {
  local entry label url target
  for entry in \
    "a different host|postgres://quota_tester@other.example.test:5433/postgres|$SECRET_PG_TARGET" \
    "a different port|postgres://quota_tester@pg-test.example.test:5436/postgres|$SECRET_PG_TARGET" \
    "a different database|postgres://quota_tester@pg-test.example.test:5433/other|$SECRET_PG_TARGET" \
    "the application database|postgres://quota_tester@pg-test.example.test:5433/prompt_ferry|$SECRET_PG_TARGET"; do
    IFS='|' read -r label url target <<< "$entry"
    run_preflight "$url" "$target"
    assert_nonzero "$label is refused" "$STATUS"
    assert_has "$label names the confirmed target" "$OUT" 'instead of the confirmed target'
    assert_not_run "$label never connects" 'psql'
    assert_not_run "$label stops before cargo" 'cargo test'
  done
}

test_the_production_service_is_refused() {
  # The shape this policy exists for: a real remote production database must not
  # become the test target by pointing the URL at it and leaving the confirmed
  # target alone, nor by confirming it in the same change without review.
  run_preflight "$PRODUCTION_PG_URL" "$SECRET_PG_TARGET"
  assert_nonzero 'the production database under the test target is refused' "$STATUS"
  assert_has 'the refusal names the mismatch' "$OUT" 'instead of the confirmed target'
  assert_not_run 'nothing connects to production' 'psql'

  run_preflight "$SECRET_PG_URL" "$PRODUCTION_PG_TARGET"
  assert_nonzero 'a production target rejects the test URL' "$STATUS"
  assert_has 'the refusal names the mismatch' "$OUT" 'instead of the confirmed target'
  assert_not_run 'nothing connects' 'psql'

  # Re-pointing the URL to the production service while confirming that service
  # is still refused for the test suite: the authority is the operator, not the
  # URL, and this preflight never decides that on its own.
  run_preflight "$PRODUCTION_PG_URL" "$PRODUCTION_PG_TARGET"
  assert_status 'a confirmed production target is structurally valid' 0 "$STATUS"
  assert_same 'it connects to the confirmed database only' \
    "$(sed -n 's/^PGDATABASE=//p' "$CAPTURE/psql-env")" 'prompt_ferry'
}

# --- malformed inputs fail closed --------------------------------------------

test_malformed_urls_and_targets_are_refused() {
  local entry label url target
  for entry in \
    "a value that is not a URL|not-a-url|$SECRET_PG_TARGET|unsupported test database URL scheme" \
    "an unsupported scheme|mysql://quota_tester@pg-test.example.test:5433/postgres|$SECRET_PG_TARGET|unsupported test database URL scheme" \
    "no username|postgres://pg-test.example.test:5433/postgres|$SECRET_PG_TARGET|must carry a username" \
    "no database|postgres://quota_tester@pg-test.example.test:5433/|$SECRET_PG_TARGET|unsupported database name" \
    "connection options|postgres://quota_tester@pg-test.example.test:5433/postgres?options=-csearch_path%3Dpublic|$SECRET_PG_TARGET|must not carry query parameters" \
    "a fragment|postgres://quota_tester@pg-test.example.test:5433/postgres#frag|$SECRET_PG_TARGET|must not carry a fragment" \
    "an invalid port|postgres://quota_tester@pg-test.example.test:port/postgres|$SECRET_PG_TARGET|invalid port" \
    "a bracketed host|postgres://quota_tester@[::1]:5433/postgres|$SECRET_PG_TARGET|cannot be confirmed" \
    "a malformed escape|postgres://quota_tester@pg-test.example%ZZtest:5433/postgres|$SECRET_PG_TARGET|unsupported host" \
    "a target without a database|postgres://quota_tester@pg-test.example.test:5433/postgres|pg-test.example.test:5433|must read host:port/database" \
    "a target with a deeper path|postgres://quota_tester@pg-test.example.test:5433/postgres|pg-test.example.test/postgres/extra|beyond the database name" \
    "a target without a host|postgres://quota_tester@pg-test.example.test:5433/postgres|:5433/postgres|unsupported host" \
    "a target with an invalid port|postgres://quota_tester@pg-test.example.test:5433/postgres|pg-test.example.test:0/postgres|invalid port"; do
    IFS='|' read -r label url target fragment <<< "$entry"
    run_preflight "$url" "$target"
    assert_nonzero "$label is refused" "$STATUS"
    assert_has "$label reports why" "$OUT" "$fragment"
    assert_not_run "$label never connects" 'psql'
    assert_not_run "$label stops before cargo" 'cargo test'
  done
}

test_a_refusal_never_echoes_the_url_or_password() {
  run_preflight "$PRODUCTION_PG_URL" "$SECRET_PG_TARGET"
  assert_nonzero 'the production URL is refused' "$STATUS"
  assert_absent 'the URL is not echoed' "$OUT" "$PRODUCTION_PG_URL"
  assert_absent 'the password is not echoed' "$OUT" 'app-fixture-secret'
}

# --- the parser module is a separate, pure unit -----------------------------

test_the_target_parser_is_a_pure_importable_module() {
  fresh
  # `require-test-postgres.py` imports its sibling parser, so this also proves
  # the sandbox carries both halves. The parser must decide the target on its
  # own: importing it starts no process, opens no socket, and touches no
  # database, and the preflight re-exports the same objects so existing import
  # hooks resolve unchanged.
  python3 - "$SANDBOX/scripts/ci" > "$CAPTURE/parser-out" 2>&1 <<'PY'
import importlib.util
import pathlib
import sys

directory = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(directory))
import test_pg_target as parser

missing = [name for name in parser.__all__ if not hasattr(parser, name)]
assert not missing, f"declared names are missing: {missing}"

confirmed = "db.example.test:5433/postgres"
accepted = "postgres://quota_tester:pg-fixture-secret@db.example.test:5433/postgres"
target = parser.parse_target(accepted, confirmed)
assert target.label() == confirmed
assert target.password == "pg-fixture-secret"

refused = [
    ("postgres://q@db.example.test:5436/postgres", confirmed),
    ("postgres://q@db.example.test:5433/other", confirmed),
    ("postgres://q@other.example.test:5433/postgres", confirmed),
    (accepted, "other.test:5433/postgres"),
]
for url, target_confirmed in refused:
    try:
        parser.parse_target(url, target_confirmed)
        raise SystemExit(f"{url} against {target_confirmed} must be refused")
    except parser.PreflightError:
        pass

spec = importlib.util.spec_from_file_location(
    "rtp", directory / "require-test-postgres.py"
)
preflight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preflight)
assert preflight.parse_target is parser.parse_target
assert preflight.PreflightError is parser.PreflightError
print("parser-ok")
PY
  local status=$?
  assert_status 'the parser module imports and agrees with the preflight' 0 "$status"
  assert_has 'the parser reports its own result' "$(cat "$CAPTURE/parser-out")" 'parser-ok'
  assert_absent 'no credential from the checked fixture is printed' \
    "$(cat "$CAPTURE/parser-out")" 'pg-fixture-secret'
}

test_a_missing_parser_module_fails_closed() {
  fresh
  only_valkey_env
  rm -f "$SANDBOX/scripts/ci/test_pg_target.py"
  AMBIENT="PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=$SECRET_PG_URL PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=$SECRET_PG_TARGET"
  run_tests
  assert_nonzero 'a missing parser module fails the preflight' "$STATUS"
  assert_not_run 'nothing connects without the parser' 'psql'
  assert_not_run 'nothing runs without the parser' 'cargo test'
}

test_preflight_rejects_unusable_postgres_databases() {
  local cases=(
    "unreachable database|connect_fail|refused the connection"
    "rejected credentials|auth_fail|refused the connection"
    "unmigrated database|no_tables|is not migrated"
  )
  local entry label mode fragment
  for entry in "${cases[@]}"; do
    IFS='|' read -r label mode fragment <<< "$entry"
    run_preflight "$SECRET_PG_URL" "$SECRET_PG_TARGET"
    set_psql_mode "$mode"
    run_tests
    assert_nonzero "$label is refused" "$STATUS"
    assert_has "$label reports why" "$OUT" "$fragment"
    assert_not_run "$label stops before cargo" 'cargo test'
  done
}

# --- the connected database must be the confirmed one ------------------------

test_the_connected_database_is_checked_against_the_confirmed_one() {
  run_preflight "$SECRET_PG_URL" "$SECRET_PG_TARGET"
  # The stub answers `current_database()` from the PGDATABASE it was handed, so
  # the identity check passes here and a redirect would be caught by it.
  assert_status 'a matching identity is accepted' 0 "$STATUS"
  assert_ran_once 'the identity is verified once' 'SELECT current_database()'
  assert_absent 'the username never reaches the log' "$OUT" 'quota_tester'
}
