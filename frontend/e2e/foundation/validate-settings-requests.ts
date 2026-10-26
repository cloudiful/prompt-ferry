import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type SettingsRequestsReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  scenarios?: Record<string, Record<string, ScenarioEntry>>
  screenshots?: string[]
  apiFailures?: string[]
  expectedApiFailures?: string[]
  unexpectedRequests?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
  limitations?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'settings-requests-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

const REQUIRED_SCENARIOS = [
  'route-render',
  'admin-denial',
  'mutation-update',
  'mutation-conflict-guard',
  'mutation-failure-draft',
]

const REQUIRED_READS = [
  'GET /api/v1/settings/request-content-logging',
  'GET /api/v1/settings/usage-retention',
  'GET /api/v1/settings/stream-delta-batching',
  'GET /api/v1/settings/model-route-whitelist',
]

const REQUIRED_MUTATION_KEYS = [
  'setting:rawRetention',
  'setting:streamDeltaBatching',
  'setting:modelRouteWhitelist',
]

const REMAINING_SETTINGS_PLACEHOLDERS: string[] = []

const REQUIRED_SCREENSHOTS = [
  'e2e/foundation/screenshots/settings-requests-en-light-1440.png',
  'e2e/foundation/screenshots/settings-requests-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-requests-en-light-390.png',
  'e2e/foundation/screenshots/settings-requests-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-requests-retention-guard-en-light-1440.png',
  'e2e/foundation/screenshots/settings-requests-failure-draft-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused settings-requests report is missing; run run-settings-requests.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-REQUESTS')

assert(
  row?.foundationTest === 'e2e/foundation/settings-requests.playwright',
  'R-SETTINGS-REQUESTS is not mapped to the focused settings-requests browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-REQUESTS no longer requires the five verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_READS].sort()),
  'the requests row does not read exactly the four requests-scoped settings',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_MUTATION_KEYS].sort()),
  'the requests row does not declare the real shared section keys',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the requests row lost its settings/write gate identity',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the requests row does not cite the current-run screenshots',
)

if (existsSync(authShellPath)) {
  const authShell = readFileSync(authShellPath, 'utf8')
  const pendingRoutes =
    authShell.match(/const pendingRoutes = \[([\s\S]*?)\n  \]/)?.[1] ?? ''
  const pendingIds = [
    ...pendingRoutes.matchAll(/\['\/[^']+', '([^']+)'\]/g),
  ].map((match) => match[1] ?? '')
  assert(
    pendingIds.length === 0 &&
      REMAINING_SETTINGS_PLACEHOLDERS.every((id) => pendingIds.includes(id)) &&
      !pendingIds.includes('R-SETTINGS-REQUESTS'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as SettingsRequestsReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('settings-requests-'),
    'report is not from the settings-requests runner',
  )
  const runTimestamp = Number(report.runId?.slice('settings-requests-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'settings-requests runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'settings-requests report is stale or has invalid run timestamps',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-REQUESTS'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the requests route does not carry exactly the five required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-REQUESTS']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-REQUESTS:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-REQUESTS' &&
      routeRender.routeTab === 'requests' &&
      routeRender.cardCount === 3,
    'the requests route identity or its three cards are unverified',
  )
  assert(
    Array.isArray(routeRender.readPaths) &&
      routeRender.readPaths.length === 4 &&
      new Set(routeRender.readPaths).size === 4,
    'the mount did not read exactly the four requests-scoped settings once each',
  )
  const loaded = routeRender.loaded ?? {}
  assert(
    loaded.rawDays === '7' &&
      loaded.approvalDays === '30' &&
      loaded.flushWindow === '40' &&
      loaded.maxBytes === '65536',
    'the loaded settings values are unverified',
  )
  assert(
    checks.loadedOutOfRangePreserved === true,
    'the out-of-range loaded buffer size was not preserved',
  )

  const update = checks.mutationUpdate ?? {}
  assert(
    update.writes === 5 &&
      update.loggingBody?.mode === 'off' &&
      update.rawBody?.raw_retention_days === 10 &&
      update.whitelistBody?.enabled === true &&
      update.batchingBody?.flush_window_ms === 500 &&
      update.batchingBody?.max_buffer_chars === 4096 &&
      update.batchingBody?.max_buffer_bytes === 8192 &&
      update.batchingBody?.flush_on_sentence_end === false,
    'the control roundtrips did not autosave their real bodies',
  )
  assert(
    update.retentionBody?.approval_retention_days === 45 &&
      update.retentionBody?.content_retention_days === 30 &&
      update.retentionBody?.metadata_retention_days === 90 &&
      update.retentionBody?.raw_backend === 'local' &&
      update.retentionBody?.replay_enabled === true &&
      update.retentionBody?.raw_retention_days === 10,
    'the retention write did not carry the fields this tab hides',
  )
  assert(
    update.storedRetention?.approval_retention_days === 45 &&
      update.storedRetention?.content_retention_days === 30 &&
      update.storedRetention?.metadata_retention_days === 90 &&
      update.storedRetention?.raw_backend === 'local' &&
      update.storedRetention?.replay_enabled === true &&
      update.storedLogging?.mode === 'off' &&
      update.storedLogging?.raw_retention_days === 10,
    'the stored records lost a hidden field or the shared raw days',
  )
  const reload = checks.reload ?? {}
  assert(
    reload.reads === 4 &&
      reload.writes === 0 &&
      reload.raw === '10' &&
      reload.approval === '45' &&
      reload.whitelist === 'checked' &&
      reload.flush === '500' &&
      reload.sentenceEnd === 'unchecked',
    'the saved settings did not survive a reload without a write',
  )

  const guard = checks.conflictGuard ?? {}
  assert(
    guard.claimantSpinners === 1 &&
      guard.bothDisabled === true &&
      guard.heldWritesBeforeRelease === 0,
    'the shared retention key did not disable both forms with one claimant spinner',
  )
  assert(
    guard.independentKeyWrites === 1,
    'an independent settings key was blocked by the shared retention write',
  )
  assert(
    guard.loggingWrites === 1 &&
      guard.retentionWrites === 1 &&
      guard.retentionBody?.approval_retention_days === 50 &&
      guard.retentionBody?.raw_retention_days === 10,
    'the held retention draft was not submitted once on release, or lost the shared raw days',
  )

  const failure = checks.failureDraft ?? {}
  assert(
    failure.failedWrites === 1 &&
      failure.draftKept === '640' &&
      failure.retryWrites === 2 &&
      failure.retryBody?.flush_window_ms === 640,
    'a failed write looped, dropped the draft, or did not retry it',
  )
  const newer = checks.newerDraft ?? {}
  assert(
    newer.writes === 2 && newer.final === '720',
    'an edit made during a write was overwritten by the confirmed response',
  )

  const refresh = checks.scopedRefresh ?? {}
  assert(
    refresh.reads === 4 &&
      refresh.refreshWrites === 0 &&
      refresh.draftWrites === 1 &&
      refresh.disabledWhileDirty === true &&
      refresh.enabledAfterSave === true,
    'the scoped refresh did not read only the four settings or wrote a setting',
  )
  const stale = checks.staleRead ?? {}
  assert(
    stale.keptDraft === '55' && stale.storedApproval === 55,
    'a read that started before an edit reverted the newer draft',
  )

  const noRefetch = checks.localeThemeNoRefetch ?? {}
  assert(
    noRefetch.reads === 0 && noRefetch.writes === 0,
    'a locale/theme rerender refetched or wrote the requests settings',
  )

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the requests route',
  )
  assert(
    denial.forgedPatch === 403 &&
      denial.forgedRead === 403 &&
      denial.anonymousRead === 401,
    'the fixture did not answer forged or anonymous settings requests with 403/401',
  )
  assert(
    denial.generalIntact === true && denial.redactionIntact === 200,
    'the ordinary-user general or redaction behaviour regressed',
  )

  const responsive = checks.responsive as
    | Array<{ width?: number; documentWidth?: number; controls?: number }>
    | undefined
  assert(
    responsive?.length === 4,
    'the responsive profile does not cover four widths',
  )
  for (const width of [1440, 1024, 768, 390]) {
    const entry = responsive?.find((item) => item.width === width)
    assert(
      (entry?.documentWidth ?? width + 1) <= width && entry?.controls === 10,
      `the requests tab overflows or hides a control at ${width}px`,
    )
  }

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the requests tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  const allowedFailures = (report.expectedApiFailures ?? []).every(
    (entry) =>
      entry === '401 GET /api/v1/auth/me' ||
      /^403 (GET|PATCH) \/api\/v1\/settings\//.test(entry) ||
      entry === '401 GET /api/v1/settings/usage-retention' ||
      entry === '503 PATCH /api/v1/settings/stream-delta-batching',
  )
  assert(
    allowedFailures && (report.expectedApiFailures ?? []).length > 0,
    'the expected API failures were not recorded as the only failures',
  )
  assert(report.consoleErrorCount === 0, 'browser console errors were reported')
  assert(report.pageErrorCount === 0, 'browser page errors were reported')

  const screenshots = report.screenshots ?? []
  for (const screenshot of screenshots) {
    assert(
      existsSync(join(root, screenshot.replace(/^e2e\/foundation\//, ''))),
      `missing screenshot: ${screenshot}`,
    )
  }
  for (const screenshot of REQUIRED_SCREENSHOTS) {
    assert(
      screenshots.includes(screenshot),
      `the required requests screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('Settings requests focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Settings requests focused browser verification passed.')
