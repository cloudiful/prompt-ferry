#!/usr/bin/env bash
# Fail fast when a dedicated test service is absent or unreachable.
#
# Two services back the test-only settings this run loads: the quota integration
# PostgreSQL database and the Valkey instance the response-affinity regression
# tests use. Without them such a test group either returns early and reports
# itself as skipped, or fails outright, so these preflights are the
# no-silent-skip guarantee: they run before cargo starts, and a missing
# prerequisite fails the run instead of reporting coverage the suite did not
# perform.
#
# The settings themselves come from scripts/ci/test-env.sh, which the caller runs
# first (scripts/ci/with-test-env.sh, or the self-wrapping scripts/ci/run-tests.sh).
# This script only checks that the database is dedicated, reachable, and migrated.
# It never prints a connection URL or its userinfo; it reports only the scheme,
# host, port, and database name, and it never migrates or provisions anything.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

: "${PROMPT_FERRY_QUOTA_TEST_DATABASE_URL:?PROMPT_FERRY_QUOTA_TEST_DATABASE_URL is required for the quota integration tests}"
python3 "$SCRIPT_DIR/require-test-postgres.py"

: "${PROMPT_FERRY_TEST_VALKEY_URL:?PROMPT_FERRY_TEST_VALKEY_URL is required for Valkey-backed tests}"

ATTEMPTS="${SERVICE_PROBE_ATTEMPTS:-30}"
INTERVAL="${SERVICE_PROBE_INTERVAL:-2}"

# Print "<scheme> <host> <port>" for a supported URL, otherwise exit non-zero
# with a userinfo-free message.
parse_endpoint() {
  URL="$1" python3 - <<'PY'
import os
import sys
from urllib.parse import urlsplit

parsed = urlsplit(os.environ["URL"])
scheme = parsed.scheme or "<none>"
host = parsed.hostname or "<none>"
if parsed.scheme not in {"redis", "rediss"} or not parsed.hostname:
    # Never echo the URL: it can carry a password in the userinfo segment.
    sys.exit(f"unsupported service URL: scheme={scheme} host={host}")
print(f"{parsed.scheme} {parsed.hostname} {parsed.port or 6379}")
PY
}

probe() {
  local label="$1" url="$2"
  local endpoint
  if ! endpoint="$(parse_endpoint "$url")"; then
    echo "error: invalid ${label} service URL" >&2
    return 1
  fi

  local scheme host port
  scheme="${endpoint%% *}"
  endpoint="${endpoint#* }"
  host="${endpoint%% *}"
  port="${endpoint#* }"

  local attempt
  for attempt in $(seq 1 "$ATTEMPTS"); do
    if python3 - "$host" "$port" <<'PY'
import socket, sys

host, port = sys.argv[1], int(sys.argv[2])
try:
    with socket.create_connection((host, port), timeout=5):
        pass
except OSError:
    sys.exit(1)
PY
    then
      echo "service ready: ${label} (${scheme}) ${host}:${port}"
      return 0
    fi
    if [ "$attempt" -eq "$ATTEMPTS" ]; then
      echo "error: ${label} service not reachable at ${host}:${port}" >&2
      return 1
    fi
    sleep "$INTERVAL"
  done
}

probe "valkey" "$PROMPT_FERRY_TEST_VALKEY_URL"
