#!/usr/bin/env python3
"""Unit checks for the request-record partition guard in require-test-postgres.py.

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
import re
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

EXPECTED_PARENTS = (
    "request_records",
    "request_record_content",
    "request_record_block_refs",
    "usage_prompt_blocks",
    "request_record_assistant_artifacts",
    "request_record_tool_calls",
    "request_record_replay_snapshots",
)
DEFAULT_PARENT_SHAPE = "p|r|RANGE (created_at)|timestamp with time zone|0"


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


ROW = re.compile(r"\('([a-z_]+)', '([a-z_0-9]+)', '")


def script_pairs(script: str) -> set[tuple[str, str]]:
    """The unique (parent, child) pairs a bounded script can create."""
    return set(ROW.findall(script))


def covered_tomorrow(*parents: str) -> dict[str, set[str]]:
    covered = {parent: set() for parent in EXPECTED_PARENTS}
    for parent in EXPECTED_PARENTS:
        for day in DAYS[:2]:
            covered[parent].add(guard.partition_name(parent, day))
    for parent in parents:
        covered[parent].add(guard.partition_name(parent, DAYS[2]))
    return covered


class FakeDB:
    """Mocked psql surface: controlled UTC day, parents, children, failures."""

    def __init__(self) -> None:
        self.day = DAY
        self.covered: dict[str, set[str]] = covered_tomorrow(*EXPECTED_PARENTS)
        self.foreign: dict[str, dict[str, tuple[bool, str]]] = {
            parent: {} for parent in EXPECTED_PARENTS
        }
        self.parent_shapes: dict[str, str] = {
            parent: DEFAULT_PARENT_SHAPE for parent in EXPECTED_PARENTS
        }
        self.missing_parents: set[str] = set()
        self.scripts: list[str] = []
        self.fail_script: str | None = None
        self.create_applies = True

    def parent_rows(self) -> str:
        lines = []
        for parent in sorted(EXPECTED_PARENTS):
            if parent in self.missing_parents:
                continue
            lines.append(f"{parent}|{self.parent_shapes[parent]}")
        return "\n".join(lines) + ("\n" if lines else "")

    def child_rows(self) -> str:
        entries: list[tuple[str, str, bool, str]] = []
        for parent in EXPECTED_PARENTS:
            for day in DAYS:
                name = guard.partition_name(parent, day)
                if name in self.covered[parent]:
                    start, end = guard.day_bounds(day)
                    entries.append(
                        (parent, name, True,
                         f"FOR VALUES FROM ('{start}') TO ('{end}')")
                    )
            for name, (attached, bound) in self.foreign[parent].items():
                entries.append((parent, name, attached, bound))
        lines = [f"{parent}|{name}|{'t' if attached else 'f'}|{bound}"
                 for parent, name, attached, bound in sorted(entries)]
        return "\n".join(lines) + ("\n" if lines else "")

    def run_psql(self, target, sql: str, timeout: int) -> str:
        if "pg-partition-guard: db-day" in sql:
            return self.day.isoformat() + "\n"
        if "pg-partition-guard: parents" in sql:
            return self.parent_rows()
        if "pg-partition-guard: children" in sql:
            return self.child_rows()
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
            for parent in EXPECTED_PARENTS:
                for day in DAYS:
                    name = guard.partition_name(parent, day)
                    if f"'{parent}', '{name}'" in script:
                        self.covered[parent].add(name)
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


# --- managed parents -------------------------------------------------------------

def test_managed_parents_are_exact() -> None:
    check("the guard manages exactly the seven parent tables",
          guard.PARTITION_PARENTS == EXPECTED_PARENTS,
          f"got {guard.PARTITION_PARENTS}")
    check("raw payloads are never managed",
          "request_record_raw_payloads" not in guard.PARTITION_PARENTS, "")
    check("the metadata parent stays first",
          guard.PARTITION_PARENTS[0] == "request_records", "")


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
    check("a content child name is parent_YYYYMMDD",
          guard.partition_name("request_record_content", day)
          == "request_record_content_20261014", "")
    check("the metadata child name is unchanged",
          guard.partition_name("request_records", day) == "request_records_20261014", "")
    start, end = guard.day_bounds(day)
    check("bounds are exact UTC day edges",
          (start, end) == ("2026-10-14 00:00:00+00", "2026-10-15 00:00:00+00"),
          f"got {(start, end)}")


# --- readonly coverage -----------------------------------------------------------

def test_covered_dates_run_no_ddl() -> None:
    expect_coverage_ok("covered dates run no DDL without the flag", FakeDB(), None)


def test_covered_dates_ignore_the_flag() -> None:
    expect_coverage_ok("covered dates run no DDL with the flag set", FakeDB(), "true")


def test_missing_content_coverage_fails_without_ddl_by_default() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow("request_records")
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                message = str(error)
                check("a missing content child fails before any DDL by default",
                      not fake.scripts
                      and "request_record_content_20261015" in message
                      and "2026-10-15" in message, message)
                return
            check("a missing content child fails before any DDL by default", False,
                  "was accepted")
        with_flag(None, body)
    finally:
        restore()


def test_foreign_same_named_relation_blocks_even_opt_in() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow(*EXPECTED_PARENTS)
    fake.covered["request_record_content"].discard("request_record_content_20261015")
    fake.foreign["request_record_content"]["request_record_content_20261015"] = (False, "")
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("a foreign same-named content relation blocks even opt-in",
                      not fake.scripts
                      and "request_record_content_20261015" in str(error),
                      str(error))
                return
            check("a foreign same-named content relation blocks even opt-in", False,
                  "was accepted")
        with_flag("true", body)
    finally:
        restore()


def test_misbound_partition_blocks_without_modification() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow(*EXPECTED_PARENTS)
    fake.covered["usage_prompt_blocks"].discard("usage_prompt_blocks_20261014")
    fake.foreign["usage_prompt_blocks"]["usage_prompt_blocks_20261014"] = (
        True, "FOR VALUES FROM ('2026-10-14 12:00:00+00')")
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


def test_relation_of_another_parent_is_not_coverage() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow(*EXPECTED_PARENTS)
    # The metadata child exists but the content child with the same day does
    # not: coverage must be checked per parent, never by name alone.
    fake.covered["request_record_content"].discard("request_record_content_20261015")
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("one parent's child is not another parent's coverage",
                      not fake.scripts
                      and "request_record_content_20261015" in str(error),
                      str(error))
                return
            check("one parent's child is not another parent's coverage", False,
                  "was accepted")
        with_flag(None, body)
    finally:
        restore()


# --- opt-in backfill ---------------------------------------------------------------

def test_opt_in_creates_only_the_missing_children() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow("request_records", "request_record_content")
    install(fake)
    try:
        def body() -> None:
            try:
                guard.require_partition_coverage(TARGET, 5)
            except guard.PreflightError as error:
                check("opt-in creates only the missing parent+day children", False,
                      f"refused: {error}")
                return
            if len(fake.scripts) != 1:
                check("opt-in creates only the missing parent+day children", False,
                      f"ran {len(fake.scripts)} scripts")
                return
            script = fake.scripts[0]
            expected = {
                (parent, guard.partition_name(parent, DAYS[2]))
                for parent in EXPECTED_PARENTS
                if parent not in ("request_records", "request_record_content")
            }
            check("opt-in creates only the missing parent+day children",
                  script.count("CREATE TABLE %I PARTITION OF") == 1
                  and script_pairs(script) == expected
                  and "request_records_20261013" not in script
                  and "DROP" not in script,
                  f"pairs={sorted(script_pairs(script))}")
        with_flag("true", body)
    finally:
        restore()


def test_backfill_is_one_guarded_transaction() -> None:
    missing = [(parent, DAYS[2]) for parent in EXPECTED_PARENTS]
    script = guard.build_backfill_script(missing)
    upper = script.upper()
    checks = [
        ("a single transaction", script.count("BEGIN;") == 1 and script.count("COMMIT;") == 1),
        ("UTC pinned", "SET LOCAL TIME ZONE 'UTC'" in script),
        ("bounded lock wait", "SET LOCAL lock_timeout = '5s'" in script),
        ("bounded statement", "SET LOCAL statement_timeout = '60s'" in script),
        ("one advisory xact lock",
         script.count("pg_advisory_xact_lock") == 1
         and str(guard.ADVISORY_LOCK_KEY_1) in script
         and str(guard.ADVISORY_LOCK_KEY_2) in script),
        ("no DROP anywhere", "DROP TABLE" not in upper
         and "DROP PARTITION" not in upper),
        ("one bounded create template",
         script.count("CREATE TABLE %I PARTITION OF") == 1),
        ("every managed parent is named",
         script_pairs(script)
         == {(parent, guard.partition_name(parent, DAYS[2]))
             for parent in EXPECTED_PARENTS}),
        ("raw payloads stay untouched", "request_record_raw_payloads" not in script),
        ("verification before commit",
         "verification failed" in script
         and script.index("verification failed") < script.index("COMMIT;")),
    ]
    bad = [label for label, ok in checks if not ok]
    check("the backfill is one guarded transaction", not bad, f"failed: {bad}")


def test_failed_backfill_rolls_back_loudly() -> None:
    fake = FakeDB()
    fake.covered = covered_tomorrow("request_records")
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
    first.covered = covered_tomorrow("request_records")
    second.covered = covered_tomorrow("request_records")
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


# --- parent shape / DEFAULT child gates ------------------------------------------

def expect_guard_refused(label: str, mutate, fragment: str) -> None:
    """A bad parent refuses with and without the flag, running no DDL."""
    for flag in (None, "true"):
        fake = FakeDB()
        fake.covered = covered_tomorrow("request_records")
        mutate(fake)
        install(fake)
        try:
            def body() -> None:
                try:
                    guard.require_partition_coverage(TARGET, 5)
                except guard.PreflightError as error:
                    message = str(error)
                    check(label, not fake.scripts and fragment in message, message)
                    return
                check(label, False, f"bad parent was accepted (flag={flag!r})")
            with_flag(flag, body)
        finally:
            restore()


def test_list_partitioned_parent_is_refused() -> None:
    expect_guard_refused(
        "a LIST-partitioned parent is refused",
        lambda fake: fake.parent_shapes.update(
            {"request_record_content": "p|l|LIST (created_at)|timestamp with time zone|0"}),
        "unexpected shape")


def test_wrong_key_parent_is_refused() -> None:
    expect_guard_refused(
        "a parent on another column is refused",
        lambda fake: fake.parent_shapes.update(
            {"usage_prompt_blocks": "p|r|RANGE (observed_at)|timestamp with time zone|0"}),
        "unexpected shape")


def test_plain_table_parent_is_refused() -> None:
    expect_guard_refused(
        "a non-partitioned table is refused",
        lambda fake: fake.parent_shapes.update(
            {"request_records": "r|||timestamp with time zone|0"}),
        "unexpected shape")


def test_wrong_key_type_parent_is_refused() -> None:
    expect_guard_refused(
        "a parent on a non-timestamptz key is refused",
        lambda fake: fake.parent_shapes.update(
            {"request_record_tool_calls":
             "p|r|RANGE (created_at)|timestamp without time zone|0"}),
        "unexpected shape")


def test_default_child_parent_is_refused() -> None:
    expect_guard_refused(
        "a parent with a DEFAULT child is refused before any DDL",
        lambda fake: fake.parent_shapes.update(
            {"request_record_assistant_artifacts":
             "p|r|RANGE (created_at)|timestamp with time zone|1"}),
        "DEFAULT child")


def test_unreadable_parent_is_refused() -> None:
    expect_guard_refused(
        "an unreadable parent catalog is refused",
        lambda fake: fake.parent_shapes.update({"request_record_block_refs": "p"}),
        "unreadable partition parent")


def test_missing_parent_is_refused() -> None:
    expect_guard_refused(
        "an unmigrated managed parent is refused",
        lambda fake: fake.missing_parents.add("request_record_replay_snapshots"),
        "missing partition parents")


def test_transaction_rechecks_parent_shape_before_any_create() -> None:
    script = guard.build_backfill_script([("request_record_content", DAYS[2])])
    lock_at = script.index("pg_advisory_xact_lock")
    strategy_at = script.index("pg_partitioned_table")
    keydef_at = script.index("pg_get_partkeydef(parent_oid)")
    keytype_at = script.index("format_type(a.atttypid")
    default_at = script.index("has a DEFAULT child")
    create_at = script.index("CREATE TABLE %I")
    checks = [
        ("strategy recheck", lock_at < strategy_at < create_at
         and "is not RANGE-partitioned" in script),
        ("key recheck", lock_at < keydef_at < create_at
         and "is not partitioned on created_at" in script),
        ("key-type recheck", lock_at < keytype_at < create_at
         and "unexpected partition key type" in script),
        ("default recheck", lock_at < default_at < create_at),
    ]
    bad = [label for label, ok in checks if not ok]
    check("the transaction rechecks the parent shape before any create", not bad,
          f"failed: {bad}")


def main() -> int:
    test_missing_flag_stays_readonly()
    test_blank_flag_stays_readonly()
    test_strict_true_false()
    test_invalid_flag_fails_closed()
    test_managed_parents_are_exact()
    test_month_year_rollover()
    test_leap_day_window()
    test_names_and_bounds()
    test_covered_dates_run_no_ddl()
    test_covered_dates_ignore_the_flag()
    test_missing_content_coverage_fails_without_ddl_by_default()
    test_relation_of_another_parent_is_not_coverage()
    test_foreign_same_named_relation_blocks_even_opt_in()
    test_misbound_partition_blocks_without_modification()
    test_opt_in_creates_only_the_missing_children()
    test_backfill_is_one_guarded_transaction()
    test_list_partitioned_parent_is_refused()
    test_wrong_key_parent_is_refused()
    test_plain_table_parent_is_refused()
    test_wrong_key_type_parent_is_refused()
    test_default_child_parent_is_refused()
    test_unreadable_parent_is_refused()
    test_missing_parent_is_refused()
    test_transaction_rechecks_parent_shape_before_any_create()
    test_failed_backfill_rolls_back_loudly()
    test_concurrent_runners_serialize_on_the_same_lock()
    test_redaction_holds_on_backfill_errors()
    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    raise SystemExit(main())
