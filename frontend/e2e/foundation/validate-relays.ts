import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type SafeSecret = {
  mode?: string
  present?: boolean
  digest?: string
}

type RelayReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  screenshots?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'relays-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

function sameJson(actual: unknown, expected: unknown): boolean {
  return JSON.stringify(actual) === JSON.stringify(expected)
}

assert(
  existsSync(reportPath),
  'focused Relay browser result is missing; run run-relays.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(read(reportPath)) as RelayReport
  const checks = report.checks ?? {}
  const requiredChecks = [
    'adminDenial',
    'routeAndNavigation',
    'summaryAndSecretFlags',
    'initialRead',
    'pagination',
    'listFailureRetainsRowsAndRecovers',
    'dialogOpen',
    'createFailureDraftRetryAndRefreshGuard',
    'backendValidationRetainsDraft',
    'syntheticFixtureMaterial',
    'secretPatchSemantics',
    'secretFlagsReopenWithoutEcho',
    'reconnectDisabledAndConflictGuard',
    'deleteConfirmation',
    'localeThemeRendering',
    'timestampLocaleAndZone',
    'responsiveContainment',
    'noHostRoleOrSecretEcho',
    'secretRequestDigests',
    'relayResponseStatuses',
    'scenarioCoverage',
    'expectedApiFailures',
    'apiFailures',
    'consoleErrors',
    'pageErrors',
  ]

  assert(
    typeof report.runId === 'string' && report.runId.startsWith('relays-'),
    'focused report is not from the Relay runner',
  )
  assert(
    typeof report.startedAt === 'string' &&
      typeof report.finishedAt === 'string',
    'focused Relay report has no run timestamps',
  )
  const runTimestamp = Number(report.runId?.slice('relays-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'Relay report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'Relay report is stale or has invalid run timestamps',
  )
  for (const name of requiredChecks) {
    assert(name in checks, `current-run Relay check is missing: ${name}`)
  }

  const denial = checks.adminDenial as
    | {
        redirectedToApiKeys?: boolean
        hiddenNavigation?: boolean
        adminRelayCalls?: number
      }
    | undefined
  assert(
    denial?.redirectedToApiKeys === true &&
      denial.hiddenNavigation === true &&
      denial.adminRelayCalls === 0,
    'non-admin redirect/navigation/API-isolation evidence is incomplete',
  )
  assert(
    sameJson(checks.initialRead, { first: 0, rows: 10 }),
    'initial Relay query did not use the stored ten-row page size',
  )
  assert(
    checks.pagination?.secondPageRead === true &&
      checks.pagination?.distinctVisibleRows === true &&
      checks.pagination?.pageSizePersisted === true,
    'Relay pagination, distinct rows, or page-size persistence was not verified',
  )
  assert(
    checks.listFailureRetainsRowsAndRecovers === true,
    'Relay list failure did not prove retained rows and successful recovery',
  )
  assert(
    checks.dialogOpen === true &&
      checks.createFailureDraftRetryAndRefreshGuard === true,
    'Relay dialog open, create retry, or refresh-held create guard is missing',
  )

  const validation = checks.backendValidationRetainsDraft as
    | {
        invalidTlsScheme?: boolean
        normalizedDuplicateUrl?: boolean
        missingMtlsCertificateAndKey?: boolean
        missingRequiredBridgeKey?: boolean
      }
    | undefined
  assert(
    validation?.invalidTlsScheme === true &&
      validation.normalizedDuplicateUrl === true &&
      validation.missingMtlsCertificateAndKey === true &&
      validation.missingRequiredBridgeKey === true,
    'Relay fixture URL/TLS/secret validation failures did not retain the dialog draft',
  )
  assert(
    checks.syntheticFixtureMaterial?.requiredMaterialAcceptedByFixture ===
      true &&
      checks.syntheticFixtureMaterial?.fixtureValidatesPemCryptography ===
        false &&
      checks.syntheticFixtureMaterial?.liveConnectionAttempted === false,
    'synthetic Relay fixture limits are not recorded accurately',
  )
  assert(
    checks.secretPatchSemantics === true &&
      checks.secretFlagsReopenWithoutEcho === true,
    'four-secret edit, clear/reopen flags, or no-echo evidence is missing',
  )

  const reconnect = checks.reconnectDisabledAndConflictGuard as
    | {
        disabledReconnectNoRequest?: boolean
        sameRowControlsDisabled?: boolean
        unrelatedRowAvailable?: boolean
        lockHeldThroughRefresh?: boolean
      }
    | undefined
  assert(
    reconnect?.disabledReconnectNoRequest === true &&
      reconnect.sameRowControlsDisabled === true &&
      reconnect.unrelatedRowAvailable === true &&
      reconnect.lockHeldThroughRefresh === true,
    'Relay reconnect disabled path or shared-row guard evidence is incomplete',
  )
  assert(
    checks.deleteConfirmation?.cancelNoRequest === true &&
      checks.deleteConfirmation?.confirmedDelete === true &&
      checks.deleteConfirmation?.nativeConfirmCalls === 2,
    'Relay native-confirm cancel/delete evidence is incomplete',
  )

  const responsive = checks.responsiveContainment as
    | Array<{
        locale?: string
        theme?: string
        width?: number
        documentWidth?: number
        viewportWidth?: number
        actionsContained?: boolean
        dialogContained?: boolean
        dialogSaveVisible?: boolean
        timestampFormatted?: boolean
      }>
    | undefined
  const expectedResponsiveProfiles = ['en-US', 'zh-CN'].flatMap((locale) =>
    [1440, 1024, 768, 390].map((width) => ({
      locale,
      theme: locale === 'en-US' ? 'light' : 'dark',
      width,
    })),
  )
  assert(
    sameJson(
      responsive?.map(({ locale, theme, width }) => ({
        locale,
        theme,
        width,
      })),
      expectedResponsiveProfiles,
    ),
    'Relay responsive evidence does not cover both locales/themes at all four widths',
  )
  for (const item of responsive ?? []) {
    assert(
      item.documentWidth !== undefined &&
        item.viewportWidth !== undefined &&
        item.documentWidth <= item.viewportWidth &&
        item.actionsContained === true &&
        item.dialogContained === true &&
        item.dialogSaveVisible === true &&
        item.timestampFormatted === true,
      `Relay page, action reachability, or dialog containment failed at ${item.width}px`,
    )
  }
  const localeTheme = checks.localeThemeRendering as
    | {
        zhDark?: { lang?: string; dark?: boolean; colorScheme?: string }
        enLight?: { lang?: string; dark?: boolean; colorScheme?: string }
      }
    | undefined
  assert(
    localeTheme?.zhDark?.lang === 'zh-CN' &&
      localeTheme.zhDark.dark === true &&
      localeTheme.zhDark.colorScheme === 'dark' &&
      localeTheme.enLight?.lang === 'en-US' &&
      localeTheme.enLight.dark === false &&
      localeTheme.enLight.colorScheme === 'light',
    'Relay zh-CN dark / en-US light evidence is incomplete',
  )

  const hostAndSecret = checks.noHostRoleOrSecretEcho as
    | {
        hostRoleCalls?: number
        routeId?: string
        secretValuesEchoedByApi?: boolean
      }
    | undefined
  assert(
    hostAndSecret?.hostRoleCalls === 0 &&
      hostAndSecret.routeId === 'R-RELAYS' &&
      hostAndSecret.secretValuesEchoedByApi === false,
    'Relay route called host-role APIs or reported secret echo',
  )

  const coverage = checks.scenarioCoverage as
    Record<string, boolean> | undefined
  const requiredScenarios = [
    'route-render',
    'admin-denial',
    'pagination',
    'dialog-open',
    'mutation-create',
    'mutation-reconnect',
    'mutation-delete',
    'mutation-conflict-guard',
  ]
  for (const scenario of requiredScenarios) {
    assert(
      coverage?.[scenario] === true,
      `current-run R-RELAYS scenario is missing or false: ${scenario}`,
    )
  }

  const responseStatuses = checks.relayResponseStatuses as
    Array<{ method?: string; path?: string; status?: number }> | undefined
  const createStatuses = responseStatuses
    ?.filter(
      (response) =>
        response.method === 'POST' && response.path === '/api/v1/admin/relays',
    )
    .map((response) => response.status)
  assert(
    sameJson(createStatuses, [503, 200, 400, 400, 400, 400, 200]),
    'Relay create failure/retry and four fixture validation statuses are missing',
  )
  assert(
    responseStatuses?.some(
      (response) =>
        response.method === 'PATCH' &&
        response.path === '/api/v1/admin/relays/relay-1002' &&
        response.status === 200,
    ),
    'created Relay row was not edited by its identified id',
  )
  assert(
    responseStatuses?.some(
      (response) =>
        response.method === 'DELETE' &&
        response.path === '/api/v1/admin/relays/relay-1002' &&
        response.status === 204,
    ),
    'confirmed delete did not return 204 for the identified created Relay',
  )
  assert(
    JSON.stringify(checks.expectedApiFailures) ===
      JSON.stringify([
        '503 GET /api/v1/admin/relays',
        '503 POST /api/v1/admin/relays',
        '400 POST /api/v1/admin/relays',
        '400 POST /api/v1/admin/relays',
        '400 POST /api/v1/admin/relays',
        '400 POST /api/v1/admin/relays',
      ]),
    'expected Relay list/create/validation failures are missing or unexpected',
  )
  assert(
    Array.isArray(checks.apiFailures) && checks.apiFailures.length === 0,
    'unexpected Relay browser API failures were reported',
  )
  assert(
    Array.isArray(checks.consoleErrors) && checks.consoleErrors.length === 0,
    'Relay browser console errors were reported',
  )
  assert(
    Array.isArray(checks.pageErrors) && checks.pageErrors.length === 0,
    'Relay browser page errors were reported',
  )

  const secretBodies = checks.secretRequestDigests as
    | Array<{
        method?: string
        name?: string
        bridge_encryption_mode?: string
        secrets?: Record<string, SafeSecret>
      }>
    | undefined
  const secretCreate = secretBodies?.find(
    (body) =>
      body.method === 'POST' &&
      body.name === 'Fixture mTLS relay' &&
      body.bridge_encryption_mode === 'required' &&
      body.secrets?.bridge_encryption_key?.mode === 'replace',
  )
  assert(
    secretCreate?.bridge_encryption_mode === 'required' &&
      secretCreate.secrets?.relay_ca_pem?.mode === 'replace' &&
      secretCreate.secrets?.client_cert_pem?.mode === 'replace' &&
      secretCreate.secrets?.client_key_pem?.mode === 'replace' &&
      secretCreate.secrets?.bridge_encryption_key?.mode === 'replace',
    'valid synthetic mTLS/bridge create payload did not capture all four replacements',
  )
  const secretPatch = secretBodies?.find(
    (body) => body.method === 'PATCH' && body.name === 'Fixture mTLS relay',
  )
  assert(
    secretPatch?.secrets?.relay_ca_pem?.mode === 'clear' &&
      secretPatch.secrets.client_cert_pem?.mode === 'replace' &&
      secretPatch.secrets.client_key_pem?.mode === 'keep' &&
      secretPatch.secrets.bridge_encryption_key?.mode === 'clear',
    'safe digest ledger does not match the four independent edit-secret modes',
  )
  for (const body of [
    ...(secretCreate ? [secretCreate] : []),
    ...(secretPatch ? [secretPatch] : []),
  ]) {
    for (const secret of Object.values(body.secrets ?? {})) {
      if (secret.mode === 'replace') {
        assert(
          secret.present === true &&
            typeof secret.digest === 'string' &&
            /^[0-9a-f]{64}$/.test(secret.digest),
          'secret replacement is not represented by a redacted digest',
        )
      }
    }
  }
  const reportText = JSON.stringify(report)
  for (const value of [
    'U3ludGhldGljIFJlbGF5IENBIEZpeHR1cmU=',
    'U3ludGhldGljIENsaWVudCBDZXJ0aWZpY2F0ZQ==',
    'U3ludGhldGljIFJlbGF5IFByaXZhdGUgS2V5',
    'synthetic-relay-bridge-key-fixture-only',
    'replacement-certificate-material-fixture-only',
    'ignored-clear-material-fixture-only',
  ]) {
    assert(
      !reportText.includes(value),
      'synthetic secret plaintext leaked into the report',
    )
  }

  const expectedScreenshots = [
    'e2e/foundation/screenshots/relays-admin-en-light-1440.png',
    'e2e/foundation/screenshots/relay-editor-en-light-1440.png',
    'e2e/foundation/screenshots/relays-admin-zh-dark-1440.png',
    'e2e/foundation/screenshots/relays-admin-en-light-390.png',
    'e2e/foundation/screenshots/relay-editor-en-light-390.png',
    'e2e/foundation/screenshots/relays-admin-zh-dark-390.png',
    'e2e/foundation/screenshots/relay-editor-zh-dark-1440.png',
    'e2e/foundation/screenshots/relay-editor-zh-dark-390.png',
  ]
  for (const screenshot of expectedScreenshots) {
    assert(
      report.screenshots?.includes(screenshot),
      `screenshot is missing from the current Relay run: ${screenshot}`,
    )
    assert(
      existsSync(join(root, '..', '..', screenshot)),
      `Relay screenshot file is missing: ${screenshot}`,
    )
  }
}

const route = ROUTES.find((candidate) => candidate.id === 'R-RELAYS')
const requiredScenarios = [
  'route-render',
  'admin-denial',
  'pagination',
  'dialog-open',
  'mutation-create',
  'mutation-reconnect',
  'mutation-delete',
  'mutation-conflict-guard',
]
assert(
  route?.foundationTest === 'e2e/foundation/relays.playwright',
  'R-RELAYS does not map to the focused React Relay run',
)
assert(
  sameJson(route?.mutationKeys, ['relay:<id>', 'relay:create']),
  'R-RELAYS mutation key mapping differs from the shared relay guard keys',
)
assert(
  route?.reads.includes('GET /api/v1/admin/relays') &&
    route.reads.includes('GET /api/v1/bridge/status'),
  'R-RELAYS read mapping is missing the admin list or shell bridge status',
)
for (const scenario of requiredScenarios) {
  assert(
    route?.requiredScenarios.includes(scenario),
    `R-RELAYS matrix is missing ${scenario}`,
  )
}
assert(
  route?.status === 'baseline-verified',
  'R-RELAYS baseline status changed',
)

const authCallback = read(join(root, 'auth-shell.playwright'))
assert(
  !authCallback.includes("['/relays', 'R-RELAYS']"),
  'ported Relay route remains in the common placeholder list',
)
const pendingRoutes =
  authCallback.match(/const pendingRoutes = \[([\s\S]*?)\n  \]/)?.[1] ?? ''
const pendingIds = [...pendingRoutes.matchAll(/\['\/[^']+', '([^']+)'\]/g)].map(
  (match) => match[1],
)
assert(
  pendingIds.length === 0 && !pendingIds.includes('R-RELAYS'),
  'auth-shell must not retain a settings placeholder',
)

const app = read(join(root, '../../src/App.tsx'))
assert(
  app.includes("case 'R-RELAYS'") &&
    app.includes('<AdminGuard>') &&
    app.includes('<RelaysPage />'),
  'React Relay page is not mounted behind AdminGuard',
)
const page = read(join(root, '../../src/pages/RelaysPage.tsx'))
assert(
  page.includes('data-foundation-route-id="R-RELAYS"'),
  'React Relay route is missing its foundation route identity',
)
const handler = read(join(root, '../fixtures/handlers/mcp-relays.ts'))
assert(
  handler.includes('if (!relay.enabled)') &&
    handler.includes("return error('relay_disabled'") &&
    !handler.includes('relay.enabled = true'),
  'Relay fixture still force-enables a disabled relay on reconnect',
)

if (failures.length > 0) {
  console.error(failures.map((failure) => `FAIL ${failure}`).join('\n'))
  process.exitCode = 1
} else {
  console.log(
    'R-RELAYS current-run route, mutation, secret-safety, fixture, and responsive evidence is valid.',
  )
}
