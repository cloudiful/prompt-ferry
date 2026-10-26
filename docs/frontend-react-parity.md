# React migration feature parity

Executable acceptance for issue #1060. The machine-readable source is
`frontend/e2e/parity.matrix.ts`; the browser evidence is
`frontend/e2e/baseline/evidence.json` plus its screenshots. This document is the
readable inventory and the rules a port has to keep.

Baseline: `origin/main` `6fe05da10202be5c84dad9abdf9faf4b7be1a522` (v1.42.30),
the Vue SPA that the Rust admin server embedded at that commit. That capture is
frozen history: the React SPA is what the server embeds now, and the baseline
files under `frontend/e2e/baseline/**` are never regenerated in place. Evidence
records: recon 20315 (inventory) and 20316 (peer-range and fold corrections).
Baseline screenshots compare functionality, not pixels.

## Route matrix

`authenticated` means any signed-in Worker user; `admin` means the route is
admin-only and a non-admin is redirected to the first visible section; `public`
is `/login`; `relay-session` authenticates against the relay management token
instead of a Worker session.

| ID                        | Route                      | Scope         | Domain           | Gate | Status   | Sources / actions                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------- | -------------------------- | ------------- | ---------------- | ---- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R-LOGIN                   | `/login`                   | public        | auth             | P3   | verified | Baseline subject: `LoginPage.vue`, `stores/session.ts`, `router.ts` guards. React: `src/pages/LoginPage.tsx`, `src/lib/session.ts`; login name, password reveal, validation/error, redirect, locale toggle, logout                                                                                                                                                                                                                                                                                        |
| R-API-KEYS                | `/api-keys`                | authenticated | api-keys         | P4   | verified | Baseline subject: `ApiKeysPage.vue`, `stores/api-keys*`. React: `src/pages/ApiKeysPage.tsx`; owner-scoped list/create/reveal/copy/rename/toggle/delete and pagination                                                                                                                                                                                                                                                                                                                                     |
| R-AVAILABLE-MODELS        | `/available-models`        | authenticated | available-models | P4   | verified | Baseline subject: `AvailableModelsPage.vue`, `stores/available-models.ts`. React: `src/pages/AvailableModelsPage.tsx`; read-only model table, refresh and pagination                                                                                                                                                                                                                                                                                                                                      |
| R-USERS                   | `/users`                   | admin         | users            | P4   | verified | Baseline subject: `UsersPage.vue`, `stores/users.ts`. React: `src/pages/UsersPage.tsx`; list, create, edit/save, reset password, delete, pagination                                                                                                                                                                                                                                                                                                                                                       |
| R-ENDPOINTS-UPSTREAMS     | `/endpoints/upstreams`     | admin         | endpoints        | P4   | verified | Baseline subject: `EndpointsPage.vue`, `stores/endpoints*`, `stores/endpoint-actions.ts`. React: `src/pages/EndpointsPage.tsx`; list/cards, enable, test, edit/delete, API-key rows, OAuth, token-plan/quota dialogs                                                                                                                                                                                                                                                                                      |
| R-ENDPOINTS-ROUTES        | `/endpoints/routes`        | admin         | model-routes     | P4   | verified | Baseline subject: `EndpointsPage.vue`, `stores/endpoint-actions.ts`. React: `src/pages/ModelRoutesPage.tsx`, model-route components and `endpoints-routes.playwright`; list, editor, probe, ordered targets/settings and keyboard/drag reorder                                                                                                                                                                                                                                                            |
| R-MCP                     | `/mcp`                     | authenticated | mcp              | P4   | verified | Baseline subject: `McpPage.vue`, `stores/mcp*`. React: `src/pages/McpPage.tsx`; list, create/edit, provider/auth/transport/filter/lifecycle/proxy, test, toggle, delete, catalog                                                                                                                                                                                                                                                                                                                          |
| R-RELAYS                  | `/relays`                  | admin         | relays           | P4   | verified | Baseline subject: `RelaysPage.vue`, `stores/relays*`. React: `src/pages/RelaysPage.tsx`; list/pagination, TLS/bridge editor and secret flags, reconnect, delete, retained Worker-list error state                                                                                                                                                                                                                                                                                                         |
| R-RELAY-HOST              | `/relay-host`              | relay-session | relay-host       | P3   | verified | React `src/pages/RelayHostPage.tsx` and `src/react-tests/relay-host.test.tsx`; independent token login/logout, host/status/settings, role cards, admin-bind save, restart banner/actions, worker-disconnected notice; never Worker admin                                                                                                                                                                                                                                                                  |
| R-REDACTION-RULES         | `/redaction/rules`         | authenticated | redaction        | P5   | verified | Baseline subject: `RedactionPage.vue`. React: `src/pages/RedactionPage.tsx`, `src/lib/redaction.ts`, `src/components/redaction/RedactionRulesPanel.tsx`; built-ins, global/user scope, custom rule editing, search/paging, warnings, reveal, guarded save, unsaved draft shared with the preview route                                                                                                                                                                                                    |
| R-REDACTION-PREVIEW       | `/redaction/preview`       | authenticated | redaction        | P5   | verified | Baseline subject: `RedactionPage.vue`. React: `src/pages/RedactionPage.tsx`, `src/components/redaction/RedactionPreviewPanel.tsx`, `src/lib/redaction-preview.ts`; input kind, preview run, statistics, findings/replacements, selection highlight, copy, stale-response protection                                                                                                                                                                                                                       |
| R-USAGE-AI                | `/request-records/ai`      | authenticated | usage-ai         | P4   | partial  | React `src/pages/UsagePage.tsx`, `src/components/usage/*`, `src/lib/usage*`; overview metrics/trend/distribution, upstream detail fold, records table/filters/sort/pagination, detail dialog/transcript/session routing; browser evidence in `e2e/foundation/usage.playwright`; partial only because the fixture has no multi-model endpoint and no unknown-token breakdown row                                                                                                                           |
| R-USAGE-MCP               | `/request-records/mcp`     | authenticated | usage-mcp        | P4   | verified | React `src/pages/UsagePage.tsx`, `src/components/usage/*`, `src/lib/usage*`; MCP overview scope/gateway/service, metrics/trend/distribution, records, usage detail, clear history; browser evidence in `e2e/foundation/usage.playwright`                                                                                                                                                                                                                                                                  |
| R-BILLING                 | `/billing`                 | authenticated | billing          | P5   | partial  | Baseline subject: `BillingPage.vue`, `stores/billing*`. React: `src/pages/BillingPage.tsx`, `src/components/billing/*`, `src/lib/billing.ts`; period/filter summary, charges/detail, price rules (admin), reprice, CSV details/monthly                                                                                                                                                                                                                                                                    |
| R-APPROVALS               | `/approvals`               | admin         | approvals        | P5   | verified | Baseline subject: `ApprovalsPage.vue`, `stores/approvals*`. React: `src/pages/ApprovalsPage.tsx`, `src/components/approvals/*`, `src/lib/approvals.ts`; pending/resolved, detail, approve/reject, wait/status, pagination                                                                                                                                                                                                                                                                                 |
| R-SETTINGS-GENERAL        | `/settings/general`        | authenticated | settings         | P3   | verified | React `src/pages/SettingsPage.tsx`, `src/components/settings/SettingsGeneralTab.tsx`, `SettingsCard.tsx`, `SettingsTimeZoneSelect.tsx`; language, theme and searchable display time zone bound to the existing locale/theme/zone providers, with browser evidence in `e2e/foundation/general-settings.playwright`                                                                                                                                                                                         |
| R-SETTINGS-REQUESTS       | `/settings/requests`       | admin         | settings         | P5   | verified | React `src/pages/SettingsPage.tsx`, `src/components/settings/SettingsRequestsTab.tsx`, `src/components/shared/StreamDeltaBatchingFields.tsx`, `src/lib/settings-requests*.ts`, `src/lib/settings-keys.ts`; content logging/raw retention, usage retention, stream batching, model-route whitelist with autosave, one shared retention key and a scoped refresh; browser evidence in `e2e/foundation/settings-requests.playwright`                                                                         |
| R-SETTINGS-NETWORK        | `/settings/network`        | admin         | settings         | P5   | verified | React `src/components/settings/SettingsNetworkTab.tsx`, `src/lib/settings-network*.ts`; relay IP allowlist and trusted proxy CIDRs with a manual guarded save, per-field accepted-value merge and a policy-scoped refresh; browser evidence in `e2e/foundation/settings-network.playwright`                                                                                                                                                                                                               |
| R-SETTINGS-REVIEW         | `/settings/review`         | admin         | settings         | P5   | verified | React `src/components/settings/SettingsReviewTab.tsx`, `LlmReviewSettingsCard.tsx`, `src/lib/settings-review*.ts`; LLM review enable/webhook/headers/policy with a manual guarded save, leaf-level accepted-value merge and a review-scoped refresh; browser evidence in `e2e/foundation/settings-review.playwright`                                                                                                                                                                                      |
| R-SETTINGS-STORAGE        | `/settings/storage`        | admin         | settings         | P5   | verified | React `src/components/settings/SettingsStorageTab.tsx`, `SettingsStorageCredentials.tsx`, `src/lib/settings-storage*.ts`; raw object store local/S3/disabled branches with write-only secrets as keep/replace patches, an explicit capability-unavailable state with retry and a storage-scoped refresh; browser evidence in `e2e/foundation/settings-storage.playwright`                                                                                                                                 |
| R-SETTINGS-CACHE-ALERT    | `/settings/cache-alert`    | admin         | settings         | P5   | verified | React `src/components/settings/SettingsCacheAlertTab.tsx`, `src/lib/settings-cache-alert*.ts`; enabled switch, bounded window/min-turns/cooldown and float threshold, webhook and a write-only secret with server presence plus a local keep/replace badge, a manual guarded PUT save, an explicit capability-unavailable state with retry and a cache-alert-scoped refresh; browser evidence in `e2e/foundation/settings-cache-alert.playwright`                                                         |
| R-SETTINGS-CONFIG-ARCHIVE | `/settings/config-archive` | admin         | settings         | P5   | verified | React `src/components/settings/SettingsConfigArchiveTab.tsx`, `ConfigArchiveExportCard.tsx`, `ConfigArchiveImportCard.tsx`, `ConfigArchiveImportPreview.tsx`, `ConfigArchiveImportConfirmDialog.tsx`, `ConfigArchiveAuditTable.tsx`, `src/lib/config-archive*.ts`; passphrase export with the fixed download name, import file/preview/destructive confirm/apply, the transient keep-and-clear secret lifecycle and the paged audit trail; browser evidence in `e2e/foundation/config-archive.playwright` |

`partial` means the route and role boundary are baseline-verified but the row's
remaining required scenarios are listed in the matrix `notes`. `verified` means
every required scenario for that row ran in the baseline. No row is claimed
verified without files in `evidence`. The `status` field in `parity.matrix.ts`
is the Vue-baseline status and stays history; the React coverage is reported by
the focused foundation results under `frontend/e2e/foundation/*-result.json`.

## React foundation checks

Most route rows map `foundationTest` to `e2e/foundation/auth-shell.playwright`,
which verifies the shared login/redirect/role boundary, relay-token shell,
responsive navigation, and route identity. `R-API-KEYS` and
`R-AVAILABLE-MODELS` map to `e2e/foundation/api-keys-models.playwright`, and
`R-USERS` maps to `e2e/foundation/users.playwright`, `R-ENDPOINTS-ROUTES`
maps to `e2e/foundation/endpoints-routes.playwright`, `R-RELAYS` maps to
`e2e/foundation/relays.playwright`, and the billing, approvals, and
request-record rows map to `e2e/foundation/billing.playwright`,
`e2e/foundation/approvals.playwright`, and `e2e/foundation/usage.playwright`,
the general settings row maps to
`e2e/foundation/general-settings.playwright`, the requests settings row maps to
`e2e/foundation/settings-requests.playwright`, the network settings row maps to
`e2e/foundation/settings-network.playwright`, the review settings row maps to
`e2e/foundation/settings-review.playwright`, the storage settings row maps to
`e2e/foundation/settings-storage.playwright`, the cache alert row maps to
`e2e/foundation/settings-cache-alert.playwright`, and the config archive row maps
to `e2e/foundation/config-archive.playwright`;
these focused runs check React behavior
against the stateful fixture. A `data-foundation-placeholder`
marker proves only that an unported route remains reachable; it is not feature
parity. The baseline `status` and screenshot evidence are frozen history of the
Vue capture; focused React results are stored separately.

No domain placeholder remains: every route in the matrix is a mounted React
port. The archive evidence uses the synthetic fixture archive format described in
`e2e/fixtures/config-archive.ts`, not the production `.pfce` encoding, and its
audit empty-tail page correction and 32 KB base64 chunking are covered by the
unit suite rather than the browser run.
`e2e/foundation/run-general-settings.sh` records the general settings route's
focused React coverage in `e2e/foundation/general-settings-result.json`: the
authenticated route identity for an ordinary user and an admin, the
ordinary-user nav exposing only the general settings link, the three preference
cards with no server save, refresh or read, the immediate language effect on the
document language, storage, title and copy, the immediate theme effect on the
document class, colour scheme and storage, both surviving a route change and a
reload, the automatic zone naming the browser zone, a filtered searchable zone
list that never rewrites the stored preference, an explicit keyboard-selected
zone persisting to storage, the records time cell rendering the selected zone
for the same instant, the general choice surviving the return visit, the
automatic zone clearing the stored choice, and contained card grids at
1440/1024/768/390 in both locales. The report also proves the tab issues no
settings or business API request while it is settled.

`e2e/foundation/run-settings-review.sh` records the admin review settings
route's focused React coverage in `e2e/foundation/settings-review-result.json`:
the mounted route with its twelve controls, three native disclosure groups and
the PostgreSQL storage-mode hint, password-only secrets that never reach text,
titles or storage, a single read and no write on mount, no autosave, one manual
save sending every field plus the assembled webhook with first-colon header
values preserved, the failure policy switching to `fail_open`, a locally rejected
malformed header line with zero requests, the server 400 for a base URL ending in
`/v1` with the draft retained and a corrected retry, a disabled review accepting
semantically invalid values, a double click sending one request, a draft typed
during a delayed save surviving, a 503 reported once with a Save retry, the
scoped refresh blocked while dirty, no refetch on a locale/theme rerender, both
locales at 1440/1024/768/390, and the full ordinary-user denial with the general
and redaction settings still reachable. Secret values are reported as presence
booleans only. The evidence is the synthetic fixture; no PostgreSQL review
execution, provider call or webhook delivery is claimed.

`e2e/foundation/run-settings-network.sh` records the admin network settings
route's focused React coverage in `e2e/foundation/settings-network-result.json`:
the mounted route with its two labelled monospace resizable textareas and one
description each, a single policy read and no write on mount, no autosave on
edit, a manual save sending both arrays with blank lines removed, the normalized
policy replacing the form and the stored record, empty lists as explicit empty
arrays, an IPv6/IPv4 roundtrip, a double click sending one request and one
confirmation, a draft typed during a delayed save surviving with the scoped
refresh blocked, a 400 validation error shown once with the draft retained and a
corrected retry, a 503 transport failure reported once with a Save retry, no
refetch on a locale/theme rerender, fixture probes for malformed IPs,
out-of-range prefixes, field-named errors, omitted arrays and duplicate/blank
normalization, both locales at 1440/1024/768/390, and the full ordinary-user
denial with the general and redaction settings still reachable. The fixture
normalizes and validates the relay policy the way the backend does. The evidence
is the synthetic fixture and its stored policy; no live relay enforcement is
claimed.

`e2e/foundation/run-settings-requests.sh` records the admin requests settings
route's focused React coverage in `e2e/foundation/settings-requests-result.json`:
the mounted route reading only the four requests-scoped settings, every exposed
control roundtrip with its real autosave body, the retention write carrying the
fields this tab does not expose, the debounce coalescing, one shared retention
key that disables both forms with a single claimant spinner and submits the held
draft once on release, an independent key staying usable, a 503 that keeps the
draft and offers a section retry, an edit made during a write, the scoped
refresh, a read that started before an edit never reverting it, the out-of-range
loaded buffer size preserved, both locales and 1440/1024/768/390, and the full
ordinary-user denial with the general and redaction settings still reachable.
The fixture gates the admin settings namespace per row while the redaction and
per-user endpoint settings stay authenticated, and its logging write reproduces
the backend read-modify-write of the retention record. The evidence is the
synthetic fixture; no live provider, archive or database behaviour is claimed.

The authenticated `/redaction/rules` and `/redaction/preview` routes are
implemented. `e2e/foundation/run-redaction-rules.sh` records the rules route's
focused React coverage in `e2e/foundation/redaction-rules-result.json`: scoped
fixture reads and writes, search/paging, local validation, normalization,
failure retry, reveal masking, authorization, and responsive controls.
`e2e/foundation/run-redaction-preview.sh` records the preview route's focused
React coverage in `e2e/foundation/redaction-preview-result.json`: the request
snapshot, masked output, statistics, findings and replacements, selection
highlight, copy, stale-response suppression, the single-request guard,
retryable failures, the shared unsaved draft across both routes, and the
responsive mobile panes. The fixture preview applies the built-in email and
secret rules only; custom-rule, line-scope, and Git-diff effects are request
payload assertions rather than simulated engine behavior. Neither run claims a
live redaction engine or live persistence.

`e2e/foundation/run-usage.sh` records both request-record routes' focused React
coverage in `e2e/foundation/usage-result.json`: the AI overview cards, chart and
distribution, the upstream drilldown, the local view mode, the record list with
pagination, page size, search, filters and the preset and custom range windows,
the delayed loading surface, the error retry, the stale read suppression, the
zero-token fold, the wrapped and pinned detail headers in both locales at four
widths, the transcript paging, the error and abort blocks, the admin session
override and affinity writes with their confirmation and the pending guard, the
history clear with its validation errors, the ordinary-user scope, and the MCP
route with its scope, payload detail and error link. The fold matrix covers the
positive, success-only zero, mixed 60%-failure zero and all-failed zero entries
in both perspectives and both locales; the detail evidence includes the
`/v1/messages` caller effort that stays unmerged, the `/v1/responses`
caller-to-override effort, and a sub-session record with its parent id. The
popover unknown-token case is a browser-interception client-robustness probe
because the backend aggregate is non-null by contract.

`e2e/foundation/run-approvals.sh` records the approvals route's focused React
coverage in `e2e/foundation/approvals-result.json`: the admin-only route behind
`AdminGuard`, the pending default and resolved filter, the 10/25/50 pager with
the last-page correction and the mobile pager rule, the detail dialog that opens
only after a successful read, the shared `approval:<id>` answer gate with no
request body, the list refresh and matching detail reload behind one write, the
delayed double trigger, the answered row and dialog gates, the 503/409/404/403/401
error states, the latest-wins list and detail reads, the per-second remaining
wait that is cleared on leave, and both locales and themes at 1440/1024/768/390.

Both request-record routes are mounted React routes. `frontend/src/pages/UsagePage.tsx`
serves `/request-records/ai` and `/request-records/mcp` for every authenticated
Worker user, derives the category from the path, and is never wrapped in the
admin guard. It owns one initialization effect per identity and category, wires
the list, overview, detail, session and clear surfaces to the stores, and
closes the clear dialog only for the submitted form. `src/lib/usage-model.ts`
holds the typed list, facet and overview queries with their exact dependency
sets, the sortable-column whitelist (`usage_date` for Date, `created_at` for
Time, unsupported fields rejected), the exact-zero token fold
(`total_tokens === 0` only, with unknown values kept as `-`), the half-open UTC
range window and the per-category overview mode. `src/lib/usage.ts` holds the
identity-scoped view store with per-read generations on a fresh QueryClient.
The synthetic fixture now enforces the backend records role boundary
(authenticated self-scoped reads, a current-user clear, admin-only
prune/reset/override), applies the list filters, the backend sort whitelist
with nulls last, the scope and the window, and derives the facets and the
overview from the same visible rows: a success-only zero-token row, a
missing-token row, mixed zero-token results, and both model-perspective
`upstream_breakdown` and upstream-perspective `model_breakdown` details. The
fixture dates its synthetic records relative to its own start so a preset
window never drops the dataset while a custom window stays honestly empty. It
is a synthetic dataset, not a live record store, backend aggregation engine or
persistence claim.

The React list building block is mounted for both categories.
`src/lib/usage-records.ts` derives the row
labels, session badges and end-to-end output rate from the raw row without a
stored date, and the three pure label helpers live in
`src/admin-mappers/request-records-core.ts`, which the React list and the
detail model consume directly. `RequestRecordsTable.tsx` renders the exact AI
and MCP column sets from
the rows the store provides, with a real button in every sortable header and
`aria-sort` following the store's triple cycle, a non-sortable details
entry point and error link that both report `onOpenDetail`, and page-size and
pager callbacks that leave page persistence to the store. `RequestRecordsToolbar.tsx`
commits its search draft only on Enter, applies each select immediately against
the current facets and maps a cleared select back to a null filter;
`UsageRangePicker.tsx` keeps the preset windows and the half-open UTC custom
window in plain `Date` arithmetic, and `UsageModeSwitch.tsx` reports the mode to
the parent without persisting it. Dates repaint from the current locale and
display time zone; the Date sort keeps the accepted UTC-day field. The mounted
route wires the callbacks to the store, and `e2e/foundation/usage.playwright`
proves keyboard sorting, the row order, the query mapping and the header
bounding boxes in a real browser.

The React AI/MCP detail and clear-history building block is mounted, and the
browser run covers it. `src/lib/usage-detail.ts` is a separate
identity-scoped store with its own detail, request-full and route-options
generations, so a superseded read commits nothing and never clears a newer
loading flag; it owns the shared `conversation:<id>` (or `record:<id>`) guard
for the override save, clear and affinity reset, captures the conversation
before awaiting and reloads the options only for the conversation that is still
open. `src/lib/usage-detail-model.ts` holds the pure detail view, reasoning
extraction, route/override views, transcript items and the request-full merge.
`UsageDetailDialog.tsx` renders the context, the admin-only session-routing
controls, the shared error/abort block and the newest-first ten-message
transcript with a show-more button and a 240px sentinel; the MCP dialog renders
only the request and response payloads through an element-based JSON view that
never injects HTML. `UsageClearDialog.tsx` is a controlled form with the
role-scoped scopes, delete-all, local date-time inputs and the warning, while
`src/lib/usage-clear.ts` converts the local wall-clock value to a UTC instant
and the store's `clearHistory` action runs inside the `usage:maintenance`
guard and refreshes every read. The mounted route owns the
form snapshot and the close rules, and `e2e/foundation/usage.playwright` proves
the AI and MCP dialogs, the transcript paging, the admin session writes, and the
clear dialog in a real browser.

The billing route is mounted at `/billing` for every authenticated Worker user
and is never wrapped in the admin guard. `frontend/src/pages/BillingPage.tsx`
composes the period and filter controls, the summary, the charge list, the
charge detail dialog, the admin price rules, and the CSV exports; an ordinary
user keeps the self scope with the summary, the charges, the detail, and the
exports, and never receives the owner and endpoint filters, the price rules,
the reprice action, or the admin user and endpoint reads. The page keeps the
data foundation it builds on: `frontend/src/lib/billing-model.ts` owns the
shared date, filter, form, amount, and rate helpers,
`frontend/src/api/billing.ts` takes its filter type from that pure module, and
`frontend/src/lib/billing.ts` holds the React billing
state: identity-scoped filters, period, independent charge and price-rule
pagers, the selected charge detail, the price-rule draft, and the guarded
writes. The server reads run through the project query client with keys that
capture the identity, the filters, and the page; the store keeps no second
network cache. The price-rule pager is public and independent of the charge
pager, and every list read and the charge detail use a latest-wins generation
so a late response cannot overwrite a newer page, filter, or selection.

`e2e/foundation/run-billing.sh` records the route's focused React coverage in
`e2e/foundation/billing-result.json`: the mounted route with its summary,
charges, and price rules, the single initial reads, the independent charge and
price-rule pagers, the period and filter behavior, the charge detail, the
price-rule create payload, the server-failure draft retry, the duplicate-write
guard, the toggle and the native-confirm delete, the reprice count, both CSV
downloads with their real bytes, byte order marks, and file names, the export
failure without a download, the ordinary-user scope with a foreign charge
detail that returns 404, price-rule writes that return 403, and no admin reads
or controls, the anonymous scope that returns 401, and the 1440/1024/768/390
layouts in both locales and themes. The
report is fixture-only evidence and claims no live billing, pricing, or
persistence.

`REACT-BILLING-RULES-PAGINATION` is closed by that run for the React port: the
price-rule pager requests its own page and shows distinct rows without a charge
fetch, the charge pager requests its own page without a price-rule fetch, and
both page sizes persist under their own storage keys. The Vue baseline defect
record stays in `FOCUSED_SCENARIO_EVIDENCE` and `e2e/baseline/evidence.json`
as history and keeps the baseline row `partial`.

The billing React test mapping is `src/react-tests/billing.test.tsx` for the
page and component behavior, `src/react-tests/billing-model.test.ts` and
`src/react-tests/billing-state.test.ts` for the pure helpers and the store, and
`e2e/foundation/billing.playwright` with `e2e/foundation/validate-billing.ts`
for the browser parity and the rule-pager fix.

The React upstream list and editor are mounted at `/endpoints/upstreams` under
`AdminGuard`. The focused stateful-fixture run is `e2e/foundation/run-endpoints.sh`
and its current report is `e2e/foundation/endpoints-result.json`. It exercises
route access, endpoint writes and retryable drafts, synthetic OAuth flows
including delayed provider-change status, scoped table/mobile quota pills,
quota-history and lazy-chart requests, and responsive layouts. The current
report records this route's React fixture coverage; it does not prove live
persistence. OpenAI Admin API key state remains mapper-only with no rendered
field, and ordinary saves omit the untouched secret. Endpoint writes invalidate
model-route page queries in the `model-routes-page` cache namespace. The
separate `/endpoints/routes` page uses that same route-query namespace and loads
all endpoint records in 200-row pages for editor options and protocol lookup;
its pager is independent of the upstream table. Model catalogs are cached by
endpoint and effective protocol for 60 seconds, while failures remain retryable.

`e2e/foundation/run-endpoints-routes.sh` records the model-route React acceptance
in `e2e/foundation/endpoints-routes-result.json`. It verifies route rendering and
admin denial, editor open and CRUD/toggle/conflict behavior, all-page endpoint
resolution, catalog scoping/cache behavior, target ordering/settings, probe
result details, long values, and responsive/light-dark/locale states. The report
is fixture-only evidence and does not claim live persistence.

`e2e/foundation/run-mcp.sh` records authenticated MCP management in
`e2e/foundation/mcp-result.json`. It verifies admin and owner-scoped user flows,
provider presets and masked credentials, stdio environment editing, hidden-field
round trips, per-server catalog caching/filtering, guarded writes and probes,
retryable drafts, and responsive layouts. The synthetic fixture verifies UI and
request behavior only; it does not contact MCP providers or prove live
persistence.

`e2e/foundation/run-relays.sh` records the admin Relay page in
`e2e/foundation/relays-result.json`. It checks non-admin denial, persisted
pagination and correction, read-failure recovery, create/edit secret semantics,
fixture validation drafts, reconnect/delete confirmation, refresh-held row
guards, and 1440/1024/768/390 layouts in both locales and themes. Synthetic secret
values are replaced by digests in the report and are not captured in screenshots;
the fixture verifies presence and request behavior only, not PEM cryptography or
live connections.

The endpoint editor integrates the OAuth status, device-login, browser-login,
refresh, and clear controls. The shared quota cache, the foreground policy, the
neutral calculation and formatting foundations, the shared quota badges, the
current-usage dialog, and the subscription and per-key quota histories with
their lazy trend charts are all in place.

## Redirects

The first six rows are the redirect entries of `parity.matrix.ts`; the last two
are the session and role guard rules of the route tree.

| From                            | To                                            | Reason                        |
| ------------------------------- | --------------------------------------------- | ----------------------------- |
| `/`                             | `/api-keys`                                   | default authenticated section |
| `/endpoints`                    | `/endpoints/upstreams`                        | first child                   |
| `/redaction`                    | `/redaction/rules`                            | first child                   |
| `/request-records`              | `/request-records/ai`                         | first child                   |
| `/settings`                     | `/settings/general`                           | first child                   |
| unknown path                    | `/api-keys`                                   | catch-all                     |
| admin route as non-admin        | `/api-keys`                                   | first visible section         |
| authenticated visit to `/login` | requested `redirect` or first visible section | guard                         |

## Role boundary

- The worker session is the only source of `isAdmin`; admin-only nav entries and
  routes are filtered and guarded in both navigation and routing.
- `/relay-host` bootstraps `relay/auth/me` and `relay/host|status|settings` with
  its own cookie, renders its token form when unauthenticated, and works with no
  Worker session. It never reads `session.me` or `isAdmin`.
- A non-admin may open every `authenticated` route; every `admin` route lands on
  `/api-keys`. The baseline capture asserts both lists.

## Shared mutation keys

These keys are part of the behavior contract; the React port keeps them
identical so the lifecycle tests stay meaningful.

| Domain         | Keys                                                                                                                                                                                        |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| session        | `session:authentication`, `relay-session:authentication`                                                                                                                                    |
| endpoints      | `endpoint:<id>`, `endpoint:create`, `networktest:endpoint:<id>`                                                                                                                             |
| model routes   | `modelroute:<id>`, `modelroute:create`, `networktest:modelroute:<id>`                                                                                                                       |
| MCP            | `mcp:<id>`, `mcp:create`, `mcptest:<id>`                                                                                                                                                    |
| users          | `user:<id>`, `user:create`                                                                                                                                                                  |
| client keys    | `clientkey:<key_id>`, `clientkey:create:<user_id>`                                                                                                                                          |
| relays         | `relay:<id>`, `relay:create`                                                                                                                                                                |
| relay host     | `host:local`                                                                                                                                                                                |
| settings       | `setting:<section>` (`rawRetention`, `streamDeltaBatching`, `modelRouteWhitelist`, `relayIpWhitelist`, `llmReview`, `storage`, `cacheAlert`; raw retention and content logging share a key) |
| redaction      | `redaction:rules`, `redaction:preview`                                                                                                                                                      |
| config archive | `configArchive:export`, `configArchive:import`                                                                                                                                              |
| billing        | `billingrule:<id>`, `billing:reprice`                                                                                                                                                       |
| approvals      | `approval:<id>`                                                                                                                                                                             |
| usage          | `usage:maintenance`, `usage:perspective`, `conversation:<id>`, `record:<id>`                                                                                                                |

Reads, search, pagination, bootstrap, status and quota refresh are not mutation
keys. Stale read responses are still discarded by generation/sequence checks.
The baseline capture proves the guard synchronously rejects a second trigger and
holds the key through the refresh; `src/react-tests/async-action.test.tsx`
proves the same rule in the current guard.

## Legacy test disposition

The legacy Vue suite is retired. `frontend/e2e/parity.matrix.ts` keeps the
134-name inventory of the modules that existed at the CL1 plan baseline — 130
under `frontend/tests/` plus the four Vitest form-mapper suites under
`src/admin-mappers/forms/` — and records for each name the state it is in now:

| State     | Count | Meaning                                                        |
| --------- | ----: | -------------------------------------------------------------- |
| preserved |     6 | the module still exists on disk and still runs                 |
| ported    |    19 | the legacy assertions were moved into a named React test       |
| replaced  |   109 | the observable is covered by a React suite or a browser result |

Three helper modules outside that inventory are tracked separately: one is
replaced and two are obsolete.

The six preserved modules are the two retained native suites
`tests/theme-first-paint.test.ts` and `tests/dev-proxy-scope.test.ts` — both run
by `bun run test:legacy` — and the four form-mapper suites
`src/admin-mappers/forms/config-archive.test.ts`,
`src/admin-mappers/forms/endpoint.test.ts`,
`src/admin-mappers/forms/model-route.test.ts` and
`src/admin-mappers/forms/relay-host.test.ts`.

`classifyLegacyTest` keeps the historical classification of every name, and
`validateLegacyModuleDispositions` fails when a name has no state, when a
`preserved` file is missing from disk, when the evidence path of a state does
not exist, and when a `ported` or `replaced` entry names a React test title
that is not in the file. The state metadata records what is known today; it
never authorizes a deletion by itself.

## Baseline evidence

The committed baseline under `frontend/e2e/baseline/**` is immutable history: it
is never regenerated in place and never edited. Its capture script,
`bash e2e/baseline/run-baseline.sh`, produced that capture at the baseline
commit and is not a command that refreshes the committed files. The strict
read-only check is `cd frontend && bun e2e/baseline/validate-matrix.ts`, which
reads the frozen evidence and fails on any drift. The current
`evidence.json` contains a run manifest, current callback reports, and
`scenarioCoverage` for every required `ROW:scenario` pair. The validator rejects
missing or unknown coverage, callback failures, missing evidence pointers, and
reports outside the current run.

- All live routes were captured in zh-CN and en-US at 1440 px; representative
  light-theme and 1024/768/390 px captures cover responsive layouts.
- Login, invalid credentials, logout, redirects, admin denial, and independent
  relay-token authentication are exercised through the rendered app.
- Create/update/delete/toggle/reconnect, error/retry, pagination, file, stale
  response, role and settings flows use the in-memory typed fixture API and its
  request ledger.
- Long values are checked for visibility and layout containment on seven routes;
  the key and redaction pagers assert distinct row identities and query ranges.
- Twenty route rows are baseline-verified. AI usage sorting and billing price-rule
  pagination remain the two approved baseline defects listed below; both have
  mandatory React repairs and are not parity waivers. The React port now proves
  the AI usage sorting repair in the current browser run; the P0 defect record
  stays as history until the baseline row is retired.

### Screenshot regression scenarios

**Zero-token fold.** Baseline capture (history): upstream detail rendered 2
visible rows and hid 5 all-failed zero-token rows behind
`已折叠 5 个全部失败且无 token 的明细` (zh) / `5 fully failed rows hidden` (en).
Expanding showed all rows. The 5-request, 0-token, 60%-failure mixed row was
visible in the collapsed state.

Target (user decision 2026-10-23): every upstream row with `total_tokens === 0`
folds by default, including success-only, mixed-failure and all-failed rows;
expand reveals them; totals, counts, errors, filters, drilldowns and backend
aggregation do not change; missing/unknown tokens are never coerced to `0`; both
localized labels describe zero tokens rather than all failures. The baseline
capture keeps the old behavior and label; the React port uses the zero-token
label and the zero-token fold.

React status: the port folds every row whose `total_tokens` is exactly `0`,
hides those rows behind a count of the hidden requests, keeps an unknown
total visible as `-`, and reveals the folded rows without a new read, a
changed total, a changed filter, or a changed drilldown. The fixture seeds a
positive entry beside a success-only, a 60%-failure mixed and an all-failed
exact-zero entry in both perspectives, so the browser run proves a hidden
request count of 8, one visible row when collapsed, four rows when expanded
with the positive one first and literal `0` totals, unchanged server totals and
no read on expand, in both locales. The nullable-token contract is proven on
the real record 4 list cells, which show `-` rather than `0`; the backend
aggregate is non-null by contract, so the unknown-token popover case is
exercised as a browser-interception client-robustness probe instead.

**Header stress.** Measured header cells in the popover at 1440 px (baseline
capture):

| Locale | Header               | Width | Overflow |
| ------ | -------------------- | ----: | -------- |
| zh-CN  | 缓存 token 覆盖率    | 80 px | yes      |
| en-US  | Request share        | 80 px | yes      |
| en-US  | Cache token coverage | 80 px | yes      |

The target uses compact two-line wrapping or responsive stacked summaries with
minimum widths/controlled scrolling, never overlapping adjacent headers or
values; long endpoint/model IDs truncate inline with accessible full names, and
column alignment/sort controls and all metrics stay.

React status: the detail popover headers wrap inside their own column width at
1440, 1024, 768 and 390 px in both locales, no two adjacent headers intersect,
the pinned identifier cell keeps an opaque background, and the document never
overflows horizontally. The current browser run measures the nine headers in
both locales at the four widths.

## Known baseline defects

- **AI usage sorting** is inert in the P0 baseline: headers do not change
  `aria-sort` or issue a sort request. React adds accessible header controls,
  keyboard activation, `aria-sort`, and `sort_field`/`sort_order` query mapping
  under `REACT-AI-SORT-ACCESSIBLE`. The mounted React route answers Tab, Enter
  and Space on real header buttons, cycles ascending, descending and unsorted,
  maps Date to `usage_date` and Time to `created_at`, keeps the current page
  offset, changes the server-sorted row order, and leaves non-sortable headers
  without a control. The P0 record stays as history.
- **Billing price-rule pagination** dispatches no request and changes no rows;
  the charge pager works. React restores an independent rule pager and query
  state under `REACT-BILLING-RULES-PAGINATION`. The React port restores it and
  the current run records the independent pager assertions; the Vue baseline
  record stays as history.

These defects are separately recorded in `FOCUSED_SCENARIO_EVIDENCE` and the
current `evidence.json`; neither is treated as a passing scenario or removed from
the React target.

## Fixture limits

The stateful fixture proves rendered UI and request behavior, not live persistence
or upstream behavior. No production account, endpoint, database, or credential is
used.

The focused React reports are per-run artifacts: each runner rewrites its own
result file, so freshness means the last execution of that runner. They are not
the final proof of the migration, and the final independent audit of the whole
issue is still open.

One known limitation is not identified: the `usage-page` mount case
`loads each read once on mount and shows the overview` has failed
intermittently, and the failing assertion and its cause are unknown. Later
green runs are samples of a rare event with an unknown signature; they neither
identify the failing gate nor exclude it. No sample reproduced it, so nothing
here claims the failure is fixed, pre-existing, or attributable to one change.
