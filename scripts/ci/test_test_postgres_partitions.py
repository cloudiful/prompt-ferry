#!/usr/bin/env python3
"""Unit checks for the request_records partition guard in require-test-postgres.py.

psql is fully mocked: no process, no network, no database, and no repository
`.env` is read. UTC days are controlled fixtures, including month/year rollover
and leap day. Run with `uv run --no-project python
scripts/ci/test_test_postgres_partitions.py` from the repository root.
"""

from __future__ import annotations

import datetime
import importlib.util
import os
import pathlib
import sys

_HERE = pathlib.Path(os.path.abspath(__file__)).parent
sys.path.insert(0, str(_HERE))

_spec = importlib.util.spec_from_file_location(
    "require_test_postgres_guarded",
    _HERE / "require-test-postgres.py",
)
assert _spec is not None and _spec.loader is not None
guard = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(guard)

PASS = 0
FAIL = 0

FLAG = guard.AUTO_CREATE_PARTITIONS_KEY
TARGET = guard.Target(
    host="pg-test.example.test",
    port=5433,
    database="postgres",
    user="quota_tester",
    password="pg-fixture-secret",
)

DAY = datetime.date(2026, 10, 14)
DAYS = [datetime.date(2026, 10, 13), DAY, datetime.date(2026, 10, 15)]


def check(label: str, ok: bool, detail: str = "") -> None:
    global PASS, FAIL
    if ok:
        PASS += 1
        print(f"ok   {label}")
    else:
        FAIL += 1
        print(f"FAIL {label}")
        if detail:
            print(f"     {detail}")


def with_flag(value: str | None, body) -> None:
    saved = os.environ.get(FLAG)
    try:
        if value is None:
            os.environ.pop(FLAG, None)
        else:
            os.environ[FLAG] = value
        body()
    finally:
        if saved is None:
            os.environ.pop(FLAG, None)
        else:
            os.environ[FLAG] = saved


class FakeDB:
    """Mocked psql surface: controlled UTC day, children, and failure modes."""

    def __init__(self) -> None:
        self.day = DAY
        self.covered: set[str] = {guard.partition_name(day) for day in DAYS}
        self.foreign: dict[str, tuple[bool, str]] = {}
        self.scripts: list[str] = []
        self.fail_script: str | None = None
        self.create_applies = True
        # relkind|strategy|partkeydef|keytype; the default is the exact shape
        # the guard requires, so shape-negative cases set their own.
        self.parent_shape = "p|r|RANGE (created_at)|timestamp with time zone"

    def child_rows(self) -> dict[str, tuple[bool, str]]:
        rows: dict[str, tuple[bool, str]] = {}
        for day in DAYS:
            name = guard.partition_name(day)
            if name in self.covered:
                start, end = guard.day_bounds(day)
                rows[name] = (True, f"FOR VALUES FROM ('{start}') TO ('{end}')")
        rows.update(self.foreign)
        return rows

    def run_psql(self, target, sql: str, timeout: int) -> str:
        if "pg-partition-guard: db-day" in sql:
            return self.day.isoformat() + "\n"
        if "pg-partition-guard: parent" in sql:
            return self.parent_shape + "\n"
        if "pg-partition-guard: children" in sql:
            lines = [f"{name}|{'t' if attached else 'f'}|{bound}"
                     for name, (attached, bound) in sorted(self.child_rows().items())]
            return "\n".join(lines) + ("\n" if lines else "")
        raise AssertionError(f"unexpected readonly query: {sql[:80]}")

    def run_psql_script(self, target, script: str, timeout: int) -> str:
        self.scripts.append(script)
        if self.fail_script is not None:
            # Mirror the real runner: psql stderr is redacted before it becomes
            # the error, so the secret never reaches the caller unredacted.
            raise guard.PreflightError(
                f"the partition backfill failed: {guard.redact(self.fail_script, target)}"
            )
        if self.create_applies:
            for day in DAYS:
                name = guard.partition_name(day)
                if f"'{name}'::timestamptz" in script or f"'{name}', " in script:
                    self.covered.add(name)
        return ""


def install(fake: FakeDB) -> None:
    guard.run_psql = fake.run_psql
    guard.run_psql_script = fake.run_psql_script


ORIGINAL_RUN_PSQL = guard.run_psql
ORIGINAL_RUN_SCRIPT = guard.run_psql_script


def restore() -> None:
    guard.run_psql = ORIGINAL_RUN_PSQL
    guard.run_psql_script = ORIGINAL_RUN_SCRIPT


def expect_coverage_ok(label: str, fake: FakeDB, flag: str | None) -> None:
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check(label, False, f"refused covered dates: {error}")
                return
            check(label, not fake.scripts, f"readonly path ran {len(fake.scripts)} scripts")
        with_flag(flag, body)
    finally:
        restore()


# --- flag grammar ------------------------------------------------------------

def test_missing_flag_stays_readonly() -> None:
    check("a missing flag parses to readonly", guard.parse_auto_create_flag(None) is False, "")


def test_blank_flag_stays_readonly() -> None:
    for raw in ("", "   "):
        try:
            result = guard.parse_auto_create_flag(raw)
        except guard.PreflightError as error:
            check("a blank flag stays readonly", False, f"{raw!r} raised: {error}")
            return
        if result is not False:
            check("a blank flag stays readonly", False, f"{raw!r} parsed true")
            return
    check("a blank flag stays readonly", True, "")


def test_strict_true_false() -> None:
    try:
        ok = guard.parse_auto_create_flag("true") is True
        ok = ok and guard.parse_auto_create_flag("false") is False
    except guard.PreflightError as error:
        check("strict true/false parse", False, f"rejected: {error}")
        return
    check("strict true/false parse", ok, "")


def test_invalid_flag_fails_closed() -> None:
    for raw in ("yes", "1", "TRUE", "on"):
        try:
            guard.parse_auto_create_flag(raw)
        except guard.PreflightError:
            continue
        check("an invalid flag fails closed", False, f"{raw!r} was accepted")
        return
    check("an invalid flag fails closed", True, "")


# --- date window ---------------------------------------------------------------

def test_month_year_rollover() -> None:
    days = guard.coverage_days(datetime.date(2026, 1, 1))
    check("a January day reaches back into December",
          [day.isoformat() for day in days] == ["2025-12-31", "2026-01-01", "2026-01-02"],
          f"got {[str(day) for day in days]}")


def test_leap_day_window() -> None:
    days = guard.coverage_days(datetime.date(2024, 2, 29))
    check("a leap day covers Feb 28 through Mar 1",
          [day.isoformat() for day in days] == ["2024-02-28", "2024-02-29", "2024-03-01"],
          f"got {[str(day) for day in days]}")


def test_names_and_bounds() -> None:
    day = datetime.date(2026, 10, 14)
    check("the child name is parent_YYYYMMDD",
          guard.partition_name(day) == "request_records_20261014", "")
    start, end = guard.day_bounds(day)
    check("bounds are exact UTC day edges",
          (start, end) == ("2026-10-14 00:00:00+00", "2026-10-15 00:00:00+00"),
          f"got {(start, end)}")


# --- readonly coverage -----------------------------------------------------------

def test_covered_dates_run_no_ddl() -> None:
    expect_coverage_ok("covered dates run no DDL without the flag", FakeDB(), None)


def test_covered_dates_ignore_the_flag() -> None:
    expect_coverage_ok("covered dates run no DDL with the flag set", FakeDB(), "true")


def test_missing_coverage_fails_without_ddl_by_default() -> None:
    fake = FakeDB()
    fake.covered = {guard.partition_name(DAYS[0]), guard.partition_name(DAYS[1])}
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                message = str(error)
                check("missing coverage fails before any DDL by default",
                      not fake.scripts and "2026-10-15" in message, message)
                return
            check("missing coverage fails before any DDL by default", False, "was accepted")
        with_flag(None, body)
    finally:
        restore()


def test_foreign_same_named_relation_blocks_even_opt_in() -> None:
    fake = FakeDB()
    fake.covered = set()
    fake.foreign = {guard.partition_name(DAYS[2]): (False, "")}
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("a foreign same-named relation blocks even opt-in",
                      not fake.scripts and guard.partition_name(DAYS[2]) in str(error),
                      str(error))
                return
            check("a foreign same-named relation blocks even opt-in", False, "was accepted")
        with_flag("true", body)
    finally:
        restore()


def test_misbound_partition_blocks_without_modification() -> None:
    fake = FakeDB()
    fake.covered = set()
    fake.foreign = {guard.partition_name(DAYS[1]): (True, "FOR VALUES FROM ('2026-10-14 12:00:00+00')")}
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("a misbound partition fails without modification",
                      not fake.scripts, str(error))
                return
            check("a misbound partition fails without modification", False, "was accepted")
        with_flag("true", body)
    finally:
        restore()


# --- opt-in backfill ---------------------------------------------------------------

def test_opt_in_creates_only_the_missing_child() -> None:
    fake = FakeDB()
    fake.covered = {guard.partition_name(DAYS[0]), guard.partition_name(DAYS[1])}
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("opt-in creates only the missing child", False, f"refused: {error}")
                return
            if len(fake.scripts) != 1:
                check("opt-in creates only the missing child", False,
                      f"ran {len(fake.scripts)} scripts")
                return
            script = fake.scripts[0]
            missing_name = guard.partition_name(DAYS[2])
            creates = [line for line in script.splitlines() if "CREATE TABLE" in line]
            check("opt-in creates only the missing child",
                  len(creates) == 1 and missing_name in creates[0]
                  and "DROP" not in script, script[:400])
        with_flag("true", body)
    finally:
        restore()


def test_backfill_is_one_guarded_transaction() -> None:
    days = [datetime.date(2026, 10, 15)]
    script = guard.build_backfill_script(days)
    upper = script.upper()
    checks = [
        ("a single transaction" , script.count("BEGIN;") == 1 and script.count("COMMIT;") == 1),
        ("UTC pinned", "SET LOCAL TIME ZONE 'UTC'" in script),
        ("bounded lock wait", "SET LOCAL lock_timeout = '5s'" in script),
        ("bounded statement", "SET LOCAL statement_timeout = '30s'" in script),
        ("one advisory xact lock",
         script.count("pg_advisory_xact_lock") == 1
         and str(guard.ADVISORY_LOCK_KEY_1) in script
         and str(guard.ADVISORY_LOCK_KEY_2) in script),
        ("no DROP anywhere", "DROP TABLE" not in upper
         and "DROP PARTITION" not in upper),
        ("no other parent", script.count("PARTITION OF public.request_records") == 1),
        ("verification before commit",
         "verification failed" in script and script.index("verification failed") < script.index("COMMIT;")),
    ]
    bad = [label for label, ok in checks if not ok]
    check("the backfill is one guarded transaction", not bad, f"failed: {bad}")


def test_failed_backfill_rolls_back_loudly() -> None:
    fake = FakeDB()
    fake.covered = {guard.partition_name(DAYS[0])}
    fake.create_applies = False
    fake.fail_script = "simulated psql failure for pg-fixture-secret"
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                message = str(error)
                check("a failed backfill rolls back loudly",
                      "pg-fixture-secret" not in message and "failed" in message, message)
                return
            check("a failed backfill rolls back loudly", False, "was accepted")
        with_flag("true", body)
    finally:
        restore()


def test_concurrent_runners_serialize_on_the_same_lock() -> None:
    first = FakeDB()
    second = FakeDB()
    first.covered = {guard.partition_name(DAYS[0])}
    second.covered = {guard.partition_name(DAYS[0])}
    lock_line = (f"SELECT pg_advisory_xact_lock({guard.ADVISORY_LOCK_KEY_1}, "
                 f"{guard.ADVISORY_LOCK_KEY_2});")
    scripts = []
    for fake in (first, second):
        install(fake)
        try:
            def body() -> None:
                try:
                    guard.require_partition_coverage(TARGET, 5)
                except guard.PreflightError:
                    pass
            with_flag("true", body)
        finally:
            restore()
        scripts.extend(fake.scripts)
    check("concurrent runners serialize on the same lock",
          len(scripts) == 2 and all(lock_line in script for script in scripts),
          f"ran {len(scripts)} scripts")


def test_redaction_holds_on_backfill_errors() -> None:
    try:
        raise guard.PreflightError("boom for pg-fixture-secret user")
    except guard.PreflightError as error:
        cleaned = guard.redact(str(error), TARGET)
    check("backfill errors never leak the password",
          "pg-fixture-secret" not in cleaned and "[redacted]" in cleaned, cleaned)


def expect_parent_refused(label: str, parent_shape: str,
                          fragment: str = "unexpected shape") -> None:
    """A wrong-shaped parent refuses with and without the flag, running no DDL."""
    for flag in (None, "true"):
        fake = FakeDB()
        fake.covered = set()
        fake.parent_shape = parent_shape
        install(fake)
        try:
            def body() -> None:
                try:
                    guard.require_partition_coverage(TARGET, 5)
                except guard.PreflightError as error:
                    message = str(error)
                    check(label,
                          not fake.scripts and fragment in message, message)
                    return
                check(label, False, f"wrong-shaped parent was accepted (flag={flag!r})")
            with_flag(flag, body)
        finally:
            restore()


def test_list_partitioned_parent_is_refused() -> None:
    expect_parent_refused("a LIST-partitioned parent is refused",
                          "p|l|LIST (created_at)|timestamp with time zone")


def test_wrong_key_parent_is_refused() -> None:
    expect_parent_refused("a parent on another timestamp column is refused",
                          "p|r|RANGE (observed_at)|timestamp with time zone")


def test_plain_table_parent_is_refused() -> None:
    expect_parent_refused("a non-partitioned table is refused",
                          "r|||timestamp with time zone")


def test_wrong_key_type_parent_is_refused() -> None:
    expect_parent_refused("a parent on a non-timestamptz key is refused",
                          "p|r|RANGE (created_at)|timestamp without time zone")


def test_unreadable_parent_is_refused() -> None:
    expect_parent_refused("an unreadable parent catalog is refused", "p",
                          "unreadable partition parent")


def test_transaction_rechecks_parent_shape_before_any_create() -> None:
    days = [datetime.date(2026, 10, 15)]
    script = guard.build_backfill_script(days)
    lock_at = script.index("pg_advisory_xact_lock")
    strategy_at = script.index("pg_partitioned_table")
    keydef_at = script.index("pg_get_partkeydef(parent_oid)")
    keytype_at = script.index("format_type(a.atttypid")
    create_at = script.index("CREATE TABLE %I")
    checks = [
        ("strategy recheck", lock_at < strategy_at < create_at
         and "is not RANGE-partitioned" in script),
        ("key recheck", lock_at < keydef_at < create_at
         and "is not partitioned on created_at" in script),
        ("key-type recheck", lock_at < keytype_at < create_at
         and "has an unexpected type" in script),
        ("no relkind-only shortcut", script.count("relkind") == 1),
    ]
    bad = [label for label, ok in checks if not ok]
    check("the transaction rechecks the parent shape before any create", not bad,
          f"failed: {bad}")


def main() -> int:
    test_missing_flag_stays_readonly()
    test_blank_flag_stays_readonly()
    test_strict_true_false()
    test_invalid_flag_fails_closed()
    test_month_year_rollover()
    test_leap_day_window()
    test_names_and_bounds()
    test_covered_dates_run_no_ddl()
    test_covered_dates_ignore_the_flag()
    test_missing_coverage_fails_without_ddl_by_default()
    test_foreign_same_named_relation_blocks_even_opt_in()
    test_misbound_partition_blocks_without_modification()
    test_opt_in_creates_only_the_missing_child()
    test_backfill_is_one_guarded_transaction()
    test_list_partitioned_parent_is_refused()
    test_wrong_key_parent_is_refused()
    test_plain_table_parent_is_refused()
    test_wrong_key_type_parent_is_refused()
    test_unreadable_parent_is_refused()
    test_transaction_rechecks_parent_shape_before_any_create()
    test_failed_backfill_rolls_back_loudly()
    test_concurrent_runners_serialize_on_the_same_lock()
    test_redaction_holds_on_backfill_errors()
    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    raise SystemExit(main())
