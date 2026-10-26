import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type SettingsNetworkReport = {
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
const reportPath = join(root, 'settings-network-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
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
  'e2e/foundation/screenshots/settings-network-en-light-1440.png',
  'e2e/foundation/screenshots/settings-network-zh-dark-1440.png',
  'e2e/foundation/screenshots/settings-network-en-light-390.png',
  'e2e/foundation/screenshots/settings-network-zh-dark-390.png',
  'e2e/foundation/screenshots/settings-network-validation-en-light-1440.png',
  'e2e/foundation/screenshots/settings-network-failure-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused settings-network report is missing; run run-settings-network.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-NETWORK')

assert(
  row?.foundationTest === 'e2e/foundation/settings-network.playwright',
  'R-SETTINGS-NETWORK is not mapped to the focused settings-network browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-NETWORK no longer requires the four verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])]) ===
    JSON.stringify(['GET /api/v1/settings/relay-ip-whitelist']),
  'the network row does not read exactly the relay IP policy',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])]) ===
    JSON.stringify(['setting:relayIpWhitelist']),
  'the network row does not declare the real relay policy key',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the network row lost its settings/write gate identity',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the network row does not cite the current-run screenshots',
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
      !pendingIds.includes('R-SETTINGS-NETWORK'),
    'the shared placeholder list does not hold exactly the zero remaining settings routes',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as SettingsNetworkReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('settings-network-'),
    'report is not from the settings-network runner',
  )
  const runTimestamp = Number(report.runId?.slice('settings-network-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'settings-network runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'settings-network report is stale or has invalid run timestamps',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-NETWORK'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the network route does not carry exactly the four required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-NETWORK']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-NETWORK:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  const fields = routeRender.fields ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-NETWORK' &&
      routeRender.routeTab === 'network' &&
      routeRender.readCount === 1,
    'the network route identity or its single read is unverified',
  )
  assert(
    fields.textareas === 2 &&
      fields.allowed?.name === 'settings-allowed-cidrs' &&
      fields.trusted?.name === 'settings-trusted-proxy-cidrs' &&
      fields.allowed?.rows === 5 &&
      fields.trusted?.rows === 5 &&
      fields.allowed?.monospace === true &&
      fields.trusted?.monospace === true &&
      fields.allowed?.resize.includes('vertical') === true &&
      fields.trusted?.resize.includes('vertical') === true,
    'the two labelled monospace textareas are not the contract the phase requires',
  )
  assert(
    fields.allowed?.value === '10.0.0.0/8\n192.168.0.0/16' &&
      fields.trusted?.value === '127.0.0.1/32',
    'the loaded policy was not rendered into the textareas',
  )
  assert(checks.noAutosave?.writes === 0, 'editing a list autosaved the policy')

  const update = checks.mutationUpdate ?? {}
  assert(
    update.saves === 4,
    `the control roundtrips wrote ${update.saves} times instead of four deliberate saves`,
  )
  assert(
    JSON.stringify(update.updateBody?.allowed_cidrs) ===
      JSON.stringify(['10.0.0.0/8', '10.0.0.0/8', '192.168.0.0/16']) &&
      JSON.stringify(update.updateBody?.trusted_proxy_cidrs) ===
        JSON.stringify([]),
    'the save did not submit both arrays with blank lines removed',
  )
  assert(
    update.normalized?.allowed === '10.0.0.0/8\n192.168.0.0/16' &&
      update.normalized?.trusted === '',
    'the normalized policy did not reach the form',
  )
  assert(
    JSON.stringify(update.stored) ===
      JSON.stringify({
        allowed_cidrs: ['10.0.0.0/8', '192.168.0.0/16'],
        trusted_proxy_cidrs: [],
      }),
    'the fixture did not store the normalized policy',
  )
  assert(
    JSON.stringify(update.blankBody) ===
      JSON.stringify({ allowed_cidrs: [], trusted_proxy_cidrs: [] }) &&
      JSON.stringify(update.storedBlank) ===
        JSON.stringify({ allowed_cidrs: [], trusted_proxy_cidrs: [] }),
    'empty lists were not sent and stored as explicit empty arrays',
  )
  assert(
    JSON.stringify(update.validBody?.allowed_cidrs) ===
      JSON.stringify(['2001:db8::/32', '::1', '10.0.0.1']) &&
      (update.storedValid?.allowed_cidrs ?? []).length === 3,
    'the IPv6/IPv4 mix did not roundtrip',
  )
  assert(
    checks.doubleSave?.writes === 1,
    'a double save sent more than one request',
  )
  assert(
    checks.newerDraft?.allowed === '10.4.0.0/16' &&
      checks.newerDraft?.trusted === '10.0.0.0/8' &&
      checks.newerDraft?.refreshDisabled === true,
    'a draft typed during the save was overwritten or the refresh was not blocked',
  )

  const validation = checks.validation400 ?? {}
  assert(
    validation.writes === 2 &&
      validation.errorAlerts === 1 &&
      validation.keptDraft === '999.1.1.1' &&
      String(validation.alertText).includes(
        'invalid relay IP whitelist: allowed_cidrs[0]',
      ),
    'the server validation error was not shown once with the draft retained',
  )
  assert(
    JSON.stringify(validation.corrected?.allowed_cidrs) ===
      JSON.stringify(['10.0.0.0/8']),
    'the corrected draft was not accepted after the 400',
  )

  const failure = checks.failureDraft ?? {}
  assert(
    failure.writes === 2 &&
      failure.transportAlerts === 1 &&
      failure.retained === '10.0.0.0/8' &&
      JSON.stringify(failure.retried?.trusted_proxy_cidrs) ===
        JSON.stringify(['10.0.0.0/8']),
    'a 503 did not report once, keep the draft and retry through Save',
  )

  const refresh = checks.scopedRefresh ?? {}
  assert(
    refresh.reads === 1 &&
      refresh.refreshWrites === 0 &&
      refresh.draftWrites === 1 &&
      refresh.disabledWhileDirty === true,
    'the scoped refresh did not read only the policy, wrote it, or stayed enabled over a draft',
  )
  assert(
    checks.localeThemeNoRefetch?.reads === 0 &&
      checks.localeThemeNoRefetch?.writes === 0,
    'a locale/theme rerender refetched or wrote the network settings',
  )

  const probes = checks.probes ?? {}
  assert(
    probes.malformed?.status === 400 &&
      probes.ipv4Prefix?.status === 400 &&
      probes.ipv6Prefix?.status === 400 &&
      probes.trustedMalformed?.status === 400,
    'a malformed IP or an out-of-range prefix was accepted',
  )
  assert(
    JSON.stringify(probes.omitted?.payload) ===
      JSON.stringify({ allowed_cidrs: [], trusted_proxy_cidrs: [] }) &&
      JSON.stringify(probes.duplicates?.payload) ===
        JSON.stringify({
          allowed_cidrs: ['10.0.0.0/8', '192.168.0.0/16'],
          trusted_proxy_cidrs: ['127.0.0.1/32'],
        }),
    'omitted arrays or duplicate/blank normalization do not match the backend contract',
  )

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the network route',
  )
  assert(
    denial.forgedPatch === 403 &&
      denial.forgedRead === 403 &&
      denial.anonymousRead === 401,
    'the fixture did not answer forged or anonymous policy requests with 403/401',
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
      `the network tab overflows horizontally at ${width}px`,
    )
  }

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the network tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  const allowedFailures = (report.expectedApiFailures ?? []).every(
    (entry) =>
      entry === '401 GET /api/v1/auth/me' ||
      /^(400|401|403|503) (GET|PATCH) \/api\/v1\/settings\/relay-ip-whitelist$/.test(
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
      `the required network screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )
}

if (failures.length > 0) {
  console.error('Settings network focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Settings network focused browser verification passed.')
