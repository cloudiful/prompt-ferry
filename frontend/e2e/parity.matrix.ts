/**
 * Executable feature-parity contract for the Vue -> React single-track
 * migration (issue #1060).
 *
 * The matrix is data, not a test harness: `frontend/e2e/baseline/validate-matrix.ts`
 * enforces its invariants, and `frontend/e2e/baseline/capture.playwright` imports the
 * route list so the browser baseline and this contract cannot drift.
 *
 * Source of truth for the baseline is the current Vue app at
 * origin/main 6fe05da (v1.42.30). A row is only `baseline-verified` after the
 * Playwright CLI baseline actually observed it; later React ports must keep the
 * row's scenarios green or replace its mapped tests before deletion.
 */

export type RoleScope = 'public' | 'authenticated' | 'admin' | 'relay-session'

export type Domain =
  | 'shell'
  | 'auth'
  | 'relay-host'
  | 'api-keys'
  | 'available-models'
  | 'users'
  | 'endpoints'
  | 'model-routes'
  | 'mcp'
  | 'relays'
  | 'redaction'
  | 'usage-ai'
  | 'usage-mcp'
  | 'billing'
  | 'approvals'
  | 'settings'

export type GatePhase =
  'P1-foundation' | 'P3-shell-auth' | 'P4-reads' | 'P5-writes' | 'P6-final'

export type MatrixStatus =
  'baseline-verified' | 'baseline-partial' | 'baseline-pending'

export type ScenarioId =
  | 'route-render'
  | 'nav-visibility'
  | 'admin-denial'
  | 'redirect'
  | 'login'
  | 'logout'
  | 'relay-independent-auth'
  | 'empty-state'
  | 'loading-state'
  | 'error-state'
  | 'pagination'
  | 'stale-response'
  | 'dialog-open'
  | 'mutation-create'
  | 'mutation-update'
  | 'mutation-delete'
  | 'mutation-toggle'
  | 'mutation-reconnect'
  | 'mutation-conflict-guard'
  | 'mutation-failure-draft'
  | 'destructive-confirm'
  | 'file-export'
  | 'file-import'
  | 'locale-switch'
  | 'theme-switch'
  | 'responsive-width'
  | 'long-values'
  | 'zero-token-fold'

export type RouteRow = {
  id: string
  route: string
  scope: RoleScope
  navSection: string
  domain: Domain
  /** Stable read endpoints or store loaders the page depends on. */
  reads: string[]
  /** Exact shared exclusion keys the page's writes must use. */
  mutationKeys: string[]
  /** Legacy test files that currently carry this route's intent. */
  legacyTests: string[]
  requiredScenarios: ScenarioId[]
  gate: GatePhase
  status: MatrixStatus
  foundationTest?: string
  evidence: string[]
  notes?: string
}

export type RedirectRow = {
  id: string
  from: string
  to: string
  reason: string
  evidence: string[]
}

export type LegacyTestDisposition =
  | 'preserved-framework-neutral'
  | 'preserved-form-mapper'
  | 'pending-react-replacement'
  | 'retire-source-string'
  | 'support-fixture'

export type LegacyTestEntry = {
  file: string
  disposition: LegacyTestDisposition
  rows: string[]
  /** Observable replacement that must exist before removal. */
  counterpart: string
}

export const APP_ORIGIN = 'http://127.0.0.1:5171'
export const FIXTURE_ORIGIN = 'http://127.0.0.1:8789'

export const ROUTES: RouteRow[] = [
  {
    id: 'R-LOGIN',
    route: '/login',
    scope: 'public',
    navSection: 'none',
    domain: 'auth',
    reads: ['GET /api/v1/auth/me'],
    mutationKeys: ['session:authentication'],
    legacyTests: [
      'session-login.lifecycle.test.ts',
      'session-login-ux.test.ts',
      'session-login.fixtures.ts',
      'async-action.test.ts',
      'async-actions-review.test.ts',
      'route-titles.test.ts',
    ],
    notes:
      'Login, invalid-credential error, logout, locale and theme first paint were exercised.',
    requiredScenarios: [
      'route-render',
      'login',
      'logout',
      'error-state',
      'locale-switch',
      'theme-switch',
    ],
    gate: 'P3-shell-auth',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/auth-shell.playwright',
    evidence: [
      'e2e/baseline/screenshots/auth_01_login_zh_dark_1440.png',
      'e2e/baseline/screenshots/auth_02_invalid_credentials_zh_dark_1440.png',
    ],
  },
  {
    id: 'R-API-KEYS',
    route: '/api-keys',
    scope: 'authenticated',
    navSection: 'api-keys',
    domain: 'api-keys',
    reads: [
      'GET /api/v1/admin/users/options',
      'GET /api/v1/me/client-keys',
      'GET /api/v1/admin/users/{user_id}/client-keys',
    ],
    mutationKeys: ['clientkey:<key_id>', 'clientkey:create:<user_id>'],
    legacyTests: [
      'api-keys-store.test.ts',
      'api-key-selection.lifecycle.test.ts',
      'api-key-selection.fixtures.ts',
      'row-switch-labels.test.ts',
      'app-shell-mobile-nav.test.ts',
      'dev-proxy-scope.test.ts',
    ],
    notes:
      'React component tests and the stateful browser run cover owner-scoped reads/writes, stale-owner error suppression, create/reveal/copy, rename/toggle/delete, guarded refresh, retry, pagination and responsive long labels.',
    requiredScenarios: [
      'route-render',
      'nav-visibility',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-delete',
      'mutation-conflict-guard',
      'mutation-failure-draft',
      'long-values',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/api-keys-models.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_api-keys.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_api-keys.png',
      'e2e/baseline/screenshots/dialog_api-keys_zh_dark_1440.png',
      'e2e/baseline/screenshots/mutation_api_key_create_zh_dark_1440.png',
      'e2e/baseline/screenshots/mutation_api_key_failure_draft_zh_dark_1440.png',
    ],
  },
  {
    id: 'R-AVAILABLE-MODELS',
    route: '/available-models',
    scope: 'authenticated',
    navSection: 'available-models',
    domain: 'available-models',
    reads: ['GET /api/v1/me/models'],
    mutationKeys: [],
    legacyTests: [],
    requiredScenarios: [
      'route-render',
      'nav-visibility',
      'pagination',
      'empty-state',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/api-keys-models.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_available-models.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_available-models.png',
    ],
    notes:
      'React component tests cover the localized empty table; the stateful browser run verifies populated pagination, refresh, stale-page isolation and responsive rendering.',
  },
  {
    id: 'R-USERS',
    route: '/users',
    scope: 'admin',
    navSection: 'users',
    domain: 'users',
    reads: ['GET /api/v1/admin/users'],
    mutationKeys: ['user:<id>', 'user:create'],
    legacyTests: [
      'users-store.test.ts',
      'record-actions.gates.test.ts',
      'record-actions.lifecycle.test.ts',
      'record-actions.sfc.test.ts',
      'row-switch-labels.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-update',
      'mutation-delete',
      'mutation-failure-draft',
      'long-values',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/users.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_users.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_users.png',
      'e2e/baseline/screenshots/dialog_users_zh_dark_1440.png',
    ],
    notes:
      'The focused React run covers admin denial, paging and correction, create/update/delete/reset outcomes, failure drafts, and long values; baseline evidence remains the Vue capture.',
  },
  {
    id: 'R-ENDPOINTS-UPSTREAMS',
    route: '/endpoints/upstreams',
    scope: 'admin',
    navSection: 'endpoints',
    domain: 'endpoints',
    reads: [
      'GET /api/v1/admin/endpoints',
      'GET /api/v1/admin/users/options',
      'GET /api/v1/admin/endpoints/{id}/models',
      'GET /api/v1/admin/endpoints/{id}/token-plan-usage',
      'GET /api/v1/admin/endpoints/{id}/quota-history',
      'GET /api/v1/admin/endpoints/{id}/quota-observations',
      'GET /api/v1/admin/endpoints/{id}/quota-observations/trend',
      'GET /api/v1/admin/endpoints/{id}/oauth',
    ],
    mutationKeys: [
      'endpoint:<id>',
      'endpoint:create',
      'networktest:endpoint:<id>',
    ],
    legacyTests: [
      'endpoints-store.test.ts',
      'endpoint-actions.test.ts',
      'endpoint-actions.gates.test.ts',
      'endpoint-actions.fixtures.ts',
      'endpoint-page-actions.test.ts',
      'endpoint-page-actions.lifecycle.test.ts',
      'endpoint-page-actions.sfc.test.ts',
      'endpoint-page-actions.fixtures.ts',
      'endpoint-oauth.test.ts',
      'endpoint-oauth-actions.fixtures.ts',
      'endpoint-oauth-device-flow.test.ts',
      'endpoint-oauth-browser-flow.test.ts',
      'endpoint-oauth-status.test.ts',
      'endpoint-model-input.test.ts',
      'endpoint-admin-api-key.test.ts',
      'endpoint-reasoning-select.test.ts',
      'endpoint-service-tier.test.ts',
      'endpoint-service-tier-field.test.ts',
      'endpoint-service-tier-subpage.test.ts',
      'endpoints-quota-messages.test.ts',
      'provider-icon.test.ts',
      'canonical-quota-windows.test.ts',
      'quota-chart-layout.test.ts',
      'quota-display-unknown.test.ts',
      'quota-legend-localization.test.ts',
      'quota-observation-charts.test.ts',
      'quota-observation-trend.test.ts',
      'quota-progress-semantics.test.ts',
      'quota-provider.test.ts',
      'subscription-quota-history.test.ts',
      'subscription-quota-windows.test.ts',
      'token-plan-badge-surface.test.ts',
      'token-plan-cache-expiry.test.ts',
      'token-plan-foreground-refresh.test.ts',
      'token-plan-key-status.test.ts',
      'token-plan-quota-summary.test.ts',
      'token-plan-reset-duration.test.ts',
      'token-plan-usage-cache-state.test.ts',
      'token-plan-usage-dialog.test.ts',
      'token-plan-usage-request-policy.test.ts',
      'use-token-plan-badge-states.test.ts',
      'use-token-plan-badges.test.ts',
      'use-token-plan-usage-cache.test.ts',
      'key-quota-observation-chart.test.ts',
      'key-quota-observation-history.test.ts',
      'schedule-all-day.test.ts',
      'use-endpoint-oauth-actions.test.ts',
    ],
    notes:
      'The current focused synthetic-fixture run exercises the mounted route, endpoint writes, OAuth flows including delayed provider-change status, scoped quota badges and histories, and responsive states. The report is current React evidence; historical P0 baseline evidence remains unchanged and the fixture does not prove live persistence.',
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-update',
      'mutation-delete',
      'mutation-conflict-guard',
      'mutation-failure-draft',
      'long-values',
      'loading-state',
      'error-state',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/endpoints.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_endpoints_upstreams.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_endpoints_upstreams.png',
      'e2e/baseline/screenshots/dialog_endpoint-edit_zh_dark_1440.png',
      'e2e/baseline/screenshots/mutation_endpoint_toggle_conflict_zh_dark_1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1024_endpoints_upstreams.png',
      'e2e/baseline/screenshots/admin_zh_dark_768_endpoints_upstreams.png',
      'e2e/baseline/screenshots/admin_zh_dark_390_endpoints_upstreams.png',
    ],
  },
  {
    id: 'R-ENDPOINTS-ROUTES',
    route: '/endpoints/routes',
    scope: 'admin',
    navSection: 'endpoints',
    domain: 'model-routes',
    reads: [
      'GET /api/v1/admin/model-routes?first=<offset>&rows=<size>',
      'GET /api/v1/admin/endpoints?first=<all-records-page>&rows=200',
      'GET /api/v1/admin/endpoints/{endpoint_id}/models?native_api=<effective-protocol>',
      'GET /api/v1/admin/users/options',
      'POST /api/v1/admin/model-routes/test',
    ],
    mutationKeys: [
      'modelroute:<id>',
      'modelroute:create',
      'networktest:modelroute:<id>',
    ],
    legacyTests: [
      'model-route-service-tier.test.ts',
      'model-route-suggestion-protocol.test.ts',
      'model-route-target-drag.test.ts',
      'model-route-targets-summary.test.ts',
      'model-route-test-results.test.ts',
      'embedding-dimension-mapping.test.ts',
      'endpoint-service-tier.test.ts',
      'endpoint-reasoning-select.test.ts',
      'service-tier-eligibility.test.ts',
    ],
    notes:
      'React acceptance covers route create/update/delete, enablement conflict guard, target payload ordering, full endpoint resolution, protocol-scoped catalogs, probes, and long identifiers.',
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-update',
      'mutation-delete',
      'mutation-conflict-guard',
      'long-values',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/endpoints-routes.playwright',
    evidence: ['e2e/foundation/endpoints-routes-result.json'],
  },
  {
    id: 'R-MCP',
    route: '/mcp',
    scope: 'authenticated',
    navSection: 'mcp',
    domain: 'mcp',
    reads: [
      'GET /api/v1/admin/mcp-providers',
      'GET /api/v1/admin/mcp-servers',
      'GET /api/v1/admin/mcp-servers/{id}/catalog',
      'POST /api/v1/admin/mcp-servers/{id}/test',
    ],
    mutationKeys: ['mcp:create', 'mcp:<id>', 'mcptest:<id>'],
    legacyTests: [
      'mcp-store.test.ts',
      'mcp-dialog-protocol-version-select.test.ts',
    ],
    notes:
      'The current React run verifies authenticated owner-scoped MCP management, provider/token and stdio/environment editing, hidden-field preservation, catalog caching and filtering, guarded writes/tests, pagination, failure-draft retry, and responsive rendering. Request-record features are mapped only to R-USAGE-MCP.',
    requiredScenarios: [
      'route-render',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-delete',
      'mutation-failure-draft',
      'long-values',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/mcp.playwright',
    evidence: ['e2e/foundation/mcp-result.json'],
  },
  {
    id: 'R-RELAYS',
    route: '/relays',
    scope: 'admin',
    navSection: 'relays',
    domain: 'relays',
    reads: ['GET /api/v1/admin/relays', 'GET /api/v1/bridge/status'],
    mutationKeys: ['relay:<id>', 'relay:create'],
    legacyTests: [
      'relays-store.test.ts',
      'relay-actions.lifecycle.test.ts',
      'relay-actions.sfc.test.ts',
      'relay-actions.fixtures.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-reconnect',
      'mutation-delete',
      'mutation-conflict-guard',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/relays.playwright',
    evidence: [
      'e2e/foundation/relays-result.json',
      'e2e/foundation/screenshots/relays-admin-en-light-1440.png',
      'e2e/foundation/screenshots/relay-editor-en-light-1440.png',
      'e2e/foundation/screenshots/relays-admin-zh-dark-1440.png',
      'e2e/foundation/screenshots/relays-admin-en-light-390.png',
      'e2e/foundation/screenshots/relay-editor-en-light-390.png',
      'e2e/foundation/screenshots/relays-admin-zh-dark-390.png',
      'e2e/foundation/screenshots/relay-editor-zh-dark-1440.png',
      'e2e/foundation/screenshots/relay-editor-zh-dark-390.png',
    ],
    notes:
      'React page/model tests and the current-run fixture browser check cover role denial, paging/correction, retained read/write drafts, all four secret patch modes, reconnect/delete guards, and responsive layout. Synthetic secret materials are masked from artifacts; fixture acceptance does not validate PEM cryptography or live connections.',
  },
  {
    id: 'R-RELAY-HOST',
    route: '/relay-host',
    scope: 'relay-session',
    navSection: 'relay-host',
    domain: 'relay-host',
    reads: [
      'GET /api/v1/relay/auth/me',
      'GET /api/v1/relay/host',
      'GET /api/v1/relay/status',
      'GET /api/v1/relay/settings',
    ],
    mutationKeys: ['relay-session:authentication', 'host:local'],
    legacyTests: [
      'relay-actions.sfc.test.ts',
      'relay-host-actions.lifecycle.test.ts',
      'relay-host-page.test.ts',
      'relay-host-role.test.ts',
      'relay-session-guard.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'relay-independent-auth',
      'mutation-update',
      'mutation-failure-draft',
      'responsive-width',
    ],
    gate: 'P3-shell-auth',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/auth-shell.playwright',
    evidence: [
      'e2e/baseline/screenshots/relay_host_01_token_form_zh_dark_1440.png',
      'e2e/baseline/screenshots/relay_host_02_authenticated_zh_dark_1440.png',
      'e2e/baseline/screenshots/relay_host_03_authenticated_zh_dark_768.png',
      'e2e/baseline/screenshots/relay_host_04_authenticated_zh_dark_390.png',
      'e2e/baseline/screenshots/relay_host_05_role_change_zh_dark_1440.png',
    ],
    notes:
      'React RelayHostPage and relay-host.test.tsx cover authenticated host/settings/status reads, shared host:local writes, direct role response application, settings rereads, retryable drafts, restart origins, worker-disconnected controls, and 1440/1024/768/390 layouts in both themes. The auth-shell current run preserves independent relay-token login/logout; relay-actions.sfc.test.ts remains mapped for its host guard assertions.',
  },
  {
    id: 'R-REDACTION-RULES',
    route: '/redaction/rules',
    scope: 'authenticated',
    navSection: 'redaction',
    domain: 'redaction',
    reads: [
      'GET /api/v1/settings/redaction',
      'GET /api/v1/settings/redaction/custom-strings',
      'GET /api/v1/admin/users/options',
    ],
    mutationKeys: ['redaction:rules'],
    legacyTests: [
      'redaction-rules.test.ts',
      'redaction-store.test.ts',
      'redaction-actions.fixtures.ts',
    ],
    requiredScenarios: [
      'route-render',
      'pagination',
      'dialog-open',
      'mutation-toggle',
      'mutation-update',
      'mutation-delete',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/redaction-rules.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_redaction_rules.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_redaction_rules.png',
      'e2e/baseline/screenshots/mutation_redaction_save_zh_dark_1440.png',
    ],
    notes:
      'The focused React run covers authenticated global/user scopes, self-only access, query-backed and dirty-local search/paging, normalized custom rules, warnings, reveal masking, local delete confirmation, retryable saves, and responsive UI. The unsaved draft is shared with the preview route.',
  },
  {
    id: 'R-REDACTION-PREVIEW',
    route: '/redaction/preview',
    scope: 'authenticated',
    navSection: 'redaction',
    domain: 'redaction',
    reads: ['POST /api/v1/settings/redaction/preview'],
    mutationKeys: ['redaction:preview'],
    legacyTests: ['redaction-reveal.test.ts', 'redaction-rules.test.ts'],
    requiredScenarios: [
      'route-render',
      'mutation-update',
      'stale-response',
      'mutation-conflict-guard',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/redaction-preview.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_redaction_preview.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_redaction_preview.png',
    ],
    notes:
      'The focused current React run verifies fixture-bounded email/secret preview, request snapshots, independent mutations, stale suppression, selection/highlight/copy, the shared unsaved draft, and responsive panes. Custom-rule and Git-diff effects are not simulated by the fixture.',
  },
  {
    id: 'R-USAGE-AI',
    route: '/request-records/ai',
    scope: 'authenticated',
    navSection: 'request-records',
    domain: 'usage-ai',
    reads: [
      'GET /api/v1/admin/request-records/overview',
      'GET /api/v1/admin/request-records',
      'GET /api/v1/admin/request-records/facets',
      'GET /api/v1/admin/request-records/{id}',
      'GET /api/v1/admin/request-records/{id}/request-full',
      'GET /api/v1/admin/request-records/{id}/session-route-options',
      'GET /api/v1/admin/users/options',
    ],
    mutationKeys: [
      'conversation:<id>',
      'record:<id>',
      'usage:maintenance',
      'usage:perspective',
    ],
    legacyTests: [
      'request-overview.test.ts',
      'request-overview-charts.test.ts',
      'request-record-detail.test.ts',
      'request-records-mapper.test.ts',
      'request-records-store.test.ts',
      'request-record-actions.fixtures.ts',
      'record-actions.fixtures.ts',
      'record-actions.gates.test.ts',
      'record-actions.lifecycle.test.ts',
      'record-actions.sfc.test.ts',
      'request-record-state-badges.test.ts',
      'breakdown-upstream-popover.test.ts',
      'breakdown-upstream-pinning.test.ts',
      'usage-actions.lifecycle.test.ts',
      'usage-actions.gates.test.ts',
      'usage-actions.sfc.test.ts',
      'usage-actions.fixtures.ts',
      'usage-formatting.test.ts',
      'usage-range-picker.test.ts',
      'use-usage-formatting.test.ts',
      'table-pagination-labels.test.ts',
      'display-time-zone-chart.test.ts',
      'display-time-zone-settings.test.ts',
      'display-time-zone.test.ts',
      'app-datetime-format.test.ts',
    ],
    notes:
      'The mounted React route verifies in a real browser: the overview cards, chart and distribution, the upstream drilldown, the local view mode, pagination and page size, search and filters, the preset and custom range windows, a delayed loading surface, an error retry, stale read suppression, the zero-token fold, the transcript paging, the error and abort blocks, the admin session override and affinity writes with their confirmation and pending guard, the history clear with its validation errors, the ordinary-user scope, both locales and the four widths. REACT-AI-SORT-ACCESSIBLE is implemented and browser-verified: real header buttons answer Tab, Enter and Space, cycle ascending, descending and unsorted, map Date to usage_date and Time to created_at, keep the current page offset, and leave non-sortable headers without a control. The approved P0 defect entry stays as history. The zero-token fold matrix is verified in both perspectives and both locales: a positive entry plus success-only, 60%-failure mixed and all-failed exact-zero entries collapse to one visible row, report a hidden request count of 8, expand to four rows with the positive one first and literal 0 totals, change no server total and issue no read. The nullable-token contract is proven on the real record 4 list cells, and the unknown-token popover case is a browser-interception client-robustness probe because the backend aggregate is non-null by contract. The detail evidence includes a /v1/messages caller effort that stays unmerged, a /v1/responses caller-to-override effort, and a sub-session record with its parent id.',
    requiredScenarios: [
      'route-render',
      'empty-state',
      'loading-state',
      'error-state',
      'pagination',
      'stale-response',
      'dialog-open',
      'mutation-update',
      'destructive-confirm',
      'zero-token-fold',
      'long-values',
      'locale-switch',
      'responsive-width',
    ],
    gate: 'P4-reads',
    status: 'baseline-partial',
    foundationTest: 'e2e/foundation/usage.playwright',
    evidence: [
      'e2e/foundation/screenshots/usage-ai-overview-en-light-1440.png',
      'e2e/foundation/screenshots/usage-fold-en-light-1440.png',
      'e2e/foundation/screenshots/usage-fold-en-light-1440-expanded.png',
      'e2e/foundation/screenshots/usage-fold-en-light-390.png',
      'e2e/foundation/screenshots/usage-fold-zh-dark-1440.png',
      'e2e/foundation/screenshots/usage-fold-zh-dark-390.png',
      'e2e/foundation/screenshots/usage-detail-error-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_request-records_ai.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_request-records_ai.png',
      'e2e/baseline/screenshots/regression_fold_zh-CN_collapsed.png',
      'e2e/baseline/screenshots/regression_fold_zh-CN_expanded.png',
      'e2e/baseline/screenshots/regression_fold_en-US_collapsed.png',
      'e2e/baseline/screenshots/regression_fold_en-US_expanded.png',
      'e2e/baseline/screenshots/state_usage_empty_zh_dark_1440.png',
      'e2e/baseline/screenshots/state_usage_error_zh_dark_1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1024_request-records_ai.png',
      'e2e/baseline/screenshots/admin_zh_dark_768_request-records_ai.png',
      'e2e/baseline/screenshots/admin_zh_dark_390_request-records_ai.png',
    ],
  },
  {
    id: 'R-USAGE-MCP',
    route: '/request-records/mcp',
    scope: 'authenticated',
    navSection: 'request-records',
    domain: 'usage-mcp',
    reads: [
      'GET /api/v1/admin/request-records/overview',
      'GET /api/v1/admin/request-records',
      'GET /api/v1/admin/request-records/facets',
      'GET /api/v1/admin/request-records/{id}',
      'GET /api/v1/admin/users/options',
    ],
    mutationKeys: ['usage:maintenance'],
    legacyTests: [
      'request-records-store.test.ts',
      'mcp-record-scope.test.ts',
      'request-records-mapper.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'empty-state',
      'pagination',
      'dialog-open',
      'mutation-update',
      'destructive-confirm',
    ],
    gate: 'P4-reads',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/usage.playwright',
    evidence: [
      'e2e/foundation/screenshots/usage-mcp-records-en-light-1440.png',
      'e2e/foundation/screenshots/usage-mcp-detail-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_request-records_mcp.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_request-records_mcp.png',
    ],
    notes:
      'The mounted React route verifies in a real browser: the five MCP cards, the record scope select, the gateway scope query, the record table columns, the raw request and response payload detail, the error link dialog, the page-two row change, the empty state of a past window, and a cancelled clear that issues no write.',
  },
  {
    id: 'R-BILLING',
    route: '/billing',
    scope: 'authenticated',
    navSection: 'billing',
    domain: 'billing',
    reads: [
      'GET /api/v1/admin/billing/summary',
      'GET /api/v1/admin/billing/charges',
      'GET /api/v1/admin/billing/charges/{id}',
      'GET /api/v1/admin/billing/price-rules',
      'GET /api/v1/admin/billing/export',
      'GET /api/v1/admin/endpoints',
      'GET /api/v1/admin/users/options',
    ],
    mutationKeys: ['billingrule:<id>', 'billing:reprice'],
    legacyTests: [
      'billing-store.test.ts',
      'billing-actions.fixtures.ts',
      'billing-actions.lifecycle.test.ts',
      'billing-actions.sfc.test.ts',
      'billing-responsive-layout.test.ts',
    ],
    notes:
      'React mounts the authenticated billing route with the period/filter controls, the summary, the charge list, the charge detail dialog, the admin price rules and the CSV exports; ordinary users keep the self scope without the admin filters, rules, or reprice. The approved baseline defect REACT-BILLING-RULES-PAGINATION stays recorded as P0 history, and the React fix is closed by the current run in e2e/foundation/billing-result.json#checks.independentPagers: the rule pager changes its own query and rows without a charge fetch, the charge pager changes its own query without a rule fetch, and both page sizes persist under their own keys.',
    requiredScenarios: [
      'route-render',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-delete',
      'mutation-update',
      'mutation-conflict-guard',
      'file-export',
      'long-values',
      'responsive-width',
    ],
    gate: 'P5-writes',
    status: 'baseline-partial',
    foundationTest: 'e2e/foundation/billing.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_billing.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_billing.png',
      'e2e/baseline/screenshots/dialog_billing-price-rule_zh_dark_1440.png',
      'e2e/baseline/screenshots/mutation_billing_reprice_zh_dark_1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1024_billing.png',
      'e2e/baseline/screenshots/admin_zh_dark_768_billing.png',
      'e2e/baseline/screenshots/admin_zh_dark_390_billing.png',
      'e2e/foundation/screenshots/billing-en-light-1440.png',
      'e2e/foundation/screenshots/billing-second-page-en-light-1440.png',
      'e2e/foundation/screenshots/billing-operator-en-light-1440.png',
      'e2e/foundation/screenshots/billing-responsive-en-light-1440.png',
      'e2e/foundation/screenshots/billing-zh-dark-390.png',
    ],
  },
  {
    id: 'R-APPROVALS',
    route: '/approvals',
    scope: 'admin',
    navSection: 'approvals',
    domain: 'approvals',
    reads: ['GET /api/v1/admin/approvals', 'GET /api/v1/admin/approvals/{id}'],
    mutationKeys: ['approval:<id>'],
    legacyTests: [
      'approvals-store.test.ts',
      'remaining-actions.lifecycle.test.ts',
      'remaining-actions.gates.test.ts',
      'remaining-actions.sfc.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'pagination',
      'dialog-open',
      'mutation-update',
      'mutation-conflict-guard',
      'error-state',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/approvals.playwright',
    evidence: [
      'e2e/baseline/screenshots/admin_zh_dark_1440_approvals.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_approvals.png',
      'e2e/baseline/screenshots/mutation_approval_approve_zh_dark_1440.png',
      'e2e/foundation/screenshots/approvals-en-light-1440.png',
      'e2e/foundation/screenshots/approvals-detail-en-light-1440.png',
      'e2e/foundation/screenshots/approvals-responsive-en-light-1440.png',
      'e2e/foundation/screenshots/approvals-zh-dark-390.png',
    ],
    notes:
      'React mounts the admin approvals route behind AdminGuard with the pending default, the resolved filter, the 10/25/50 pager with last-page correction, the mobile pager only when rows overflow, the detail dialog that opens only after a successful read, the shared approval:<id> answer gate, and the per-second remaining wait that is cleared on leave. Current-run coverage in e2e/foundation/approvals-result.json maps approve/reject with no request body, the list refresh and matching detail reload behind one write, the delayed double trigger, the answered row and dialog gates, 503/409/404/403/401 states, latest-wins list and detail reads, and both locales and themes at 1440/1024/768/390.',
  },
  {
    id: 'R-SETTINGS-GENERAL',
    route: '/settings/general',
    scope: 'authenticated',
    navSection: 'settings',
    domain: 'settings',
    reads: [],
    mutationKeys: [],
    legacyTests: [
      'general-settings-copy.test.ts',
      'display-time-zone-settings.test.ts',
      'theme-first-paint.test.ts',
      'i18n-hotspot-copy.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'nav-visibility',
      'locale-switch',
      'theme-switch',
    ],
    gate: 'P3-shell-auth',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/general-settings.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-general-en-light-1440.png',
      'e2e/foundation/screenshots/settings-general-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-general-en-light-390.png',
      'e2e/foundation/screenshots/settings-general-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-general-admin-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_general.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_general.png',
      'e2e/baseline/screenshots/admin_zh_light_1440_settings_general.png',
    ],
    notes:
      'The mounted React route renders the three local preference cards with no server save, refresh or read. The current browser run verifies the authenticated route identity for an ordinary user and an admin, the ordinary-user nav showing only the general settings link, the immediate language and theme effects on the document, storage, title and copy, their persistence across a route change and a reload, the automatic zone naming the browser zone, a filtered searchable zone list, an explicit keyboard-selected zone persisting to storage, and the records time cell rendering the same instant with that zone while the general choice survives the return visit. The web is synthetic-fixture rendered UI and local storage; the tab owns no API.',
  },
  {
    id: 'R-SETTINGS-REQUESTS',
    route: '/settings/requests',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: [
      'GET /api/v1/settings/request-content-logging',
      'GET /api/v1/settings/usage-retention',
      'GET /api/v1/settings/stream-delta-batching',
      'GET /api/v1/settings/model-route-whitelist',
    ],
    mutationKeys: [
      'setting:rawRetention',
      'setting:streamDeltaBatching',
      'setting:modelRouteWhitelist',
    ],
    legacyTests: [
      'settings-store.test.ts',
      'settings-store.fixtures.ts',
      'settings-action-data.fixtures.ts',
      'settings-actions.fixtures.ts',
      'settings-actions.lifecycle.test.ts',
      'settings-actions.gates.test.ts',
      'settings-actions.sfc.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'mutation-update',
      'mutation-conflict-guard',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/settings-requests.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-requests-en-light-1440.png',
      'e2e/foundation/screenshots/settings-requests-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-requests-en-light-390.png',
      'e2e/foundation/screenshots/settings-requests-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-requests-retention-guard-en-light-1440.png',
      'e2e/foundation/screenshots/settings-requests-failure-draft-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_requests.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_requests.png',
      'e2e/baseline/screenshots/mutation_settings_toggle_zh_dark_1440.png',
    ],
    notes:
      'The mounted React route reads only the four requests-scoped settings and autosaves content logging, the two retention fields, stream delta batching and the model-route whitelist under the real shared section keys setting:rawRetention, setting:streamDeltaBatching and setting:modelRouteWhitelist. The current browser run covers the mounted route and its four reads, a full admin denial for an ordinary user with forged 403 and anonymous 401 answers, every exposed control roundtrip with the hidden retention fields preserved, the shared-key claimant spinner and queued draft release, an independent key staying usable, a 503 draft with its retry control, an edit made during a write, the scoped refresh, a stale read that cannot revert a newer edit, both locales and 1440/1024/768/390. The evidence is the synthetic fixture and its stored settings records; no live provider or database behaviour is claimed.',
  },
  {
    id: 'R-SETTINGS-NETWORK',
    route: '/settings/network',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: ['GET /api/v1/settings/relay-ip-whitelist'],
    mutationKeys: ['setting:relayIpWhitelist'],
    legacyTests: [
      'settings-store.test.ts',
      'settings-actions.lifecycle.test.ts',
      'settings-actions.gates.test.ts',
      'settings-actions.sfc.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'mutation-update',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/settings-network.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-network-en-light-1440.png',
      'e2e/foundation/screenshots/settings-network-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-network-en-light-390.png',
      'e2e/foundation/screenshots/settings-network-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-network-validation-en-light-1440.png',
      'e2e/foundation/screenshots/settings-network-failure-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_network.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_network.png',
    ],
    notes:
      'The mounted React route reads only GET /api/v1/settings/relay-ip-whitelist and saves with a manual button under the real setting:relayIpWhitelist key. The current browser run covers the route identity, the two labelled monospace resizable textareas with their single description each, one mount read and no write, no autosave on edit, one save sending both arrays with blank lines removed, the normalized response reaching the form and the store, empty lists as explicit empty arrays, an IPv6/IPv4 roundtrip, a double click sending one request and one confirmation, a draft typed during a delayed save surviving, a 400 server validation error with the draft retained and a corrected retry, a 503 transport failure with one error and a Save retry, the scoped refresh blocked while dirty, no refetch on a locale/theme rerender, fixture probes for malformed IPs, out-of-range prefixes, field-named errors, omitted arrays and duplicate/blank normalization, both locales and 1440/1024/768/390, and a full ordinary-user denial with the general and redaction settings intact. The evidence is the synthetic fixture and its stored policy; no live relay enforcement is claimed.',
  },
  {
    id: 'R-SETTINGS-REVIEW',
    route: '/settings/review',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: ['GET /api/v1/settings/llm-review'],
    mutationKeys: ['setting:llmReview'],
    legacyTests: [
      'settings-store.test.ts',
      'settings-actions.lifecycle.test.ts',
      'settings-actions.gates.test.ts',
      'settings-actions.sfc.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'mutation-update',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/settings-review.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-review-en-light-1440.png',
      'e2e/foundation/screenshots/settings-review-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-review-en-light-390.png',
      'e2e/foundation/screenshots/settings-review-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-review-failure-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_review.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_review.png',
    ],
    notes:
      'The mounted React route reads only GET /api/v1/settings/llm-review and saves the whole object with a manual button under the real setting:llmReview key. The current browser run covers the route identity, all twelve controls with three native disclosure groups and the PostgreSQL hint, password-only secrets that never reach text/titles/storage, one read and no write on mount, no autosave, one save sending every field plus the assembled webhook with first-colon header values preserved, the failure policy switching to fail_open, a locally rejected malformed header line with zero requests, the server 400 for an invalid base URL with the draft retained and a corrected retry, a disabled review accepting semantically invalid values, a double click sending one request, a draft typed during a delayed save surviving, a 503 reported once with a Save retry, the scoped refresh blocked while dirty, no refetch on a locale/theme rerender, both locales at 1440/1024/768/390, and the full ordinary-user denial with the general and redaction settings intact. Secret values are reported as presence booleans only. The evidence is the synthetic fixture; no PostgreSQL review execution, provider call or webhook delivery is claimed.',
  },
  {
    id: 'R-SETTINGS-STORAGE',
    route: '/settings/storage',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: ['GET /api/v1/settings/raw-object-store'],
    mutationKeys: ['setting:storage'],
    legacyTests: [
      'settings-store.test.ts',
      'settings-actions.lifecycle.test.ts',
      'settings-actions.gates.test.ts',
      'settings-actions.sfc.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'mutation-update',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/settings-storage.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-storage-en-light-1440.png',
      'e2e/foundation/screenshots/settings-storage-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-storage-en-light-390.png',
      'e2e/foundation/screenshots/settings-storage-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-storage-failure-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_storage.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_storage.png',
    ],
    notes:
      'The mounted React route reads only GET /api/v1/settings/raw-object-store and saves the whole object with a manual header button under the real setting:storage key. The current browser run covers the route identity, the Local file/S3 compatible/Disabled branches with the server defaults, password-only write-only credentials whose stored values never reach text/titles/storage, one read and no write on mount, no autosave, one save sending the whole object with the server prefix and region normalization, the keep patch for blank drafts and the replace patch for typed ones, no clear control while the API clear mode is verified directly against the fixture, a locally rejected missing S3 bucket with zero requests, the server 400 for a missing bucket and an empty secret, the 501 capability answer shown as an explicit warning with its retry read, a double click sending one request and one confirmation, a draft typed during a delayed save surviving, a 503 reported once with a Save retry, the scoped refresh blocked while dirty or failed, no refetch on a locale/theme rerender, both locales at 1440/1024/768/390, and the full ordinary-user denial with the general and redaction settings intact. Secret values are reported as presence booleans and patch modes only. The evidence is the synthetic fixture; no real directory, bucket, S3 provider or credential validation is claimed.',
  },
  {
    id: 'R-SETTINGS-CACHE-ALERT',
    route: '/settings/cache-alert',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: ['GET /api/v1/settings/cache-alert'],
    mutationKeys: ['setting:cacheAlert'],
    legacyTests: [
      'cache-alert-secret-presence.test.ts',
      'settings-store.test.ts',
      'settings-actions.gates.test.ts',
      'settings-actions.lifecycle.test.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'mutation-update',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/settings-cache-alert.playwright',
    evidence: [
      'e2e/foundation/screenshots/settings-cache-alert-en-light-1440.png',
      'e2e/foundation/screenshots/settings-cache-alert-zh-dark-1440.png',
      'e2e/foundation/screenshots/settings-cache-alert-en-light-390.png',
      'e2e/foundation/screenshots/settings-cache-alert-zh-dark-390.png',
      'e2e/foundation/screenshots/settings-cache-alert-failure-en-light-1440.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_cache-alert.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_cache-alert.png',
    ],
    notes:
      'The mounted React route reads only GET /api/v1/settings/cache-alert and saves the whole policy with a manual header button under the real setting:cacheAlert key, using the PUT this endpoint requires. The current browser run covers the route identity, the enabled switch, the four bounded numbers with the float threshold exact (0.35 saved, 0 and 1 accepted, 0.05 step) and every help line once, the webhook URL, a password-only write-only secret with both the server presence badge and the local keep/replace badge, one read and no write on mount, no autosave, one save sending every field with a blank secret keeping the stored one, a trimmed replace followed by the draft resetting to keep, no clear control, the server 400s for all four out-of-range fields, a partial payload taking the backend defaults instead of merging, the 501 capability answer shown as an explicit warning with its retry read while the page refresh stays disabled, a double click sending one request and one confirmation, a value typed during a delayed save surviving, a 503 reported once with a Save retry, the scoped refresh blocked while dirty or failed, no refetch on a locale/theme rerender, both locales at 1440/1024/768/390, and the full ordinary-user denial with the general and redaction settings intact. Secret values are reported as presence booleans only; the fixture has no path that empties a stored secret, so a false presence badge is covered by the component test. The evidence is the synthetic fixture; no alert detection, DingTalk delivery or production behaviour is claimed.',
  },
  {
    id: 'R-SETTINGS-CONFIG-ARCHIVE',
    route: '/settings/config-archive',
    scope: 'admin',
    navSection: 'settings',
    domain: 'settings',
    reads: ['GET /api/v1/admin/config-audit'],
    mutationKeys: ['configArchive:export', 'configArchive:import'],
    legacyTests: [
      'config-archive.test.ts',
      'config-archive-actions.test.ts',
      'config-archive-actions.fixtures.ts',
    ],
    requiredScenarios: [
      'route-render',
      'admin-denial',
      'pagination',
      'file-export',
      'file-import',
      'destructive-confirm',
      'mutation-failure-draft',
    ],
    gate: 'P5-writes',
    status: 'baseline-verified',
    foundationTest: 'e2e/foundation/config-archive.playwright',
    evidence: [
      'e2e/foundation/screenshots/config-archive-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-export-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-preview-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-confirm-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-applied-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-failure-en-light-1440.png',
      'e2e/foundation/screenshots/config-archive-en-light-390.png',
      'e2e/foundation/screenshots/config-archive-zh-dark-1440.png',
      'e2e/foundation/screenshots/config-archive-zh-dark-390.png',
      'e2e/baseline/screenshots/admin_zh_dark_1440_settings_config-archive.png',
      'e2e/baseline/screenshots/admin_en_dark_1440_settings_config-archive.png',
      'e2e/baseline/screenshots/mutation_config_export_zh_dark_1440.png',
    ],
    notes:
      "The mounted React route reads only GET /api/v1/admin/config-audit once on mount and on explicit refresh, exports with a manual button under configArchive:export and previews/applies under the shared configArchive:import key. The evidence uses the SYNTHETIC fixture archive format (a PROMPT_FERRY_SYNTHETIC_ARCHIVE envelope with two SHA-256 digests, not the production .pfce): no real encryption, Argon2id key derivation or configuration replacement is claimed. The current browser run covers the route identity with the export card, the import wizard and the nine-column audit table from one read and zero writes on mount, password-only passphrases and the archive-only file accept list, the paged audit table with a distinct second page and the persisted config-archive-audit page size, one export sending only the passphrase and saving the real browser download under the fixed prompt-ferry-config.pfce name despite a server-chosen Content-Disposition while reporting the response backend, format, fingerprint and exact byte length and refreshing the trail inside the claim, importing that downloaded archive back (the same file twice) with every per-domain count and warning shown and no audit row from the preview, opening the destructive confirmation with zero requests and cancelling without a write or leftover wizard, the confirmed apply replacing the archive with the applied summary per domain, its audit row and a cleared flow, a wrong passphrase that keeps the file and clears only the consumed passphrase, a corrupt header, an unsupported version and a foreign backend each answering the codec's own code, a 503 keeping the confirmed wizard for a same-body retry, a double export sending one request, the three distinct capability codes recovering on retry, both locales at 1440/1024/768/390 with the audit table panning instead of dropping columns, and the full ordinary-user denial with the general and redaction settings intact. Passphrases and the archive body are reported as presence flags and lengths only. The audit empty-tail page correction and the 32 KB base64 chunking are asserted by the unit suite because the synthetic trail only grows within a run.",
  },
]

export const REDIRECTS: RedirectRow[] = [
  {
    id: 'RD-ROOT',
    from: '/',
    to: '/api-keys',
    reason: 'default authenticated section',
    evidence: [],
  },
  {
    id: 'RD-ENDPOINTS',
    from: '/endpoints',
    to: '/endpoints/upstreams',
    reason: 'first child',
    evidence: [],
  },
  {
    id: 'RD-REDACTION',
    from: '/redaction',
    to: '/redaction/rules',
    reason: 'first child',
    evidence: [],
  },
  {
    id: 'RD-RECORDS',
    from: '/request-records',
    to: '/request-records/ai',
    reason: 'first child',
    evidence: [],
  },
  {
    id: 'RD-SETTINGS',
    from: '/settings',
    to: '/settings/general',
    reason: 'first child',
    evidence: [],
  },
  {
    id: 'RD-CATCH-ALL',
    from: '/<unknown>',
    to: '/api-keys',
    reason: 'catch-all redirect',
    evidence: [],
  },
]

export const SCENARIOS: ScenarioId[] = [
  'route-render',
  'nav-visibility',
  'admin-denial',
  'redirect',
  'login',
  'logout',
  'relay-independent-auth',
  'empty-state',
  'loading-state',
  'error-state',
  'pagination',
  'stale-response',
  'dialog-open',
  'mutation-create',
  'mutation-update',
  'mutation-delete',
  'mutation-toggle',
  'mutation-reconnect',
  'mutation-conflict-guard',
  'mutation-failure-draft',
  'destructive-confirm',
  'file-export',
  'file-import',
  'locale-switch',
  'theme-switch',
  'responsive-width',
  'long-values',
  'zero-token-fold',
]

/** Routes that must exist in the live app, excluding the public login route. */
export const AUTHENTICATED_ROUTES = ROUTES.filter(
  (row) => row.scope !== 'public',
).map((row) => row.route)

/** Routes whose UI the non-admin baseline must be able to open. */
export const NON_ADMIN_ROUTES = ROUTES.filter(
  (row) => row.scope === 'authenticated' && row.route !== '/relay-host',
).map((row) => row.route)

export const ADMIN_ONLY_ROUTES = ROUTES.filter(
  (row) => row.scope === 'admin',
).map((row) => row.route)

export const RELAY_SESSION_ROUTES = ROUTES.filter(
  (row) => row.scope === 'relay-session',
).map((row) => row.route)

/** Vitest suites that live beside the pure form mappers, outside `tests/`. */
export const SRC_TEST_MODULES = [
  'src/admin-mappers/forms/config-archive.test.ts',
  'src/admin-mappers/forms/endpoint.test.ts',
  'src/admin-mappers/forms/model-route.test.ts',
  'src/admin-mappers/forms/relay-host.test.ts',
] as const

export const LEGACY_TEST_OVERRIDES: Record<string, LegacyTestDisposition> = {
  'api-key-selection.fixtures.ts': 'support-fixture',
  'billing-actions.fixtures.ts': 'support-fixture',
  'config-archive-actions.fixtures.ts': 'support-fixture',
  'endpoint-actions.fixtures.ts': 'support-fixture',
  'endpoint-oauth-actions.fixtures.ts': 'support-fixture',
  'endpoint-page-actions.fixtures.ts': 'support-fixture',
  'record-actions.fixtures.ts': 'support-fixture',
  'redaction-actions.fixtures.ts': 'support-fixture',
  'relay-actions.fixtures.ts': 'support-fixture',
  'request-record-actions.fixtures.ts': 'support-fixture',
  'session-login.fixtures.ts': 'support-fixture',
  'settings-action-data.fixtures.ts': 'support-fixture',
  'settings-actions.fixtures.ts': 'support-fixture',
  'settings-store.fixtures.ts': 'support-fixture',
  'usage-actions.fixtures.ts': 'support-fixture',
  'config-archive.test.ts': 'preserved-framework-neutral',
  'config-archive-actions.test.ts': 'pending-react-replacement',
  'relay-host-page.test.ts': 'preserved-form-mapper',
  'redaction-reveal.test.ts': 'preserved-framework-neutral',
  'redaction-rules.test.ts': 'preserved-framework-neutral',
  'request-overview.test.ts': 'preserved-framework-neutral',
  'request-overview-charts.test.ts': 'preserved-framework-neutral',
  'quota-observation-charts.test.ts': 'preserved-framework-neutral',
  'request-records-mapper.test.ts': 'preserved-framework-neutral',
  'table-pagination-labels.test.ts': 'preserved-framework-neutral',
  'app-datetime-format.test.ts': 'preserved-framework-neutral',
  'display-time-zone.test.ts': 'pending-react-replacement',
  'display-time-zone-chart.test.ts': 'pending-react-replacement',
  'display-time-zone-settings.test.ts': 'pending-react-replacement',
  'canonical-quota-windows.test.ts': 'pending-react-replacement',
  'embedding-dimension-mapping.test.ts': 'preserved-framework-neutral',
  'schedule-all-day.test.ts': 'preserved-framework-neutral',
  'quota-observation-trend.test.ts': 'pending-react-replacement',
  'subscription-quota-windows.test.ts': 'preserved-framework-neutral',
  'subscription-quota-history.test.ts': 'pending-react-replacement',
  'token-plan-cache-expiry.test.ts': 'preserved-framework-neutral',
  'token-plan-reset-duration.test.ts': 'preserved-framework-neutral',
  'token-plan-key-status.test.ts': 'preserved-framework-neutral',
  'token-plan-quota-summary.test.ts': 'preserved-framework-neutral',
  'provider-icon.test.ts': 'preserved-framework-neutral',
  'usage-formatting.test.ts': 'preserved-framework-neutral',
  'use-usage-formatting.test.ts': 'pending-react-replacement',
  'row-switch-labels.test.ts': 'pending-react-replacement',
  'i18n-hotspot-copy.test.ts': 'retire-source-string',
  'general-settings-copy.test.ts': 'retire-source-string',
}

/**
 * Classification for legacy test files. Explicit overrides win; the remaining
 * files are grouped by their observable intent so no file is silently dropped.
 * `validate-matrix.ts` reads `tests/` and fails when a file is unclassified.
 */
export function classifyLegacyTest(file: string): LegacyTestDisposition {
  const override = LEGACY_TEST_OVERRIDES[file]
  if (override) return override
  if (file.endsWith('.fixtures.ts')) return 'support-fixture'
  if (file.includes('.sfc.')) return 'retire-source-string'
  if (file.startsWith('src/') || file.includes('/forms/'))
    return 'preserved-form-mapper'
  if (
    /(charts?|format|formatting|mapper|mappers|pagination|reveal|rules|windows|expiry|duration|summary|status|semantics|provider|labels|copy|theme-first-paint|dev-proxy-scope|route-titles|auth|models?)\.test\.ts$/.test(
      file,
    ) &&
    !/(store|actions|lifecycle|gates|state|badges|page|dialog|ux|guard|selection)/.test(
      file,
    )
  ) {
    return 'preserved-framework-neutral'
  }
  return 'pending-react-replacement'
}

/**
 * Immutable inventory of the legacy modules retained at the CL1 plan baseline:
 * the 130 suites under `tests/` plus the four form-mapper suites beside the pure
 * mappers. It is a name catalogue for a future test retirement, not proof that
 * any listed suite still runs; the current matrix rows keep naming them.
 */
export const RETAINED_LEGACY_MODULE_NAMES: string[] = [
  'api-key-selection.fixtures.ts',
  'api-key-selection.lifecycle.test.ts',
  'api-keys-store.test.ts',
  'app-datetime-format.test.ts',
  'app-shell-mobile-nav.test.ts',
  'approvals-store.test.ts',
  'async-action.test.ts',
  'async-actions-review.test.ts',
  'billing-actions.fixtures.ts',
  'billing-actions.lifecycle.test.ts',
  'billing-actions.sfc.test.ts',
  'billing-responsive-layout.test.ts',
  'billing-store.test.ts',
  'breakdown-upstream-pinning.test.ts',
  'breakdown-upstream-popover.test.ts',
  'cache-alert-secret-presence.test.ts',
  'canonical-quota-windows.test.ts',
  'config-archive-actions.fixtures.ts',
  'config-archive-actions.test.ts',
  'config-archive.test.ts',
  'dev-proxy-scope.test.ts',
  'display-time-zone-chart.test.ts',
  'display-time-zone-settings.test.ts',
  'display-time-zone.test.ts',
  'embedding-dimension-mapping.test.ts',
  'endpoint-actions.fixtures.ts',
  'endpoint-actions.gates.test.ts',
  'endpoint-actions.test.ts',
  'endpoint-admin-api-key.test.ts',
  'endpoint-model-input.test.ts',
  'endpoint-oauth-actions.fixtures.ts',
  'endpoint-oauth-browser-flow.test.ts',
  'endpoint-oauth-device-flow.test.ts',
  'endpoint-oauth-status.test.ts',
  'endpoint-oauth.test.ts',
  'endpoint-page-actions.fixtures.ts',
  'endpoint-page-actions.lifecycle.test.ts',
  'endpoint-page-actions.sfc.test.ts',
  'endpoint-page-actions.test.ts',
  'endpoint-reasoning-select.test.ts',
  'endpoint-service-tier-field.test.ts',
  'endpoint-service-tier-subpage.test.ts',
  'endpoint-service-tier.test.ts',
  'endpoints-quota-messages.test.ts',
  'endpoints-store.test.ts',
  'general-settings-copy.test.ts',
  'i18n-hotspot-copy.test.ts',
  'key-quota-observation-chart.test.ts',
  'key-quota-observation-history.test.ts',
  'mcp-dialog-protocol-version-select.test.ts',
  'mcp-record-scope.test.ts',
  'mcp-store.test.ts',
  'model-route-service-tier.test.ts',
  'model-route-suggestion-protocol.test.ts',
  'model-route-target-drag.test.ts',
  'model-route-targets-summary.test.ts',
  'model-route-test-results.test.ts',
  'provider-icon.test.ts',
  'quota-chart-layout.test.ts',
  'quota-display-unknown.test.ts',
  'quota-legend-localization.test.ts',
  'quota-observation-charts.test.ts',
  'quota-observation-trend.test.ts',
  'quota-progress-semantics.test.ts',
  'quota-provider.test.ts',
  'record-actions.fixtures.ts',
  'record-actions.gates.test.ts',
  'record-actions.lifecycle.test.ts',
  'record-actions.sfc.test.ts',
  'redaction-actions.fixtures.ts',
  'redaction-reveal.test.ts',
  'redaction-rules.test.ts',
  'redaction-store.test.ts',
  'relay-actions.fixtures.ts',
  'relay-actions.lifecycle.test.ts',
  'relay-actions.sfc.test.ts',
  'relay-host-actions.lifecycle.test.ts',
  'relay-host-page.test.ts',
  'relay-host-role.test.ts',
  'relay-session-guard.test.ts',
  'relays-store.test.ts',
  'remaining-actions.gates.test.ts',
  'remaining-actions.lifecycle.test.ts',
  'remaining-actions.sfc.test.ts',
  'request-overview-charts.test.ts',
  'request-overview.test.ts',
  'request-record-actions.fixtures.ts',
  'request-record-detail.test.ts',
  'request-record-state-badges.test.ts',
  'request-records-mapper.test.ts',
  'request-records-store.test.ts',
  'route-titles.test.ts',
  'row-switch-labels.test.ts',
  'schedule-all-day.test.ts',
  'service-tier-eligibility.test.ts',
  'session-login-ux.test.ts',
  'session-login.fixtures.ts',
  'session-login.lifecycle.test.ts',
  'settings-action-data.fixtures.ts',
  'settings-actions.fixtures.ts',
  'settings-actions.gates.test.ts',
  'settings-actions.lifecycle.test.ts',
  'settings-actions.sfc.test.ts',
  'settings-store.fixtures.ts',
  'settings-store.test.ts',
  'src/admin-mappers/forms/config-archive.test.ts',
  'src/admin-mappers/forms/endpoint.test.ts',
  'src/admin-mappers/forms/model-route.test.ts',
  'src/admin-mappers/forms/relay-host.test.ts',
  'subscription-quota-history.test.ts',
  'subscription-quota-windows.test.ts',
  'table-pagination-labels.test.ts',
  'theme-first-paint.test.ts',
  'token-plan-badge-surface.test.ts',
  'token-plan-cache-expiry.test.ts',
  'token-plan-foreground-refresh.test.ts',
  'token-plan-key-status.test.ts',
  'token-plan-quota-summary.test.ts',
  'token-plan-reset-duration.test.ts',
  'token-plan-usage-cache-state.test.ts',
  'token-plan-usage-dialog.test.ts',
  'token-plan-usage-request-policy.test.ts',
  'usage-actions.fixtures.ts',
  'usage-actions.gates.test.ts',
  'usage-actions.lifecycle.test.ts',
  'usage-actions.sfc.test.ts',
  'usage-formatting.test.ts',
  'usage-range-picker.test.ts',
  'use-endpoint-oauth-actions.test.ts',
  'use-token-plan-badge-states.test.ts',
  'use-token-plan-badges.test.ts',
  'use-token-plan-usage-cache.test.ts',
  'use-usage-formatting.test.ts',
  'users-store.test.ts',
]

export type RetainedLegacyModule = {
  file: string
  disposition: LegacyTestDisposition
}

/** The retained inventory with the disposition the current classifier assigns. */
export const RETAINED_LEGACY_MODULES: RetainedLegacyModule[] =
  RETAINED_LEGACY_MODULE_NAMES.map((file) => ({
    file,
    disposition: classifyLegacyTest(file),
  }))

/** The specific test files a row's intent currently relies on. */
export const LEGACY_TEST_ENTRIES: LegacyTestEntry[] = ROUTES.flatMap((row) =>
  row.legacyTests.map((file) => ({
    file,
    disposition: classifyLegacyTest(file),
    rows: [row.id],
    counterpart:
      classifyLegacyTest(file) === 'retire-source-string'
        ? 'RTL component suite + Playwright scenario for the same row'
        : classifyLegacyTest(file) === 'pending-react-replacement'
          ? 'React hook/store suite or Playwright scenario for the same row'
          : 'Vitest import-path port with unchanged expectations',
  })),
)

// ------------------------------------------------------- legacy dispositions

export type LegacyModuleState = 'preserved' | 'ported' | 'replaced' | 'obsolete'

export type LegacyModuleEvidence =
  | { kind: 'react-suite'; path: string }
  | { kind: 'react-test'; path: string; test: string }
  | { kind: 'browser-scenario'; path: string; scenario?: string }
  | { kind: 'kept-suite'; path: string }
  | { kind: 'none'; reason: string }

export type LegacyModuleDisposition = {
  file: string
  state: LegacyModuleState
  disposition: LegacyTestDisposition
  evidence: LegacyModuleEvidence[]
}

export const LEGACY_MODULE_STATES: LegacyModuleState[] = [
  'preserved',
  'ported',
  'replaced',
  'obsolete',
]

export const LEGACY_TEST_DISPOSITIONS: LegacyTestDisposition[] = [
  'preserved-framework-neutral',
  'preserved-form-mapper',
  'pending-react-replacement',
  'retire-source-string',
  'support-fixture',
]

export const LEGACY_EVIDENCE_KINDS = [
  'react-suite',
  'react-test',
  'browser-scenario',
  'kept-suite',
  'none',
] as const

/** The project-relative path of a retained legacy module. */
export function legacyModuleSourcePath(file: string): string {
  return file.includes('/') ? file : `tests/${file}`
}

/** A React test title that carries a moved or replaced legacy assertion. */
function reactTest(path: string, test: string): LegacyModuleEvidence {
  return { kind: 'react-test', path, test }
}

/** A React suite plus the exact test title that covers the legacy observable. */
function reactEvidence(path: string, test: string): LegacyModuleEvidence[] {
  return [
    { kind: 'react-suite', path },
    { kind: 'react-test', path, test },
  ]
}

/** A browser result that covers the legacy observable. */
function browserEvidence(path: string): LegacyModuleEvidence {
  return { kind: 'browser-scenario', path }
}

/** One evidence item or a group of them. */
type LegacyEvidenceGroup = LegacyModuleEvidence | LegacyModuleEvidence[]

/** The legacy modules that stay on disk as they are. */
const PRESERVED_LEGACY_MODULES: string[] = [
  'dev-proxy-scope.test.ts',
  'theme-first-paint.test.ts',
  'src/admin-mappers/forms/config-archive.test.ts',
  'src/admin-mappers/forms/endpoint.test.ts',
  'src/admin-mappers/forms/model-route.test.ts',
  'src/admin-mappers/forms/relay-host.test.ts',
]

/** The legacy modules whose assertions were moved into a React test file. */
const PORTED_LEGACY_MODULES: Array<[string, LegacyEvidenceGroup[]]> = [
  [
    'canonical-quota-windows.test.ts',
    [
      reactTest(
        'src/react-tests/canonical-quota.test.ts',
        'canonical observations win over legacy fields',
      ),
    ],
  ],
  [
    'model-route-targets-summary.test.ts',
    [
      reactTest(
        'src/react-tests/model-routes-item.test.ts',
        'lists only enabled targets on enabled endpoints',
      ),
    ],
  ],
  [
    'model-route-test-results.test.ts',
    [
      reactTest(
        'src/react-tests/model-routes-item.test.ts',
        'marks no target passing as a red result',
      ),
    ],
  ],
  [
    'embedding-dimension-mapping.test.ts',
    [
      reactTest(
        'src/react-tests/service-tier.test.ts',
        'carries the dimension override only on the embeddings protocol',
      ),
    ],
  ],
  [
    'endpoint-service-tier.test.ts',
    [
      reactTest(
        'src/react-tests/service-tier.test.ts',
        'trims the service tier and treats a blank value as inherit',
      ),
    ],
  ],
  [
    'service-tier-eligibility.test.ts',
    [
      reactTest(
        'src/react-tests/service-tier.test.ts',
        'keeps the service tier eligible on the JSON protocols only',
      ),
    ],
  ],
  [
    'model-route-service-tier.test.ts',
    [
      reactTest(
        'src/react-tests/service-tier.test.ts',
        'defaults a new model-route target form to inherit the tier',
      ),
    ],
  ],
  [
    'relay-host-page.test.ts',
    [
      reactTest(
        'src/react-tests/relay-host-model.test.ts',
        'shows the running bind while a restart is pending',
      ),
    ],
  ],
  [
    'relay-host-role.test.ts',
    [
      reactTest(
        'src/react-tests/relay-host-model.test.ts',
        'carries only the role in a host role request',
      ),
    ],
  ],
  [
    'schedule-all-day.test.ts',
    [
      reactTest(
        'src/react-tests/schedule-windows.test.ts',
        'keeps a partially filled row invalid',
      ),
    ],
  ],
  [
    'async-action.test.ts',
    [
      reactTest(
        'src/react-tests/async-action.test.tsx',
        'runs different keys together',
      ),
    ],
  ],
  [
    'endpoint-admin-api-key.test.ts',
    [
      reactTest(
        'src/react-tests/endpoints-model.test.ts',
        'applies the OpenAI Admin API Key save, keep and clear contract',
      ),
      reactTest(
        'src/react-tests/i18n-copy.test.tsx',
        'keeps the request sequence label, the endpoint key nickname copy and the extracted OpenAI module table in both locales',
      ),
    ],
  ],
  [
    'endpoint-oauth.test.ts',
    [
      reactTest(
        'src/react-tests/endpoints-model.test.ts',
        'keeps the endpoint plan on the platform fallback away from OpenAI',
      ),
    ],
  ],
  [
    'redaction-rules.test.ts',
    [
      reactTest(
        'src/react-tests/redaction-model.test.ts',
        'flags an invalid regex and accepts the compileable ones',
      ),
    ],
  ],
  [
    'redaction-reveal.test.ts',
    [
      reactTest(
        'src/react-tests/redaction-model.test.ts',
        'toggles one row and closes it again',
      ),
    ],
  ],
  [
    'mcp-dialog-protocol-version-select.test.ts',
    [
      reactTest(
        'src/react-tests/mcp-form.test.tsx',
        'mounts without an empty option and shows the automatic sentinel',
      ),
    ],
  ],
  [
    'provider-icon.test.ts',
    [
      reactTest(
        'src/react-tests/provider-visuals.test.ts',
        'falls back to a letter avatar for the mark-less providers',
      ),
    ],
  ],
  [
    'i18n-hotspot-copy.test.ts',
    [
      reactTest(
        'src/react-tests/i18n-copy.test.tsx',
        'keeps the retired copy keys absent from both locales',
      ),
      reactTest(
        'src/react-tests/mcp-form.test.tsx',
        'shows the localized generic provider name in both locales',
      ),
    ],
  ],
  [
    'general-settings-copy.test.ts',
    [
      reactTest(
        'src/react-tests/settings-general.test.tsx',
        'renders the three preference cards without a save or refresh action',
      ),
    ],
  ],
]

/**
 * The legacy modules whose observable is covered by a React suite or by a
 * browser result. The copied helper is never the evidence: a helper is a
 * support module, so the evidence names the suite that consumes it.
 */
const REPLACED_LEGACY_MODULES: Array<[string, LegacyEvidenceGroup[]]> = [
  [
    'token-plan-foreground-refresh.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'coalesces visible resume events and shares refcounted endpoint ownership',
      ),
    ],
  ],
  [
    'token-plan-usage-cache-state.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'uses one nearest expiry timer and publishes each due id set once',
      ),
    ],
  ],
  [
    'token-plan-usage-request-policy.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'serves stale data immediately, coalesces automatic revalidation, and backs off after warm failure',
      ),
    ],
  ],
  [
    'use-token-plan-usage-cache.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'coalesces cold reads and serves a fresh snapshot without another request',
      ),
    ],
  ],
  [
    'token-plan-cache-expiry.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'uses one nearest expiry timer and publishes each due id set once',
      ),
    ],
  ],
  [
    'token-plan-quota-summary.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps stale exhausted observations historical and only marks ready pools exhausted',
      ),
    ],
  ],
  [
    'quota-progress-semantics.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps canonical empty, unknown, and real zero observations distinct',
      ),
    ],
  ],
  [
    'use-token-plan-badges.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'preserves provider-specific badge ordering and unknown-versus-zero balances',
      ),
    ],
  ],
  [
    'use-token-plan-badge-states.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps the badge layer on the shared pure calculation layer',
      ),
    ],
  ],
  [
    'token-plan-badge-surface.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps the badge layer on the shared pure calculation layer',
      ),
    ],
  ],
  [
    'quota-provider.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'preserves provider-specific badge ordering and unknown-versus-zero balances',
      ),
    ],
  ],
  [
    'quota-observation-charts.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'escapes every untrusted tooltip string and leaves only ECharts markers and separators literal',
      ),
    ],
  ],
  [
    'quota-observation-trend.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'bounds percent axes and retains zero, null gaps, and the injected day boundary',
      ),
    ],
  ],
  [
    'key-quota-observation-chart.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'groups by metric and unit with deterministic amount and percent order',
      ),
    ],
  ],
  [
    'key-quota-observation-history.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-quota-history.test.tsx',
        'defaults to the lazy 24-hour chart, renders native legends, and resets hidden series for a new range',
      ),
    ],
  ],
  [
    'quota-chart-layout.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'bounds percent axes and retains zero, null gaps, and the injected day boundary',
      ),
    ],
  ],
  [
    'quota-legend-localization.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps duplicate key identities, legend names, theme colors, and hidden series stable',
      ),
    ],
  ],
  [
    'subscription-quota-history.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-quota.test.tsx',
        'refreshes the open subscription history only when the first model observation changes',
      ),
    ],
  ],
  [
    'subscription-quota-windows.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps subscription window unknown and zero values distinct and anchors relative resets',
      ),
    ],
  ],
  [
    'quota-display-unknown.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps subscription window unknown and zero values distinct and anchors relative resets',
      ),
    ],
  ],
  [
    'token-plan-key-status.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps pure quantity, money, reset, and progress formatting explicit',
      ),
    ],
  ],
  [
    'token-plan-reset-duration.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'keeps pure quantity, money, reset, and progress formatting explicit',
      ),
    ],
  ],
  [
    'endpoints-quota-messages.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-quota-history.test.tsx',
        'defaults to the lazy 24-hour chart, renders native legends, and resets hidden series for a new range',
      ),
      reactEvidence(
        'src/react-tests/endpoints-quota.test.tsx',
        'renders provider balances, known zero, unknown values, model groups and localized observation times',
      ),
    ],
  ],
  [
    'token-plan-usage-dialog.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-quota.test.tsx',
        'renders provider balances, known zero, unknown values, model groups and localized observation times',
      ),
      reactEvidence(
        'src/react-tests/canonical-quota.test.ts',
        'balances and unlimited rows never report a percentage',
      ),
    ],
  ],
  [
    'request-overview.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-overview.test.tsx',
        'shows every AI metric with its formatted value',
      ),
    ],
  ],
  [
    'breakdown-upstream-popover.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-breakdown.test.tsx',
        'sizes the popover to its intrinsic table width on a wide viewport',
      ),
    ],
  ],
  [
    'breakdown-upstream-pinning.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-breakdown.test.tsx',
        'resets the reveal when the row identity changes',
      ),
    ],
  ],
  [
    'mcp-record-scope.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-overview.test.tsx',
        'maps the MCP gateway row to the gateway scope',
      ),
    ],
  ],
  [
    'row-switch-labels.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-model.test.ts',
        'filters AI model rows by model and upstream rows by endpoint',
      ),
    ],
  ],
  [
    'request-record-state-badges.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-records.test.tsx',
        'renders the state chips with all three severity variants',
      ),
    ],
  ],
  [
    'use-usage-formatting.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-format.test.ts',
        'renders an instant in the explicit display time zone',
      ),
    ],
  ],
  [
    'usage-formatting.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-format.test.ts',
        'renders an instant in the explicit display time zone',
      ),
    ],
  ],
  [
    'request-records-mapper.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-detail.test.tsx',
        'renders the reasoning effort with the override arrow and hides it when unknown',
      ),
    ],
  ],
  [
    'request-record-detail.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-detail.test.tsx',
        'renders the reasoning effort with the override arrow and hides it when unknown',
      ),
    ],
  ],
  [
    'usage-range-picker.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-range.test.ts',
        'builds the half-open UTC window for a two-day selection',
      ),
    ],
  ],
  [
    'usage-actions.gates.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-store.test.ts',
        'refuses a duplicate clear while one is pending and reports nothing',
      ),
    ],
  ],
  [
    'usage-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-store.test.ts',
        'posts the instant payload inside the maintenance guard and refreshes',
      ),
    ],
  ],
  [
    'usage-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-clear.test.tsx',
        'freezes the dialog and only the clear spinner while pending',
      ),
    ],
  ],
  [
    'request-overview-charts.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-charts.test.ts',
        'renders the four stacked token series and the cache-rate line',
      ),
    ],
  ],
  [
    'request-records-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-store.test.ts',
        'refuses a duplicate clear while one is pending and reports nothing',
      ),
      reactEvidence(
        'src/react-tests/usage-store.test.ts',
        'guards the perspective refresh and refreshes only the overview',
      ),
    ],
  ],
  [
    'endpoint-model-input.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoint-models.test.ts',
        'coalesces concurrent requests and retries a failed catalog without caching the failure',
      ),
    ],
  ],
  [
    'model-route-suggestion-protocol.test.ts',
    [
      reactEvidence(
        'src/react-tests/model-routes.test.tsx',
        'uses off-page endpoint protocol in the editor and submits target-associated settings',
      ),
    ],
  ],
  [
    'model-route-target-drag.test.ts',
    [
      reactEvidence(
        'src/react-tests/model-routes.test.tsx',
        'moves the same target object by keyboard and cancels a removed drag source',
      ),
    ],
  ],
  [
    'endpoint-service-tier-field.test.ts',
    [
      reactEvidence(
        'src/react-tests/service-tier.test.ts',
        'lights the target settings gear for a configured tier only',
      ),
    ],
  ],
  [
    'endpoint-service-tier-subpage.test.ts',
    [
      reactEvidence(
        'src/react-tests/service-tier.test.ts',
        'names the service tier control in the endpoint settings copy',
      ),
    ],
  ],
  [
    'endpoint-actions.gates.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'keeps endpoint writes row-scoped and lets a separate probe run concurrently',
      ),
    ],
  ],
  [
    'endpoint-actions.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'keeps endpoint writes row-scoped and lets a separate probe run concurrently',
      ),
    ],
  ],
  [
    'endpoint-page-actions.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'holds the endpoint dialog open until its guarded save and refresh finish',
      ),
    ],
  ],
  [
    'endpoint-page-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'holds the endpoint dialog open until its guarded save and refresh finish',
      ),
    ],
  ],
  [
    'endpoint-page-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'holds the endpoint dialog open until its guarded save and refresh finish',
      ),
    ],
  ],
  [
    'endpoints-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'keeps endpoint writes row-scoped and lets a separate probe run concurrently',
      ),
    ],
  ],
  [
    'endpoint-oauth-browser-flow.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-oauth.test.tsx',
        'clears browser flow only after complete and rereads status, not login endpoint data',
      ),
    ],
  ],
  [
    'endpoint-oauth-device-flow.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-oauth.test.tsx',
        'polls device login through the shared action and completes with a status reread',
      ),
    ],
  ],
  [
    'endpoint-oauth-status.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-oauth.test.tsx',
        'polls device login through the shared action and completes with a status reread',
      ),
    ],
  ],
  [
    'use-endpoint-oauth-actions.test.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-oauth.test.tsx',
        'polls device login through the shared action and completes with a status reread',
      ),
    ],
  ],
  [
    'mcp-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/mcp.test.tsx',
        'keeps admin scope and owner fields available and forces a non-admin create to its caller',
      ),
    ],
  ],
  [
    'endpoint-reasoning-select.test.ts',
    [
      reactEvidence(
        'src/react-tests/model-routes.test.tsx',
        'uses off-page endpoint protocol in the editor and submits target-associated settings',
      ),
    ],
  ],
  [
    'relay-host-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/relay-host.test.tsx',
        'gates reads behind relay authentication, loads all three resources once, and clears only relay data on logout',
      ),
    ],
  ],
  [
    'relay-session-guard.test.ts',
    [
      reactEvidence(
        'src/react-tests/relay-host.test.tsx',
        'gates reads behind relay authentication, loads all three resources once, and clears only relay data on logout',
      ),
    ],
  ],
  [
    'relays-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'shares a row guard across mutations while leaving another row operable',
      ),
    ],
  ],
  [
    'relay-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'holds the create guard through refresh and freezes its dialog',
      ),
    ],
  ],
  [
    'relay-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'holds the create guard through refresh and freezes its dialog',
      ),
    ],
  ],
  [
    'api-keys-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'uses owner-scoped admin create, update, and delete paths',
      ),
    ],
  ],
  [
    'api-key-selection.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'shares one key guard across rename, toggle, and delete confirmation',
      ),
    ],
  ],
  [
    'users-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/users.test.tsx',
        'shares each row guard across reset/save/delete while different users remain independent',
      ),
    ],
  ],
  [
    'record-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/users.test.tsx',
        'shares each row guard across reset/save/delete while different users remain independent',
      ),
    ],
  ],
  [
    'record-actions.gates.test.ts',
    [
      reactEvidence(
        'src/react-tests/mcp.test.tsx',
        'keeps admin scope and owner fields available and forces a non-admin create to its caller',
      ),
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'shares one key guard across rename, toggle, and delete confirmation',
      ),
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'shares a row guard across mutations while leaving another row operable',
      ),
    ],
  ],
  [
    'record-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'holds the create guard through refresh and freezes its dialog',
      ),
      reactEvidence(
        'src/react-tests/usage-clear.test.tsx',
        'freezes the dialog and only the clear spinner while pending',
      ),
    ],
  ],
  [
    'redaction-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/redaction-model.test.ts',
        'runs preview and save independently and keeps both guards separate',
      ),
    ],
  ],
  [
    'approvals-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/approvals.test.tsx',
        'disables the answered row while its own key is held and leaves other rows answerable',
      ),
    ],
  ],
  [
    'billing-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/billing-state.test.ts',
        'refuses a duplicate rule action and keeps another rule independent',
      ),
    ],
  ],
  [
    'billing-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/billing-state.test.ts',
        'holds the rule key through the deferred refresh of a write',
      ),
    ],
  ],
  [
    'billing-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/billing.test.tsx',
        'creates a price rule with the captured payload and closes the dialog on success',
      ),
    ],
  ],
  [
    'billing-responsive-layout.test.ts',
    [
      reactEvidence(
        'src/react-tests/billing.test.tsx',
        'renders the authenticated route with the summary, the charges, and the admin price rules',
      ),
    ],
  ],
  [
    'config-archive.test.ts',
    [
      reactEvidence(
        'src/react-tests/config-archive.test.tsx',
        'sends only the passphrase, downloads once and refreshes the audit inside the claim',
      ),
    ],
  ],
  [
    'config-archive-actions.test.ts',
    [
      reactEvidence(
        'src/react-tests/config-archive.test.tsx',
        'keeps the confirmed wizard on a 503 and retries the same body',
      ),
    ],
  ],
  [
    'settings-store.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-review.test.tsx',
        'keeps the draft on a failed save, reports once and retries with Save',
      ),
    ],
  ],
  [
    'settings-actions.gates.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-requests.test.tsx',
        'keeps the two retention forms on one key and retries the held one on release',
      ),
    ],
  ],
  [
    'settings-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-review.test.tsx',
        'keeps the draft on a failed save, reports once and retries with Save',
      ),
    ],
  ],
  [
    'settings-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-review.test.tsx',
        'renders the controls, native collapsible groups and the storage hint',
      ),
    ],
  ],
  [
    'cache-alert-secret-presence.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-cache-alert.test.tsx',
        'copies presence from the server and never derives it from a draft',
      ),
    ],
  ],
  [
    'session-login.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/parity-routes.test.ts',
        'redirects every concrete source to its matrix target',
      ),
      browserEvidence('e2e/foundation/browser-result.json'),
    ],
  ],
  [
    'session-login-ux.test.ts',
    [
      reactEvidence(
        'src/react-tests/locale-theme.test.tsx',
        'restores and persists the existing locale and theme settings',
      ),
    ],
  ],
  [
    'app-shell-mobile-nav.test.ts',
    [
      reactEvidence(
        'src/react-tests/parity-routes.test.ts',
        'keeps the nav ids, paths and sections aligned with the matrix rows',
      ),
    ],
  ],
  [
    'route-titles.test.ts',
    [
      reactEvidence(
        'src/react-tests/parity-routes.test.ts',
        'routes exactly the matrix route set',
      ),
    ],
  ],
  [
    'async-actions-review.test.ts',
    [
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'preserves a failed create draft and shows only the flat one-time secret on retry',
      ),
    ],
  ],
  [
    'display-time-zone.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-general.test.tsx',
        'shows the automatic zone first and keeps the manual zones reachable',
      ),
    ],
  ],
  [
    'display-time-zone-settings.test.ts',
    [
      reactEvidence(
        'src/react-tests/settings-general.test.tsx',
        'selects an explicit zone with the keyboard and persists it',
      ),
    ],
  ],
  [
    'display-time-zone-chart.test.ts',
    [
      reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'delegates date, timezone, and chart theme through explicit neutral cores',
      ),
    ],
  ],
  [
    'app-datetime-format.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-format.test.ts',
        'keeps a logical UTC day stable in a negative-offset zone',
      ),
    ],
  ],
  [
    'table-pagination-labels.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-records.test.tsx',
        'reports the next page and a new page size',
      ),
    ],
  ],
  [
    'remaining-actions.gates.test.ts',
    [
      reactEvidence(
        'src/react-tests/usage-store.test.ts',
        'refuses a duplicate clear while one is pending and reports nothing',
      ),
    ],
  ],
  [
    'remaining-actions.lifecycle.test.ts',
    [
      reactEvidence(
        'src/react-tests/billing-state.test.ts',
        'refuses a duplicate rule action and keeps another rule independent',
      ),
    ],
  ],
  [
    'remaining-actions.sfc.test.ts',
    [
      reactEvidence(
        'src/react-tests/approvals.test.tsx',
        'disables the answered row while its own key is held and leaves other rows answerable',
      ),
      reactEvidence(
        'src/react-tests/settings-requests.test.tsx',
        'keeps the two retention forms on one key and retries the held one on release',
      ),
    ],
  ],
  [
    'api-key-selection.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'uses owner-scoped admin create, update, and delete paths',
      ),
    ],
  ],
  [
    'billing-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/billing.test.tsx',
        'creates a price rule with the captured payload and closes the dialog on success',
      ),
    ],
  ],
  [
    'config-archive-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/config-archive.test.tsx',
        'sends only the passphrase, downloads once and refreshes the audit inside the claim',
      ),
    ],
  ],
  [
    'endpoint-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'keeps endpoint writes row-scoped and lets a separate probe run concurrently',
      ),
    ],
  ],
  [
    'endpoint-page-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'holds the endpoint dialog open until its guarded save and refresh finish',
      ),
    ],
  ],
  [
    'endpoint-oauth-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/endpoints-oauth.test.tsx',
        'polls device login through the shared action and completes with a status reread',
      ),
    ],
  ],
  [
    'record-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/api-keys.test.tsx',
        'shares one key guard across rename, toggle, and delete confirmation',
      ),
    ],
  ],
  [
    'redaction-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/redaction.test.tsx',
        'blocks invalid regex before PATCH and exposes the local validation error',
      ),
    ],
  ],
  [
    'relay-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/relays.test.tsx',
        'holds the create guard through refresh and freezes its dialog',
      ),
    ],
  ],
  [
    'request-record-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/usage-detail-store.test.ts',
        'keeps the conversation key shared by save, clear and reset',
      ),
    ],
  ],
  [
    'session-login.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/relay-host.test.tsx',
        'gates reads behind relay authentication, loads all three resources once, and clears only relay data on logout',
      ),
      browserEvidence('e2e/foundation/browser-result.json'),
    ],
  ],
  [
    'settings-action-data.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/settings-requests.test.tsx',
        'keeps the two retention forms on one key and retries the held one on release',
      ),
    ],
  ],
  [
    'settings-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/settings-review.test.tsx',
        'keeps the draft on a failed save, reports once and retries with Save',
      ),
    ],
  ],
  [
    'settings-store.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/settings-review.test.tsx',
        'renders the controls, native collapsible groups and the storage hint',
      ),
    ],
  ],
  [
    'usage-actions.fixtures.ts',
    [
      reactEvidence(
        'src/react-tests/usage-page.test.tsx',
        'closes the clear dialog only for the submitted form and refreshes once',
      ),
    ],
  ],
]

/**
 * The disposition of every retained legacy module: the six modules that stay
 * on disk, the nineteen whose assertions were moved into a React test and the
 * hundred and nine whose observable is covered by a React suite or by a
 * browser result. The metadata records what is known today; it never
 * authorizes a deletion.
 */
export const LEGACY_MODULE_DISPOSITIONS: LegacyModuleDisposition[] = [
  ...PRESERVED_LEGACY_MODULES.map((file) => ({
    file,
    state: 'preserved' as LegacyModuleState,
    disposition: classifyLegacyTest(file),
    evidence: [
      { kind: 'kept-suite' as const, path: legacyModuleSourcePath(file) },
    ],
  })),
  ...PORTED_LEGACY_MODULES.map(([file, evidence]) => ({
    file,
    state: 'ported' as LegacyModuleState,
    disposition: classifyLegacyTest(file),
    evidence: evidence.flat(),
  })),
  ...REPLACED_LEGACY_MODULES.map(([file, evidence]) => ({
    file,
    state: 'replaced' as LegacyModuleState,
    disposition: classifyLegacyTest(file),
    evidence: evidence.flat(),
  })),
]

/**
 * The legacy helper modules that sit outside the 134-name inventory: the
 * quota fixture whose copy is now consumed by the React quota suites, and the
 * two legacy test bootstraps whose only importers are the retiring suites.
 */
export const LEGACY_SUPPORT_FILES: LegacyModuleDisposition[] = [
  {
    file: 'tests/helpers/token-plan-quota-fixtures.ts',
    state: 'replaced',
    disposition: 'support-fixture',
    evidence: [
      ...reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'coalesces cold reads and serves a fresh snapshot without another request',
      ),
      ...reactEvidence(
        'src/react-tests/endpoints.test.tsx',
        'prefetches only eligible quota rows and shares ordered badges across table and card',
      ),
    ],
  },
  {
    file: 'tests/helpers/quota-list-renderer.ts',
    state: 'obsolete',
    disposition: 'support-fixture',
    evidence: [
      {
        kind: 'none',
        reason:
          'Its only importer is the retiring legacy suite tests/quota-provider.test.ts, which is itself retired; the rendered quota rows are covered by the React quota suites src/react-tests/quota-core.test.ts (preserves provider-specific badge ordering and unknown-versus-zero balances) and src/react-tests/endpoints-quota.test.tsx (renders provider balances, known zero, unknown values, model groups and localized observation times). The renderer was a legacy DOM helper, not a user-observable behaviour.',
      },
      ...reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'preserves provider-specific badge ordering and unknown-versus-zero balances',
      ),
    ],
  },
  {
    file: 'tests/helpers/token-plan-test-setup.ts',
    state: 'obsolete',
    disposition: 'support-fixture',
    evidence: [
      {
        kind: 'none',
        reason:
          'Its only importers are the retiring legacy suites tests/use-token-plan-badges.test.ts, tests/use-token-plan-badge-states.test.ts, tests/quota-display-unknown.test.ts and tests/token-plan-usage-cache-state.test.ts; the badge and cache behaviour they set up is covered by src/react-tests/quota-core.test.ts (preserves provider-specific badge ordering and unknown-versus-zero balances, uses one nearest expiry timer and publishes each due id set once) and src/react-tests/endpoints-quota.test.tsx (renders provider balances, known zero, unknown values, model groups and localized observation times). The helper was a legacy test bootstrap, not a user-observable behaviour.',
      },
      ...reactEvidence(
        'src/react-tests/quota-core.test.ts',
        'uses one nearest expiry timer and publishes each due id set once',
      ),
    ],
  },
]

export type LegacyDispositionChecks = {
  exists: (path: string) => boolean
  read: (path: string) => string
}

const ALLOWED_EVIDENCE_PREFIXES = ['tests/', 'src/', 'e2e/']

function isAllowedEvidencePath(path: string): boolean {
  if (!path || path.startsWith('/') || path.includes('\\')) return false
  if (path.split('/').includes('..')) return false
  return ALLOWED_EVIDENCE_PREFIXES.some((prefix) => path.startsWith(prefix))
}

/**
 * Checks the disposition metadata against the retained catalogue. It reads the
 * tree through the injected callbacks and never mutates it.
 */
export function validateLegacyModuleDispositions(
  dispositions: readonly LegacyModuleDisposition[],
  expectedFiles: readonly string[],
  checks: LegacyDispositionChecks,
): string[] {
  const problems: string[] = []
  const expected = new Set(expectedFiles)
  const counts = new Map<string, number>()
  for (const entry of dispositions) {
    counts.set(entry.file, (counts.get(entry.file) ?? 0) + 1)
  }
  for (const [file, count] of counts) {
    if (count > 1) problems.push(`duplicate disposition for ${file}`)
    if (!expected.has(file)) problems.push(`unknown legacy module ${file}`)
  }

  for (const file of expectedFiles) {
    const entry = dispositions.find((candidate) => candidate.file === file)
    if (!entry) {
      problems.push(`missing disposition for ${file}`)
      continue
    }
    if (!LEGACY_MODULE_STATES.includes(entry.state)) {
      problems.push(`unknown state ${entry.state} for ${file}`)
    }
    if (!LEGACY_TEST_DISPOSITIONS.includes(entry.disposition)) {
      problems.push(`unknown disposition ${entry.disposition} for ${file}`)
    }
    if (entry.state !== 'obsolete' && entry.evidence.length === 0) {
      problems.push(`missing evidence for ${file}`)
    }

    let existingEvidence = 0
    for (const item of entry.evidence) {
      if (item.kind === 'none') {
        if (!item.reason.trim()) problems.push(`empty reason for ${file}`)
        continue
      }
      if (!isAllowedEvidencePath(item.path)) {
        problems.push(`invalid evidence path ${item.path} for ${file}`)
        continue
      }
      if (!checks.exists(item.path)) {
        problems.push(`dangling evidence path ${item.path} for ${file}`)
        continue
      }
      existingEvidence += 1
      if (item.kind === 'react-test') {
        if (!item.test.trim()) {
          problems.push(`empty test title for ${file}`)
          continue
        }
        if (!checks.read(item.path).includes(item.test)) {
          problems.push(`test title not found in ${item.path} for ${file}`)
        }
      }
    }
    if (entry.state !== 'obsolete' && existingEvidence === 0) {
      problems.push(`no existing evidence path for ${file}`)
    }
    if (entry.state === 'preserved') {
      const source = legacyModuleSourcePath(file)
      if (!checks.exists(source)) {
        problems.push(`preserved legacy file is missing: ${source}`)
      }
    }
    if (entry.state === 'ported') {
      const ported = entry.evidence.some(
        (item) =>
          item.kind === 'react-test' &&
          item.test.trim().length > 0 &&
          checks.exists(item.path) &&
          checks.read(item.path).includes(item.test),
      )
      if (!ported) {
        problems.push(`ported entry lacks a react-test evidence: ${file}`)
      }
    }
    if (entry.state === 'replaced') {
      const replaced = entry.evidence.some(
        (item) =>
          (item.kind === 'react-suite' || item.kind === 'browser-scenario') &&
          checks.exists(item.path),
      )
      if (!replaced) {
        problems.push(
          `replaced entry lacks a react-suite or browser-scenario evidence: ${file}`,
        )
      }
    }
    if (entry.state === 'obsolete') {
      const explained = entry.evidence.some(
        (item) => item.kind === 'none' && item.reason.trim().length > 0,
      )
      if (!explained) {
        problems.push(`obsolete entry lacks a technical reason: ${file}`)
      }
    }
  }

  return problems
}

export const FOCUSED_SCENARIO_EVIDENCE = {
  'R-ENDPOINTS-UPSTREAMS:quota-stale': 'e2e/baseline/quota-stale.json',
  'R-ENDPOINTS-UPSTREAMS:plan-provider-protocol-key-payloads':
    'e2e/baseline/upstream-payloads.json',
  'R-ENDPOINTS-UPSTREAMS:oauth-negative-cancel-quota-list-retry':
    'e2e/baseline/auth-quota-negative.json',
  'R-ENDPOINTS-ROUTES:dimension-schedule-chat-ordering':
    'e2e/baseline/form-payloads.json',
  'R-BILLING:rule-pagination': {
    disposition: 'known-baseline-defect',
    evidence: 'e2e/baseline/four-slices.json#checks.billing',
    source:
      'frontend/src/pages/BillingPage.vue:222; billing.refreshPriceRules is not a function',
    futureFixId: 'REACT-BILLING-RULES-PAGINATION',
    targetRequired: true,
  },
  'R-ENDPOINTS-UPSTREAMS:oauth-quota': 'e2e/baseline/advanced.json',
  'R-AVAILABLE-MODELS:empty-state':
    'e2e/baseline/advanced.json#checks.emptyModels',
  'R-SETTINGS-CONFIG-ARCHIVE:file-import':
    'e2e/baseline/four-slices.json#checks.archive',
  'R-BILLING:filter-and-charge-pagination':
    'e2e/baseline/four-slices.json#checks.billing',
  'R-REDACTION-RULES:validation-search-payload':
    'e2e/foundation/redaction-rules-result.json#checks.validationSearchPayload',
  'R-USAGE-AI:sorting': {
    disposition: 'known-baseline-defect',
    evidence: 'e2e/baseline/four-slices.json#checks.sort',
    source: 'frontend/src/components/usage/UsagePanel.vue',
    futureFixId: 'REACT-AI-SORT-ACCESSIBLE',
    targetRequired: true,
  },
} as const

export const PLAN_CONSTANTS = {
  issue: 1060,
  baselineCommit: '6fe05da10202be5c84dad9abdf9faf4b7be1a522',
  baselineVersion: '1.42.30',
  routes: ROUTES.length,
  redirects: REDIRECTS.length,
  scenarios: SCENARIOS.length,
  fixtureOnly: true as const,
} as const

export function rowsForScenario(scenario: ScenarioId): RouteRow[] {
  return ROUTES.filter((row) => row.requiredScenarios.includes(scenario))
}

export function rowsForDomain(domain: Domain): RouteRow[] {
  return ROUTES.filter((row) => row.domain === domain)
}
