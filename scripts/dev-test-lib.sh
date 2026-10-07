#!/usr/bin/env bash
# Shared harness for scripts/dev.test.sh.
#
# Sourced by the test cases: builds a throwaway sandbox around the real
# entrypoint and provides the assertion vocabulary. Each sandbox gets a PATH
# holding only stub tools plus a few coreutils, so no real service starts, no
# real dependency is used, and the repository .env is never read. A stub
# supervisor records the argv and the environment the entrypoint handed it.
#
# The database URL here is a synthetic fixture. Its value is never printed: the
# forwarding check compares silently and only reports a boolean.

LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$LIB_DIR/.." && pwd)"
DEV_SH="$REPO_ROOT/scripts/dev.sh"
DEV_ENV_SH="$REPO_ROOT/scripts/dev-env.sh"
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
  local captured="$(sed -n 's/^DATABASE_URL=//p' "$CAPTURE/env" 2> /dev/null)"
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

  # The entrypoint sources its env helper from beside itself, so both land in
  # the sandbox together.
  cp "$DEV_SH" "$DEV_ENV_SH" "$SANDBOX/scripts/"

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

# Same, plus ambient "KEY=VALUE" pairs, to exercise the resolve fallback. The
# first argument is split into separate pairs on purpose.
run_dev_ambient() {
  # shellcheck disable=SC2086
  OUT="$(cd / && PATH="$SANDBOX/path" SANDBOX_CAPTURE="$CAPTURE" env $1 bash "$SANDBOX/scripts/dev.sh" "${@:2}" 2>&1)"
  STATUS=$?
  return 0
}

captured_env() { sed -n "s/^$1=//p" "$CAPTURE/env" 2> /dev/null; }
write_env() { cat > "$SANDBOX/.env"; }
minimal_env() { printf 'DATABASE_URL=%s\n' "$SENTINEL_DB_URL" | write_env; }
fresh() { new_sandbox && minimal_env; }
