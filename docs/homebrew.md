# Homebrew

`prompt-ferry` is distributed through the public [cloudiful/homebrew-tap](https://github.com/cloudiful/homebrew-tap) tap. The formula pins an exact GitHub Release and verifies the published SHA-256 digest of each binary.

## Install

```sh
brew tap cloudiful/tap
brew install cloudiful/tap/prompt-ferry
```

Upgrade with `brew upgrade cloudiful/tap/prompt-ferry`. Installing and upgrading need no GitHub secret; the release binaries are public.

## Supported targets

| Platform | Support |
| --- | --- |
| macOS ARM64 (Apple Silicon) | Supported |
| Linux x86_64 | Supported; requires glibc 2.39 or newer |
| Linux ARM64 | Supported; requires glibc 2.39 or newer |
| Intel Mac | Not supported; no Intel macOS build is published |
| Windows | Not supported; use a release binary directly |

The Linux binaries depend on glibc 2.39 or newer and are not tested against older distribution floors. macOS Intel is not supported because the release publishes no Intel macOS artifact.

## Automatic formula updates

`.github/workflows/bump-homebrew-tap.yml` updates the tap after a successful `Release` workflow run or through an explicit `workflow_dispatch` with a stable `vX.Y.Z` tag and optional `dry_run`.

Before writing anything the job:

- reads the trusted default-branch copy of `.github/scripts/homebrew/generate_formula.py` (`persist-credentials: false`) instead of checking out the release tag;
- rejects any `workflow_run` that is not a successful same-repository push;
- parses a strict stable tag and, for `workflow_run`, dereferences annotated tags and requires the tag commit to equal the release run's `head_sha`;
- requires the release to be non-draft, non-prerelease, with the requested tag, the three expected assets in the `uploaded` state, exact download URLs, and a GitHub-reported `sha256:` digest;
- checks that the tag's `Cargo.toml` `[package].version` and `frontend/package.json` version both equal the tag, using only `gh api`, `tomllib`, and `json` — no tag code runs.

The generator renders in memory and replaces only `Formula/prompt-ferry.rb` atomically. Identical input is a no-op; a downgrade, a same-version formula with different contents, or any missing, duplicate, unuploaded, wrong-URL, wrong-state, or wrong-digest asset fails closed without a write. Runner values are passed through environment variables, never interpolated into shell source. A `dry_run` dispatch is read-only and needs no tap token.

## Updater secret

A real update pushes to a second repository, so the default `GITHUB_TOKEN` cannot be used. An operator must configure `HOMEBREW_TAP_TOKEN` on `cloudiful/prompt-ferry` as either a fine-grained personal access token or a GitHub App installation token, scoped to `cloudiful/homebrew-tap` with Contents read/write only. The workflow fails with a clear message when the token is missing, never logs or accepts the token as a command-line value, and does not modify tap files other than `Formula/prompt-ferry.rb`. Creating or rotating the token is an operator action in the GitHub UI; the workflow never provisions it.
