#!/usr/bin/env bash
# Shared sandbox harness for scripts/ci/test-env.test.sh.
#
# Sourced by the runner before test-env-test-assertions.sh: this file owns the
# throwaway sandbox around the real test entrypoints, the recording stubs, and
# the PASS/FAIL counters the assertions report into. Every sandbox holds only
# stub tools plus a few coreutils, so no real PostgreSQL or Valkey is needed and
# nothing leaves loopback: `psql` is a recording stub whose answer comes from a
# mode file, the Valkey probe talks to a throwaway listener this harness starts,
# and `cargo` records its argv instead of building the workspace.
#
# The `.env` fixtures are synthetic. Their values are never printed: a check
# that compares one prints a boolean, and a check that asserts absence reports a
# fixed message.

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$LIB_DIR/../.." && pwd)"
WITH_TEST_ENV_SH="$REPO_ROOT/scripts/ci/with-test-env.sh"
TEST_ENV_SH="$REPO_ROOT/scripts/ci/test-env.sh"
RUN_TESTS_SH="$REPO_ROOT/scripts/ci/run-tests.sh"
REQUIRE_SERVICES_SH="$REPO_ROOT/scripts/ci/require-services.sh"
REQUIRE_TEST_POSTGRES_PY="$REPO_ROOT/scripts/ci/require-test-postgres.py"
TEST_PG_TARGET_PY="$REPO_ROOT/scripts/ci/test_pg_target.py"

# Synthetic fixtures. SECRET_PG_PASSWORD is a literal string, not a credential.
# The URL deliberately addresses a remote host and the default `postgres`
# database, because that is the shape the confirmed-target policy exists for.
SECRET_PG_PASSWORD='pg-fixture-secret'
SECRET_PG_URL="postgres://quota_tester:${SECRET_PG_PASSWORD}@pg-test.example.test:5433/postgres"
SECRET_PG_TARGET='pg-test.example.test:5433/postgres'
# The production service the policy must never accept as a test target.
PRODUCTION_PG_URL="postgres://app_user:app-fixture-secret@pg-test.example.test:5436/prompt_ferry"
PRODUCTION_PG_TARGET='pg-test.example.test:5436/prompt_ferry'
SECRET_VK_URL='redis://127.0.0.1:6379/0'
QUOTA_DB_NAME='postgres'

SANDBOX=""
CAPTURE=""
OUT=""
STATUS=0
PASS=0
FAIL=0
AMBIENT=""
RUN_CWD="/"
CHILD_CMD="env"
VALKEY_PID=""

cleanup() {
  stop_fake_valkey
  [ -n "$SANDBOX" ] && rm -rf "$SANDBOX"
  return 0
}
trap cleanup EXIT

write_env() { cat > "$SANDBOX/.env"; }
clear_env() { rm -f "$SANDBOX/.env"; }

# Standard synthetic .env: both dedicated service settings, their confirmed
# test target, and no temporary directory, so the default applies.
full_env() {
  write_env <<EOF
DATABASE_URL=postgres://app_user:app-fixture-secret@10.20.30.40:5432/prompt_ferry
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=app-fixture-key
PROMPT_FERRY_QUOTA_TEST_DATABASE_URL=$SECRET_PG_URL
PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET=$SECRET_PG_TARGET
PROMPT_FERRY_TEST_VALKEY_URL=$SECRET_VK_URL
EOF
}

new_sandbox() {
  SANDBOX="$(mktemp -d)"
  CAPTURE="$SANDBOX/capture"
  AMBIENT=""
  RUN_CWD="/"
  CHILD_CMD="env"
  mkdir -p "$CAPTURE" "$SANDBOX/path" "$SANDBOX/scripts/ci"

  cp "$REPO_ROOT/scripts/dev-env.sh" "$SANDBOX/scripts/"
  cp "$TEST_ENV_SH" "$WITH_TEST_ENV_SH" "$RUN_TESTS_SH" "$REQUIRE_SERVICES_SH" \
    "$REQUIRE_TEST_POSTGRES_PY" "$TEST_PG_TARGET_PY" \
    "$REPO_ROOT/scripts/ci/check-test-summary.py" "$SANDBOX/scripts/ci/"

  # Minimal PATH: the coreutils the entrypoints and stubs need, never a real
  # psql or cargo, so a missing-tool case cannot pass by accident.
  local tool
  for tool in bash env dirname cat mkdir rm sed sort grep seq sleep tee touch python3; do
    ln -s "$(command -v "$tool")" "$SANDBOX/path/$tool"
  done

  printf 'ok\n' > "$SANDBOX/psql-mode"
  add_stub git
  add_stub psql
  add_stub cargo
}

# Stubs derive the sandbox from their own location, because the PostgreSQL
# client is deliberately invoked with a minimal environment that carries no
# harness variables.
add_stub() {
  cat > "$SANDBOX/path/$1"
  chmod +x "$SANDBOX/path/$1"
}

install_stubs() {
  add_stub git <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$(cd "$(dirname "$0")/.." && pwd)"
STUB

  # Answers the three read-only statements the preflight runs, from the PGDATABASE
  # it was actually given, so an identity check cannot pass by accident.
  add_stub psql <<'STUB'
#!/usr/bin/env bash
sandbox="$(cd "$(dirname "$0")/.." && pwd)"
mode="$(cat "$sandbox/psql-mode" 2> /dev/null || printf 'ok')"
printf 'psql %s\n' "$*" >> "$sandbox/capture/calls"
env | sort > "$sandbox/capture/psql-env"
case "$mode" in
  connect_fail)
    printf 'psql: error: connection to server at "pg-test.example.test", port 1 failed: Connection refused\n' >&2
    exit 2 ;;
  auth_fail)
    printf 'psql: error: FATAL:  password authentication failed\n' >&2
    exit 2 ;;
  leak_error)
    # A hostile client message that echoes the password: the preflight must
    # redact it before the message reaches the log.
    printf 'psql: error: password %s rejected for user\n' "$PGPASSWORD" >&2
    exit 2 ;;
esac
if printf '%s' "$*" | grep -q to_regclass; then
  if [ "$mode" = "no_tables" ]; then
    exit 0
  fi
  printf 'chatgpt_quota_snapshots\nendpoint_oauth_tokens\nprovider_endpoints\nrequest_records\nusage_charges\n'
elif printf '%s' "$*" | grep -q current_database; then
  printf '%s\n' "$PGDATABASE"
else
  printf '1\n'
fi
exit 0
STUB

  add_stub cargo <<'STUB'
#!/usr/bin/env bash
sandbox="$(cd "$(dirname "$0")/.." && pwd)"
printf 'cargo %s\n' "$*" >> "$sandbox/capture/calls"
env | sort > "$sandbox/capture/cargo-env"
printf 'test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; 0 finished\n'
exit "$(cat "$sandbox/cargo-status" 2> /dev/null || printf '0')"
STUB
}

set_psql_mode() { printf '%s\n' "$1" > "$SANDBOX/psql-mode"; }
set_cargo_status() { printf '%s\n' "$1" > "$SANDBOX/cargo-status"; }
psql_argv() { cat "$CAPTURE/calls" 2> /dev/null | sed -n 's/^psql //p'; }

fresh() { new_sandbox && install_stubs && full_env; }

# Runs the wrapper from RUN_CWD with AMBIENT ("KEY=VALUE" pairs, one string) and
# captures the child's environment in $CAPTURE/child-env.
run_wrapper() {
  local -a prefix=("PATH=$SANDBOX/path")
  if [ -n "$AMBIENT" ]; then
    # shellcheck disable=SC2086
    prefix+=($AMBIENT)
  fi
  : > "$CAPTURE/calls"
  ( cd "$RUN_CWD" && env "${prefix[@]}" bash "$SANDBOX/scripts/ci/with-test-env.sh" "$CHILD_CMD" ) \
    > "$CAPTURE/child-env" 2> "$CAPTURE/stderr"
  STATUS=$?
  OUT="$(cat "$CAPTURE/stderr")"
  return 0
}

# Runs the full runner. Probes are pinned to a single attempt so an unreachable
# fixture fails immediately instead of waiting out the retry budget.
run_tests() {
  local -a prefix=(
    "PATH=$SANDBOX/path"
    "SERVICE_PROBE_ATTEMPTS=1"
    "SERVICE_PROBE_INTERVAL=0"
  )
  if [ -n "$AMBIENT" ]; then
    # shellcheck disable=SC2086
    prefix+=($AMBIENT)
  fi
  : > "$CAPTURE/calls"
  ( cd "$RUN_CWD" && env "${prefix[@]}" bash "$SANDBOX/scripts/ci/run-tests.sh" ) \
    > "$CAPTURE/stdout" 2> "$CAPTURE/stderr"
  STATUS=$?
  OUT="$(cat "$CAPTURE/stdout" "$CAPTURE/stderr")"
  return 0
}

start_fake_valkey() {
  local ready="$SANDBOX/valkey.port"
  stop_fake_valkey
  rm -f "$ready"
  python3 - "$ready" <<'PY' &
import socket
import sys

ready = sys.argv[1]
listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", 0))
listener.listen(32)
with open(ready, "w", encoding="utf-8") as handle:
    handle.write(str(listener.getsockname()[1]))
while True:
    connection, _ = listener.accept()
    connection.close()
PY
  VALKEY_PID=$!

  local waited=0
  while [ ! -s "$ready" ] && [ "$waited" -lt 100 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  [ -s "$ready" ] || fail 'fake valkey listener did not start'
}

stop_fake_valkey() {
  if [ -n "$VALKEY_PID" ]; then
    kill "$VALKEY_PID" 2> /dev/null
    wait "$VALKEY_PID" 2> /dev/null
    VALKEY_PID=""
  fi
  return 0
}

valkey_url() { printf 'redis://127.0.0.1:%s' "$(cat "$SANDBOX/valkey.port" 2> /dev/null)"; }
