import { existsSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type EndpointsReport = {
  status?: string
  runId?: string
  startedAt?: string
  finishedAt?: string
  callbacks?: string[]
  checks?: Record<string, any>
  screenshots?: string[]
  segments?: Array<{ name: string; startedAt: string; finishedAt: string }>
}

/** The ten source slices the runner merges, in their serial order. */
export const EXPECTED_CALLBACKS = [
  'endpoints',
  'endpoints-create',
  'endpoints-update',
  'endpoints-mutations',
  'endpoints-delete',
  'endpoints-quota',
  'endpoints-oauth-status',
  'endpoints-oauth-device',
  'endpoints-oauth-browser',
  'endpoints-finish',
] as const

const requiredScenarios = [
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
]

const requiredChecks = [
  'adminDenial',
  'routeAndNavigation',
  'adminUserOptionsRead',
  'pagination',
  'pageSizePersistence',
  'loadingState',
  'errorState',
  'createDialogAndRetry',
  'createPayloads',
  'updateFailureDraftRetry',
  'keyRowsBehavior',
  'proxyClearAndScheduleEmpty',
  'toggleAndIndependentProbe',
  'deleteCancelAndSuccess',
  'genericQuotaIneligible',
  'fixtureQuotaStatusScope',
  'tableMobileQuotaPills',
  'oauthNegativeCancelQuotaListRetry',
  'keyQuotaHistory',
  'oauthQuota',
  'subscriptionQuotaHistory',
  'oauthStatusAndStartFailures',
  'deviceOAuthCompletion',
  'browserOAuth',
  'oauthGenerationGuards',
  'responsiveContainment',
  'localeThemeRendering',
  'quotaStale',
  'expectedApiFailures',
  'apiFailures',
  'consoleErrors',
  'pageErrors',
  'scenarioCoverage',
  'syntheticSecretSafety',
  'listGeometry',
]

export const EXPECTED_SCREENSHOTS = [
  'e2e/foundation/screenshots/endpoints-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-created-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-quota-current-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-quota-chart-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-quota-history-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-subscription-history-en-light-1440.png',
  'e2e/foundation/screenshots/endpoints-en-light-1024.png',
  'e2e/foundation/screenshots/endpoints-en-light-768.png',
  'e2e/foundation/screenshots/endpoints-en-light-390.png',
  'e2e/foundation/screenshots/endpoints-zh-dark-1440.png',
  'e2e/foundation/screenshots/endpoints-en-light-1440-final.png',
  'e2e/foundation/screenshots/endpoints-quota-fresh-en-light-1440.png',
]

/** Files are allowed a small tolerance around the run window for clock precision. */
export const RUN_WINDOW_TOLERANCE_MS = 5000

export type ScreenshotStat =
  { exists: true; mtimeMs: number } | { exists: false }

/**
 * Parse a report body without throwing.
 *
 * An invalid or non-object payload is reported as such, so a partially written
 * or raw CLI file never becomes a stack trace or a false pass.
 */
export function parseEndpointsReport(
  text: string,
): { ok: true; report: EndpointsReport } | { ok: false; failure: string } {
  let value: unknown
  try {
    value = JSON.parse(text)
  } catch {
    return {
      ok: false,
      failure:
        'the focused endpoints report is not valid JSON; an aborted or raw callback report cannot stand in for a completed run',
    }
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return {
      ok: false,
      failure: 'the focused endpoints report is not a report object',
    }
  }
  return { ok: true, report: value as EndpointsReport }
}

/**
 * The completeness gate, evaluated before any downstream assertion.
 *
 * A report that is missing its complete marker, its exact ordered callback list,
 * or a usable per-callback window is an incomplete run. Reporting it as such
 * keeps a missing screenshot from being blamed on a run that never finished.
 */
export function completenessFailures(report: EndpointsReport): string[] {
  const failures: string[] = []
  if (report.status !== 'complete') {
    failures.push(
      `the focused endpoints run is incomplete: the report has no complete marker (status=${String(report.status)})`,
    )
  }
  const names = (report.segments ?? []).map((segment) => segment?.name)
  if (JSON.stringify(names) !== JSON.stringify([...EXPECTED_CALLBACKS])) {
    failures.push(
      `the focused endpoints run is incomplete: expected the ten serial segments ${EXPECTED_CALLBACKS.join(', ')} in order, found ${names.join(', ') || 'none'}`,
    )
  }
  if (
    JSON.stringify(report.callbacks ?? []) !==
    JSON.stringify([...EXPECTED_CALLBACKS])
  ) {
    failures.push(
      `the focused endpoints run is incomplete: the completed callback list is ${(report.callbacks ?? []).join(', ') || 'missing'}`,
    )
  }
  for (const segment of report.segments ?? []) {
    const started = Date.parse(segment?.startedAt ?? '')
    const finished = Date.parse(segment?.finishedAt ?? '')
    if (!Number.isFinite(started) || !Number.isFinite(finished)) {
      failures.push(
        `the focused endpoints run is incomplete: callback ${String(segment?.name)} has no usable start/finish window`,
      )
    }
  }
  return failures
}

/** Run-window check for one screenshot file, so an old PNG cannot satisfy it. */
export function screenshotFreshnessFailures(
  screenshot: string,
  stat: ScreenshotStat,
  window: { startedAt: number; finishedAt: number },
): string[] {
  if (!stat.exists) {
    return [`upstream screenshot file is missing: ${screenshot}`]
  }
  const mtime = stat.mtimeMs
  if (
    !Number.isFinite(mtime) ||
    mtime < window.startedAt - RUN_WINDOW_TOLERANCE_MS ||
    mtime > window.finishedAt + RUN_WINDOW_TOLERANCE_MS
  ) {
    return [`upstream screenshot is stale and predates this run: ${screenshot}`]
  }
  return []
}

/**
 * Evaluate a parsed report. `stat` and `now` are injected so the browser-free
 * negative probes can drive the same rules without touching any artifact.
 */
export function evaluateEndpointsReport(
  report: EndpointsReport,
  options: {
    stat: (screenshot: string) => ScreenshotStat
    now: number
  },
): { failures: string[]; complete: boolean } {
  const failures = [...completenessFailures(report)]
  if (failures.length > 0) return { failures, complete: false }

  const checks = report.checks ?? {}
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  const runTimestamp = Number(report.runId?.slice('endpoints-'.length))

  if (
    typeof report.runId !== 'string' ||
    !report.runId.startsWith('endpoints-')
  ) {
    failures.push('report runId is missing or malformed')
  }
  if (!Number.isFinite(startedAt) || !Number.isFinite(finishedAt)) {
    failures.push('report timestamps are incomplete')
  } else {
    if (
      !Number.isFinite(runTimestamp) ||
      Math.abs(runTimestamp - startedAt) > 1000
    ) {
      failures.push('report runId is not current to its start timestamp')
    }
    if (finishedAt < startedAt) {
      failures.push('report finish timestamp precedes its start')
    } else if (options.now - finishedAt > 15 * 60 * 1000) {
      failures.push(
        'report is stale: this run finished more than 15 minutes ago',
      )
    }
  }

  for (const check of requiredChecks) {
    if (checks[check] === undefined) {
      failures.push(`current-run upstream check is missing: ${check}`)
    }
  }
  for (const scenario of requiredScenarios) {
    if (checks.scenarioCoverage?.[scenario] !== true) {
      failures.push(
        `current-run upstream scenario is missing or false: ${scenario}`,
      )
    }
  }

  if (
    checks.routeAndNavigation?.routeId !== 'R-ENDPOINTS-UPSTREAMS' ||
    checks.routeAndNavigation?.navVisible !== true
  ) {
    failures.push(
      'admin upstream route identity or navigation evidence is missing',
    )
  }
  if (checks.adminUserOptionsRead !== true) {
    failures.push('admin upstream owner-options read was not observed')
  }
  const listGeometry = checks.listGeometry as
    Record<string, any> | undefined | null
  if (!listGeometry || typeof listGeometry !== 'object') {
    failures.push('current-run upstream list geometry evidence is missing')
  } else {
    for (const key of [
      'desktop-1440',
      'desktop-1024',
      'desktop-768',
      'mobile-390',
    ]) {
      const measurement = listGeometry[key]
      if (!measurement || typeof measurement !== 'object') {
        failures.push(
          `the upstream list geometry is missing the ${key} measurement`,
        )
        continue
      }
      const urls = Array.isArray(measurement.urls) ? measurement.urls : []
      if (urls.length === 0) {
        failures.push(
          `the upstream list geometry recorded no endpoint URL at ${key}`,
        )
      }
      for (const url of urls) {
        if (url?.wordBreak === 'break-all') {
          failures.push(`an endpoint URL still breaks mid token at ${key}`)
        }
        if (url?.overflowWrap !== 'break-word') {
          failures.push(
            `an endpoint URL does not wrap at word boundaries at ${key}`,
          )
        }
        if (
          typeof url?.text !== 'string' ||
          url.text.length === 0 ||
          url.text !== url?.title
        ) {
          failures.push(
            `an endpoint URL is not fully present in the DOM and its title at ${key}`,
          )
        }
        if (
          !Number.isFinite(url?.lines) ||
          !Number.isFinite(url?.safeBreaks) ||
          url.lines < 1 ||
          url.lines > url.safeBreaks
        ) {
          failures.push(
            `an endpoint URL wraps beyond its safe break points at ${key}`,
          )
        }
      }
      const metas = Array.isArray(measurement.metaLabels)
        ? measurement.metaLabels
        : []
      if (metas.length === 0) {
        failures.push(
          `the upstream list geometry recorded no endpoint meta line at ${key}`,
        )
      }
      for (const labels of metas) {
        if (!Array.isArray(labels) || labels.length === 0) {
          failures.push(`an endpoint meta line is empty at ${key}`)
          continue
        }
        if (new Set(labels).size !== labels.length) {
          failures.push(`an endpoint meta line repeats a label at ${key}`)
        }
      }
      if (!key.startsWith('desktop-')) continue
      const headNames = Array.isArray(measurement.headNames)
        ? measurement.headNames
        : []
      if (
        JSON.stringify(headNames) !==
        JSON.stringify(['Endpoint', 'Status', 'Usage', 'Test', 'Actions'])
      ) {
        failures.push(
          `the upstream list header set is not the honest column set at ${key}: ${JSON.stringify(headNames)}`,
        )
      }
      if (measurement.actionHeaderName !== 'Actions') {
        failures.push(
          `the upstream list action header has no accessible name at ${key}`,
        )
      }
      const alignment = Array.isArray(measurement.headAlignment)
        ? measurement.headAlignment
        : []
      if (
        alignment.length !== headNames.length ||
        alignment.some(
          (entry: any) => !Number.isFinite(entry?.delta) || entry.delta > 1,
        )
      ) {
        failures.push(
          `the upstream list headers do not align with their cells at ${key}`,
        )
      }
      if (measurement.tableColumns !== headNames.length) {
        failures.push(
          `the upstream list row columns do not match the header set at ${key}`,
        )
      }
      if (measurement.documentOverflow !== false) {
        failures.push(
          `the upstream list overflows the document horizontally at ${key}`,
        )
      }
      if (measurement.tableScrolls !== 'auto') {
        failures.push(
          `the upstream list table container does not own the horizontal scroll at ${key}`,
        )
      }
      const badges = Array.isArray(measurement.badgeOverflow)
        ? measurement.badgeOverflow
        : []
      for (const badge of badges) {
        if (badge?.overflowsCell !== false) {
          failures.push(
            `the upstream usage badges overflow their cell at ${key}`,
          )
        }
        if (badge?.wrap !== 'wrap') {
          failures.push(
            `the upstream usage badges do not wrap inside their cell at ${key}`,
          )
        }
      }
      const rowQuota = Array.isArray(measurement.rowQuota)
        ? measurement.rowQuota
        : []
      for (const entry of rowQuota) {
        if (typeof entry?.empty !== 'boolean') {
          failures.push(
            `an upstream row did not record its quota cell state at ${key}`,
          )
          continue
        }
        if (entry.empty !== !entry.hasQuotaButton) {
          failures.push(
            `an inapplicable upstream quota cell is not empty at ${key}`,
          )
        }
      }
    }
  }
  if (
    checks.adminDenial?.redirectedToApiKeys !== true ||
    checks.adminDenial?.hiddenNavigation !== true ||
    checks.adminDenial?.adminApiCalls !== 0
  ) {
    failures.push('non-admin upstream route denial or API isolation failed')
  }
  if (
    checks.pagination?.initialRows !== 10 ||
    checks.pagination?.secondPageRows !== 3 ||
    new Set(checks.pagination?.secondPageIds ?? []).size !== 3
  ) {
    failures.push('endpoint pagination/distinct row evidence is incomplete')
  }
  if (checks.pageSizePersistence !== true) {
    failures.push('endpoint page-size persistence was not verified')
  }
  if (
    checks.loadingState !== true ||
    checks.errorState?.visible !== true ||
    checks.errorState?.retried !== true
  ) {
    failures.push('loading or list-error retry behavior was not verified')
  }
  if (
    checks.createDialogAndRetry?.pending !== true ||
    checks.createDialogAndRetry?.dismissalBlocked !== true ||
    checks.createDialogAndRetry?.draftRetained !== true
  ) {
    failures.push(
      'create pending, dismissal, or retryable-draft evidence is missing',
    )
  }
  if (
    checks.createPayloads?.length !== 2 ||
    !checks.createPayloads.every(
      (payload: Record<string, any>) =>
        payload.provider === 'minimax' &&
        payload.scope === 'user' &&
        payload.owner_user_id === 2 &&
        payload.base_url_trimmed === true &&
        payload.api_key_present === true &&
        payload.admin_api_key_present === false,
    )
  ) {
    failures.push(
      'create payloads did not preserve trimmed provider/owner/key semantics',
    )
  }
  if (
    checks.updateFailureDraftRetry?.rowToggleDisabled !== true ||
    checks.updateFailureDraftRetry?.independentProbeEnabled !== true ||
    checks.updateFailureDraftRetry?.otherEndpointAvailable !== true ||
    checks.updateFailureDraftRetry?.draftRetained !== true ||
    checks.updateFailureDraftRetry?.retryStatus !== 200
  ) {
    failures.push('shared endpoint guard or retryable update draft failed')
  }
  if (
    checks.keyRowsBehavior?.replacementSent !== true ||
    checks.keyRowsBehavior?.replacementReopenedSaved !== true ||
    checks.keyRowsBehavior?.removedSavedRow !== true
  ) {
    failures.push(
      'saved endpoint key keep/replace/remove behavior is incomplete',
    )
  }
  if (
    checks.proxyClearAndScheduleEmpty?.proxyEmptySent !== true ||
    checks.proxyClearAndScheduleEmpty?.emptyScheduleSent !== true ||
    checks.proxyClearAndScheduleEmpty?.settingsReopened !== true
  ) {
    failures.push(
      'proxy clear or empty schedule payload/reopen evidence is incomplete',
    )
  }
  if (
    checks.toggleAndIndependentProbe?.probeResultRendered !== true ||
    checks.toggleAndIndependentProbe?.restored !== true
  ) {
    failures.push('endpoint toggle/probe behavior is incomplete')
  }
  if (
    checks.deleteCancelAndSuccess?.cancelledWrites !== 0 ||
    checks.deleteCancelAndSuccess?.successfulDelete !== true
  ) {
    failures.push('endpoint delete cancel/success behavior is incomplete')
  }
  if (
    checks.genericQuotaIneligible?.actionCount !== 0 ||
    checks.genericQuotaIneligible?.apiCalls !== 0
  ) {
    failures.push('generic endpoint exposed quota controls or requests')
  }
  if (
    checks.fixtureQuotaStatusScope?.quotaStatusScope !== 'text_inference' ||
    checks.fixtureQuotaStatusScope?.fiveHourWindowKind !== 'five_hour'
  ) {
    failures.push('quota fixture status scope or canonical window kind changed')
  }
  if (
    checks.tableMobileQuotaPills?.rendered !== true ||
    checks.tableMobileQuotaPills?.identical !== true ||
    !(checks.tableMobileQuotaPills?.tablePillCount > 0) ||
    !(checks.tableMobileQuotaPills?.cardPillCount > 0)
  ) {
    failures.push(
      'eligible table and mobile quota pills were missing or differed',
    )
  }
  if (
    checks.keyQuotaHistory?.actualCanvas !== true ||
    checks.keyQuotaHistory?.lazyImportObserved !== true ||
    checks.keyQuotaHistory?.ranges?.join(',') !== '24h,7d,30d'
  ) {
    failures.push(
      'key quota chart was not observed as a lazy real chart across all ranges',
    )
  }
  if (
    checks.oauthNegativeCancelQuotaListRetry?.quotaErrorVisible !== true ||
    checks.oauthNegativeCancelQuotaListRetry?.quotaRetry !== true ||
    checks.oauthNegativeCancelQuotaListRetry?.listPreservedOnChartRoundtrip !==
      true
  ) {
    failures.push(
      'quota list failure/retry and chart round-trip evidence is incomplete',
    )
  }
  if (
    checks.subscriptionQuotaHistory?.query?.limit !== '50' ||
    checks.subscriptionQuotaHistory?.sourceRendered !== true ||
    checks.subscriptionQuotaHistory?.noLoadMore !== true
  ) {
    failures.push(
      'subscription history did not verify its first page and fixture cursor behavior',
    )
  }
  if (
    checks.oauthStatusAndStartFailures?.refreshStatuses?.join(',') !==
      '500,200' ||
    checks.oauthStatusAndStartFailures?.deviceStartFailureVisible !== true ||
    checks.oauthStatusAndStartFailures?.idlePollCancellation !== true
  ) {
    failures.push('OAuth status/refresh/device-start behavior is incomplete')
  }
  if (
    checks.deviceOAuthCompletion?.pendingThenComplete !== true ||
    checks.deviceOAuthCompletion?.noVerificationNavigation !== true
  ) {
    failures.push(
      'device OAuth polling completion or external-navigation guard failed',
    )
  }
  if (
    checks.browserOAuth?.completionStatuses?.join(',') !== '503,200' ||
    checks.browserOAuth?.redirectMasked !== true ||
    checks.browserOAuth?.redirectDraftRetained !== true ||
    checks.browserOAuth?.noExternalNavigation !== true
  ) {
    failures.push(
      'browser OAuth failure/retry or secret-safety evidence is incomplete',
    )
  }
  if (
    checks.oauthGenerationGuards?.providerChangeBrowserVerified !== true ||
    checks.oauthGenerationGuards?.endpointSwitchRetained !== true
  ) {
    failures.push(
      'stale OAuth response overwrote a changed provider or endpoint draft',
    )
  }
  if (
    checks.responsiveContainment
      ?.map((entry: { width: number }) => entry.width)
      .join(',') !== '1440,1024,768,390' ||
    !checks.responsiveContainment.every(
      (entry: { contained: boolean; endpointVisible: boolean }) =>
        entry.contained && entry.endpointVisible,
    )
  ) {
    failures.push('upstream responsive containment did not pass all viewports')
  }
  if (
    checks.localeThemeRendering?.zhDark?.lang !== 'zh-CN' ||
    checks.localeThemeRendering.zhDark.dark !== true ||
    checks.localeThemeRendering?.enLight?.lang !== 'en-US' ||
    checks.localeThemeRendering.enLight.dark !== false
  ) {
    failures.push(
      'both locale/theme states were not verified on the upstream route',
    )
  }
  if (
    checks.quotaStale?.staleLabelsVisible !== true ||
    checks.quotaStale.staleNumbersRetained !== true ||
    checks.quotaStale.freshLabelsCleared !== true
  ) {
    failures.push(
      'stale quota retained-value and fresh-label reset checks failed',
    )
  }
  if (
    !Array.isArray(checks.expectedApiFailures) ||
    checks.expectedApiFailures.length !== 8 ||
    checks.apiFailures?.length !== 0
  ) {
    failures.push(
      'current-run injected/unexpected API failure ledger is incomplete',
    )
  }
  if (checks.consoleErrors?.length !== 0 || checks.pageErrors?.length !== 0) {
    failures.push('browser console or page errors were reported')
  }
  if (
    checks.syntheticSecretSafety?.reportRedacted !== true ||
    checks.syntheticSecretSafety?.endpointLedgerRedacted !== true
  ) {
    failures.push('synthetic credentials were not verified redacted')
  }

  for (const screenshot of EXPECTED_SCREENSHOTS) {
    if (!(report.screenshots ?? []).includes(screenshot)) {
      failures.push(`current report is missing screenshot ${screenshot}`)
    }
  }
  const window = { startedAt, finishedAt }
  for (const screenshot of EXPECTED_SCREENSHOTS) {
    failures.push(
      ...screenshotFreshnessFailures(
        screenshot,
        options.stat(screenshot),
        window,
      ),
    )
  }

  return { failures, complete: true }
}

function main(): void {
  const root = import.meta.dir
  const reportPath = join(root, 'endpoints-result.json')
  const failures: string[] = []
  const assert = (condition: unknown, message: string): void => {
    if (!condition) failures.push(message)
  }

  if (!existsSync(reportPath)) {
    console.error(
      'Upstream focused validation failed:\n- current upstream browser report is missing; run run-endpoints.sh',
    )
    process.exit(1)
  }

  const parsed = parseEndpointsReport(readFileSync(reportPath, 'utf8'))
  if (!parsed.ok) {
    console.error(`Upstream focused validation failed:\n- ${parsed.failure}`)
    process.exit(1)
  }
  const report = parsed.report

  const evaluation = evaluateEndpointsReport(report, {
    stat: (screenshot) => {
      const path = join(root, '..', '..', screenshot)
      return existsSync(path)
        ? { exists: true, mtimeMs: statSync(path).mtimeMs }
        : { exists: false }
    },
    now: Date.now(),
  })
  failures.push(...evaluation.failures)

  const route = ROUTES.find(
    (candidate) => candidate.id === 'R-ENDPOINTS-UPSTREAMS',
  )
  assert(
    route?.foundationTest === 'e2e/foundation/endpoints.playwright',
    'R-ENDPOINTS-UPSTREAMS does not map to the current focused React run',
  )
  assert(
    route?.status === 'baseline-verified',
    'historical P0 upstream status changed',
  )
  assert(
    JSON.stringify(route?.mutationKeys) ===
      JSON.stringify([
        'endpoint:<id>',
        'endpoint:create',
        'networktest:endpoint:<id>',
      ]),
    'endpoint mutation key mapping does not match the shared endpoint/network-test keys',
  )
  assert(
    route?.reads.includes('GET /api/v1/admin/users/options'),
    'endpoint user-options read is missing from the route mapping',
  )
  for (const scenario of requiredScenarios) {
    assert(
      route?.requiredScenarios.includes(
        scenario as (typeof route.requiredScenarios)[number],
      ),
      `matrix is missing ${scenario}`,
    )
  }
  assert(
    route?.evidence.includes(
      'e2e/baseline/screenshots/admin_zh_dark_1440_endpoints_upstreams.png',
    ),
    'historical P0 upstream screenshot evidence changed',
  )
  assert(
    route?.legacyTests.includes('endpoint-oauth.test.ts') &&
      route?.legacyTests.includes('token-plan-usage-dialog.test.ts'),
    'upstream legacy test disposition lost OAuth or quota suites',
  )

  const authCallback = readFileSync(join(root, 'auth-shell.playwright'), 'utf8')
  assert(
    !authCallback.includes("['/endpoints/upstreams', 'R-ENDPOINTS-UPSTREAMS']"),
    'ported upstream route remains in the placeholder list',
  )
  assert(
    (authCallback.match(/\['\/[^']+', 'R-[^']+'\]/g) ?? []).length === 0,
    'auth-shell must not retain a route placeholder',
  )

  if (failures.length > 0) {
    console.error(
      `Upstream focused validation failed:\n- ${failures.join('\n- ')}`,
    )
    process.exitCode = 1
  } else {
    console.log(
      'Mounted upstream route, current-run evidence and matrix mapping are valid.',
    )
  }
}

if (import.meta.main) main()
