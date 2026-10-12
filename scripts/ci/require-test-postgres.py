#!/usr/bin/env python3
"""Preflight the dedicated PostgreSQL quota test database.

The dedicated target itself is decided by `test_pg_target.py`, the pure grammar
for the `host:port/database` an operator confirmed; every name it defines is
re-exported below, so the checked fixtures and any existing import hook resolve
the same symbols with the same messages. This module is only the connecting
half: it never decides policy, and it never migrates, seeds, writes, or
provisions anything. It connects, confirms the connected database's own
identity, runs `SELECT 1`, and confirms the test-required tables exist.

It never prints the URL or its password. The connection goes through the
existing `psql` client with the credentials in `PG*` environment variables,
`.pgpass` disabled, and a bounded connect timeout.
"""

from __future__ import annotations

import os
import subprocess
import sys
import time

# The parser is a sibling module, so resolve it against this file rather than
# the caller's working directory or the launcher's import path.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from test_pg_target import (  # noqa: E402  (needs the path above)
    CONFIRMED_TARGET_KEY,
    DATABASE_CHARS,
    DATABASE_URL_KEY,
    DEFAULT_PORT,
    HOST_CHARS,
    SUPPORTED_SCHEMES,
    Identity,
    PreflightError,
    Target,
    check_charset,
    decode,
    normalize_host,
    parse_confirmed_target,
    parse_database,
    parse_target,
    split_host_port,
)

# Every table the quota integration tests touch. Their absence means the test
# database is not migrated; this reports that instead of running a migration.
REQUIRED_TABLES = (
    "chatgpt_quota_snapshots",
    "endpoint_oauth_tokens",
    "provider_endpoints",
    "request_records",
    "usage_charges",
)

CONNECT_SQL = "SELECT 1"
IDENTITY_SQL = "SELECT current_database()"
SCHEMA_SQL = (
    "SELECT v.name FROM (VALUES "
    + ", ".join(f"('{table}')" for table in REQUIRED_TABLES)
    + ") AS v(name) WHERE to_regclass('public.' || v.name) IS NOT NULL ORDER BY v.name"
)


def run_psql(target: Target, sql: str, connect_timeout: int) -> str:
    """Runs one read-only statement and returns its rows."""
    # A minimal environment: no dotenv value, application URL, or provider key
    # is inherited by the client, and PGPASSFILE=/dev/null disables .pgpass.
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LC_ALL": "C",
        "LANG": "C",
        "PGHOST": target.host,
        "PGPORT": str(target.port),
        "PGDATABASE": target.database,
        "PGUSER": target.user,
        "PGCONNECT_TIMEOUT": str(connect_timeout),
        "PGPASSFILE": os.devnull,
    }
    if target.password:
        env["PGPASSWORD"] = target.password

    # A rejected URL or a mismatched target already failed in the parser.
    command = ["psql", "--no-psqlrc", "--quiet", "--no-align", "--tuples-only",
               "--set=ON_ERROR_STOP=1", "--command", sql]
    try:
        completed = subprocess.run(
            command,
            env=env,
            capture_output=True,
            text=True,
            timeout=connect_timeout * (len(REQUIRED_TABLES) + 2) + 15,
            check=False,
        )
    except FileNotFoundError:
        raise PreflightError("psql is required to verify the dedicated test database") from None
    except subprocess.TimeoutExpired:
        raise PreflightError(
            f"the test database did not answer within {connect_timeout}s per statement"
        ) from None

    if completed.returncode != 0:
        detail = " ".join(completed.stderr.split()) or f"psql exited {completed.returncode}"
        raise PreflightError(f"the test database refused the connection: {redact(detail, target)}")

    return completed.stdout


def redact(message: str, target: Target) -> str:
    """Removes anything credential-shaped from a client message."""
    cleaned = message
    if target.password:
        cleaned = cleaned.replace(target.password, "[redacted]")
    return " ".join(cleaned.split())


def positive_int(env_name: str, fallback: int) -> int:
    raw = os.environ.get(env_name, "")
    return int(raw) if raw.isdigit() and int(raw) > 0 else fallback


def probe(target: Target, attempts: int, interval: int, connect_timeout: int) -> None:
    last_error = ""
    for attempt in range(1, attempts + 1):
        try:
            run_psql(target, CONNECT_SQL, connect_timeout)
            return
        except PreflightError as error:
            last_error = str(error)
        if attempt < attempts:
            time.sleep(interval)

    raise PreflightError(f"the test database at {target.label()} is not usable: {last_error}")


def require_identity(target: Target, connect_timeout: int) -> None:
    """Confirms the connected database is the confirmed one, not a redirect."""
    connected = " ".join(run_psql(target, IDENTITY_SQL, connect_timeout).split())
    if connected != target.database:
        raise PreflightError(f"the connection at {target.host}:{target.port} reports database "
                             f"{connected or '<none>'}, not the confirmed {target.database}")


def require_schema(target: Target, connect_timeout: int) -> None:
    found = {row.strip() for row in run_psql(target, SCHEMA_SQL, connect_timeout).splitlines()
             if row.strip()}
    missing = [table for table in REQUIRED_TABLES if table not in found]
    if missing:
        raise PreflightError(f"the test database at {target.label()} is not migrated; missing "
                             f"tables: {', '.join(missing)}. Apply the repository migrations to "
                             f"{target.database} yourself; this preflight never migrates.")


def main() -> int:
    target = parse_target(os.environ.get(DATABASE_URL_KEY, ""),
                          os.environ.get(CONFIRMED_TARGET_KEY, ""))
    attempts = positive_int("SERVICE_PROBE_ATTEMPTS", 3)
    interval = positive_int("SERVICE_PROBE_INTERVAL", 2)
    connect_timeout = positive_int("PGCONNECT_TIMEOUT", 5)

    probe(target, attempts, interval, connect_timeout)
    require_identity(target, connect_timeout)
    require_schema(target, connect_timeout)

    print(f"service ready: postgresql {target.label()} (quota test schema verified)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PreflightError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
