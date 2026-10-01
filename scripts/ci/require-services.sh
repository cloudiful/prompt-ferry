#!/usr/bin/env bash
# Fail fast when the Valkey test service is absent or unreachable.
#
# Valkey is the only service the Rust workspace test run needs: the
# response-affinity regression tests run against a real instance, and they skip
# silently without it, so this preflight is the no-silent-skip guarantee.
# Database-shaped test helpers are lazy pools scoped to an isolated schema that
# never connects, so no PostgreSQL URL is read or probed here. This script never
# prints a connection URL or its userinfo; it reports only the scheme, host and
# port.
set -euo pipefail

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
