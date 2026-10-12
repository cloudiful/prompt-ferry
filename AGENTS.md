# Agent Instructions

## Test databases and temporary files

- Use dedicated test-service settings from the project-root `.env`: `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` for PostgreSQL quota integration tests and `PROMPT_FERRY_TEST_VALKEY_URL` for Valkey tests. Never substitute the application `DATABASE_URL` or application Valkey settings. Do not read or print unrelated secrets.
- Run test commands through the documented entrypoints: `bash scripts/ci/run-tests.sh` for the full workspace run, `bash scripts/ci/with-test-env.sh <command>` for a focused command. A direct `cargo test` does not load `.env`; it sees only variables already exported in the environment. With them unset, the Valkey response-affinity tests and the quota snapshot/service tests return early and report themselves as skipped, while `tests/support/sql_lookup_regression.rs` fails outright because it requires `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL`; with them exported, the tests connect to whatever those variables name. So a bare `cargo test` result proves nothing about service-backed coverage in either direction. The entrypoints parse `.env` as data and never execute it, and export only the dedicated test keys. If required settings or services are missing, report the prerequisite instead of claiming complete database integration coverage.
- PostgreSQL quota tests run against exactly one database: the one `PROMPT_FERRY_QUOTA_TEST_DATABASE_URL` addresses and the one `PROMPT_FERRY_QUOTA_TEST_CONFIRMED_TARGET` confirms as `host:port/database`. Both keys are required and must agree, and the guard fails closed on a mismatch or a malformed value, so pointing the test URL at another service — a production database included — is refused rather than used. The tests never read `DATABASE_URL`. Confirming a new target is an operator decision: reuse that test database with fixture cleanup, do not create another cluster per agent session, and never connect to a production database. Provisioning and destructive cleanup require explicit authorization. The full runner preflights it, confirms the connected database's own identity and its tables, and never migrates or provisions it.
- Tests that need only a pool handle and issue no query use the lazy `pfy_test_*`-schema fixture from `tests/support/test_db_url.rs`, which never connects; only a test that really queries takes the confirmed dedicated target.
- SQLite fixtures create separate temporary files and do not consume a shared SQLite database URL. The entrypoints export `PROMPT_FERRY_TEST_TMPDIR` as an absolute project-local `TMPDIR` (default `<repository>/target/test-tmp`) before tests start, creating the directory and never purging it. This also keeps SQLite WAL/SHM sidecars and interrupted-test leftovers in one disposable location.
- Keep test fixtures isolated and close pools before cleanup. Remove only confirmed project-owned temporary files after test processes have exited; never broadly purge `/tmp` or shared cache directories. Do not silently skip configured database tests or describe skipped tests as verified.

## Release versions

For a `vX.Y.Z` application release, set both versions to `X.Y.Z` (without the `v`):

- Root `Cargo.toml` → `[package].version`
- `frontend/package.json` → `version`

Keep these values aligned with each other and the tag. Internal library crates and `tools/db-init` stay at `0.1.0`; do not bump them. `openapi/admin-api.yaml` has its own API contract version. Do not commit `Cargo.lock` or `frontend/bun.lock`.

## Frontend async actions

- Route remote mutations through one store/domain-owned `useAsyncAction` guard per exclusion unit; writes that conflict on one resource share its key. Capture identifiers and payloads before awaiting, and keep the key through required refreshes.
- Propagate pending state to the conflicting controls, disable duplicate actions and dialog dismissal, and show a spinner only on the action that claimed the key. Keep unrelated resources available and preserve retryable errors and drafts.
- Perform success notifications, navigation, dialog close, or draft reset only for an outcome with `executed: true`. Do not create a component-local guard around a shared store action.
- Keep read/search/pagination loading separate from mutation gates. For an independent async read button, Nuxt UI `loading-auto` is valid only when the handler returns the actual promise; it does not replace a shared mutation guard.
