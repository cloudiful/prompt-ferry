#!/usr/bin/env bash
# Fail fast when the PostgreSQL / Valkey test services are absent or unreachable.
#
# Every service-backed Rust test skips only when its two service env vars are
# absent, so requiring them here (plus a TCP reachability probe) is the primary
# no-silent-skip guarantee. This script never prints a connection URL or its
# userinfo; it reports only the scheme, host and port.
set -euo pipefail

: "${PROMPT_FERRY_TEST_DATABASE_URL:?PROMPT_FERRY_TEST_DATABASE_URL is required for integration tests}"
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
if parsed.scheme not in {"postgres", "postgresql", "redis", "rediss"} or not parsed.hostname:
    # Never echo the URL: it can carry a password in the userinfo segment.
    sys.exit(f"unsupported service URL: scheme={scheme} host={host}")
default = 5432 if parsed.scheme.startswith("postgres") else 6379
print(f"{parsed.scheme} {parsed.hostname} {parsed.port or default}")
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

probe "postgres" "$PROMPT_FERRY_TEST_DATABASE_URL"
probe "valkey" "$PROMPT_FERRY_TEST_VALKEY_URL"
