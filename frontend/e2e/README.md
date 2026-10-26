# Frontend end-to-end baseline

Issue #1060's executable parity contract is `parity.matrix.ts`. The current
full-app browser run records screenshots, request evidence and a per-scenario
coverage map in `baseline/evidence.json`.

The baseline uses the Vue app with a stateful in-memory fixture API. It does not
use production accounts, endpoints, databases or credentials, and it does not
prove persistence or upstream behavior.

## Run

From `frontend/`:

```bash
bash e2e/baseline/run-baseline.sh
```

The script reuses listeners already bound to `127.0.0.1:8789` and
`127.0.0.1:5171`; it starts and stops only servers it owns. It opens one named
Chromium session (`pf-baseline`), runs the full route capture and browser
scenario callbacks serially with fixture resets, merges their reports, then
runs `bun run e2e/baseline/validate-matrix.ts`. The Vite proxy remains unchanged.

For an isolated callback, start the fixture server and Vite dev server using the
commands above, open the named Playwright CLI session, then use:

```bash
bun x --package @playwright/cli playwright-cli -s=pf-baseline --raw run-code --filename=e2e/baseline/attempt11-missing.playwright
```

The other attempt-11 callbacks are `attempt11-settings.playwright` and
`attempt11-long-values.playwright`. Focused callback results are diagnostic;
only the current combined run is the baseline evidence for matrix promotion.

## Fixture API

`e2e/fixtures/` implements a typed, stateful mock of the generated admin API.
Each reset restores seeded users, keys, models, endpoints, routes, MCP servers,
relays, approvals, billing records, request records, settings, redaction rules,
archive audit rows and relay-host state.

- `handlers/*.ts` implement the API reads and writes and use generated response
  types and the application's error envelope.
- Unimplemented `/api/` paths return `404 unmocked_endpoint`; they never silently
  fall through to success.
- Faults can inject bounded errors and delays. The request ledger supplies
  methods, paths, queries, statuses and durations for mutation and pagination
  assertions.

Control endpoints are CORS-enabled for browser callbacks:

| Request                       | Effect                                                                     |
| ----------------------------- | -------------------------------------------------------------------------- |
| `POST /__fixtures/reset`      | Restore fixture state and clear faults                                     |
| `POST /__fixtures/faults`     | Inject `{pathPrefix, method?, status?, code?, message?, delayMs?, count?}` |
| `DELETE /__fixtures/faults`   | Clear injected faults                                                      |
| `GET /__fixtures/log?limit=N` | Read a bounded request ledger                                              |
| `POST /__fixtures/overview`   | Set AI/MCP overview state for `{category, empty?, unknownTokens?}`         |
| `GET /__fixtures/summary`     | Read fixture collection counts                                             |

## Current evidence

The current `evidence.json` has a unique run ID and ordered callback manifest.
Each residual report carries the same run ID and timestamps as the manifest.
`scenarioCoverage` maps every required `ROW:scenario` pair to a current-run
assertion; the validator rejects missing or unknown pairs, unresolved evidence
pointers, callback failures and stale reports. The matrix has 20
`baseline-verified` rows. `R-USAGE-AI` and `R-BILLING` remain partial only because
their separately recorded baseline defects are mandatory React repairs.

The combined callbacks assert, among other behavior:

- route and role access, redirects, relay-token independence, locale and theme;
- create/update/delete/toggle/reconnect, failure drafts, retries and shared
  mutation guards;
- pagination query ranges and distinct row sets, loading/error recovery, stale
  response suppression, archive and CSV files, and MCP history maintenance;
- long-value visibility and containment on API keys, users, endpoints, model
  routes, MCP, AI usage and billing;
- zero-token folding and the existing header-overflow measurement.

## Approved baseline defects

- `R-USAGE-AI` sorting does not change `aria-sort` or issue a sort request.
  React must add accessible sortable headers, keyboard activation,
  `aria-sort`, and `sort_field`/`sort_order` mapping (`REACT-AI-SORT-ACCESSIBLE`).
- The billing charge pager works, but the price-rule pager issues no request and
  changes no rows. React must restore independent rule pagination
  (`REACT-BILLING-RULES-PAGINATION`).

These findings are not passing coverage or waivers. Their observed behavior,
source evidence and required fixes remain in the matrix and current evidence.

## Parity contract

`parity.matrix.ts` is the migration acceptance source:

- `ROUTES` maps each route's role, reads, shared mutation keys, legacy tests,
  required scenarios, gate, current status and screenshot evidence.
- `REDIRECTS` contains the route redirects; `SCENARIOS` is the required scenario
  vocabulary.
- `classifyLegacyTest` maps every legacy test module and form-mapper suite. No
  test is deleted until its observable React counterpart exists.

`validate-matrix.ts` checks route/nav drift, redirect and role evidence,
callback inventory and failures, current-run timestamps, scenario coverage,
known baseline defects, screenshots, downloads and legacy test mapping. A green
validator means the current baseline assertions passed; it does not certify live
backend persistence.

The React target must preserve the route/action matrix, role boundaries, API
contracts and mutation exclusion keys. The zero-token behavior and two required
baseline defect repairs are described in the architecture and parity documents.
