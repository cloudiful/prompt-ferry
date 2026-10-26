import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type GeneralSettingsReport = {
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
const reportPath = join(root, 'general-settings-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

const REQUIRED_SCENARIOS = [
  'route-render',
  'nav-visibility',
  'locale-switch',
  'theme-switch',
]

const REMAINING_SETTINGS_PLACEHOLDERS: string[] = []

const REQUIRED_SCREENSHOTS = [
  'e2e/foundation/screenshots/settings-general-en-light-1440.png',
  'e2e/foundation/screenshots/settings-general-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-general-en-light-390.png',
  'e2e/foundation/screenshots/settings-general-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-general-admin-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused general-settings report is missing; run run-general-settings.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-GENERAL')

assert(
  row?.foundationTest === 'e2e/foundation/general-settings.playwright',
  'R-SETTINGS-GENERAL is not mapped to the focused general-settings browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-GENERAL no longer requires the four verified scenarios',
)
assert(
  row?.scope === 'authenticated' &&
    (row?.reads.length ?? -1) === 0 &&
    (row?.mutationKeys.length ?? -1) === 0,
  'the general settings row must stay authenticated with no read and no mutation key',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the general settings row does not cite the current-run screenshots',
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
      !pendingIds.includes('R-SETTINGS-GENERAL'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as GeneralSettingsReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('general-settings-'),
    'report is not from the general-settings runner',
  )
  const runTimestamp = Number(report.runId?.slice('general-settings-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'general-settings runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'general-settings report is stale or has invalid run timestamps',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-GENERAL'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the general settings route does not carry exactly the four required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-GENERAL']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-GENERAL:${scenario} has no passing current-run assertion`,
    )
  }

  assert(
    checks.zeroRequests?.settings === 0 &&
      checks.settingsApiRequests === 0 &&
      Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the general tab issued a settings or business server request',
  )
  assert(
    checks.admin?.cards === 3 && checks.admin?.adminNavVisible === true,
    'the admin does not get the same general preference cards',
  )
  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )

  const defaultZone = checks.timeZoneDefault ?? {}
  assert(
    typeof defaultZone.label === 'string' &&
      typeof defaultZone.browserZone === 'string' &&
      defaultZone.label.includes(defaultZone.browserZone),
    'the automatic zone label does not name the browser zone',
  )
  assert(
    checks.timeZoneSelect?.persisted === 'Asia/Tokyo' &&
      checks.timeZoneSelect?.filtered >= 2,
    'the explicit zone was not filtered, selected, and persisted',
  )
  const crossPage = checks.crossPage ?? {}
  assert(
    typeof crossPage.rendered === 'string' &&
      crossPage.rendered === crossPage.expectedTokyo &&
      crossPage.rendered !== crossPage.utc &&
      crossPage.zoneKept === true,
    'the records time cell does not render the selected zone, or the choice was lost',
  )
  assert(
    checks.timeZoneClear?.clearedToAuto === true &&
      checks.timeZoneClear?.reselected === 'Asia/Tokyo',
    'clearing to the automatic zone or reselecting an explicit zone is unverified',
  )

  const responsive = checks.responsive as
    | Array<{ width?: number; twoColumns?: boolean; documentWidth?: number }>
    | undefined
  assert(
    responsive?.length === 4,
    'the responsive profile does not cover four widths',
  )
  for (const width of [1440, 1024, 768, 390]) {
    const entry = responsive?.find((item) => item.width === width)
    assert(
      (entry?.documentWidth ?? width + 1) <= width,
      `the general tab overflows horizontally at ${width}px`,
    )
    assert(
      entry?.twoColumns === width >= 640,
      `the card grid column count is wrong at ${width}px`,
    )
  }

  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  assert(
    Array.isArray(report.expectedApiFailures) &&
      report.expectedApiFailures.length > 0 &&
      report.expectedApiFailures.every(
        (entry) => entry === '401 GET /api/v1/auth/me',
      ),
    'the anonymous session probes were not recorded as the only expected failures',
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
      `the required general settings screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('General settings focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('General settings focused browser verification passed.')
