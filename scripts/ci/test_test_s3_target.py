#!/usr/bin/env python3
"""Unit checks for the dedicated S3 test-target grammar.

Pure fixtures only: no network, no object store, no real credentials, and no
repository `.env` is read. Run with `uv run --no-project python
scripts/ci/test_test_s3_target.py` from the repository root.
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import test_s3_target as validator

PASS = 0
FAIL = 0

ENDPOINT = "http://s3-test.example.test:9002"
BUCKET = "prompt-ferry-test"
REGION = "us-east-1"


def settings(**overrides: str) -> dict[str, str]:
    base = {
        "PROMPT_FERRY_TEST_S3_ENDPOINT": ENDPOINT,
        "PROMPT_FERRY_TEST_S3_BUCKET": BUCKET,
        "PROMPT_FERRY_TEST_S3_REGION": REGION,
        "PROMPT_FERRY_TEST_S3_ACCESS_KEY": "s3-fixture-access",
        "PROMPT_FERRY_TEST_S3_SECRET_KEY": "s3-fixture-secret",
        "PROMPT_FERRY_TEST_S3_ALLOW_HTTP": "true",
        "PROMPT_FERRY_TEST_S3_PATH_STYLE": "true",
        "PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET": f"{ENDPOINT}/{BUCKET}",
    }
    base.update(overrides)
    return base


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


def expect_ok(label: str, env: dict[str, str], want_label: str) -> None:
    try:
        target = validator.validate_s3_env(env)
    except validator.PreflightError as error:
        check(label, False, f"refused a valid target: {error}")
        return
    check(label, target.label() == want_label, f"got {target.label()}")


def expect_refused(label: str, env: dict[str, str], fragment: str) -> None:
    try:
        validator.validate_s3_env(env)
    except validator.PreflightError as error:
        check(label, fragment in str(error), f"message was: {error}")
        return
    check(label, False, "a bad target was accepted")


def main() -> int:
    missing = [name for name in validator.__all__ if not hasattr(validator, name)]
    check("the validator exports every declared name", not missing, f"missing: {missing}")

    expect_ok("a complete dedicated target is accepted",
              settings(), f"{ENDPOINT}/{BUCKET}")
    expect_ok("a trailing slash normalizes to the same root",
              settings(PROMPT_FERRY_TEST_S3_ENDPOINT=ENDPOINT + "///"),
              f"{ENDPOINT}/{BUCKET}")
    expect_ok("https without the HTTP opt-in is accepted",
              settings(PROMPT_FERRY_TEST_S3_ENDPOINT="https://s3-test.example.test:9002",
                       PROMPT_FERRY_TEST_S3_ALLOW_HTTP="false",
                       PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET=
                       "https://s3-test.example.test:9002/prompt-ferry-test"),
              "https://s3-test.example.test:9002/prompt-ferry-test")

    cases = [
        ("a missing endpoint is refused",
         {k: v for k, v in settings().items()
          if k != "PROMPT_FERRY_TEST_S3_ENDPOINT"},
         "PROMPT_FERRY_TEST_S3_ENDPOINT"),
        ("a missing secret is refused",
         {k: v for k, v in settings().items()
          if k != "PROMPT_FERRY_TEST_S3_SECRET_KEY"},
         "PROMPT_FERRY_TEST_S3_SECRET_KEY"),
        ("an unsupported scheme is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT="ftp://s3-test.example.test:9002"),
         "http or https"),
        ("userinfo is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT="http://user@s3-test.example.test:9002"),
         "userinfo"),
        ("a query string is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT=ENDPOINT + "?x=1"),
         "query parameters"),
        ("a fragment is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT=ENDPOINT + "#frag"),
         "fragment"),
        ("a non-root path is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT=ENDPOINT + "/minio"),
         "service root"),
        ("an empty host is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT="http://:9002"),
         "must name a host"),
        ("an invalid port is refused",
         settings(PROMPT_FERRY_TEST_S3_ENDPOINT="http://s3-test.example.test:notaport"),
         "invalid port"),
        ("another bucket is refused",
         settings(PROMPT_FERRY_TEST_S3_BUCKET="someone-elses-bucket"),
         "dedicated test bucket"),
        ("an empty region is refused",
         settings(PROMPT_FERRY_TEST_S3_REGION="  "),
         "PROMPT_FERRY_TEST_S3_REGION"),
        ("a loose boolean is refused",
         settings(PROMPT_FERRY_TEST_S3_ALLOW_HTTP="yes"),
         "exactly true or false"),
        ("an unset path-style flag is refused",
         {k: v for k, v in settings().items()
          if k != "PROMPT_FERRY_TEST_S3_PATH_STYLE"},
         "PROMPT_FERRY_TEST_S3_PATH_STYLE"),
        ("plain HTTP without the opt-in is refused",
         settings(PROMPT_FERRY_TEST_S3_ALLOW_HTTP="false"),
         "PROMPT_FERRY_TEST_S3_ALLOW_HTTP"),
        ("a mismatched confirmed target is refused",
         settings(PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET=
                  "http://s3-test.example.test:9002/other-bucket"),
         "PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET"),
        ("a confirmed target with a rewritten host is refused",
         settings(PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET=
                  "http://s3-other.example.test:9002/prompt-ferry-test"),
         "PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET"),
    ]
    for label, env, fragment in cases:
        expect_refused(label, env, fragment)

    # A refusal never echoes credential-shaped values.
    try:
        validator.validate_s3_env(settings(PROMPT_FERRY_TEST_S3_BUCKET="x"))
        check("refusals stay value-free", False, "a bad target was accepted")
    except validator.PreflightError as error:
        message = str(error)
        check("refusals stay value-free",
              "s3-fixture-secret" not in message and "s3-fixture-access" not in message,
              f"message was: {message}")

    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    raise SystemExit(main())
