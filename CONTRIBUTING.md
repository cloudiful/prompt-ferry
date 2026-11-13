# Contributing

Thanks for contributing to `prompt-ferry`. The public project uses native Rust
and Bun tooling, and generated OpenAPI files as checked-in contract artifacts.

## Prerequisites

- Rust stable with Cargo
- Bun 1.3.11 or a compatible Bun 1.x release
- Bash 4 or newer for the local development scripts

The backend test suite is unit-only: every test is pure or in-memory, so it
needs no database, Valkey, S3, or other external service.

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
bash scripts/ci/run-tests.sh
```

`run-tests.sh` runs `cargo test --workspace --lib`, rejects failures, ignored
tests, or a run with no results, and prints elapsed time. It needs no `.env`,
database, or other service. `SQLX_OFFLINE=true cargo check --workspace` is the
equivalent compile gate.

Frontend checks:

```bash
cd frontend
bun install --no-save
bun run format:check
bun run typecheck
bun run build
```

The admin UI is embedded into the binary from `frontend/dist`, so run the
frontend build before `cargo build` when you want the UI in a local binary. A
source checkout without `frontend/dist` still compiles and serves a notice
page instead of the UI. Frontend iteration can skip the Rust rebuild entirely
by pointing `PROMPT_FERRY_FRONTEND_DIST` at a `dist` directory.

Tests stay pure: connect no database, Valkey, S3, socket, or subprocess, and do
not mutate the environment or write fixtures to disk. Migrations are validated
in the development environment with `cargo run --bin db_init`; do not use the
`public` schema by hand or edit `_sqlx_migrations`.

## Generated Contracts

The backend is the source of truth for the admin OpenAPI document:

```bash
cargo run -- openapi export
cd frontend
bun run openapi-ts
```

The frontend command installs its exact-pinned generator and TypeScript 6.0.3
under `frontend/tools/codegen/` with the same seven-day release-age gate, then
runs code generation with Node. The application typecheck remains on TypeScript
7.0.2; the codegen compiler is not part of the app graph or bundle.

Commit generated changes to `openapi/admin-api.yaml` and
`frontend/src/generated/admin-api/**` together with the source change. Do not
hand-edit generated files or add handwritten admin API DTOs. The document covers
the worker admin API and the relay management API; the relay routes are served
by the relay's own loopback management listener but share the one `/api/v1`
contract, so adding a route means adding its `utoipa` stub in the same change.

SQLx query metadata is generated after database or SQL changes:

```bash
cargo run --bin db_init
cargo sqlx prepare --workspace -- --all-targets
```

The `.sqlx` directory is tracked so GitHub Actions can compile with
`SQLX_OFFLINE=true`.

## Persistence Architecture

The backend writes PostgreSQL through two layers that share one schema and one
pool.

- Ordinary configuration and authentication CRUD runs natively through the
  SeaORM entities in `src/db/entities/`: the PostgreSQL tables
  `worker_settings`, `managed_relays`, `mcp_servers`, `mcp_credentials`,
  `config_archive_audit`, `users` and `client_keys`, plus the standalone SQLite
  settings, relay, MCP server, config audit, `standalone_users` and
  `standalone_client_keys` tables. `src/db/seaorm.rs` borrows the existing SQLx
  pool, so the native layer adds no second pool and owns no shutdown.
- A standalone SQLite client-key update or delete targets only the row that
  matches its `user_id` and `key_id`. The update changes the label, enabled flag
  and `updated_at` while leaving the encrypted secret, prefix and `created_at`
  untouched; the delete removes that key. Neither operation rewrites the rest
  of the configuration, unlike the snapshot publication used for full imports
  and exports.
- The PostgreSQL configuration import and the standalone snapshot and import
  stay SQLx-owned atomic islands. Each runs as one whole operation and is
  never split into native transactions or mixed with them.
- The entities also declare the native schema metadata for those tables,
  including checks and indexes. The declarations describe the schema the
  applied migrations already create; they are not a second schema applier. The
  migrations in `migrations/` remain the only initializer and upgrade path,
  and the standalone schema version is unchanged.
- The usage and request-record write paths stay SQLx. The request record
  insert returns its own `created_at`, and the raw payload reference row
  binds that timestamp directly instead of selecting it back from the daily
  request-family partitions.

Migrate a query only after checking that no SQLx island or shared helper
still depends on it, and regenerate the `.sqlx` metadata in the same change.

## Frontend Standards

The admin UI is React with TypeScript, Bun, shadcn/ui and Tailwind CSS.

- HTTP calls go through the generated admin client in
  `frontend/src/generated/admin-api/**`. Its TanStack Query integration emits
  the query options and the mutation options of every endpoint; use those
  instead of hand-written request wrappers.
- Server state belongs to TanStack Query. A read is a `useQuery` over a
  generated query option, a write is a `useMutation` over a generated mutation
  option, and every server-owned slice belongs in the query key.
- A business list renders through the shared `DataTable`, which uses native
  TanStack Table. Pagination, sorting and row actions stay with the domain
  that owns the resource.
- A submission form uses TanStack Form. Search filters, URL state and
  standalone controls are not forms.
- Do not introduce a private query, table or form abstraction layer.

The admin UI has no test suite. Verify frontend changes with the typecheck,
the build and a bounded browser check of the affected routes.

## Version Freeze

Internal library crates and `tools/db-init` stay at `0.1.0` forever.
Release commits must not bump them; only the root `prompt-ferry`
binary follows the `1.x` release line. Reviewers must check this
on every release commit.

## Pull Requests

Keep changes focused and include unit regression coverage for behavior changes.
Run `cargo fmt --all -- --check`, `bash scripts/ci/run-tests.sh`, and the
frontend typecheck/build before opening a pull request. Never include real
credentials, provider keys, private URLs, or local filesystem paths in source,
fixtures, documentation, or test output.

Security issues should follow [SECURITY.md](SECURITY.md), not a public issue.
