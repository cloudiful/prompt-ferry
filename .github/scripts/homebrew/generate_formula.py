#!/usr/bin/env python3
"""Render the prompt-ferry Homebrew formula from GitHub release metadata.

The script validates the release, its three binary assets, and the requested
tag's own Cargo.toml and frontend/package.json versions before rendering.
Writing is limited to Formula/prompt-ferry.rb inside an explicitly supplied tap
checkout; it never runs git or touches any other path.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from string import Template

REPO = "cloudiful/prompt-ferry"
FORMULA_PATH = Path("Formula/prompt-ferry.rb")
TEMPLATE_PATH = Path(__file__).with_name("formula.rb.tmpl")
VERSION_FILES = ("Cargo.toml", "frontend/package.json")

STABLE_TAG = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
SHA256_DIGEST = re.compile(r"^sha256:([0-9a-f]{64})$")
FORMULA_VERSION = re.compile(r'^\s*version "([^"]*)"\s*$', re.MULTILINE)
LICENSE_ID = re.compile(r"^[A-Za-z0-9.+-]+$")
FORMULA_URL = re.compile(
    r'^\s*url "https://github\.com/'
    + re.escape(REPO)
    + r"/releases/download/"
    r"(?P<tag>v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))/"
    r'(?P<asset>prompt-ferry-[^"/]+)"\s*$',
    re.MULTILINE,
)

ASSET_TARGETS = {
    "aarch64-apple-darwin": "MACOS_ARM64",
    "aarch64-unknown-linux-gnu": "LINUX_ARM64",
    "x86_64-unknown-linux-gnu": "LINUX_X86_64",
}


class FormulaError(Exception):
    pass


def parse_version(tag: str) -> tuple[int, int, int]:
    match = STABLE_TAG.fullmatch(tag or "")
    if not match:
        raise FormulaError(
            f"release tag must be stable vMAJOR.MINOR.PATCH: {tag!r}"
        )
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def format_version(version: tuple[int, int, int]) -> str:
    return "v" + ".".join(str(part) for part in version)


def plain_version(version: tuple[int, int, int]) -> str:
    return ".".join(str(part) for part in version)


def asset_name(tag: str, target: str) -> str:
    return f"prompt-ferry-{tag}-{target}"


def expected_download_url(tag: str, target: str) -> str:
    return (
        f"https://github.com/{REPO}/releases/download/"
        f"{tag}/{asset_name(tag, target)}"
    )


def validate_release(
    release: object, tag: str | None = None
) -> tuple[tuple[int, int, int], dict[str, str]]:
    if not isinstance(release, dict):
        raise FormulaError("release metadata must be a JSON object")

    release_tag = release.get("tag_name")
    version = parse_version(str(release_tag))
    if tag is not None and release_tag != tag:
        raise FormulaError(
            f"release tag {release_tag!r} does not match requested tag {tag!r}"
        )
    if release.get("draft") is not False:
        raise FormulaError(f"release {release_tag} is a draft or has no draft flag")
    if release.get("prerelease") is not False:
        raise FormulaError(
            f"release {release_tag} is a prerelease or has no prerelease flag"
        )

    assets = release.get("assets")
    if not isinstance(assets, list):
        raise FormulaError(f"release {release_tag} has no asset list")

    by_name: dict[str, dict] = {}
    for asset in assets:
        if not isinstance(asset, dict):
            raise FormulaError(f"release {release_tag} contains a malformed asset")
        name = asset.get("name")
        if not isinstance(name, str) or not name:
            raise FormulaError(f"release {release_tag} contains an unnamed asset")
        if name in by_name:
            raise FormulaError(f"duplicate release asset: {name}")
        by_name[name] = asset

    values: dict[str, str] = {}
    for target, prefix in ASSET_TARGETS.items():
        name = asset_name(str(release_tag), target)
        asset = by_name.get(name)
        if asset is None:
            raise FormulaError(f"missing release asset: {name}")
        if asset.get("state") != "uploaded":
            raise FormulaError(
                f"release asset {name} is not uploaded: {asset.get('state')!r}"
            )

        digest = asset.get("digest")
        digest_match = SHA256_DIGEST.fullmatch(digest if isinstance(digest, str) else "")
        if not digest_match:
            raise FormulaError(
                f"release asset {name} has no sha256 digest: {digest!r}"
            )

        expected = expected_download_url(str(release_tag), target)
        if asset.get("browser_download_url") != expected:
            raise FormulaError(
                f"release asset {name} has unexpected download URL: "
                f"{asset.get('browser_download_url')!r}"
            )

        values[f"{prefix}_URL"] = expected
        values[f"{prefix}_SHA256"] = digest_match.group(1)

    return version, values


def source_text(sources: object, path: str) -> str:
    if not isinstance(sources, dict):
        raise FormulaError("version source contents must be a mapping")
    text = sources.get(path)
    if not isinstance(text, str) or not text:
        raise FormulaError(f"missing version source contents for {path}")
    return text


def validate_source_versions(
    version: tuple[int, int, int], sources: object
) -> str:
    expected = plain_version(version)

    try:
        manifest = tomllib.loads(source_text(sources, "Cargo.toml"))
    except tomllib.TOMLDecodeError as error:
        raise FormulaError(f"invalid Cargo.toml: {error}") from error
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise FormulaError("Cargo.toml has no [package] table")
    manifest_version = package.get("version")
    if manifest_version != expected:
        raise FormulaError(
            f"Cargo.toml [package].version {manifest_version!r} does not match "
            f"release {expected}"
        )
    license_id = package.get("license")
    if not isinstance(license_id, str) or not LICENSE_ID.fullmatch(license_id):
        raise FormulaError(f"Cargo.toml [package].license is invalid: {license_id!r}")

    try:
        frontend = json.loads(source_text(sources, "frontend/package.json"))
    except json.JSONDecodeError as error:
        raise FormulaError(f"invalid frontend/package.json: {error}") from error
    if not isinstance(frontend, dict):
        raise FormulaError("frontend/package.json must be a JSON object")
    frontend_version = frontend.get("version")
    if frontend_version != expected:
        raise FormulaError(
            f"frontend/package.json version {frontend_version!r} does not match "
            f"release {expected}"
        )

    return license_id


def fetch_source_texts(tag: str) -> dict[str, str]:
    parse_version(tag)
    sources: dict[str, str] = {}
    for path in VERSION_FILES:
        result = subprocess.run(
            ["gh", "api", f"repos/{REPO}/contents/{path}?ref={tag}"],
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            detail = result.stderr.strip() or f"exit code {result.returncode}"
            raise FormulaError(f"gh api failed for {path} at {tag}: {detail}")
        try:
            payload = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise FormulaError(
                f"gh api returned invalid JSON for {path} at {tag}: {error}"
            ) from error
        if (
            not isinstance(payload, dict)
            or payload.get("encoding") != "base64"
            or not isinstance(payload.get("content"), str)
        ):
            raise FormulaError(f"gh api returned no base64 contents for {path} at {tag}")
        try:
            sources[path] = base64.b64decode(payload["content"]).decode("utf-8")
        except (ValueError, UnicodeDecodeError) as error:
            raise FormulaError(
                f"gh api returned undecodable contents for {path} at {tag}: {error}"
            ) from error
    return sources


def render_formula(
    release: object, tag: str | None = None, sources: object = None
) -> tuple[tuple[int, int, int], str]:
    version, values = validate_release(release, tag)
    resolved_tag = tag if tag is not None else str(release.get("tag_name"))  # type: ignore[union-attr]
    if sources is None:
        sources = fetch_source_texts(resolved_tag)
    values["LICENSE"] = validate_source_versions(version, sources)

    template = Template(TEMPLATE_PATH.read_text(encoding="utf-8"))
    text = template.substitute(values)
    if not text.endswith("\n"):
        text += "\n"
    return version, text


def existing_version(text: str) -> tuple[int, int, int]:
    versions: set[tuple[int, int, int]] = set()
    targets: set[str] = set()
    for match in FORMULA_URL.finditer(text):
        tag = match.group("tag")
        url_version = parse_version(tag)
        asset = match.group("asset")
        matched_target = None
        for target in ASSET_TARGETS:
            if asset == asset_name(tag, target):
                matched_target = target
                break
        if matched_target is None:
            raise FormulaError(
                "existing formula canonical release URLs do not match the "
                f"expected assets (unexpected {asset} for {tag})"
            )
        versions.add(url_version)
        targets.add(matched_target)

    if not versions:
        raise FormulaError("existing formula has no canonical release download URL")
    if len(versions) > 1:
        listed = ", ".join(sorted(format_version(item) for item in versions))
        raise FormulaError(
            f"existing formula mixes release versions in download URLs: {listed}"
        )

    missing = sorted(set(ASSET_TARGETS) - targets)
    if missing:
        raise FormulaError(
            "existing formula canonical release URLs do not match the expected "
            "assets (missing target " + ", ".join(missing) + ")"
        )

    url_version = versions.pop()
    declared = FORMULA_VERSION.search(text)
    if declared is not None:
        try:
            declared_version = parse_version("v" + declared.group(1))
        except FormulaError as error:
            raise FormulaError(
                f"existing formula version is invalid: {error}"
            ) from error
        if declared_version != url_version:
            raise FormulaError(
                "existing formula declares version "
                f"{format_version(declared_version)} but its download URLs use "
                f"{format_version(url_version)}"
            )

    return url_version


def check_existing_formula(text: str, version: tuple[int, int, int]) -> None:
    current = existing_version(text)
    if current > version:
        raise FormulaError(
            f"refusing to downgrade existing {format_version(current)} "
            f"formula to {format_version(version)}"
        )
    if current == version:
        raise FormulaError(
            f"existing formula already declares {format_version(version)} "
            "with different content"
        )


def write_formula(path: Path, text: str) -> None:
    fd, temporary = tempfile.mkstemp(
        dir=path.parent, prefix=".prompt-ferry-", suffix=".rb"
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def load_release(path: Path | None, tag: str | None) -> object:
    if path is not None:
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except OSError as error:
            raise FormulaError(f"cannot read {path}: {error}") from error
        except json.JSONDecodeError as error:
            raise FormulaError(f"invalid JSON in {path}: {error}") from error

    if not tag:
        raise FormulaError("--tag is required when reading release metadata from GitHub")
    parse_version(tag)
    result = subprocess.run(
        ["gh", "api", f"repos/{REPO}/releases/tags/{tag}"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or f"exit code {result.returncode}"
        raise FormulaError(f"gh api failed for {tag}: {detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise FormulaError(f"gh api returned invalid JSON: {error}") from error


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Render the prompt-ferry Homebrew formula."
    )
    parser.add_argument("--tag", help="stable release tag, for example v1.42.30")
    parser.add_argument(
        "--tap", type=Path, help="tap checkout to write Formula/prompt-ferry.rb in"
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the formula without writing to the tap",
    )
    parser.add_argument(
        "--release-json",
        type=Path,
        help="read release metadata from this JSON file instead of gh api",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        release = load_release(args.release_json, args.tag)
        sources = None
        if isinstance(release, dict):
            sources = release.get("source_versions")
        version, text = render_formula(release, args.tag, sources)

        formula_path = None
        existing_text = None
        if args.tap is not None:
            formula_path = args.tap / FORMULA_PATH
            if formula_path.exists():
                existing_text = formula_path.read_text(encoding="utf-8")
                if existing_text != text:
                    check_existing_formula(existing_text, version)
            elif not formula_path.parent.is_dir():
                raise FormulaError(
                    f"not a tap checkout (missing {formula_path.parent}): {args.tap}"
                )

        if args.dry_run:
            sys.stdout.write(text)
            return 0

        if formula_path is None:
            raise FormulaError("--tap is required unless --dry-run is set")
        if existing_text == text:
            print(f"unchanged {formula_path}")
            return 0

        write_formula(formula_path, text)
        print(f"wrote {formula_path}")
        return 0
    except FormulaError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
