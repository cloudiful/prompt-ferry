# Agent Instructions

## Testing

- The backend suite is unit-only: every test is pure or in-memory and connects to no database, Valkey, S3, HTTP/socket server, or subprocess. Do not add service-, filesystem-, or environment-backed tests.
- Run it with `bash scripts/ci/run-tests.sh` (runs `cargo test --workspace --lib`, checks the summary, prints elapsed time) or directly with `SQLX_OFFLINE=true cargo test --workspace --lib`. No `.env`, provisioned service, or test-only setting is needed.
- `cargo fmt --all -- --check` and `SQLX_OFFLINE=true cargo check --workspace` are the other backend gates.
- The frontend has no test suites; `bun run typecheck` and `bun run build` remain.
- Keep fixtures isolated and remove only confirmed project-owned temporary files after test processes have exited; never broadly purge `/tmp` or shared cache directories.

## Release versions

For a `vX.Y.Z` application release, set both versions to `X.Y.Z` (without the `v`):

- Root `Cargo.toml` → `[package].version`
- `frontend/package.json` → `version`

Keep these values aligned with each other and the tag. Internal library crates and `tools/db-init` stay at `0.1.0`; do not bump them. `openapi/admin-api.yaml` has its own API contract version. Do not commit `Cargo.lock` or `frontend/bun.lock`.

## Frontend async actions

- Route remote mutations through one store/domain-owned `useAsyncAction` guard per exclusion unit; writes that conflict on one resource share its key. Capture identifiers and payloads before awaiting, and keep the key through required refreshes.
- Propagate pending state to the conflicting controls, disable duplicate actions and dialog dismissal, and show a spinner only on the action that claimed the key. Keep unrelated resources available and preserve retryable errors and drafts.
- Perform success notifications, navigation, dialog close, or draft reset only for an outcome with `executed: true`. Do not create a component-local guard around a shared store action.
- Keep read/search/pagination loading separate from mutation gates. An independent async read button keeps its own pending state, and its spinner is honest only when the handler returns the actual promise; it does not replace a shared mutation guard.
