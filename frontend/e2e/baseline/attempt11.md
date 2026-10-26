# P0 attempt 11

The current combined evidence is run ID
`p0-2026-10-23T19:40:19.490Z-1792785164058`. All 18 explicitly enumerated
scenario rows from recon #20733 are authoritative; this attempt adds no scope
beyond them and the stated cross-cutting coverage mappings.

## Browser assertions

`attempt11-missing.playwright` exercises API-key disable/enable with exact
payloads and refreshed state, a held same-key guard, distinct page IDs/query,
endpoint create/update 503 drafts with same-payload retry, endpoint list error
and recovery, model-route and relay conflict guards, a built-in redaction rule
payload plus server-failure retry, redaction-rule page IDs/query, preview guard,
AI overview loading/recovery, billing-rule toggle/guard, and approval guard and
list-error retry.

`attempt11-settings.playwright` asserts 503 draft retention and same-payload
200 retry for request-content logging, usage retention, stream-delta batching and
LLM review; confirms raw-retention forms share a held exclusion; and switches the
rendered settings UI to English/light before checking the translated UI.

`attempt11-long-values.playwright` checks visible/contained long values on
API-key, user, endpoint, model-route, MCP, AI usage and billing routes.
`mcp-maintenance.playwright` now also asserts the MCP records page query and
that page two contains a different row set.

The redaction save failure retained the changed checkbox and displayed the
injected error, but no unsaved badge was visible. The report records
`dirtyBadge: false` and does not claim one appeared.

## Current-run validation

- `bash e2e/baseline/run-baseline.sh` — 143 screenshots, 38 scenario flows;
  all 152 required matrix pairs have current-run coverage; strict matrix
  validation passed.
- `bun run format:check` — passed.
- `bun run typecheck` — passed.
- `bun run test` — 4 files / 41 tests passed.
- `bun test tests` — 883 tests across 115 files passed (existing Vue Test Utils warnings were emitted).
- `bun x tsgo --ignoreConfig --noEmit --strict --target ES2022 --module ESNext --moduleResolution Bundler --allowImportingTsExtensions --skipLibCheck e2e/baseline/scenario-coverage.ts e2e/parity.matrix.ts` — passed.
- `bun run e2e/baseline/validate-matrix.ts` — passed after the combined run.
- `git diff --check` — passed.
