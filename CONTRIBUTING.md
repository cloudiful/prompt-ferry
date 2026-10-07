# Contributing

Thanks for contributing to `prompt-ferry`. The public project uses native Rust
and Bun tooling, Valkey for the service-backed integration tests, and generated
OpenAPI files as checked-in contract artifacts.

## Prerequisites

- Rust stable with Cargo
- Bun 1.3.11 or a compatible Bun 1.x release
- Bash 4 or newer for the local development scripts
- Valkey for the response-affinity integration tests

Do not commit `.env`, `Cargo.lock`, or `frontend/bun.lock`. The frontend uses a
seven-day package release age policy from `frontend/bunfig.toml`.

## Local Development

Create a local `.env` with a development database URL, then initialize the
database through the repository entrypoint:

```bash
cargo run --bin db_init
```

Use the local scripts when convenient:

```bash
bash scripts/dev.sh backend
bash scripts/dev.sh full
```

The root `.env` is the source of truth for these scripts, and `DATABASE_URL` is
required. Avoid exporting conflicting `PROMPT_FERRY_*` variables in the shell.

## Validation

Backend checks:

```bash
cargo fmt --all -- --check
cargo test --workspace
```

Frontend checks:

```bash
cd frontend
bun install --no-save
bun run format:check
bun run typecheck
bun run build
```

Database-shaped test helpers build lazy pools and URLs pinned to a fresh
isolated `pfy_test_*` schema through an explicit `search_path`, so a test never
reaches a shared schema and the suite needs no `DATABASE_URL`. PostgreSQL
migrations are validated in the development environment with
`cargo run --bin db_init`; do not use the `public` schema by hand or edit
`_sqlx_migrations`. Export `PROMPT_FERRY_TEST_VALKEY_URL` before
`cargo test --workspace` so the response-affinity tests run against a real
Valkey instead of skipping.

## Generated Contracts

The backend is the source of truth for the admin OpenAPI document:

```bash
cargo run -- openapi export
cd frontend
bun run openapi-ts
```

Commit generated changes to `openapi/admin-api.yaml` and
`frontend/src/generated/admin-api/**` together with the source change. Do not
hand-edit generated files or add handwritten admin API DTOs.

SQLx query metadata is generated after database or SQL changes:

```bash
cargo run --bin db_init
cargo sqlx prepare --workspace -- --all-targets
```

The `.sqlx` directory is tracked so GitHub Actions can compile with
`SQLX_OFFLINE=true`.

## Version Freeze

Internal library crates and `tools/db-init` stay at `0.1.0` forever.
Release commits must not bump them; only the root `prompt-ferry`
binary follows the `1.x` release line. Reviewers must check this
on every release commit.

## Pull Requests

Keep changes focused and include regression coverage for behavior changes.
Run the same commands used by `.github/workflows/_quality.yml` before opening a
pull request. Never include real credentials, provider keys, private URLs, or
local filesystem paths in source, fixtures, documentation, or test output.

Security issues should follow [SECURITY.md](SECURITY.md), not a public issue.
