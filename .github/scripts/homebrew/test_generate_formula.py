import copy
import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

import generate_formula as generator

HERE = Path(__file__).resolve().parent
FIXTURE = HERE / "fixtures" / "release-v1.42.30.json"


def load_release():
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def version_sources(cargo="1.42.30", frontend="1.42.30", license_id="Apache-2.0"):
    return {
        "Cargo.toml": (
            '[package]\nname = "prompt-ferry"\n'
            f'version = "{cargo}"\nlicense = "{license_id}"\n'
        ),
        "frontend/package.json": json.dumps(
            {"name": "prompt-ferry-frontend", "version": frontend}
        ),
    }


class RenderTest(unittest.TestCase):
    def setUp(self):
        self.release = load_release()
        self.sources = version_sources()

    def render(self, tag=None):
        return generator.render_formula(self.release, tag, self.sources)

    def test_renders_release_assets_and_install_name(self):
        version, text = self.render("v1.42.30")
        self.assertEqual(version, (1, 42, 30))
        self.assertNotRegex(text, r'(?m)^\s*version "')
        self.assertIn('license "Apache-2.0"', text)
        self.assertIn(
            "https://github.com/cloudiful/prompt-ferry/releases/download/"
            "v1.42.30/prompt-ferry-v1.42.30-aarch64-apple-darwin",
            text,
        )
        self.assertIn(
            "18bedf8b4645aa63adea224bf4867b96418191ee46b85dff3c9ce7a0f7f2d9aa",
            text,
        )
        self.assertIn(
            "https://github.com/cloudiful/prompt-ferry/releases/download/"
            "v1.42.30/prompt-ferry-v1.42.30-aarch64-unknown-linux-gnu",
            text,
        )
        self.assertIn(
            "17ef1ba843510997c208d6f5bc2ffb830e250b88ff16883f3b260fa0148d41b0",
            text,
        )
        self.assertIn(
            "https://github.com/cloudiful/prompt-ferry/releases/download/"
            "v1.42.30/prompt-ferry-v1.42.30-x86_64-unknown-linux-gnu",
            text,
        )
        self.assertIn(
            "720d27212a0df99d712acb7241d77281eae6d1a0e960259a559907d11b4e3f73",
            text,
        )
        self.assertIn("on_macos do", text)
        self.assertIn("depends_on arch: :arm64", text)
        self.assertIn("on_arm do", text)
        self.assertIn("on_linux do", text)
        self.assertIn("on_intel do", text)
        self.assertIn(
            'bin.install Dir["prompt-ferry-*"].first => "prompt-ferry"', text
        )
        self.assertIn('shell_output("#{bin}/prompt-ferry --version")', text)
        self.assertIn("glibc 2.39", text)
        self.assertNotIn("x86_64-apple-darwin", text)
        self.assertNotIn("windows", text)
        self.assertNotIn("SHA256SUMS", text)

    def test_rejects_draft_release(self):
        self.release["draft"] = True
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_prerelease(self):
        self.release["prerelease"] = True
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_tag_mismatch(self):
        with self.assertRaises(generator.FormulaError):
            self.render("v1.42.29")

    def test_rejects_non_stable_tag(self):
        self.release["tag_name"] = "v1.42.30-rc.1"
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_shell_injection_tag(self):
        with self.assertRaises(generator.FormulaError):
            self.render("v1.42.30; rm -rf /")

    def test_rejects_missing_asset(self):
        self.release["assets"] = [
            asset
            for asset in self.release["assets"]
            if asset["name"] != "prompt-ferry-v1.42.30-aarch64-unknown-linux-gnu"
        ]
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_duplicate_asset(self):
        self.release["assets"].append(copy.deepcopy(self.release["assets"][0]))
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_missing_digest(self):
        self.release["assets"][0].pop("digest")
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_malformed_digest(self):
        self.release["assets"][0]["digest"] = "sha256:abc"
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_unuploaded_asset(self):
        self.release["assets"][0]["state"] = "starter"
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_unexpected_download_url(self):
        self.release["assets"][0]["browser_download_url"] = (
            "https://example.com/prompt-ferry-v1.42.30-aarch64-apple-darwin"
        )
        with self.assertRaises(generator.FormulaError):
            self.render()

    def test_rejects_malformed_metadata(self):
        with self.assertRaises(generator.FormulaError):
            generator.render_formula([], "v1.42.30", self.sources)
        self.release["assets"] = "not a list"
        with self.assertRaises(generator.FormulaError):
            self.render()


class SourceVersionTest(unittest.TestCase):
    def test_accepts_matching_manifest_versions(self):
        self.assertEqual(
            generator.validate_source_versions(
                (1, 42, 30), version_sources()
            ),
            "Apache-2.0",
        )

    def test_fixture_embeds_matching_source_versions(self):
        release = load_release()
        self.assertEqual(
            generator.validate_source_versions(
                (1, 42, 30), release["source_versions"]
            ),
            "Apache-2.0",
        )

    def test_rejects_cargo_version_mismatch(self):
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions(
                (1, 42, 30), version_sources(cargo="1.42.29")
            )

    def test_rejects_frontend_version_mismatch(self):
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions(
                (1, 42, 30), version_sources(frontend="1.42.31")
            )

    def test_rejects_missing_source_contents(self):
        sources = version_sources()
        sources.pop("Cargo.toml")
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions((1, 42, 30), sources)
        sources = version_sources()
        sources.pop("frontend/package.json")
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions((1, 42, 30), sources)

    def test_rejects_malformed_sources(self):
        sources = version_sources()
        sources["Cargo.toml"] = "[package\nversion = '1.42.30'"
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions((1, 42, 30), sources)
        sources = version_sources()
        sources["frontend/package.json"] = "{not json"
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions((1, 42, 30), sources)

    def test_rejects_missing_package_table_and_invalid_license(self):
        sources = version_sources()
        sources["Cargo.toml"] = '[workspace]\nmembers = ["."]\n'
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions((1, 42, 30), sources)
        with self.assertRaises(generator.FormulaError):
            generator.validate_source_versions(
                (1, 42, 30), version_sources(license_id="not a license")
            )


class TapWriteTest(unittest.TestCase):
    def setUp(self):
        self.release = load_release()
        self.sources = version_sources()
        self.temporary = tempfile.TemporaryDirectory()
        self.tap = Path(self.temporary.name)
        (self.tap / "Formula").mkdir()

    def tearDown(self):
        self.temporary.cleanup()

    def run_cli(self, *arguments, fixture=FIXTURE, fetch_error=None):
        if fetch_error is not None:
            fetch = patch.object(
                generator, "fetch_source_texts", side_effect=fetch_error
            )
        else:
            fetch = patch.object(
                generator,
                "fetch_source_texts",
                return_value=self.sources,
            )
        stdout = io.StringIO()
        stderr = io.StringIO()
        with fetch, redirect_stdout(stdout), redirect_stderr(stderr):
            code = generator.main(
                [
                    "--tag",
                    "v1.42.30",
                    "--release-json",
                    str(fixture),
                    *arguments,
                ]
            )
        return code, stdout.getvalue(), stderr.getvalue()

    def fixture_with_sources(self, **versions):
        release = load_release()
        release["source_versions"] = version_sources(**versions)
        path = Path(self.temporary.name) / "release.json"
        path.write_text(json.dumps(release), encoding="utf-8")
        return path

    def fixture_without_sources(self):
        release = load_release()
        release.pop("source_versions")
        path = Path(self.temporary.name) / "release-without-sources.json"
        path.write_text(json.dumps(release), encoding="utf-8")
        return path

    def formula_text(self) -> str:
        return (self.tap / generator.FORMULA_PATH).read_text(encoding="utf-8")

    def render_for(self, tag: str) -> str:
        release = copy.deepcopy(self.release)
        release["tag_name"] = tag
        for asset in release["assets"]:
            for target in generator.ASSET_TARGETS:
                if asset["name"].endswith(target):
                    asset["name"] = generator.asset_name(tag, target)
                    asset["browser_download_url"] = generator.expected_download_url(
                        tag, target
                    )
        tag_version = tag[1:]
        sources = version_sources(cargo=tag_version, frontend=tag_version)
        _, text = generator.render_formula(release, None, sources)
        return text

    def write_existing(self, text: str) -> str:
        (self.tap / generator.FORMULA_PATH).write_text(text, encoding="utf-8")
        return text

    def formula_files(self):
        return sorted(path.name for path in (self.tap / "Formula").iterdir())

    def test_writes_once_and_identical_rerun_is_noop(self):
        code, stdout, _ = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 0)
        self.assertIn("wrote", stdout)
        first = self.formula_text()
        self.assertNotIn('version "', first)

        code, stdout, _ = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 0)
        self.assertIn("unchanged", stdout)
        self.assertEqual(self.formula_text(), first)

    def test_dry_run_prints_formula_without_writing(self):
        code, stdout, _ = self.run_cli("--dry-run")
        self.assertEqual(code, 0)
        self.assertIn(
            "https://github.com/cloudiful/prompt-ferry/releases/download/"
            "v1.42.30/prompt-ferry-v1.42.30-aarch64-apple-darwin",
            stdout,
        )
        self.assertNotIn('version "', stdout)
        self.assertFalse((self.tap / generator.FORMULA_PATH).exists())

    def test_fixture_dry_run_needs_no_network(self):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with patch.object(
            generator,
            "fetch_source_texts",
            side_effect=AssertionError("network lookup in fixture mode"),
        ), redirect_stdout(stdout), redirect_stderr(stderr):
            code = generator.main(
                [
                    "--tag",
                    "v1.42.30",
                    "--release-json",
                    str(FIXTURE),
                    "--dry-run",
                ]
            )
        self.assertEqual(code, 0)
        self.assertIn('license "Apache-2.0"', stdout.getvalue())

    def test_derives_version_from_urls_only(self):
        existing = self.write_existing(self.render_for("v1.42.29"))
        self.assertEqual(generator.existing_version(existing), (1, 42, 29))

        code, stdout, _ = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 0)
        self.assertIn("wrote", stdout)
        self.assertEqual(self.formula_text(), self.render_for("v1.42.30"))

    def test_parses_matching_declared_version_but_refuses_drift(self):
        base = self.render_for("v1.42.30")
        existing = base.replace(
            '  license "Apache-2.0"\n',
            '  version "1.42.30"\n  license "Apache-2.0"\n',
        )
        self.assertNotEqual(existing, base)
        self.write_existing(existing)

        self.assertEqual(generator.existing_version(existing), (1, 42, 30))

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("different content", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_declared_version_mismatch(self):
        base = self.render_for("v1.42.30")
        existing = self.write_existing(
            base.replace(
                '  license "Apache-2.0"\n',
                '  version "1.42.29"\n  license "Apache-2.0"\n',
            )
        )
        with self.assertRaises(generator.FormulaError):
            generator.existing_version(existing)

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("declares version", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_inconsistent_url_versions(self):
        base = self.render_for("v1.42.30")
        existing = self.write_existing(
            base.replace(
                "releases/download/v1.42.30/prompt-ferry-v1.42.30-aarch64-unknown-linux-gnu",
                "releases/download/v1.42.29/prompt-ferry-v1.42.29-aarch64-unknown-linux-gnu",
            )
        )
        with self.assertRaises(generator.FormulaError):
            generator.existing_version(existing)

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("mixes release versions", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_malformed_url_version(self):
        base = self.render_for("v1.42.30")
        existing = self.write_existing(
            base.replace(
                "releases/download/v1.42.30/prompt-ferry-v1.42.30-aarch64-apple-darwin",
                "releases/download/v01.42.30/prompt-ferry-v01.42.30-aarch64-apple-darwin",
            )
        )
        with self.assertRaises(generator.FormulaError):
            generator.existing_version(existing)

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("do not match the expected assets", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_formula_without_canonical_urls(self):
        existing = self.write_existing(
            'class PromptFerry < Formula\n  version "1.42.30"\nend\n'
        )
        with self.assertRaises(generator.FormulaError):
            generator.existing_version(existing)

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("no canonical release download URL", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_downgrade(self):
        existing = self.write_existing(self.render_for("v1.43.0"))

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("downgrade", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_same_version_with_different_content(self):
        existing = self.write_existing(
            self.render_for("v1.42.30").replace(
                "OpenAI-compatible relay and worker for upstream ferrying",
                "Different description",
            )
        )

        code, _, stderr = self.run_cli("--tap", str(self.tap))
        self.assertEqual(code, 1)
        self.assertIn("different content", stderr)
        self.assertEqual(self.formula_text(), existing)

    def test_rejects_missing_formula_directory(self):
        empty = Path(self.temporary.name) / "empty"
        empty.mkdir()
        code, _, stderr = self.run_cli("--tap", str(empty))
        self.assertEqual(code, 1)
        self.assertIn("not a tap checkout", stderr)

    def test_source_version_mismatch_blocks_write(self):
        code, _, stderr = self.run_cli(
            "--tap",
            str(self.tap),
            fixture=self.fixture_with_sources(cargo="1.42.29"),
        )
        self.assertEqual(code, 1)
        self.assertIn("Cargo.toml", stderr)
        self.assertEqual(self.formula_files(), [])

    def test_fetch_failure_blocks_write(self):
        code, _, stderr = self.run_cli(
            "--tap",
            str(self.tap),
            fixture=self.fixture_without_sources(),
            fetch_error=generator.FormulaError("gh api failed for Cargo.toml"),
        )
        self.assertEqual(code, 1)
        self.assertIn("gh api failed", stderr)
        self.assertEqual(self.formula_files(), [])

    def test_failed_update_leaves_existing_formula_untouched(self):
        existing = self.write_existing(self.render_for("v1.42.30"))
        code, _, _ = self.run_cli(
            "--tap",
            str(self.tap),
            fixture=self.fixture_with_sources(frontend="9.9.9"),
        )
        self.assertEqual(code, 1)
        self.assertEqual(self.formula_files(), ["prompt-ferry.rb"])
        self.assertEqual(self.formula_text(), existing)


if __name__ == "__main__":
    unittest.main()
