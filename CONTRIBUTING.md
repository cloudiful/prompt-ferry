# Contributing

Thanks for contributing to `prompt-ferry`. The public project uses native Rust
and Bun tooling, Valkey for the service-backed integration tests, and generated
OpenAPI files as checked-in contract artifacts.

## Prerequisites

- Rust stable with Cargo
- Bun 1.3.11 or a compatible Bun 1.x release
- Bash 4 or newer for the local development scripts
- PostgreSQL client (`psql`) for the quota test database preflight
- Valkey and a dedicated PostgreSQL test database, confirmed as
  `host:port/database` in `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET`, for the
  service-backed integration tests; both are provisioned outside this
  repository, and no script here creates, migrates, or starts them

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

A direct `cargo test` does not load `.env`. Run the service-backed suite through
the documented entrypoints below.

Frontend checks:

```bash
cd frontend
bun install --no-save
bun run format:check
bun run typecheck
bun run test
bun run build
```

The admin UI is embedded into the binary from `frontend/dist`, so run the
frontend build before `cargo build` when you want the UI in a local binary. A
source checkout without `frontend/dist` still compiles and serves a notice
page instead of the UI. Frontend iteration can skip the Rust rebuild entirely
by pointing `PROMPT_FERRY_FRONTEND_DIST` at a `dist` directory.

Database-shaped test helpers build lazy pools and URLs pinned to a fresh
isolated `pfy_test_*` schema through an explicit `search_path`, so a test never
reaches a shared schema and the suite needs no `DATABASE_URL`. A test that only
needs a pool handle and issues no query uses that lazy fixture; only a test that
really queries takes the confirmed dedicated target through
`tests/support/test_pg_target.rs`.
PostgreSQL migrations are validated in the development environment with
`cargo run --bin db_init`; do not use the `public` schema by hand or edit
`_sqlx_migrations`.

## Test Services

Two test-only settings come from the project `.env`:
`PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` names the dedicated PostgreSQL database
the quota integration tests use, and
`PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET` confirms the `host:port/database`
that URL is allowed to address.
`PROMPT_FERRY_TEST_VALKEY_URL` names the Valkey instance the response-affinity
tests use. A bare `cargo test` loads no `.env`, so those tests see only variables
already exported in the environment: unset, the Valkey and quota
snapshot/service tests return early and report themselves as skipped, while
`tests/support/sql_lookup_regression.rs` fails outright because it requires the
quota URL; exported, they connect to whatever those variables name. Load them
through the wrapper instead:

```bash
bash scripts/ci/run-tests.sh                                # full workspace run
bash scripts/ci/with-test-env.sh cargo test -p prompt-ferry   # focused run
```

The wrapper reads `.env` as data (it is never executed) and exports only those
three settings plus `PROMPT_FERRY_TEST_TMPDIR`, an absolute project-local
`TMPDIR` (default `<repo>/target/test-tmp`) that holds the SQLite fixtures. It
never exports `DATABASE_URL` or any provider key. The full runner additionally
preflights both services before cargo starts — reachable Valkey, plus a
reachable, authenticated, migrated database whose own identity matches the
confirmed target — so an unavailable service fails the run instead of being
skipped; it never provisions or migrates anything.
A focused SQLite-only run needs no database service, but it cannot claim
PostgreSQL coverage. `scripts/ci/README.md` documents the resolution rules and
the preflight.

Both the preflight and the Rust test helper apply the same rule: the dedicated
URL and the confirmed target must both be set and must agree on
`host:port/database`, and a malformed or mismatched pair fails closed. That is
what keeps the suite off any other server — a production database included —
when only the URL is repointed. Confirming a different target is an operator
decision, so change `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET` deliberately and
provision and migrate that database yourself.

## Generated Contracts

The backend is the source of truth for the admin OpenAPI document:

```bash
cargo run -- openapi export
cd frontend
bun run openapi-ts
```

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
