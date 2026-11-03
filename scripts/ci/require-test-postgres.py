#!/usr/bin/env python3
"""Preflight the dedicated PostgreSQL quota test database.

The dedicated target itself is decided by `test_pg_target.py`, the pure grammar
for the `host:port/database` an operator confirmed; every name it defines is
re-exported below, so the checked fixtures and any existing import hook resolve
the same symbols with the same messages. This module is only the connecting
half: it never decides policy, and it never migrates, seeds, or provisions
anything else. It connects, confirms the connected database's own identity,
runs `SELECT 1`, confirms the test-required tables exist, and verifies the
daily-partition coverage of every managed request-record parent.

Coverage checks are read-only by default: a missing UTC yesterday/today/
tomorrow child fails before cargo with a safe parent/date diagnostic and no
DDL. The single bounded exception is
`PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS` set to exactly `true`, which
authorizes creating only those missing daily children in one guarded
transaction — never drops, never other tables, never a parent that already has
a DEFAULT child. `request_record_raw_payloads` is deliberately not managed
here: it owns a DEFAULT child and moving its rows is out of scope.

It never prints the URL or its password. The connection goes through the
existing `psql` client with the credentials in `PG*` environment variables,
`.pgpass` disabled, and a bounded connect timeout.
"""

from __future__ import annotations

import datetime
import os
import re
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
    "quota_key_observations",
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

# Opt-in flag for bounded test-partition creation. Missing, blank, or `false`
# stays read-only; any other non-`true` value fails closed.
AUTO_CREATE_PARTITIONS_KEY = "PROMPT_FERRY_QUOTA_TEST_AUTO_CREATE_PARTITIONS"

# The managed parents, metadata first, then the product content family minus
# `request_record_raw_payloads`. Raw payloads have a DEFAULT child and their
# rows would need moving, so the guard never creates daily ranges for them.
# Daily UTC children are named `<parent>_<YYYYMMDD>` with exact
# `[day 00:00+00, day+1 00:00+00)` bounds.
PARTITION_PARENTS = (
    "request_records",
    "request_record_content",
    "request_record_block_refs",
    "usage_prompt_blocks",
    "request_record_assistant_artifacts",
    "request_record_tool_calls",
    "request_record_replay_snapshots",
)
PARTITION_SCHEMA = "public"
DEFAULT_CHILD_BOUND = "DEFAULT"

# Advisory-lock namespace for the backfill transaction: the first 8 bytes of
# sha256("prompt-ferry/test-partition-guard/v1") as two signed int32s. The
# xact-scoped lock serializes concurrent backfills and releases on COMMIT or
# ROLLBACK by itself; it never touches foreign processes.
ADVISORY_LOCK_KEY_1 = -731166763
ADVISORY_LOCK_KEY_2 = -509159360


def run_psql(target: Target, sql: str, connect_timeout: int) -> str:
    """Runs one read-only statement and returns its rows.

    Bounded opt-in DDL never flows through here; it goes through
    run_psql_script only, inside one guarded transaction.
    """
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


def parse_auto_create_flag(raw: str | None) -> bool:
    """Parses the strict opt-in flag; missing or blank stays read-only."""
    text = (raw or "").strip()
    if text == "":
        return False
    if text == "true":
        return True
    if text == "false":
        return False
    raise PreflightError(f"{AUTO_CREATE_PARTITIONS_KEY} must read exactly true or false")


def coverage_days(db_day: datetime.date) -> list[datetime.date]:
    """The bounded UTC window every run needs: yesterday, today, tomorrow."""
    return [db_day + datetime.timedelta(days=offset) for offset in (-1, 0, 1)]


def partition_name(parent: str, day: datetime.date) -> str:
    return f"{parent}_{day.strftime('%Y%m%d')}"


def day_bounds(day: datetime.date) -> tuple[str, str]:
    start = day.strftime("%Y-%m-%d 00:00:00+00")
    end = (day + datetime.timedelta(days=1)).strftime("%Y-%m-%d 00:00:00+00")
    return start, end


def canonical_bound(start: str, end: str) -> str:
    """The exact bound text our DDL produces, whitespace removed."""
    return re.sub(r"\s+", "", f"FOR VALUES FROM ('{start}') TO ('{end}')")


def run_psql_script(target: Target, script: str, timeout: int) -> str:
    """Runs a multi-statement script on one psql session and returns stdout."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LC_ALL": "C",
        "LANG": "C",
        "PGHOST": target.host,
        "PGPORT": str(target.port),
        "PGDATABASE": target.database,
        "PGUSER": target.user,
        "PGCONNECT_TIMEOUT": "5",
        "PGPASSFILE": os.devnull,
    }
    if target.password:
        env["PGPASSWORD"] = target.password

    command = ["psql", "--no-psqlrc", "--quiet", "--no-align", "--tuples-only",
               "--set=ON_ERROR_STOP=1", "--command", script]
    try:
        completed = subprocess.run(
            command,
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError:
        raise PreflightError("psql is required to verify the dedicated test database") from None
    except subprocess.TimeoutExpired:
        raise PreflightError("the test database did not answer the partition script in time") from None

    if completed.returncode != 0:
        detail = " ".join(completed.stderr.split()) or f"psql exited {completed.returncode}"
        raise PreflightError(f"the partition backfill failed: {redact(detail, target)}")

    return completed.stdout


def fetch_db_utc_day(target: Target, connect_timeout: int) -> datetime.date:
    """The database server's own UTC day: inserts land on its clock, not ours."""
    raw = " ".join(
        run_psql(target,
                 "SELECT (now() AT TIME ZONE 'UTC')::date -- pg-partition-guard: db-day",
                 connect_timeout).split()
    )
    try:
        return datetime.date.fromisoformat(raw)
    except ValueError:
        raise PreflightError("the test database returned an unreadable UTC day") from None


# The exact parent shape this guard understands: RANGE-partitioned on the
# `created_at` timestamptz column with no DEFAULT child. The read-only check
# and the under-lock recheck both require every fact, so a same-named daily
# layout on a different key or strategy is refused instead of accepted.
EXPECTED_PARTITION_STRATEGY = "r"
EXPECTED_PARTITION_KEY = "created_at"
EXPECTED_PARTITION_KEY_TYPE = "timestamp with time zone"


def expected_partkeydef() -> str:
    """The canonical `pg_get_partkeydef` text for our parents, whitespace removed."""
    return re.sub(r"\s+", "", f"RANGE ({EXPECTED_PARTITION_KEY})")


def require_partition_parents(target: Target, connect_timeout: int) -> None:
    """Fails closed unless every managed parent is RANGE(created_at timestamptz).

    A managed parent that unexpectedly owns a DEFAULT child is refused as well:
    daily ranges cannot be created under it without moving rows, which this
    guard must never do.
    """
    values = ", ".join(f"('{parent}')" for parent in PARTITION_PARENTS)
    raw = run_psql(
        target,
        "SELECT v.name, COALESCE(c.relkind::text, ''), "
        "COALESCE(t.partstrat::text, ''), COALESCE(pg_get_partkeydef(c.oid), ''), "
        "COALESCE(format_type(a.atttypid, a.atttypmod), ''), "
        "COALESCE((SELECT count(*) FROM pg_inherits i JOIN pg_class ch "
        "ON ch.oid = i.inhrelid WHERE i.inhparent = c.oid "
        f"AND pg_get_expr(ch.relpartbound, ch.oid) = '{DEFAULT_CHILD_BOUND}'), 0) "
        f"FROM (VALUES {values}) AS v(name) "
        "LEFT JOIN pg_class c ON c.relname = v.name "
        f"AND c.relnamespace = '{PARTITION_SCHEMA}'::regnamespace "
        "LEFT JOIN pg_partitioned_table t ON t.partrelid = c.oid "
        f"LEFT JOIN pg_attribute a ON a.attrelid = c.oid "
        f"AND a.attname = '{EXPECTED_PARTITION_KEY}' AND NOT a.attisdropped "
        "ORDER BY v.name -- pg-partition-guard: parents",
        connect_timeout,
    )
    seen: dict[str, tuple[str, str, str, str, int]] = {}
    for line in raw.splitlines():
        if not line.strip():
            continue
        fields = [field.strip() for field in line.split("|")]
        if len(fields) != 6:
            raise PreflightError("the test database returned an unreadable partition parent")
        name, relkind, strategy, keydef, keytype, default_children = fields
        seen[name] = (relkind, strategy, keydef, keytype, int(default_children or 0))
    missing = [parent for parent in PARTITION_PARENTS if parent not in seen]
    if missing:
        raise PreflightError(f"the test database at {target.label()} is missing partition "
                             f"parents: {', '.join(missing)}. Apply the repository migrations "
                             f"to {target.database} yourself; this preflight never migrates.")
    for parent in PARTITION_PARENTS:
        relkind, strategy, keydef, keytype, default_children = seen[parent]
        shape_ok = (
            relkind == "p"
            and strategy == EXPECTED_PARTITION_STRATEGY
            and re.sub(r"\s+", "", keydef).lower() == expected_partkeydef().lower()
            and keytype == EXPECTED_PARTITION_KEY_TYPE
        )
        if not shape_ok:
            raise PreflightError(
                f"public.{parent} at {target.label()} is not the expected "
                f"RANGE({EXPECTED_PARTITION_KEY} {EXPECTED_PARTITION_KEY_TYPE}) parent; "
                "refusing to check coverage or create children under an unexpected shape")
        if default_children:
            raise PreflightError(
                f"public.{parent} at {target.label()} already has a DEFAULT child; daily "
                "ranges cannot be added without moving rows, which this preflight never does")


def fetch_children(target: Target, connect_timeout: int) -> dict[str, dict[str, tuple[bool, str]]]:
    """Maps each managed parent to its daily children: {child: (attached, bound)}."""
    values = ", ".join(f"('{parent}')" for parent in PARTITION_PARENTS)
    raw = run_psql(
        target,
        "SELECT p.relname, c.relname, (i.inhrelid IS NOT NULL), "
        "COALESCE(pg_get_expr(c.relpartbound, c.oid), '') "
        f"FROM (VALUES {values}) AS v(name) "
        "JOIN pg_class p ON p.relname = v.name "
        f"AND p.relnamespace = '{PARTITION_SCHEMA}'::regnamespace "
        "JOIN pg_class c ON c.relnamespace = p.relnamespace "
        "AND c.relname ~ ('^' || v.name || '_[0-9]{8}$') "
        "LEFT JOIN pg_inherits i ON i.inhrelid = c.oid AND i.inhparent = p.oid "
        "ORDER BY p.relname, c.relname -- pg-partition-guard: children",
        connect_timeout,
    )
    children: dict[str, dict[str, tuple[bool, str]]] = {
        parent: {} for parent in PARTITION_PARENTS
    }
    for line in raw.splitlines():
        if not line.strip():
            continue
        parts = line.split("|", 3)
        if len(parts) != 4:
            raise PreflightError("the test database returned an unreadable partition catalog")
        parent, name, attached, bound = (part.strip() for part in parts)
        children.setdefault(parent, {})[name] = (attached == "t", bound)
    return children


def check_coverage(children: dict[str, dict[str, tuple[bool, str]]],
                   days: list[datetime.date]) -> tuple[list[tuple[str, datetime.date]], list[str]]:
    """Splits the wanted parent+day pairs into missing children and conflicts."""
    missing: list[tuple[str, datetime.date]] = []
    conflicts: list[str] = []
    for parent in PARTITION_PARENTS:
        for day in days:
            name = partition_name(parent, day)
            start, end = day_bounds(day)
            want = canonical_bound(start, end)
            entry = children.get(parent, {}).get(name)
            if entry is None:
                missing.append((parent, day))
                continue
            attached, bound = entry
            if not attached or re.sub(r"\s+", "", bound) != want:
                conflicts.append(name)
    return missing, conflicts


def build_backfill_script(missing: list[tuple[str, datetime.date]]) -> str:
    """One bounded transaction: lock, recheck, create only the missing, verify."""
    rows = []
    for parent, day in missing:
        name = partition_name(parent, day)
        start_text, end_text = day_bounds(day)
        rows.append(f"('{parent}', '{name}', '{start_text}', '{end_text}')")
    values = ("SELECT * FROM (VALUES\n      " + ",\n      ".join(rows)
              + "\n    ) AS v(parent_name, child_name, start_text, end_text)")
    return "\n".join([
        "BEGIN;",
        "SET LOCAL TIME ZONE 'UTC';",
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '60s';",
        f"SELECT pg_advisory_xact_lock({ADVISORY_LOCK_KEY_1}, {ADVISORY_LOCK_KEY_2});",
        "DO $request_records_partition_guard$",
        "DECLARE",
        "  entry record;",
        "  parent_oid oid;",
        "  parent_strategy text;",
        "  parent_keydef text;",
        "  parent_keytype text;",
        "  default_children int;",
        "  child_oid oid;",
        "  bound_text text;",
        "  want_bound text;",
        "BEGIN",
        "  FOR entry IN",
        f"    {values}",
        "  LOOP",
        "    SELECT c.oid INTO parent_oid FROM pg_class c",
        "    JOIN pg_namespace n ON n.oid = c.relnamespace",
        f"    WHERE n.nspname = '{PARTITION_SCHEMA}'",
        "      AND c.relname = entry.parent_name AND c.relkind = 'p';",
        "    IF NOT FOUND THEN",
        "      RAISE EXCEPTION 'public.% is not a partitioned parent', entry.parent_name;",
        "    END IF;",
        "    SELECT t.partstrat::text INTO parent_strategy FROM pg_partitioned_table t",
        "    WHERE t.partrelid = parent_oid;",
        f"    IF NOT FOUND OR parent_strategy <> '{EXPECTED_PARTITION_STRATEGY}' THEN",
        "      RAISE EXCEPTION 'public.% is not RANGE-partitioned', entry.parent_name;",
        "    END IF;",
        "    SELECT pg_get_partkeydef(parent_oid) INTO parent_keydef;",
        "    IF lower(regexp_replace(parent_keydef, '\\s+', '', 'g')) <>",
        f"      '{expected_partkeydef().lower()}' THEN",
        f"      RAISE EXCEPTION 'public.% is not partitioned on {EXPECTED_PARTITION_KEY}',",
        "        entry.parent_name;",
        "    END IF;",
        "    SELECT format_type(a.atttypid, a.atttypmod) INTO parent_keytype",
        "    FROM pg_attribute a WHERE a.attrelid = parent_oid",
        f"      AND a.attname = '{EXPECTED_PARTITION_KEY}' AND NOT a.attisdropped;",
        f"    IF NOT FOUND OR parent_keytype <> '{EXPECTED_PARTITION_KEY_TYPE}' THEN",
        "      RAISE EXCEPTION 'public.% has an unexpected partition key type',",
        "        entry.parent_name;",
        "    END IF;",
        "    SELECT count(*) INTO default_children FROM pg_inherits i",
        "    JOIN pg_class ch ON ch.oid = i.inhrelid WHERE i.inhparent = parent_oid",
        f"      AND pg_get_expr(ch.relpartbound, ch.oid) = '{DEFAULT_CHILD_BOUND}';",
        "    IF default_children > 0 THEN",
        "      RAISE EXCEPTION 'public.% has a DEFAULT child; refusing to create daily ranges',",
        "        entry.parent_name;",
        "    END IF;",
        "    SELECT c.oid INTO child_oid FROM pg_class c",
        "    JOIN pg_namespace n ON n.oid = c.relnamespace",
        f"    WHERE n.nspname = '{PARTITION_SCHEMA}'",
        "      AND c.relname = entry.child_name;",
        "    IF FOUND THEN",
        "      IF NOT EXISTS (SELECT 1 FROM pg_inherits",
        "        WHERE inhrelid = child_oid AND inhparent = parent_oid) THEN",
        "        RAISE EXCEPTION 'conflicting relation % is not a partition of public.%',",
        "          entry.child_name, entry.parent_name;",
        "      END IF;",
        "      SELECT pg_get_expr(c.relpartbound, c.oid) INTO bound_text",
        "      FROM pg_class c WHERE c.oid = child_oid;",
        "      SELECT format('FOR VALUES FROM (%L) TO (%L)',",
        "        entry.start_text::timestamptz, entry.end_text::timestamptz)",
        "      INTO want_bound;",
        "      IF regexp_replace(bound_text, '\\s+', '', 'g') <>",
        "        regexp_replace(want_bound, '\\s+', '', 'g') THEN",
        "        RAISE EXCEPTION 'partition % has unexpected bounds', entry.child_name;",
        "      END IF;",
        "    ELSE",
        "      EXECUTE format('CREATE TABLE %I PARTITION OF public.%I",
        "        FOR VALUES FROM (%L) TO (%L)',",
        "        entry.child_name, entry.parent_name,",
        "        entry.start_text::timestamptz, entry.end_text::timestamptz);",
        "    END IF;",
        "  END LOOP;",
        "  FOR entry IN",
        f"    {values}",
        "  LOOP",
        "    SELECT format('FOR VALUES FROM (%L) TO (%L)',",
        "      entry.start_text::timestamptz, entry.end_text::timestamptz)",
        "    INTO want_bound;",
        "    IF NOT EXISTS (SELECT 1 FROM pg_class c",
        "      JOIN pg_namespace n ON n.oid = c.relnamespace",
        "      JOIN pg_inherits i ON i.inhrelid = c.oid",
        "      JOIN pg_class p ON p.oid = i.inhparent",
        f"      WHERE n.nspname = '{PARTITION_SCHEMA}'",
        "        AND c.relname = entry.child_name",
        "        AND p.relname = entry.parent_name",
        "        AND regexp_replace(pg_get_expr(c.relpartbound, c.oid), '\\s+', '', 'g') =",
        "          regexp_replace(want_bound, '\\s+', '', 'g')) THEN",
        "      RAISE EXCEPTION 'partition backfill verification failed for %',",
        "        entry.child_name;",
        "    END IF;",
        "  END LOOP;",
        "END $request_records_partition_guard$;",
        "COMMIT;",
    ]) + "\n"


def require_partition_coverage(target: Target, connect_timeout: int) -> None:
    """Ensures every managed parent covers UTC yesterday/today/tomorrow."""
    auto_create = parse_auto_create_flag(os.environ.get(AUTO_CREATE_PARTITIONS_KEY))
    db_day = fetch_db_utc_day(target, connect_timeout)
    days = coverage_days(db_day)
    require_partition_parents(target, connect_timeout)
    missing, conflicts = check_coverage(fetch_children(target, connect_timeout), days)
    if conflicts:
        raise PreflightError(f"unexpected relations block the test partitions at "
                             f"{target.label()}: {', '.join(conflicts)}. Rename or remove them "
                             "yourself; this preflight never drops or recreates partitions")
    span = f"{days[0].isoformat()}..{days[-1].isoformat()}"
    if not missing:
        print(f"request-record partitions cover UTC {span} for "
              f"{len(PARTITION_PARENTS)} managed parents")
        return
    wanted = ", ".join(partition_name(parent, day) for parent, day in missing)
    wanted_days = ", ".join(sorted({day.isoformat() for _, day in missing}))
    if not auto_create:
        raise PreflightError(
            f"the test database at {target.label()} is missing request-record partitions for "
            f"UTC dates: {wanted_days} ({wanted}). Rerun with "
            f"{AUTO_CREATE_PARTITIONS_KEY}=true to create exactly those bounded daily "
            "children, then rerun the suite")
    run_psql_script(target, build_backfill_script(missing), timeout=120)
    missing_after, conflicts_after = check_coverage(
        fetch_children(target, connect_timeout), days)
    if conflicts_after or missing_after:
        left = ", ".join(partition_name(parent, day) for parent, day in missing_after) or \
            ", ".join(conflicts_after)
        raise PreflightError(f"the partition backfill did not converge at {target.label()}; "
                             f"still unresolved: {left}")
    print(f"request-record partitions created for UTC {span}: {wanted}")


def main() -> int:
    target = parse_target(os.environ.get(DATABASE_URL_KEY, ""),
                          os.environ.get(CONFIRMED_TARGET_KEY, ""))
    attempts = positive_int("SERVICE_PROBE_ATTEMPTS", 3)
    interval = positive_int("SERVICE_PROBE_INTERVAL", 2)
    connect_timeout = positive_int("PGCONNECT_TIMEOUT", 5)

    probe(target, attempts, interval, connect_timeout)
    require_identity(target, connect_timeout)
    require_schema(target, connect_timeout)
    require_partition_coverage(target, connect_timeout)

    print(f"service ready: postgresql {target.label()} (quota test schema verified)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PreflightError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from None
