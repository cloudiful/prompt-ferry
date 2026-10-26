# P0 current baseline ledger

The current combined run is `p0-2026-10-23T19:40:19.490Z-1792785164058` in
`evidence.json`. It completed 18 ordered callbacks, captured 143 screenshots and
38 scenario flows, and passed `bun run e2e/baseline/validate-matrix.ts` with 152
of 152 required `ROW:scenario` entries mapped to passing current-run assertions.

Twenty route rows are `baseline-verified`. Two remain `baseline-partial` only for
the approved baseline defects, not because required matrix scenarios are missing:

- `R-USAGE-AI` sorting is inert; the React target fix is
  `REACT-AI-SORT-ACCESSIBLE`.
- `R-BILLING` price-rule pagination dispatches no request; the React target fix
  is `REACT-BILLING-RULES-PAGINATION`. Charge pagination is verified.

Attempt 11 adds browser evidence for key toggle/guard/pagination; endpoint
create/update failure-and-retry and list-error retry; model-route, relay,
redaction-preview, billing-rule and approval guards; redaction toggle/failure
retry and paginated row sets; AI usage loading; per-section request-settings
failure/retry and shared-retention exclusion; LLM-review failure/retry; actual
locale/theme changes; and long-value containment on seven routes. MCP usage
pagination now asserts the page query and distinct visible row set. On redaction
save failure, the changed rule and error were retained; the UI did not show an
unsaved badge (`dirtyBadge: false`), which is recorded as observed.

The current evidence is fixture-backed UI and request behavior, not live
persistence or upstream verification. Focused historical reports remain under
`baseline/attempt*.md` and their JSON files; `evidence.json` and its run manifest
are authoritative for the latest combined run.

Final non-browser gates passed: `bun run format:check`, `bun run typecheck`,
`bun run test` (41 Vitest tests), `bun test tests` (883 tests), focused strict
`tsgo` checking of the scenario-coverage helper and matrix, a second matrix
validation, and `git diff --check`.
