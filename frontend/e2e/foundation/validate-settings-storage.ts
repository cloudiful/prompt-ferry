import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type SettingsStorageReport = {
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
const reportPath = join(root, 'settings-storage-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const playwrightPath = join(root, 'settings-storage.playwright')
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
  'e2e/foundation/screenshots/settings-storage-en-light-1440.png',
  'e2e/foundation/screenshots/settings-storage-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-storage-en-light-390.png',
  'e2e/foundation/screenshots/settings-storage-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-storage-failure-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused settings-storage report is missing; run run-settings-storage.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-STORAGE')

assert(
  row?.foundationTest === 'e2e/foundation/settings-storage.playwright',
  'R-SETTINGS-STORAGE is not mapped to the focused settings-storage browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-STORAGE no longer requires the four verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])]) ===
    JSON.stringify(['GET /api/v1/settings/raw-object-store']),
  'the storage row does not read exactly the raw object store setting',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])]) ===
    JSON.stringify(['setting:storage']),
  'the storage row does not declare the real storage key',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the storage row lost its settings/write gate identity',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the storage row does not cite the current-run screenshots',
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
      !pendingIds.includes('R-SETTINGS-STORAGE'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(playwrightPath)) {
  const source = readFileSync(playwrightPath, 'utf8')
  // Secrets must never be written to the report, only as presences and modes.
  assert(
    !/body:\s*request\.postData\(\)/.test(source),
    'the settings request ledger records the raw request body',
  )
  assert(
    source.includes('access_value_present') &&
      source.includes('secret_value_present') &&
      source.includes('access_mode') &&
      source.includes('secret_mode'),
    'the ledger does not reduce the two secrets to presence booleans and modes',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as SettingsStorageReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}
  const serialized = JSON.stringify(report)

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('settings-storage-'),
    'report is not from the settings-storage runner',
  )
  const runTimestamp = Number(report.runId?.slice('settings-storage-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'settings-storage runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'settings-storage report is stale or has invalid run timestamps',
  )

  assert(
    !serialized.includes('synthetic-browser-access') &&
      !serialized.includes('synthetic-browser-secret') &&
      !serialized.includes('synthetic-probe-access') &&
      !serialized.includes('synthetic-probe-secret'),
    'the report contains a synthetic secret value',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-STORAGE'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the storage route does not carry exactly the four required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-STORAGE']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-STORAGE:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  const controls = routeRender.controls ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-STORAGE' &&
      routeRender.routeTab === 'storage',
    'the storage route identity is unverified',
  )
  assert(
    controls.backend === 'Local file' &&
      controls.localDir === '/var/lib/prompt-ferry/raw' &&
      controls.saveDisabled === false &&
      controls.refreshDisabled === false,
    'the loaded local branch is wrong',
  )
  assert(
    controls.cardTitle === 'Raw payload store' &&
      controls.localHint === true &&
      controls.s3Fields === false &&
      controls.unavailable === false,
    'the storage card content is wrong',
  )

  const branches = checks.backendBranches ?? {}
  const s3 = branches.s3 ?? {}
  assert(
    Array.isArray(s3.missing) && s3.missing.length === 0,
    'a required S3 control is missing',
  )
  assert(
    s3.endpoint === 'https://s3.example.internal' &&
      s3.bucket === 'prompt-ferry-raw' &&
      s3.region === 'eu-central-1' &&
      s3.prefix === 'fixture/',
    'the stored S3 values are wrong',
  )
  assert(
    s3.allowHttp === 'unchecked' && s3.pathStyle === 'unchecked',
    'the S3 switches are wrong',
  )
  assert(
    s3.accessType === 'password' &&
      s3.secretType === 'password' &&
      s3.accessValue === '' &&
      s3.secretValue === '',
    'a stored credential is visible or is not a password input',
  )
  assert(
    JSON.stringify(s3.present) === JSON.stringify(['true', 'true']) &&
      s3.hint === true &&
      s3.localDir === false,
    'the presence badges, the keep hint or the branch switch is wrong',
  )
  assert(
    branches.disabled?.hint === true &&
      branches.disabled?.bucket === false &&
      branches.disabled?.localDir === false,
    'the disabled branch is wrong',
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
        'backend',
        'local_dir',
        's3_access_key',
        's3_allow_http',
        's3_bucket',
        's3_endpoint',
        's3_path_style',
        's3_prefix',
        's3_region',
        's3_secret_key',
      ]),
    'the save body does not carry every field',
  )
  assert(
    update.body?.backend === 'local' &&
      update.body?.local_dir === '/srv/fixture-raw' &&
      update.body?.s3_bucket === 'prompt-ferry-raw' &&
      update.body?.s3_region === 'eu-central-1',
    'the saved values are wrong',
  )
  assert(
    update.body?.access_mode === 'keep' &&
      update.body?.secret_mode === 'keep' &&
      update.body?.access_value_present === false &&
      update.body?.secret_value_present === false,
    'an untouched save did not keep both stored secrets',
  )
  assert(
    update.stored?.backend === 'local' &&
      update.stored?.local_dir === '/srv/fixture-raw' &&
      update.stored?.s3_prefix === 'fixture' &&
      update.stored?.has_s3_access_key === true &&
      update.stored?.has_s3_secret_key === true,
    'the stored storage settings do not match the save',
  )

  const replace = checks.secretReplace ?? {}
  assert(
    replace.body?.access_mode === 'replace' &&
      replace.body?.secret_mode === 'replace' &&
      replace.body?.access_value_present === true &&
      replace.body?.secret_value_present === true,
    'a typed secret was not sent as a replace patch',
  )
  const visibility = replace.secretVisibility ?? {}
  assert(
    visibility.accessValue === '' && visibility.secretValue === '',
    'a consumed secret draft stayed in its input',
  )
  assert(
    JSON.stringify(visibility.present) === JSON.stringify(['true', 'true']),
    'the presence badges after a replace are wrong',
  )
  assert(
    visibility.bodyLeak === false &&
      visibility.titleLeak === false &&
      visibility.storageLeak === false,
    'a secret reached text, a title or storage',
  )
  assert(
    visibility.clearControl === false,
    'the storage tab exposes a clear-secret control',
  )

  const keep = checks.secretKeep ?? {}
  assert(
    keep.body?.access_mode === 'keep' &&
      keep.body?.secret_mode === 'keep' &&
      keep.stored?.has_s3_access_key === true &&
      keep.stored?.has_s3_secret_key === true,
    'a blank draft of a set secret did not keep it',
  )

  const probes = checks.fixtureProbes ?? {}
  assert(
    probes.bucketMissing?.status === 400 &&
      probes.bucketMissing?.payload?.error?.message ===
        's3 bucket is required for s3 backend',
    'the fixture does not reproduce the missing-bucket validation',
  )
  assert(
    probes.emptySecret?.status === 400 &&
      probes.emptySecret?.payload?.error?.message ===
        'secret value cannot be empty',
    'the fixture does not reproduce the empty-secret validation',
  )
  assert(
    probes.normalized?.status === 200 &&
      probes.normalized?.payload?.s3_bucket === 'probe-bucket' &&
      probes.normalized?.payload?.s3_prefix === 'probe/prefix' &&
      probes.normalized?.payload?.s3_region === 'auto' &&
      probes.normalized?.payload?.s3_path_style === true,
    'the fixture does not normalize the bucket, prefix, region or path style',
  )
  assert(
    probes.clearedAccess?.payload?.has_s3_access_key === false &&
      probes.clearedAccess?.payload?.has_s3_secret_key === true,
    'an API clear of one secret affected the other',
  )
  assert(
    probes.clearedSecretToo?.payload?.has_s3_access_key === false &&
      probes.clearedSecretToo?.payload?.has_s3_secret_key === false,
    'the API clear did not remove both presence flags',
  )
  assert(
    probes.replacedAccess?.status === 200 &&
      probes.replacedAccess?.payload?.has_s3_access_key === true &&
      probes.replacedAccess?.payload?.has_s3_secret_key === true,
    'the API replace did not restore presence',
  )
  assert(
    JSON.stringify(probes.replacedAccess ?? {}).includes('synthetic-probe') ===
      false,
    'the read response echoed a stored secret',
  )
  assert(
    probes.keptAccess?.payload?.has_s3_access_key === true &&
      probes.keptAccess?.payload?.has_s3_secret_key === true,
    'a keep patch did not retain the stored secrets',
  )
  assert(
    probes.disabled?.status === 200 &&
      probes.disabled?.payload?.backend === 'disabled' &&
      probes.disabled?.payload?.s3_bucket === '',
    'a disabled store did not bypass the bucket requirement',
  )

  const capability = checks.capabilityUnavailable ?? {}
  assert(
    capability.warning === true &&
      capability.detail === true &&
      capability.retry === true &&
      capability.form === false &&
      capability.saveDisabled === true &&
      capability.recovered === true,
    'the 501 capability answer is not an explicit warning with a working retry',
  )

  assert(checks.doubleSave?.writes === 1, 'a double save sent two requests')
  assert(
    checks.newerDraft?.kept === '/srv/newer',
    'a draft typed during the save was overwritten',
  )
  const failure = checks.failureDraft ?? {}
  assert(
    failure.writes === 2 && failure.stored === '/srv/transport-failure',
    'a 503 did not report once, keep the draft and retry through Save',
  )
  assert(
    checks.scopedRefresh?.reads === 1 && checks.scopedRefresh?.writes === 0,
    'the scoped refresh did not read only the storage setting or wrote it',
  )
  assert(
    checks.localeThemeNoRefetch?.reads === 0 &&
      checks.localeThemeNoRefetch?.writes === 0,
    'a locale/theme rerender refetched or saved the storage settings',
  )

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the storage route',
  )
  assert(
    denial.forgedPatch === 403 &&
      denial.forgedRead === 403 &&
      denial.anonymousRead === 401,
    'the fixture did not answer forged or anonymous storage requests with 403/401',
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
      `the storage tab overflows horizontally at ${width}px`,
    )
  }

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the storage tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  const allowedFailures = (report.expectedApiFailures ?? []).every(
    (entry) =>
      entry === '401 GET /api/v1/auth/me' ||
      /^(400|401|403|501|503) (GET|PATCH) \/api\/v1\/settings\/raw-object-store$/.test(
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
      `the required storage screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('Settings storage focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Settings storage focused browser verification passed.')
