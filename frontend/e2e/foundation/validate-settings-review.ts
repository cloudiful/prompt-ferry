import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type SettingsReviewReport = {
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
const reportPath = join(root, 'settings-review-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const playwrightPath = join(root, 'settings-review.playwright')
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
  'e2e/foundation/screenshots/settings-review-en-light-1440.png',
  'e2e/foundation/screenshots/settings-review-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-review-en-light-390.png',
  'e2e/foundation/screenshots/settings-review-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-review-failure-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused settings-review report is missing; run run-settings-review.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-REVIEW')

assert(
  row?.foundationTest === 'e2e/foundation/settings-review.playwright',
  'R-SETTINGS-REVIEW is not mapped to the focused settings-review browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-REVIEW no longer requires the four verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])]) ===
    JSON.stringify(['GET /api/v1/settings/llm-review']),
  'the review row does not read exactly the LLM review setting',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])]) ===
    JSON.stringify(['setting:llmReview']),
  'the review row does not declare the real review key',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the review row lost its settings/write gate identity',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the review row does not cite the current-run screenshots',
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
      !pendingIds.includes('R-SETTINGS-REVIEW'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(playwrightPath)) {
  const source = readFileSync(playwrightPath, 'utf8')
  // Secrets must never be written to the report, only as booleans.
  assert(
    !/body:\s*request\.postData\(\)/.test(source),
    'the settings request ledger records the raw request body',
  )
  assert(
    source.includes('review_api_key_present') &&
      source.includes('bearer_token_present'),
    'the ledger does not reduce the two secrets to presence booleans',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as SettingsReviewReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}
  const serialized = JSON.stringify(report)

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('settings-review-'),
    'report is not from the settings-review runner',
  )
  const runTimestamp = Number(report.runId?.slice('settings-review-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'settings-review runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'settings-review report is stale or has invalid run timestamps',
  )

  assert(
    !serialized.includes('synthetic-browser-key') &&
      !serialized.includes('synthetic-browser-token'),
    'the report contains a synthetic secret value',
  )

  assert(
    JSON.stringify(Object.keys(scenarios['R-SETTINGS-REVIEW'] ?? {}).sort()) ===
      JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the review route does not carry exactly the four required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-REVIEW']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-REVIEW:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  const controls = routeRender.controls ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-REVIEW' &&
      routeRender.routeTab === 'review',
    'the review route identity is unverified',
  )
  assert(
    Array.isArray(controls.missing) && controls.missing.length === 0,
    'a required review control is missing',
  )
  assert(
    Array.isArray(controls.groups) && controls.groups.length === 3,
    'the three collapsible groups are not rendered',
  )
  assert(
    controls.apiKeyType === 'password' && controls.bearerType === 'password',
    'a secret field is not a password input',
  )
  assert(
    controls.enabled === 'checked' &&
      String(controls.failurePolicy).includes('Block on failure') &&
      controls.timeout === '8000' &&
      controls.ttl === '120',
    'the loaded review values are wrong',
  )
  assert(
    controls.baseUrl === 'https://review.example.internal' &&
      controls.model === 'review-model' &&
      controls.headers === 'X-Fixture-Header: enabled',
    'the loaded review fields do not match the stored settings',
  )
  assert(
    controls.hintVisible === true,
    'the PostgreSQL storage-mode hint is not visible',
  )
  assert(checks.noAutosave?.writes === 0, 'editing a field autosaved')

  const update = checks.mutationUpdate ?? {}
  assert(
    update.writes === 1,
    `the save wrote ${update.writes} times instead of once`,
  )
  assert(
    JSON.stringify(update.body?.keys) ===
      JSON.stringify([
        'custom_policy_text',
        'enabled',
        'failure_policy',
        'pending_ttl_seconds',
        'review_api_key',
        'review_base_url',
        'review_model',
        'review_timeout_ms',
        'webhook',
      ]) &&
      JSON.stringify(update.body?.webhookKeys) ===
        JSON.stringify(['bearer_token', 'enabled', 'extra_headers', 'url']),
    'the save body does not carry every field and the whole webhook',
  )
  assert(
    update.body?.review_base_url === 'https://review2.example.internal' &&
      update.body?.review_model === 'review-model-2' &&
      update.body?.review_timeout_ms === 4500 &&
      update.body?.pending_ttl_seconds === 60 &&
      update.body?.failure_policy === 'fail_open' &&
      update.body?.custom_policy_text === 'Updated fixture policy',
    'the saved values are wrong',
  )
  assert(
    JSON.stringify(update.body?.extra_headers) ===
      JSON.stringify({ 'X-One': '1', 'X-Url': 'https://a.example/x:y' }),
    'the extra-header roundtrip lost a value or a colon',
  )
  assert(
    update.stored?.review_base_url === 'https://review2.example.internal' &&
      update.stored?.failure_policy === 'fail_open' &&
      JSON.stringify(update.stored?.webhook?.extra_headers) ===
        JSON.stringify({ 'X-One': '1', 'X-Url': 'https://a.example/x:y' }),
    'the stored review settings do not match the save',
  )

  const secrets = checks.secretLifecycle ?? {}
  assert(
    secrets.set === true &&
      secrets.loadedSecrets?.apiKeyPresent === true &&
      secrets.loadedSecrets?.bearerPresent === true,
    'a set secret did not read back into its password input',
  )
  assert(
    secrets.loadedSecrets?.bodyLeak === false &&
      secrets.loadedSecrets?.titleLeak === false &&
      secrets.loadedSecrets?.storageLeak === false,
    'a secret reached text, a title or storage',
  )
  assert(
    secrets.untouchedPresent === true,
    'an untouched save did not send the stored secret back',
  )
  assert(
    secrets.clearedPresent === false &&
      secrets.afterClear?.review_api_key_present === false &&
      secrets.afterClear?.webhook?.bearer_token_present === true,
    'an emptied field did not clear exactly that secret',
  )

  assert(
    checks.localHeaderError?.writes === 0,
    'a malformed header line still reached the server',
  )
  const validation = checks.serverValidation ?? {}
  assert(
    validation.writes === 2 &&
      (validation.alerts ?? []).some((text: string) =>
        text.includes('review_base_url must not include /v1'),
      ) &&
      validation.corrected === 'https://review3.example.internal',
    'the server validation error was not shown once and retried after correction',
  )
  const cases = checks.serverValidationCases ?? {}
  for (const expected of [
    'review_base_url is required when LLM review is enabled',
    'review_base_url must not include /v1',
    'review_model is required when LLM review is enabled',
    'review_timeout_ms must be at least 100',
    'pending_ttl_seconds must be at least 1',
    'webhook.url is required when webhook is enabled',
    'webhook.extra_headers must not contain empty header names',
    'webhook.extra_headers values must be single-line',
  ]) {
    assert(
      Object.values(cases ?? {}).some(
        (entry) =>
          typeof entry === 'object' &&
          entry !== null &&
          (entry as { status?: number }).status === 400 &&
          (entry as { payload?: { error?: { message?: string } } }).payload
            ?.error?.message === expected,
      ),
      `the fixture does not reproduce the server validation "${expected}"`,
    )
  }
  assert(
    cases.disabledInvalid?.status === 200 && cases.restore?.status === 200,
    'a disabled review did not bypass validation, or the restore patch failed',
  )

  assert(checks.doubleSave?.writes === 1, 'a double save sent two requests')
  assert(
    checks.newerDraft?.kept === 'newer policy',
    'a draft typed during the save was overwritten',
  )
  const failure = checks.failureDraft ?? {}
  assert(
    failure.writes === 2 && failure.stored === 'transport-failure policy',
    'a 503 did not report once, keep the draft and retry through Save',
  )
  assert(
    checks.scopedRefresh?.reads === 1 && checks.scopedRefresh?.writes === 0,
    'the scoped refresh did not read only the review setting or wrote it',
  )
  assert(
    checks.localeThemeNoRefetch?.reads === 0 &&
      checks.localeThemeNoRefetch?.writes === 0,
    'a locale/theme rerender refetched or saved the review settings',
  )
  assert(
    checks.collapsible?.afterOpen === true &&
      checks.collapsible?.afterSecond === true &&
      checks.collapsible?.afterClose === false,
    'the collapsible groups are not independently operable',
  )

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the review route',
  )
  assert(
    denial.forgedPatch === 403 &&
      denial.forgedRead === 403 &&
      denial.anonymousRead === 401,
    'the fixture did not answer forged or anonymous review requests with 403/401',
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
      `the review tab overflows horizontally at ${width}px`,
    )
  }

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the review tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  const allowedFailures = (report.expectedApiFailures ?? []).every(
    (entry) =>
      entry === '401 GET /api/v1/auth/me' ||
      /^(400|401|403|503) (GET|PATCH) \/api\/v1\/settings\/llm-review$/.test(
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
      `the required review screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('Settings review focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Settings review focused browser verification passed.')
