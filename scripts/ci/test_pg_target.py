#!/usr/bin/env python3
"""Pure grammar for the dedicated PostgreSQL test target.

The authority for the quota integration tests is a pair of settings: the
dedicated URL supplies credentials, and `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET`
names the `host:port/database` an operator confirmed for this project. Both are
required and must agree, so repointing only the URL — at a production database
or any other service — is refused instead of used. The application
`DATABASE_URL` is never read here.

This module is the parsing half of that decision and nothing else: it touches no
process, no network, and no SQL, so it can be exercised directly. The connecting
half lives in `require-test-postgres.py`, which imports this module and
re-exports every name below, so the checked fixtures and any existing import
hook keep resolving the same symbols with the same messages.

The grammar is deliberately the one `tests/support/test_pg_target.rs` applies,
so the shell preflight and the Rust helper accept and refuse exactly the same
targets.
"""

from __future__ import annotations

from typing import NamedTuple
from urllib.parse import unquote

DATABASE_URL_KEY = "PROMPT_FERRY_QUOTA_TEST_DATABASE_URL"
CONFIRMED_TARGET_KEY = "PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET"

SUPPORTED_SCHEMES = ("postgres", "postgresql")
DEFAULT_PORT = 5432

# The narrow character sets a target may be spelled from, mirroring the
# `host_char`/`database_char` predicates in tests/support/test_pg_target.rs.
HOST_CHARS = "abcdefghijklmnopqrstuvwxyz0123456789.-"
DATABASE_CHARS = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-$"

__all__ = [
    "CONFIRMED_TARGET_KEY",
    "DATABASE_CHARS",
    "DATABASE_URL_KEY",
    "DEFAULT_PORT",
    "HOST_CHARS",
    "SUPPORTED_SCHEMES",
    "Identity",
    "PreflightError",
    "Target",
    "check_charset",
    "decode",
    "normalize_host",
    "parse_confirmed_target",
    "parse_database",
    "parse_target",
    "split_host_port",
]


class PreflightError(Exception):
    """A prerequisite the target guard refuses to work around."""


class Identity(NamedTuple):
    """The `host:port/database` a target addresses, free of any userinfo."""

    host: str
    port: int
    database: str

    def label(self) -> str:
        return f"{self.host}:{self.port}/{self.database}"


class Target(NamedTuple):
    """A validated target: the confirmed identity plus its credentials."""

    host: str
    port: int
    database: str
    user: str
    password: str

    def label(self) -> str:
        return f"{self.host}:{self.port}/{self.database}"


def decode(value: str, key: str, what: str) -> str:
    """Percent-decodes without echoing the decoded text when it is malformed."""
    try:
        return unquote(value, errors="strict")
    except (UnicodeDecodeError, ValueError):
        raise PreflightError(f"{key} has a malformed {what}") from None


def check_charset(value: str, key: str, what: str, allowed: str) -> str:
    if not value or any(character not in allowed for character in value):
        raise PreflightError(f"{key} has an unsupported {what}")
    return value


def split_host_port(value: str, key: str) -> tuple[str, int]:
    """Splits `host[:port]`, reading an absent or empty port as 5432."""
    if value.startswith("["):
        raise PreflightError(f"{key} must use a named host; a bracketed IPv6 literal "
                             "cannot be confirmed")
    host, separator, port = value.partition(":")
    if not separator or not port:
        return host, DEFAULT_PORT
    if not port.isdigit() or not 0 < int(port) <= 65535:
        raise PreflightError(f"{key} has an invalid port")
    return host, int(port)


def parse_database(value: str, key: str) -> str:
    if "/" in value:
        raise PreflightError(f"{key} must not carry a path beyond the database name")
    decoded = decode(value, key, "database name")
    return check_charset(decoded, key, "database name", DATABASE_CHARS)


def normalize_host(value: str, key: str) -> str:
    host = decode(value, key, "host").lower()
    return check_charset(host, key, "host", HOST_CHARS)


def parse_confirmed_target(confirmed: str) -> Identity:
    """Parses the operator-confirmed `host:port/database`."""
    hostport, separator, database = confirmed.partition("/")
    if not separator:
        raise PreflightError(f"{CONFIRMED_TARGET_KEY} must read host:port/database")
    host, port = split_host_port(hostport, CONFIRMED_TARGET_KEY)
    return Identity(
        normalize_host(host, CONFIRMED_TARGET_KEY),
        port,
        parse_database(database, CONFIRMED_TARGET_KEY),
    )


def parse_target(url: str, confirmed: str) -> Target:
    """Validates the dedicated test URL and returns its connectable parts."""
    if not url.strip():
        raise PreflightError(f"{DATABASE_URL_KEY} is required for the quota integration "
                             "tests; set it in .env or the environment")
    if not confirmed.strip():
        raise PreflightError(f"{CONFIRMED_TARGET_KEY} is required whenever "
                             f"{DATABASE_URL_KEY} is set; a test database needs an "
                             "operator-confirmed host:port/database")

    raw = url.strip()
    head, separator, remainder = raw.partition(":")
    scheme = head.lower()
    if not separator or scheme not in SUPPORTED_SCHEMES:
        raise PreflightError(f"unsupported test database URL scheme: {scheme or '<none>'}")

    if not remainder.startswith("//"):
        raise PreflightError("the test database URL is not a valid PostgreSQL URL")
    rest = remainder[2:]
    for marker, what in (("#", "a fragment"), ("?", "query parameters or options")):
        # `options` could repoint the search path away from the schema these
        # tables live in, and a test target must be the confirmed identity alone.
        if marker in rest:
            raise PreflightError(f"{DATABASE_URL_KEY} must not carry {what}")

    authority, separator, database = rest.partition("/")
    if not separator:
        raise PreflightError(f"{DATABASE_URL_KEY} must name a database explicitly")
    userinfo, at, hostport = authority.rpartition("@")
    if not at:
        raise PreflightError(f"{DATABASE_URL_KEY} must carry a username")
    user, has_password, secret = userinfo.partition(":")
    user = unquote(user)
    if not user:
        raise PreflightError(f"{DATABASE_URL_KEY} must carry a username")

    host, port = split_host_port(hostport, DATABASE_URL_KEY)
    identity = Identity(
        normalize_host(host, DATABASE_URL_KEY), port, parse_database(database, DATABASE_URL_KEY)
    )
    confirmed_identity = parse_confirmed_target(confirmed.strip())
    if identity.label() != confirmed_identity.label():
        raise PreflightError(
            f"{DATABASE_URL_KEY} addresses {identity.label()} instead of the confirmed target "
            f"{confirmed_identity.label()}; a test database must be the one an operator "
            f"confirmed"
        )

    return Target(identity.host, identity.port, identity.database, user,
                  unquote(secret) if has_password else "")
