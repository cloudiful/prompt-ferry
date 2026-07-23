# Contributing

Thanks for contributing to `prompt-ferry`. The public project uses native Rust
and Bun tooling, PostgreSQL for integration tests, and generated OpenAPI files
as checked-in contract artifacts.

## Prerequisites

- Rust stable with Cargo
- Bun 1.3.11 or a compatible Bun 1.x release
- Nu shell for the local development scripts
- PostgreSQL 17 for integration tests

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
nu scripts/dev.nu backend
nu scripts/dev.nu full
```

The root `.env` is the source of truth for these scripts. Avoid exporting
conflicting `PROMPT_FERRY_*` variables in the shell.

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

Database integration tests read `PROMPT_FERRY_TEST_DATABASE_URL`. They create a
temporary schema per test run and set `search_path` to that schema; do not use
the `public` schema or manually edit `_sqlx_migrations`.

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

## Pull Requests

Keep changes focused and include regression coverage for behavior changes.
Run the same commands used by `.github/workflows/_quality.yml` before opening a
pull request. Never include real credentials, provider keys, private URLs, or
local filesystem paths in source, fixtures, documentation, or test output.

Security issues should follow [SECURITY.md](SECURITY.md), not a public issue.
