#!/usr/bin/env bash
# Assertion vocabulary for scripts/ci/test-env.test.sh.
#
# Sourced by the runner after test-env-test-lib.sh, which owns the sandbox, the
# stubs, and the PASS/FAIL counters these assertions report into. Every
# assertion here reports a boolean against a synthetic fixture: a check that
# compares a secret prints no value, and a check that asserts absence prints a
# fixed message.

# --- tallies ------------------------------------------------------------------

pass() {
  PASS=$((PASS + 1))
  printf 'ok   %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf 'FAIL %s\n' "$1"
  if [ "$#" -gt 1 ]; then printf '     %s\n' "$2"; fi
}

# --- comparisons --------------------------------------------------------------

assert_status() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "expected exit $2, got $3"; fi; }
assert_nonzero() { if [ "$2" != "0" ]; then pass "$1"; else fail "$1" 'expected a non-zero exit'; fi; }
assert_same() { if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "expected '$3', got '$2'"; fi; }
assert_different() { if [ "$2" != "$3" ]; then pass "$1"; else fail "$1" "expected different values, both were '$2'"; fi; }
assert_has() { if printf '%s' "$2" | grep -qF -- "$3"; then pass "$1"; else fail "$1" "output is missing: $3"; fi; }
assert_exists() { if [ -e "$2" ]; then pass "$1"; else fail "$1" "$2 was not created"; fi; }
assert_missing() { if [ ! -e "$2" ]; then pass "$1"; else fail "$1" "$2 was created"; fi; }

# --- recorded commands -------------------------------------------------------

assert_ran() { if grep -qF -- "$2" "$CAPTURE/calls" 2> /dev/null; then pass "$1"; else fail "$1" "expected '$2' to run"; fi; }
assert_not_run() { if grep -qF -- "$2" "$CAPTURE/calls" 2> /dev/null; then fail "$1" "expected '$2' not to run"; else pass "$1"; fi; }
assert_ran_once() {
  local hits
  hits="$(grep -cF -- "$2" "$CAPTURE/calls" 2> /dev/null)"
  if [ "$hits" = "1" ]; then pass "$1"; else fail "$1" "expected exactly one '$2' run, saw $hits"; fi
}

# --- secret-safe comparisons -------------------------------------------------

# Never prints either side.
assert_secret_same() {
  if [ "$2" = "$3" ]; then pass "$1"; else fail "$1" "$4 (value withheld)"; fi
}

# Never prints the searched value.
assert_absent() {
  local haystack="$2" needle="$3"
  if printf '%s' "$haystack" | grep -qF -- "$needle"; then
    fail "$1" 'a secret value reached the output (withheld)'
  else
    pass "$1"
  fi
}

# (label, key): the key is what gets inspected, the label is only what the tally
# prints. Every caller passes the human label first, so the key must be read
# from $2 — reading $1 passes whatever the child environment holds.
assert_env_absent() {
  local label="$1" key="$2" hits
  hits="$(grep -c "^$key=" "$CAPTURE/child-env" 2> /dev/null)"
  if [ "$hits" = "0" ]; then pass "$label"; else fail "$label" "$key leaked into the child environment"; fi
}

# The owned run directories under a root are all gone: no helper-created
# directory survives its command. Anything else under the root (legacy
# fixtures, stray files) is none of this check's business.
assert_no_run_dirs() {
  local label="$1" root="$2" leftovers=() kept=() entry
  shopt -s nullglob
  leftovers=( "$root"/run-* )
  shopt -u nullglob
  for entry in "${leftovers[@]}"; do
    if [ -d "$entry" ] && [ ! -L "$entry" ]; then kept+=("$entry"); fi
  done
  if [ "${#kept[@]}" -eq 0 ]; then pass "$label"; else fail "$label" "leftover run directories: ${kept[*]}"; fi
}

# Proves the wrapped command never produced output, so it never started.
assert_no_child_output() {
  if [ -s "$CAPTURE/child-env" ]; then fail "$1" 'the command produced output'; else pass "$1"; fi
}

child_env() { sed -n "s/^$1=//p" "$CAPTURE/child-env" 2> /dev/null; }

# Probes the absence assertion against a synthetic capture and echoes its tally
# as "<pass>,<fail>". The probe owns a zeroed tally in a subshell, so a
# deliberately failing assertion reports here instead of polluting the suite's
# PASS/FAIL, and the synthetic capture says nothing about the ambient environment.
absence_probe() {
  local key="$1" content="$2" probe_dir
  probe_dir="$(mktemp -d)"
  if [ -n "$content" ]; then
    printf '%s\n' "$content" > "$probe_dir/child-env"
  else
    : > "$probe_dir/child-env"
  fi

  (
    PASS=0
    FAIL=0
    CAPTURE="$probe_dir"
    assert_env_absent 'sensitivity probe' "$key" > /dev/null 2>&1
    printf '%s,%s' "$PASS" "$FAIL"
  )

  rm -rf "$probe_dir"
}
