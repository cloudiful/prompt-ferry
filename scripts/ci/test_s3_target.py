#!/usr/bin/env python3
"""Pure grammar for the dedicated S3 test target.

The authority for the real S3 integration tests is a set of eight dedicated
settings: the endpoint, bucket, region, credentials, two boolean flags, and
`PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET`, which names the
`endpoint-root/bucket` an operator confirmed for this project. All eight are
required together, so a partially configured environment — or a repointed
endpoint or bucket — is refused instead of used. No application S3 fallback is
read here.

This module is the parsing half of that decision and nothing else: it touches
no process, no network, and no object store, so it can be exercised directly.
The shell preflight (`require-services.sh`) executes this module, which reads
the process environment and reports only the endpoint root and bucket. The
Rust test guard (`src/raw_payload_store/s3_tests.rs`) applies the same rules
independently before constructing the production store builder.

Secrets never appear in messages: every refusal names keys, never values.
"""

from __future__ import annotations

import os
import sys
from typing import NamedTuple

ENDPOINT_KEY = "PROMPT_FERRY_TEST_S3_ENDPOINT"
BUCKET_KEY = "PROMPT_FERRY_TEST_S3_BUCKET"
REGION_KEY = "PROMPT_FERRY_TEST_S3_REGION"
ACCESS_KEY_KEY = "PROMPT_FERRY_TEST_S3_ACCESS_KEY"
SECRET_KEY_KEY = "PROMPT_FERRY_TEST_S3_SECRET_KEY"
ALLOW_HTTP_KEY = "PROMPT_FERRY_TEST_S3_ALLOW_HTTP"
PATH_STYLE_KEY = "PROMPT_FERRY_TEST_S3_PATH_STYLE"
CONFIRMED_TARGET_KEY = "PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET"

REQUIRED_KEYS = (
    ENDPOINT_KEY,
    BUCKET_KEY,
    REGION_KEY,
    ACCESS_KEY_KEY,
    SECRET_KEY_KEY,
    ALLOW_HTTP_KEY,
    PATH_STYLE_KEY,
    CONFIRMED_TARGET_KEY,
)

SUPPORTED_SCHEMES = ("http", "https")

# The only bucket the test suite may address: the dedicated one an operator
# provisioned for this project. Anything else is refused, not created.
EXPECTED_BUCKET = "prompt-ferry-test"

__all__ = [
    "ACCESS_KEY_KEY",
    "ALLOW_HTTP_KEY",
    "BUCKET_KEY",
    "CONFIRMED_TARGET_KEY",
    "ENDPOINT_KEY",
    "EXPECTED_BUCKET",
    "PATH_STYLE_KEY",
    "REGION_KEY",
    "REQUIRED_KEYS",
    "SECRET_KEY_KEY",
    "SUPPORTED_SCHEMES",
    "PreflightError",
    "S3Target",
    "normalize_root",
    "parse_bool",
    "parse_endpoint",
    "validate_s3_env",
]


class PreflightError(Exception):
    """A prerequisite the target guard refuses to work around."""


class S3Target(NamedTuple):
    """A validated target: endpoint root, bucket, region, and strict flags.

    Credentials stay out of this record on purpose: the shell preflight never
    connects, so it needs nothing beyond the confirmed identity. Every message
    derived from this record names keys, never values.
    """

    endpoint_root: str
    bucket: str
    region: str
    allow_http: bool
    path_style: bool

    def label(self) -> str:
        return f"{self.endpoint_root}/{self.bucket}"


def parse_bool(value: str, key: str) -> bool:
    """Parses a strict lowercase boolean; anything else fails closed."""
    if value == "true":
        return True
    if value == "false":
        return False
    raise PreflightError(f"{key} must read exactly true or false")


def normalize_root(value: str, key: str) -> tuple[str, str, str]:
    """Validates the service-root endpoint and returns scheme, host, root.

    Only the normalization the guard allows is applied: trailing slashes are
    stripped. Userinfo, query strings, fragments, and non-root paths are
    refused rather than interpreted, and the returned root is rebuilt from the
    parsed parts so a hostile value is never echoed back.
    """
    raw = value.strip()
    if not raw:
        raise PreflightError(f"{key} is required for the S3 integration tests")
    head, separator, remainder = raw.partition("://")
    scheme = head.lower()
    if not separator or scheme not in SUPPORTED_SCHEMES:
        raise PreflightError(f"{key} must use an http or https endpoint")
    if "@" in remainder:
        raise PreflightError(f"{key} must not carry userinfo")
    if "?" in remainder:
        raise PreflightError(f"{key} must not carry query parameters")
    if "#" in remainder:
        raise PreflightError(f"{key} must not carry a fragment")
    authority, _, path = remainder.partition("/")
    if path.strip("/"):
        raise PreflightError(f"{key} must address the service root, not a path")
    host, separator, port = authority.partition(":")
    if not host:
        raise PreflightError(f"{key} must name a host explicitly")
    if separator:
        if not port or not port.isdigit() or not 0 < int(port) <= 65535:
            raise PreflightError(f"{key} has an invalid port")
        return scheme, host.lower(), f"{scheme}://{host.lower()}:{port}"
    return scheme, host.lower(), f"{scheme}://{host.lower()}"


def parse_endpoint(value: str) -> tuple[str, str, str]:
    """Validates the endpoint and returns its scheme, host, and root."""
    return normalize_root(value, ENDPOINT_KEY)


def validate_s3_env(settings: dict[str, str]) -> S3Target:
    """Validates eight dedicated settings with no process, network, or I/O."""
    missing = [key for key in REQUIRED_KEYS if not settings.get(key, "").strip()]
    if missing:
        raise PreflightError(
            "the S3 integration tests need every dedicated setting; missing: "
            + ", ".join(missing)
        )

    scheme, _host, root = parse_endpoint(settings[ENDPOINT_KEY])

    bucket = settings[BUCKET_KEY].strip()
    if bucket != EXPECTED_BUCKET:
        raise PreflightError(
            f"{BUCKET_KEY} must name the dedicated test bucket; "
            f"{CONFIRMED_TARGET_KEY} stays the only authority for where tests run"
        )

    region = settings[REGION_KEY].strip()
    if not region:
        raise PreflightError(f"{REGION_KEY} must name a region explicitly")

    allow_http = parse_bool(settings[ALLOW_HTTP_KEY].strip(), ALLOW_HTTP_KEY)
    path_style = parse_bool(settings[PATH_STYLE_KEY].strip(), PATH_STYLE_KEY)
    if scheme == "http" and not allow_http:
        raise PreflightError(
            f"a plain-HTTP endpoint needs {ALLOW_HTTP_KEY} set to true; "
            "plaintext without an explicit opt-in is refused"
        )

    if not settings[ACCESS_KEY_KEY].strip() or not settings[SECRET_KEY_KEY].strip():
        raise PreflightError(
            f"{ACCESS_KEY_KEY} and {SECRET_KEY_KEY} must both be non-empty"
        )

    confirmed = settings[CONFIRMED_TARGET_KEY].strip()
    if not confirmed:
        raise PreflightError(f"{CONFIRMED_TARGET_KEY} is required whenever S3 settings are set")
    if confirmed != f"{root}/{bucket}":
        raise PreflightError(
            f"{CONFIRMED_TARGET_KEY} must read exactly the confirmed "
            "endpoint-root/bucket; a test target must be the one an operator confirmed"
        )

    return S3Target(root, bucket, region, allow_http, path_style)


def main() -> int:
    try:
        target = validate_s3_env(
            {key: os.environ.get(key, "") for key in REQUIRED_KEYS}
        )
    except PreflightError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    # The endpoint root and bucket are operator-confirmed constants, not
    # secrets; credentials are never printed.
    print(f"s3 test target verified: {target.label()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
