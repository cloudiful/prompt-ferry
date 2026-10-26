import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type SettingsCacheAlertReport = {
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
const reportPath = join(root, 'settings-cache-alert-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const playwrightPath = join(root, 'settings-cache-alert.playwright')
const reactTestPath = join(
  root,
  '../src/react-tests/settings-cache-alert.test.tsx',
)
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

const REQUIRED_SCENARIOS = [
  'route-render',
  'admin-denial',
  'mutation-update',
  'mutation-failure-draft',
]

const REMAINING_SETTINGS_PLACEHOLDERS: string[] = []

const REQUIRED_SCREENSHOTS = [
  'e2e/foundation/screenshots/settings-cache-alert-en-light-1440.png',
  'e2e/foundation/screenshots/settings-cache-alert-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-cache-alert-en-light-390.png',
  'e2e/foundation/screenshots/settings-cache-alert-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-cache-alert-failure-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused settings-cache-alert report is missing; run run-settings-cache-alert.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-CACHE-ALERT')

assert(
  row?.foundationTest === 'e2e/foundation/settings-cache-alert.playwright',
  'R-SETTINGS-CACHE-ALERT is not mapped to the focused settings-cache-alert browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-CACHE-ALERT no longer requires the four verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])]) ===
    JSON.stringify(['GET /api/v1/settings/cache-alert']),
  'the cache alert row does not read exactly the cache alert setting',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])]) ===
    JSON.stringify(['setting:cacheAlert']),
  'the cache alert row does not declare the real setting:cacheAlert key',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the cache alert row lost its settings/write gate identity',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the cache alert row does not cite the current-run screenshots',
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
      !pendingIds.includes('R-SETTINGS-CACHE-ALERT'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(playwrightPath)) {
  const source = readFileSync(playwrightPath, 'utf8')
  // The secret must never be written to the report, only its presence.
  assert(
    !/body:\s*request\.postData\(\)/.test(source),
    'the settings request ledger records the raw request body',
  )
  assert(
    source.includes('dingtalk_secret_present') &&
      !source.includes('dingtalk_secret_value'),
    'the ledger does not reduce the secret to a presence boolean',
  )
}

if (existsSync(reactTestPath)) {
  const test = readFileSync(reactTestPath, 'utf8')
  // The mapper is only covered by a legacy bun:test module, so the active run
  // must pin the defaults, the presence rule and the derived-flag rule itself.
  assert(
    test.includes('CACHE_ALERT_DEFAULTS') &&
      test.includes('cacheAlertToForm') &&
      test.includes('has_dingtalk_secret: false'),
    'the active test does not cover the mapper defaults and the server presence flag',
  )
  assert(
    test.includes('parseCacheAlertNumber') && test.includes("'0.85'"),
    'the active test does not cover the float threshold parser',
  )
  assert(
    test.includes('data-secret-present') && test.includes('data-secret-mode'),
    'the active test does not cover both badges',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as SettingsCacheAlertReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}
  const serialized = JSON.stringify(report)

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('settings-cache-alert-'),
    'report is not from the settings-cache-alert runner',
  )
  const runTimestamp = Number(
    report.runId?.slice('settings-cache-alert-'.length),
  )
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'settings-cache-alert runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'settings-cache-alert report is stale or has invalid run timestamps',
  )

  assert(
    !serialized.includes('synthetic-browser-secret') &&
      !serialized.includes('synthetic-probe-secret') &&
      !serialized.includes('fixture-synthetic-dingtalk'),
    'the report contains a synthetic secret value',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-CACHE-ALERT'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the cache alert route does not carry exactly the four required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-CACHE-ALERT']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-CACHE-ALERT:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  const controls = routeRender.controls ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-CACHE-ALERT' &&
      routeRender.routeTab === 'cache-alert',
    'the cache alert route identity is unverified',
  )
  assert(
    Array.isArray(controls.missing) && controls.missing.length === 0,
    'a required cache alert control is missing',
  )
  assert(
    controls.enabled === 'checked' &&
      controls.window === '30' &&
      controls.minTurns === '8' &&
      controls.threshold === '0.85' &&
      controls.cooldown === '15',
    'the loaded cache alert values are wrong',
  )
  assert(
    controls.webhook ===
      'https://oapi.dingtalk.com/robot/send?access_token=fixture',
    'the loaded webhook URL is wrong',
  )
  assert(
    controls.secretType === 'password' && controls.secretValue === '',
    'the secret field is not an empty password input',
  )
  assert(
    JSON.stringify(controls.windowBounds) ===
      JSON.stringify({ min: '5', max: '1440' }) &&
      JSON.stringify(controls.thresholdBounds) ===
        JSON.stringify({ min: '0', max: '1', step: '0.05' }),
    'the numeric bounds or the threshold step are wrong',
  )
  assert(
    controls.presence === 'true' && controls.mode === 'keep',
    'the presence or the keep/replace badge is wrong',
  )
  assert(
    controls.hint === true,
    'the PostgreSQL storage-mode hint is not shown',
  )
  assert(controls.bodyLeak === false, 'a stored secret reached the document')
  assert(controls.secretControls === 0, 'the tab exposes a clear-secret action')
  assert(checks.noAutosave?.writes === 0, 'editing a field autosaved')

  const badges = checks.secretBadges ?? {}
  assert(
    badges.before?.presence === 'true' &&
      badges.before?.mode === 'keep' &&
      badges.typed?.presence === 'true' &&
      badges.typed?.mode === 'replace' &&
      badges.blank?.presence === 'true' &&
      badges.blank?.mode === 'keep',
    'typing a secret did not move only the local mode badge',
  )

  const update = checks.mutationUpdate ?? {}
  assert(
    update.writes === 1,
    `the save wrote ${update.writes} times instead of once`,
  )
  assert(
    JSON.stringify(update.body?.keys) ===
      JSON.stringify([
        'cooldown_minutes',
        'dingtalk_secret',
        'dingtalk_webhook_url',
        'enabled',
        'min_turns',
        'threshold',
        'window_minutes',
      ]),
    'the save body does not carry every field',
  )
  assert(
    update.body?.enabled === true &&
      update.body?.window_minutes === 45 &&
      update.body?.min_turns === 10 &&
      update.body?.threshold === 0.35 &&
      update.body?.cooldown_minutes === 30 &&
      update.body?.dingtalk_webhook_url ===
        'https://oapi.dingtalk.com/robot/send?access_token=updated',
    'the saved values are wrong',
  )
  assert(
    update.body?.dingtalk_secret_present === false,
    'an untouched save did not keep the stored secret',
  )
  assert(
    update.stored?.window_minutes === 45 &&
      update.stored?.min_turns === 10 &&
      update.stored?.threshold === 0.35 &&
      update.stored?.cooldown_minutes === 30 &&
      update.stored?.dingtalk_secret_blank === true &&
      update.stored?.has_dingtalk_secret === true,
    'the stored cache alert policy does not match the save',
  )

  const replace = checks.secretReplace ?? {}
  assert(
    replace.body?.dingtalk_secret_present === true,
    'a typed secret was not sent',
  )
  assert(
    replace.afterReplace?.secretValue === '' &&
      replace.afterReplace?.mode === 'keep',
    'the consumed secret draft did not reset to keep',
  )
  assert(
    replace.afterReplace?.presence === 'true',
    'the presence badge stopped following the server response',
  )
  assert(
    replace.afterReplace?.bodyLeak === false &&
      replace.afterReplace?.titleLeak === false &&
      replace.afterReplace?.storageLeak === false,
    'a secret reached text, a title or storage',
  )
  assert(
    replace.stored?.dingtalk_secret_blank === true &&
      replace.stored?.has_dingtalk_secret === true,
    'the stored response is not redacted or lost presence',
  )

  const keep = checks.secretKeep ?? {}
  assert(
    keep.body?.dingtalk_secret_present === false &&
      keep.stored?.has_dingtalk_secret === true,
    'a blank draft of a set secret did not keep it',
  )

  const probes = checks.fixtureValidation ?? {}
  for (const [name, expected] of [
    ['windowLow', 'window_minutes must be between 5 and 1440'],
    ['windowHigh', 'window_minutes must be between 5 and 1440'],
    ['minTurnsLow', 'min_turns must be between 2 and 100'],
    ['minTurnsHigh', 'min_turns must be between 2 and 100'],
    ['thresholdLow', 'threshold must be between 0 and 1'],
    ['thresholdHigh', 'threshold must be between 0 and 1'],
    ['cooldownLow', 'cooldown_minutes must be between 5 and 1440'],
    ['cooldownHigh', 'cooldown_minutes must be between 5 and 1440'],
  ]) {
    assert(
      probes[name]?.status === 400 &&
        probes[name]?.payload?.error?.code === 'bad_request' &&
        probes[name]?.payload?.error?.message === expected,
      `the fixture does not reproduce the server validation "${expected}"`,
    )
  }
  assert(
    probes.thresholdZero?.status === 200 &&
      probes.thresholdZero?.payload?.threshold === 0 &&
      probes.thresholdOne?.status === 200 &&
      probes.thresholdOne?.payload?.threshold === 1,
    'the fixture does not accept 0 and 1 as real thresholds',
  )
  assert(
    probes.partialDefaults?.status === 200 &&
      probes.partialDefaults?.payload?.window_minutes === 30 &&
      probes.partialDefaults?.payload?.min_turns === 5 &&
      probes.partialDefaults?.payload?.threshold === 0.2 &&
      probes.partialDefaults?.payload?.cooldown_minutes === 60 &&
      probes.partialDefaults?.payload?.dingtalk_webhook_url === '',
    'a partial payload did not take the backend defaults instead of merging',
  )
  assert(
    probes.replaceSecret?.payload?.dingtalk_secret === '' &&
      probes.replaceSecret?.payload?.has_dingtalk_secret === true,
    'the replace probe does not report presence with a blanked secret',
  )
  assert(
    JSON.stringify(probes.replaceSecret ?? {}).includes('synthetic-probe') ===
      false,
    'the read response echoed a stored secret',
  )
  assert(
    probes.blankKeepsSecret?.payload?.has_dingtalk_secret === true,
    'a blank write did not keep the stored secret',
  )

  const capability = checks.capabilityUnavailable ?? {}
  assert(
    capability.warning === true &&
      capability.detail === true &&
      capability.retry === true &&
      capability.form === false &&
      capability.saveDisabled === true &&
      capability.pageRefreshDisabled === true &&
      capability.recovered === true,
    'the 501 capability answer is not an explicit warning with a working retry',
  )

  assert(checks.doubleSave?.writes === 1, 'a double save sent two requests')
  assert(
    checks.newerDraft?.kept === '0.2',
    'a value typed during the save was overwritten',
  )
  const failure = checks.failureDraft ?? {}
  assert(
    failure.writes === 2 && failure.stored === 15,
    'a 503 did not report once, keep the draft and retry through Save',
  )
  assert(
    checks.scopedRefresh?.reads === 1 && checks.scopedRefresh?.writes === 0,
    'the scoped refresh did not read only the cache alert setting or wrote it',
  )
  assert(
    checks.localeThemeNoRefetch?.reads === 0 &&
      checks.localeThemeNoRefetch?.writes === 0,
    'a locale/theme rerender refetched or saved the cache alert settings',
  )

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the cache alert route',
  )
  assert(
    denial.forgedPut === 403 &&
      denial.forgedRead === 403 &&
      denial.anonymousRead === 401,
    'the fixture did not answer forged or anonymous cache alert requests with 403/401',
  )
  assert(
    denial.generalIntact === true && denial.redactionIntact === 200,
    'the ordinary-user general or redaction behaviour regressed',
  )

  const responsive = checks.responsive as
    Array<{ width?: number; documentWidth?: number }> | undefined
  assert(
    responsive?.length === 4,
    'the responsive profile does not cover four widths',
  )
  for (const width of [1440, 1024, 768, 390]) {
    const entry = responsive?.find((item) => item.width === width)
    assert(
      (entry?.documentWidth ?? width + 1) <= width,
      `the cache alert tab overflows horizontally at ${width}px`,
    )
  }

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the cache alert tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  const allowedFailures = (report.expectedApiFailures ?? []).every(
    (entry) =>
      entry === '401 GET /api/v1/auth/me' ||
      /^(400|401|403|501|503) (GET|PUT) \/api\/v1\/settings\/cache-alert$/.test(
        entry,
      ),
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
      `the required cache alert screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('Settings cache alert focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Settings cache alert focused browser verification passed.')
