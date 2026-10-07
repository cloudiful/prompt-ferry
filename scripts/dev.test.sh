#!/usr/bin/env bash
# Focused checks for scripts/dev.sh.
#
# Every case runs the real entrypoint inside a temporary sandbox whose PATH holds
# only stub tools plus a few coreutils: no real service starts, no real
# dependency is used, and the repository .env is never read. The stub supervisor
# records the argv and the environment it was handed.
#
# The database URL here is a synthetic fixture. Its value is never printed: the
# forwarding check compares silently and only reports a boolean.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEV_SH="$REPO_ROOT/scripts/dev.sh"
SENTINEL_DB_URL='postgres://devcheck:devcheck@127.0.0.1:5432/prompt_ferry_devcheck'

SANDBOX=""
CAPTURE=""
OUT=""
STATUS=0
PASS=0
FAIL=0

cleanup() {
  [ -n "$SANDBOX" ] && rm -rf "$SANDBOX"
  return 0
}
trap cleanup EXIT

pass() {
  PASS=$((PASS + 1))
  printf 'ok   %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf 'FAIL %s\n' "$1"
  if [ "$#" -gt 1 ]; then printf '     %s\n' "$2"; fi
}

assert_status() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "expected exit $2, got $3"; fi; }
assert_nonzero() { if [ "$2" != "0" ]; then pass "$1"; else fail "$1" "expected a non-zero exit"; fi; }
assert_has() { if printf '%s' "$2" | grep -qF -- "$3"; then pass "$1"; else fail "$1" "output is missing: $3"; fi; }
assert_lacks() { if printf '%s' "$2" | grep -qF -- "$3"; then fail "$1" "output unexpectedly contains: $3"; else pass "$1"; fi; }
assert_same() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "expected '$2', got '$3'"; fi; }
assert_missing() { if [ ! -e "$2" ]; then pass "$1"; else fail "$1" "$2 was created"; fi; }
assert_ran() { if grep -qF -- "$2" "$CAPTURE/calls" 2> /dev/null; then pass "$1"; else fail "$1" "expected '$2' to run"; fi; }
assert_not_run() { if grep -qF -- "$2" "$CAPTURE/calls" 2> /dev/null; then fail "$1" "expected '$2' not to run"; else pass "$1"; fi; }

# Never prints either side.
assert_db_url_forwarded() {
  local captured="$(sed -n 's/^DATABASE_URL=//p' "$CAPTURE/env")"
  if [ "$captured" = "$SENTINEL_DB_URL" ]; then pass "$1"; else fail "$1" 'DATABASE_URL was not forwarded unchanged (value withheld)'; fi
}

# Never prints the URL.
assert_db_url_not_printed() {
  if printf '%s' "$OUT" | grep -qF -- "$SENTINEL_DB_URL"; then
    fail "$1" 'the entrypoint printed the database URL'
  else
    pass "$1"
  fi
}

new_sandbox() {
  SANDBOX="$(mktemp -d)"
  CAPTURE="$SANDBOX/capture"
  mkdir -p "$CAPTURE" "$SANDBOX/scripts" "$SANDBOX/frontend" "$SANDBOX/path"

  cp "$DEV_SH" "$SANDBOX/scripts/dev.sh"

  # Minimal PATH: coreutils the entrypoint and stubs need, never the real
  # cargo/bun/curl, so a missing-tool case cannot pass by accident.
  local tool
  for tool in bash dirname cat env mkdir rm touch; do
    ln -s "$(command -v "$tool")" "$SANDBOX/path/$tool"
  done

  cat > "$SANDBOX/scripts/dev-supervisor.sh" <<'STUB'
#!/usr/bin/env bash
printf 'supervisor %s\n' "$*" >> "$SANDBOX_CAPTURE/calls"
printf '%s\n' "$*" > "$SANDBOX_CAPTURE/argv"
env > "$SANDBOX_CAPTURE/env"
exit 0
STUB
  chmod +x "$SANDBOX/scripts/dev-supervisor.sh"

  add_stub cargo
  add_stub curl
  add_stub bun
}

add_stub() {
  local name="$1"
  cat > "$SANDBOX/path/$name" <<STUB
#!/usr/bin/env bash
printf '%s %s\n' "$name" "\$*" >> "\$SANDBOX_CAPTURE/calls"
if [ "$name" = bun ]; then
  mkdir -p node_modules
fi
exit 0
STUB
  chmod +x "$SANDBOX/path/$name"
}

remove_stub() { rm -f "$SANDBOX/path/$1"; }

# Runs from "/" to prove the entrypoint resolves the repository root itself.
run_dev() {
  OUT="$(cd / && PATH="$SANDBOX/path" SANDBOX_CAPTURE="$CAPTURE" bash "$SANDBOX/scripts/dev.sh" "$@" 2>&1)"
  STATUS=$?
  return 0
}

captured_env() { sed -n "s/^$1=//p" "$CAPTURE/env" 2> /dev/null; }
write_env() { cat > "$SANDBOX/.env"; }
minimal_env() { printf 'DATABASE_URL=%s\n' "$SENTINEL_DB_URL" | write_env; }
fresh() { new_sandbox && minimal_env; }

test_help() {
  fresh
  run_dev help
  assert_status 'help exits zero' 0 "$STATUS"
  assert_has 'help documents the usage line' "$OUT" 'Usage: bash scripts/dev.sh <help|backend|full>'
  assert_has 'help documents backend mode' "$OUT" 'backend   Start relay and worker'
  assert_has 'help documents full mode' "$OUT" 'full      Start relay, worker, and frontend Vite dev server'
  assert_has 'help documents the .env contract' "$OUT" 'treats it as the source of truth'
  assert_has 'help documents the database requirement' "$OUT" 'DATABASE_URL is required'
  assert_has 'help documents prerequisites' "$OUT" 'bash, cargo, and curl on PATH'
  assert_has 'help documents the bun requirement' "$OUT" 'full mode additionally requires bun'
  assert_not_run 'help does not start services' 'supervisor'
  assert_not_run 'help does not build' 'cargo'
}

test_default_command_is_help() {
  fresh
  run_dev
  assert_status 'no argument exits zero' 0 "$STATUS"
  assert_has 'no argument prints usage' "$OUT" 'Usage: bash scripts/dev.sh'
}

test_invalid_modes() {
  fresh
  run_dev bogus
  assert_nonzero 'invalid mode fails' "$STATUS"
  assert_has 'invalid mode is named' "$OUT" 'unknown command: bogus'
  assert_has 'invalid mode points at help' "$OUT" 'bash scripts/dev.sh help'
  assert_not_run 'invalid mode starts nothing' 'supervisor'
  assert_not_run 'invalid mode does not build' 'cargo'
  assert_not_run 'invalid mode does not install' 'bun'

  run_dev backend extra
  assert_nonzero 'extra arguments are rejected' "$STATUS"
  assert_has 'extra arguments explain the fix' "$OUT" 'expected a single command'
  assert_not_run 'extra arguments start nothing' 'supervisor'
}

test_missing_supervisor() {
  fresh
  rm -f "$SANDBOX/scripts/dev-supervisor.sh"
  run_dev backend
  assert_nonzero 'missing supervisor fails' "$STATUS"
  assert_has 'missing supervisor is actionable' "$OUT" 'complete checkout'
  assert_not_run 'missing supervisor starts nothing' 'supervisor'
}

test_prerequisites() {
  fresh
  remove_stub cargo
  run_dev backend
  assert_nonzero 'missing cargo fails' "$STATUS"
  assert_has 'missing cargo is a preflight failure' "$OUT" 'missing required tool'
  assert_has 'missing cargo is named' "$OUT" 'cargo'
  assert_has 'missing cargo explains the fix' "$OUT" 'bash scripts/dev.sh backend'
  assert_not_run 'missing cargo starts nothing' 'supervisor'

  fresh
  remove_stub bun
  run_dev full
  assert_nonzero 'full mode without bun fails' "$STATUS"
  assert_has 'missing bun is a preflight failure' "$OUT" 'missing required tool'
  assert_has 'full mode names bun' "$OUT" 'bun'
  assert_not_run 'missing bun fails before building' 'cargo build'
  assert_not_run 'missing bun starts nothing' 'supervisor'

  run_dev backend
  assert_status 'backend mode does not need bun' 0 "$STATUS"
  assert_ran 'backend mode still delegates' 'supervisor'

  fresh
  remove_stub curl
  run_dev backend
  assert_nonzero 'missing curl fails' "$STATUS"
  assert_has 'missing curl is a preflight failure' "$OUT" 'missing required tool'
  assert_has 'missing curl is named' "$OUT" 'curl'
  assert_not_run 'missing curl starts nothing' 'supervisor'
}

test_database_url_required() {
  # Absent, whitespace-only, and fully commented-out values are the same failure:
  # commenting a value out is the documented way to switch configuration off.
  local case label body
  for case in \
    "absent|# no database url here"$'\n'"PROMPT_FERRY_LOGGING__LEVEL=info" \
    "blank|DATABASE_URL=   " \
    "commented-out|# DATABASE_URL=postgres://commented-out.invalid/db"; do
    label="${case%%|*}"
    body="${case#*|}"
    new_sandbox
    printf '%s\n' "$body" | write_env
    run_dev backend
    assert_nonzero "$label database url fails" "$STATUS"
    assert_has "$label database url is named" "$OUT" 'DATABASE_URL is not set'
    assert_has "$label database url explains the fix" "$OUT" 'Set DATABASE_URL in .env'
    assert_not_run "$label database url fails before building" 'cargo'
    assert_not_run "$label database url starts nothing" 'supervisor'
  done

  new_sandbox
  rm -f "$SANDBOX/.env"
  run_dev backend
  assert_nonzero 'missing dotenv file fails' "$STATUS"
  assert_has 'missing dotenv file is named' "$OUT" 'DATABASE_URL is not set'
  assert_not_run 'missing dotenv file starts nothing' 'supervisor'
}

test_database_url_forwarded_but_hidden() {
  fresh
  run_dev backend
  assert_status 'backend mode exits zero' 0 "$STATUS"
  assert_db_url_forwarded 'database url reaches the supervisor'
  assert_db_url_not_printed 'database url is never printed'
}

test_dotenv_parsing() {
  new_sandbox
  cat > "$SANDBOX/.env" <<'ENV'
# a comment
   # an indented comment
# DATABASE_URL=postgres://commented-out.invalid/db
# PROMPT_FERRY_RELAY__CLIENT_TOKEN=commented-out

DATABASE_URL=postgres://devcheck:devcheck@127.0.0.1:5432/prompt_ferry_devcheck
PROMPT_FERRY_WORKER__RELAY_URLS=["ws://127.0.0.1:8788/ws/worker"]
PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN='quoted-admin'
PROMPT_FERRY_LOGGING__LEVEL="debug"
PROMPT_FERRY_RELAY__CLIENT_TOKEN=  spaced-token
PROMPT_FERRY_RELAY__WORKER_BIND=127.0.0.1:9999=odd
PROMPT_FERRY_WORKER__ADMIN_BIND=
EVIL_SUBST=$(touch pwned-subst.marker)
EVIL_BACKTICK=`touch pwned-backtick.marker`
EVIL_SEMICOLON=; touch pwned-semicolon.marker; :
this line has no assignment
9INVALID=nope
not a valid key=either
ENV
  run_dev backend
  assert_status 'rich dotenv parses' 0 "$STATUS"
  assert_same 'json relay urls survive intact' '["ws://127.0.0.1:8788/ws/worker"]' "$(captured_env PROMPT_FERRY_WORKER__RELAY_URLS)"
  assert_same 'single quotes are removed' 'quoted-admin' "$(captured_env PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN)"
  assert_same 'double quotes are removed' 'debug' "$(captured_env PROMPT_FERRY_LOGGING__LEVEL)"
  assert_same 'surrounding whitespace is trimmed' 'spaced-token' "$(captured_env PROMPT_FERRY_RELAY__CLIENT_TOKEN)"
  assert_same 'embedded equals is preserved' '127.0.0.1:9999=odd' "$(captured_env PROMPT_FERRY_RELAY__WORKER_BIND)"
  assert_same 'blank value falls back to the default' '127.0.0.1:8789' "$(captured_env PROMPT_FERRY_WORKER__ADMIN_BIND)"
  assert_missing 'command substitution is not executed' "$SANDBOX/pwned-subst.marker"
  assert_missing 'backticks are not executed' "$SANDBOX/pwned-backtick.marker"
  assert_missing 'command separators are not executed' "$SANDBOX/pwned-semicolon.marker"
  assert_db_url_forwarded 'the fixture database url is forwarded'
}

test_extra_dotenv_keys_forwarded() {
  fresh
  printf 'DATABASE_URL=%s\nPROMPT_FERRY_WORKER__MCP_WARMUP=none\nPROMPT_FERRY_DEVTEST_EXTRA=extra-value\n' "$SENTINEL_DB_URL" | write_env
  run_dev backend
  assert_status 'extra dotenv keys run cleanly' 0 "$STATUS"
  assert_same 'a supervisor-owned key from .env is forwarded' 'none' "$(captured_env PROMPT_FERRY_WORKER__MCP_WARMUP)"
  assert_same 'an arbitrary .env key is forwarded' 'extra-value' "$(captured_env PROMPT_FERRY_DEVTEST_EXTRA)"
}

test_defaults() {
  fresh
  run_dev backend
  assert_status 'defaults run cleanly' 0 "$STATUS"
  assert_same 'logging default' 'info' "$(captured_env PROMPT_FERRY_LOGGING__LEVEL)"
  assert_same 'relay bind default' '127.0.0.1:8787' "$(captured_env PROMPT_FERRY_RELAY__BIND)"
  assert_same 'worker listener bind default' '127.0.0.1:8788' "$(captured_env PROMPT_FERRY_RELAY__WORKER_BIND)"
  assert_same 'relay client token default' 'dev-client-token' "$(captured_env PROMPT_FERRY_RELAY__CLIENT_TOKEN)"
  assert_same 'relay worker token default' 'dev-worker-token' "$(captured_env PROMPT_FERRY_RELAY__WORKER_TOKEN)"
  assert_same 'relay timeout default' '300' "$(captured_env PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS)"
  assert_same 'worker token default' 'dev-worker-token' "$(captured_env PROMPT_FERRY_WORKER__WORKER_TOKEN)"
  assert_same 'admin bind default' '127.0.0.1:8789' "$(captured_env PROMPT_FERRY_WORKER__ADMIN_BIND)"
  assert_same 'admin login default' 'admin' "$(captured_env PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN)"
  assert_same 'admin password default' 'change-me-now' "$(captured_env PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD)"
  if [ -n "$(captured_env PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY)" ]; then
    pass 'relay secret master key default is set'
  else
    fail 'relay secret master key default is set' 'the managed dev key was not exported'
  fi
  assert_same 'relay secret master key matches the managed dev default' \
    'BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=' \
    "$(captured_env PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY)"
}

test_precedence() {
  new_sandbox
  printf 'DATABASE_URL=%s\nPROMPT_FERRY_RELAY__CLIENT_TOKEN=from-dotenv\n' "$SENTINEL_DB_URL" | write_env
  OUT="$(cd / && PATH="$SANDBOX/path" SANDBOX_CAPTURE="$CAPTURE" \
    PROMPT_FERRY_RELAY__CLIENT_TOKEN=from-ambient \
    PROMPT_FERRY_WORKER__WORKER_TOKEN=ambient-worker-token \
    bash "$SANDBOX/scripts/dev.sh" backend 2>&1)"
  STATUS=$?
  assert_status 'precedence run exits zero' 0 "$STATUS"
  assert_same 'dotenv wins over the ambient environment' 'from-dotenv' "$(captured_env PROMPT_FERRY_RELAY__CLIENT_TOKEN)"
  assert_same 'ambient value applies when dotenv omits the key' 'ambient-worker-token' "$(captured_env PROMPT_FERRY_WORKER__WORKER_TOKEN)"

  # A blank .env value is authoritative too: it overrides ambient, then falls
  # back to the managed default, matching the Nushell load-env behavior.
  new_sandbox
  printf 'DATABASE_URL=%s\nPROMPT_FERRY_WORKER__ADMIN_BIND=\n' "$SENTINEL_DB_URL" | write_env
  OUT="$(cd / && PATH="$SANDBOX/path" SANDBOX_CAPTURE="$CAPTURE" \
    PROMPT_FERRY_WORKER__ADMIN_BIND=127.0.0.1:7777 \
    bash "$SANDBOX/scripts/dev.sh" backend 2>&1)"
  STATUS=$?
  assert_status 'blank-value precedence run exits zero' 0 "$STATUS"
  assert_same 'a blank dotenv value overrides ambient and falls back to the default' \
    '127.0.0.1:8789' "$(captured_env PROMPT_FERRY_WORKER__ADMIN_BIND)"
}

test_backend_delegation() {
  fresh
  run_dev backend
  assert_status 'backend mode exits zero' 0 "$STATUS"
  assert_same 'backend mode delegates with the backend mode' 'backend' "$(cat "$CAPTURE/argv")"
  assert_ran 'backend mode builds the binary' 'cargo build'
  assert_not_run 'backend mode does not install frontend deps' 'bun'
  assert_has 'backend mode summarises the relay' "$OUT" 'relay               http://127.0.0.1:8787'
  assert_has 'backend mode summarises the worker listener' "$OUT" 'worker listener     http://127.0.0.1:8788'
  assert_has 'backend mode summarises the admin api' "$OUT" 'worker admin api    http://127.0.0.1:8789/api/v1'
  assert_lacks 'backend mode does not advertise the frontend' "$OUT" 'http://127.0.0.1:5171'
  assert_has 'backend mode explains how to stop' "$OUT" 'Stop with Ctrl+C'
}

test_full_delegation() {
  fresh
  run_dev full
  assert_status 'full mode exits zero' 0 "$STATUS"
  assert_same 'full mode delegates with the full mode' 'full' "$(cat "$CAPTURE/argv")"
  assert_ran 'full mode installs missing frontend deps' 'bun install --no-save'
  assert_has 'full mode advertises the frontend' "$OUT" 'frontend            http://127.0.0.1:5171'

  # node_modules now exists, so a second run must not install again.
  : > "$CAPTURE/calls"
  run_dev full
  assert_status 'second full run exits zero' 0 "$STATUS"
  assert_not_run 'existing frontend deps are reused' 'bun install'
}

test_external_relay_summary() {
  new_sandbox
  printf 'DATABASE_URL=%s\nPROMPT_FERRY_WORKER__RELAY_URLS=["wss://relay.example.com:443/ws/worker"]\n' \
    "$SENTINEL_DB_URL" | write_env
  run_dev backend
  assert_status 'external relay run exits zero' 0 "$STATUS"
  assert_has 'external relay is reported' "$OUT" 'relay               external wss://relay.example.com:443/ws/worker'
  assert_lacks 'a local relay endpoint is not advertised' "$OUT" 'relay               http://127.0.0.1:8787'
}

test_help_without_tools() {
  new_sandbox
  remove_stub cargo
  remove_stub curl
  remove_stub bun
  run_dev help
  assert_status 'help works without any tool installed' 0 "$STATUS"
  assert_has 'help still documents usage' "$OUT" 'Usage: bash scripts/dev.sh'
}

test_main() {
  if [ ! -f "$DEV_SH" ]; then
    printf 'FAIL missing %s\n' "$DEV_SH"
    exit 1
  fi

  local scenario
  for scenario in test_help test_default_command_is_help test_invalid_modes test_missing_supervisor \
    test_prerequisites test_database_url_required test_database_url_forwarded_but_hidden \
    test_dotenv_parsing test_extra_dotenv_keys_forwarded test_defaults test_precedence test_backend_delegation \
    test_full_delegation \
    test_external_relay_summary test_help_without_tools; do
    "$scenario"
  done

  printf '\n%s passed, %s failed\n' "$PASS" "$FAIL"
  if [ "$FAIL" -gt 0 ]; then
    exit 1
  fi
}

test_main "$@"
