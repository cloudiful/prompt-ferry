# React foundation checks

The React foundation is a local, incomplete preview. Unported routes remain
placeholders and are not feature-parity coverage or release-ready functionality.
No settings route is a placeholder any more. The upstream, model-route, MCP,
relay, redaction, billing, approvals, request-record, general-settings,
requests-settings, network-settings, review-settings, storage-settings,
cache-alert-settings, and config-archive routes have separate focused
synthetic-fixture runs with current evidence recorded in the parity matrix.

## Run

From `frontend/`:

```bash
bun run test:foundation
bun e2e/foundation/verify-app-graph.ts
bash e2e/foundation/run-foundation.sh
bash e2e/foundation/run-endpoints.sh
bun e2e/foundation/validate-endpoints.ts
bash e2e/foundation/run-mcp.sh
bun e2e/foundation/validate-mcp.ts
bash e2e/foundation/run-billing.sh
bun e2e/foundation/validate-billing.ts
bash e2e/foundation/run-approvals.sh
bun e2e/foundation/validate-approvals.ts
bash e2e/foundation/run-usage.sh
bun e2e/foundation/validate-usage.ts
bash e2e/foundation/run-general-settings.sh
bun e2e/foundation/validate-general-settings.ts
bash e2e/foundation/run-settings-requests.sh
bun e2e/foundation/validate-settings-requests.ts
bash e2e/foundation/run-settings-network.sh
bun e2e/foundation/validate-settings-network.ts
bash e2e/foundation/run-settings-review.sh
bun e2e/foundation/validate-settings-review.ts
bash e2e/foundation/run-settings-storage.sh
bun e2e/foundation/validate-settings-storage.ts
bash e2e/foundation/run-settings-cache-alert.sh
bun e2e/foundation/validate-settings-cache-alert.ts
bash e2e/foundation/run-config-archive.sh
bun e2e/foundation/validate-config-archive.ts
```

The browser callback uses the stateful generated-type fixture at
`e2e/fixtures/` and the app served by Vite. It verifies Worker login errors and
redirects, admin/non-admin route access, desktop and mobile Worker logout
(including pending, failure, and retry), the foundation route IDs, locale and
theme persistence, responsive navigation, and independent relay-token login,
role change, restart, and logout. It uses synthetic fixture credentials only.
`run-endpoints.sh` verifies the mounted upstream admin route, endpoint writes,
OAuth and quota/history controls, and responsive rendering against the same
stateful fixture. It does not prove live persistence. The combined report and
route mapping are retained in `e2e/foundation/endpoints-result.json` and
`parity.matrix.ts`.
`run-mcp.sh` checks the authenticated owner-scoped MCP management page, provider
presets, masked secrets, stdio environment rows, catalog cache and filters,
mutation guards, retryable drafts, and responsive layouts against the same
synthetic fixture. The focused report does not prove live persistence or contact
MCP providers.
`run-billing.sh` checks the mounted authenticated billing route: the summary,
the charge list and detail, the filters and period, the admin price rules with
their create, retry, toggle, and delete flows, the reprice count, both CSV
exports with their real bytes, the ordinary-user self scope, and the responsive
layouts in both locales and themes against the same synthetic fixture. The
focused report does not prove live billing, pricing, or persistence.
`run-approvals.sh` checks the admin-only approvals route behind `AdminGuard`: the
pending default and resolved filter, the pager with its last-page correction and
mobile rule, the detail dialog and its allowed fields, approve and reject through
the shared `approval:<id>` gate, the delayed double trigger, the 503/409/404/403/401
error states, the latest-wins list and detail reads, the remaining-wait ticker and
its cleanup, and the responsive layouts in both locales and themes against the
same synthetic fixture. The focused report does not prove live approval queue or
persistence.
`run-general-settings.sh` checks the mounted general settings tab: the route
identity for an ordinary user and an admin, the ordinary-user nav exposing only
the general settings link, the three local preference cards, the immediate
language and theme effects on the document, storage, title and copy, their
persistence across a route change and a reload, the automatic zone, a filtered
searchable zone list, an explicit keyboard-selected zone reaching the records
time cell for the same instant, the choice surviving the return visit, the
automatic zone clearing it, and the contained card grid at 1440/1024/768/390 in
both locales. The tab issues no settings or business API request, so its
evidence is rendered state plus an empty request ledger rather than a server
contract.
`run-settings-requests.sh` checks the mounted admin requests settings tab: the
route identity and its four settings reads, every exposed control roundtrip with
its real autosave body, the retention write keeping the fields the tab does not
expose, debounce coalescing, one shared retention key with a single claimant
spinner and a queued draft submitted on release, an independent key staying
usable, a 503 draft with a section retry, an edit made during a write, the
scoped refresh, a read that started before an edit never reverting it, the
out-of-range loaded buffer size staying visible, both locales at four widths,
and the ordinary-user denial where the general and redaction settings still
work. The evidence is the synthetic fixture and its stored settings records.
`run-settings-network.sh` checks the mounted admin network settings tab: the
route identity and its single policy read, the two labelled monospace resizable
textareas with one description each, no autosave on edit, a manual save sending
both arrays with blank lines removed, the normalized response reaching the form
and the store, empty lists as explicit arrays, an IPv6/IPv4 roundtrip, a double
click sending one request, a draft typed during a delayed save surviving, the
400 validation error and 503 transport failure each reported once with the draft
retained and retried through Save, the scoped refresh blocked while dirty, the
fixture normalization and prefix bounds probes, both locales at four widths, and
the ordinary-user denial where the general and redaction settings still work.
The evidence is the synthetic fixture and its stored policy.
`run-settings-review.sh` checks the mounted admin review settings tab: the route
identity, all twelve controls with three native disclosure groups and the
PostgreSQL hint, password-only secrets that never reach text, titles or storage,
one read and no write on mount, no autosave, one manual save sending every field
plus the assembled webhook, the failure policy change, a locally rejected
malformed header line with no request, the server 400 with the draft retained and
a corrected retry, a disabled review bypassing validation, a double click sending
one request, a draft typed during a delayed save surviving, the 503 draft with
its Save retry, the scoped refresh blocked while dirty, both locales at four
widths, and the ordinary-user denial where the general and redaction settings
still work. Secret values appear only as presence booleans.
`run-settings-storage.sh` checks the mounted admin storage settings tab: the
route identity, the Local file/S3 compatible/Disabled branches with the server
defaults and the write-only credentials shown as presence badges only, one read
and no write on mount, no autosave, one manual save sending the whole object
with the server prefix and region normalization, keep patches for blank drafts
and replace patches for typed ones, no clear control while the API clear mode is
probed directly, a locally rejected missing bucket with no request, the server
400 for a missing bucket and an empty secret, the 501 capability answer as an
explicit warning with its retry read, a double click sending one request, a
draft typed during a delayed save surviving, the 503 draft with its Save retry,
the scoped refresh blocked while dirty or failed, both locales at four widths,
and the ordinary-user denial where the general and redaction settings still
work. Secret values appear only as presence booleans and patch modes; no real
directory, bucket, S3 provider or credential validation is claimed.
`run-settings-cache-alert.sh` checks the mounted admin cache-alert settings
tab: the route identity, the enabled switch, the four bounded numbers with the
float threshold exact and every help line once, the webhook URL, a password-only
write-only secret with both the server presence badge and the local keep/replace
badge, one read and no write on mount, no autosave, one manual save sending the
whole policy with a blank secret keeping the stored one, a trimmed replace whose
draft then resets to keep, no clear control, the four server 400s, a partial
payload taking the backend defaults, the 501 capability answer as an explicit
warning with its retry read while the page refresh stays disabled, a double click
sending one request, a value typed during a delayed save surviving, the 503 draft
with its Save retry, the scoped refresh blocked while dirty or failed, both
locales at four widths, and the ordinary-user denial where the general and
redaction settings still work. Secret values appear only as presence booleans;
the fixture has no path that empties a stored secret, so a false presence badge is
covered by the component test rather than the browser run. No alert detection or
DingTalk delivery is claimed.
`run-config-archive.sh` checks the mounted admin config archive route: the
export card, the import wizard and the nine-column audit table from one audit
read and no write on mount, the paged audit table with a distinct second page and
the persisted page size, one export that saves the real browser download under
the fixed `prompt-ferry-config.pfce` name despite a server-chosen
`Content-Disposition` and reports the response backend, format, fingerprint and
exact byte length, importing that downloaded archive back with every per-domain
count and warning and no audit row from the preview, the destructive
confirmation opening with zero requests and cancelling without a write, the
confirmed apply with its applied summary and audit row, a wrong passphrase, a
corrupt header, an unsupported version and a foreign backend each answering the
codec's own code while the file survives, a 503 keeping the confirmed wizard for
a same-body retry, a double export sending one request, the three distinct
capability codes, both locales at four widths with the audit table panning
instead of dropping columns, and the ordinary-user denial. The archive is the
fixture's explicitly synthetic `PROMPT_FERRY_SYNTHETIC_ARCHIVE` envelope, not the
production `.pfce`; passphrases and the archive body appear only as presence
flags and lengths. The validator removes the disposable downloaded copy after
checking it.
`run-usage.sh` checks the mounted request-record routes: the AI overview with
its cards, chart, distribution and upstream drilldown, the local view mode, the
record list with pagination, page size, search, filters and the preset and
custom range windows, the delayed loading surface, the error retry, the stale
read suppression, the zero-token fold, the wrapped and pinned detail headers in
both locales at four widths, the transcript paging, the error and abort blocks,
the admin session override and affinity writes with their confirmation and
pending guard, the history clear with its validation errors, the ordinary-user
scope, and the MCP route with its scope, payload detail, and error link. The
focused report does not prove live record persistence, and the fold and header
evidence covers the model perspective only because the fixture seeds no endpoint
with more than one model and no breakdown row with an unknown token total.
The runner reuses only a listener that identifies itself as the expected fixture
or React entry; it never stops an unknown listener. It starts and stops only the
servers and Playwright session it owns.

Screenshots and the callback report are retained under `e2e/foundation/`.
The fixture proves rendered UI and request behavior, not backend persistence.
It makes no production, account, or database requests.

`verify-app-graph.ts` starts at `src/main.tsx`, follows reachable TypeScript and
TSX imports, inspects the production module graph emitted by the Vite build,
rejects Vue/Nuxt modules and `.vue` references, and checks for exactly one
`createRoot` call and one HTML module entry. Legacy Vue sources and their
dependencies remain outside the active app graph during this foundation
checkpoint.
