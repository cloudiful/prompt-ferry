# Agent Instructions

## Release versions

For a `vX.Y.Z` application release, set both versions to `X.Y.Z` (without the `v`):

- Root `Cargo.toml` → `[package].version`
- `frontend/package.json` → `version`

Keep these values aligned with each other and the tag. Internal library crates and `tools/db-init` stay at `0.1.0`; do not bump them. `openapi/admin-api.yaml` has its own API contract version. Do not commit `Cargo.lock` or `frontend/bun.lock`.
