import { existsSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type MpcReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  screenshots?: string[]
  segments?: Array<{ name: string; startedAt: string; finishedAt: string }>
}

const root = import.meta.dir
const reportPath = join(root, 'mcp-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

assert(
  existsSync(reportPath),
  'current MCP browser report is missing; run run-mcp.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(read(reportPath)) as MpcReport
  const checks = report.checks ?? {}
  const requiredScenarios = [
    'route-render',
    'pagination',
    'dialog-open',
    'mutation-create',
    'mutation-toggle',
    'mutation-delete',
    'mutation-failure-draft',
    'long-values',
  ]
  const scenarioCoverage = checks.scenarioCoverage ?? {}
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  const runTimestamp = Number(report.runId?.slice('mcp-'.length))

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('mcp-') &&
      Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'MCP report runId is missing, malformed, or not current to its start',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'MCP report is stale or has invalid finish timestamp',
  )
  assert(
    report.segments?.map((segment) => segment.name).join(',') ===
      'owner-role,admin-paging-delete,catalog-editor,guarded-mutations,create-secrets-responsive',
    'focused MCP run did not include every serial callback segment',
  )
  for (const scenario of requiredScenarios) {
    assert(
      scenarioCoverage[scenario] === true,
      `current-run MCP scenario is missing or false: ${scenario}`,
    )
  }

  const owner = checks.ownerRole ?? {}
  assert(
    owner.routeId === 'R-MCP' &&
      owner.navVisible === true &&
      owner.ownRows === 1,
    'authenticated owner-scoped MCP route evidence is incomplete',
  )
  assert(
    owner.anonymousStatuses?.providers === 401 &&
      owner.anonymousStatuses?.servers === 401 &&
      owner.anonymousStatuses?.catalog === 401 &&
      owner.anonymousStatuses?.test === 401,
    'anonymous MCP requests were not denied',
  )
  assert(
    owner.foreignStatuses?.catalog === 404 &&
      owner.foreignStatuses?.test === 404 &&
      owner.foreignStatuses?.update === 404,
    'foreign-owner MCP catalog/test/update requests were not denied',
  )
  assert(
    owner.unrelatedAdminStatuses?.endpoints === 403 &&
      owner.unrelatedAdminStatuses?.relays === 403 &&
      owner.unrelatedAdminStatuses?.unknownMcpPath === 404,
    'MCP fixture pass-through widened unrelated admin access or accepted an unknown path',
  )
  assert(
    owner.adminUserOptionsCalls === 0 &&
      owner.recordsCalls === 0 &&
      owner.disabledCatalogNotFetched === true &&
      owner.disabledCatalogEmpty === true &&
      owner.disabledTestFalse === true,
    'non-admin isolation or disabled-server behavior is incomplete',
  )
  assert(
    owner.createScope === 'user' &&
      owner.createOwner === 2 &&
      owner.updateScope === 'user' &&
      owner.updateOwner === 2,
    'ordinary-user create/update did not force the caller owner and user scope',
  )
  assert(
    checks.listTestAction?.testColumnAbsent === true &&
      JSON.stringify(checks.listTestAction?.headers) ===
        JSON.stringify(['Name', 'Status', 'Edit']),
    'the MCP list kept a separate test column',
  )

  const pagination = checks.pagination ?? {}
  assert(
    pagination.initialRows === 10 &&
      pagination.firstPageIds?.length === 10 &&
      new Set(pagination.firstPageIds).size === 10 &&
      pagination.secondPageRows === 2 &&
      pagination.secondPageIds?.length === 2 &&
      new Set(pagination.secondPageIds).size === 2,
    'MCP query pagination did not return distinct first and second page rows',
  )
  assert(
    pagination.lastPageCorrection === true &&
      pagination.deleteCancelledWrites === 0 &&
      pagination.deleteSuccesses === 2 &&
      pagination.nativeConfirmApi === true &&
      pagination.pageSizePersistence === true &&
      pagination.adminUserOptionsRead === true,
    'MCP deletion, out-of-range correction, page-size, or admin owner-options evidence is incomplete',
  )

  const catalog = checks.catalogEditor ?? {}
  assert(
    catalog.firstCatalogCalls > 0 &&
      catalog.unchangedAfterRefresh === true &&
      catalog.invalidatedOnSave === true &&
      catalog.searchableTools === true &&
      catalog.missingCatalogChipPreserved === true &&
      catalog.filterModeRoundTrip === true,
    'MCP catalog cache, search, or controlled filter behavior is incomplete',
  )
  assert(
    catalog.automaticProtocolSentinel === true &&
      catalog.explicitProtocolRoundTrip === true &&
      catalog.hiddenHeaderPreserved === true &&
      catalog.hiddenTimeoutPreserved === true &&
      catalog.hiddenControlsAbsent === true,
    'MCP lifecycle protocol or hidden-field round trip is incomplete',
  )
  assert(
    checks.dialogGenerationLateAnswer === true,
    'late MCP catalog response overwrote the currently edited server',
  )

  const toggles = checks.toggleAndIndependentProbe ?? {}
  assert(
    toggles.sameRowWriteControlsGated === true &&
      toggles.singleWrite === true &&
      toggles.otherRowAvailable === true &&
      toggles.independentProbeAvailable === true &&
      toggles.probeResultRendered === true &&
      toggles.listCallsUnchangedByProbe === true &&
      toggles.guardHeldThroughRefresh === true &&
      toggles.toggled === true,
    'MCP write/test mutual exclusion or refresh hold behavior is incomplete',
  )

  const failureDraft = checks.failureDraftRetry ?? {}
  assert(
    failureDraft.pendingSaveGatesDialog === true &&
      failureDraft.draftRetained === true &&
      failureDraft.sameBodyRetry === true &&
      failureDraft.retryStatus === 200 &&
      failureDraft.savedTokenPresent === true &&
      failureDraft.hiddenHeaderPreserved === true &&
      failureDraft.hiddenTimeoutPreserved === true &&
      failureDraft.secretsMasked === true,
    'MCP failure draft, same-payload retry, or secret-safe preservation is incomplete',
  )

  const create = checks.createDialog ?? {}
  assert(
    create.hostedCreated != null &&
      create.privateOwner === 2 &&
      JSON.stringify(create.providerRegistryChoices) ===
        JSON.stringify(['Generic', 'Context7', 'Firecrawl']) &&
      create.hostedProviderPinned === true &&
      create.hostedBearerPresent === true &&
      create.bearerMasked === true &&
      create.bearerAddRemove === true &&
      create.lastEnabledTokenGate === true,
    'MCP hosted provider, owner, or bearer-token behavior is incomplete',
  )
  assert(
    create.basicCreated === true &&
      create.basicPasswordMasked === true &&
      create.basicPasswordNotEchoed === true &&
      create.basicKeep === true &&
      create.basicReplacement === true,
    'MCP Basic password mask/keep/replace behavior is incomplete',
  )
  assert(
    create.proxyKeep === true &&
      create.proxyClear === true &&
      create.proxyReplacement === true,
    'MCP proxy keep/clear/replace behavior is incomplete',
  )

  const stdio = checks.stdioEnvironment ?? {}
  assert(
    stdio.savedDirectMasked === true &&
      stdio.directKeep === true &&
      stdio.workerReferenceRoundTrip === true &&
      stdio.workerCreate === true &&
      stdio.invalidWorkerValueBlocked === true,
    'MCP stdio environment secret/reference/validation behavior is incomplete',
  )
  assert(
    checks.requestRecordCalls === 0,
    'MCP management made an out-of-scope request-record API call',
  )

  const longValues = checks.longValues ?? {}
  assert(
    longValues.longNameLength > 40 &&
      longValues.longEndpointLength > 60 &&
      longValues.longCommand > 40 &&
      longValues.nameAndCommandContainedAt390 === true,
    'long MCP name, endpoint, command, or editor containment evidence is incomplete',
  )
  assert(
    checks.responsiveContainment
      ?.map((state: { width: number }) => state.width)
      .join(',') === '1440,1024,768,390' &&
      checks.responsiveContainment.every(
        (state: { contained: boolean; actionsReachable: boolean[] }) =>
          state.contained === true &&
          state.actionsReachable.length === 4 &&
          state.actionsReachable.every(Boolean),
      ),
    'MCP table/actions were not verified at all required widths',
  )
  const dialogGeometry = checks.dialogGeometry as
    Array<Record<string, any>> | undefined
  const expectedDialogWidths: Record<number, number> = {
    1440: 896,
    1024: 896,
    768: 736,
    390: 358,
  }
  if (!Array.isArray(dialogGeometry) || dialogGeometry.length !== 4) {
    assert(false, 'MCP dialog geometry evidence is missing')
  } else {
    for (const width of [1440, 1024, 768, 390]) {
      const entry = dialogGeometry.find((state) => state?.viewport === width)
      if (!entry) {
        assert(
          false,
          `MCP dialog geometry is missing the ${width}px measurement`,
        )
        continue
      }
      assert(
        entry.horizontalScroll === false,
        `MCP dialog has a horizontal scrollbar at ${width}px`,
      )
      assert(
        entry.documentOverflow === false,
        `MCP dialog overflows the document at ${width}px`,
      )
      assert(
        entry.escapedControls === 0 && entry.controlCount > 0,
        `an MCP dialog control escapes the dialog at ${width}px`,
      )
      assert(
        entry.providerBearerSentence === false,
        `the redundant provider bearer sentence is still rendered at ${width}px`,
      )
      assert(
        typeof entry.transportWidth === 'number' &&
          typeof entry.providerWidth === 'number' &&
          Math.abs(entry.transportWidth - entry.providerWidth) <= 1,
        `MCP transport and provider selects do not share one width at ${width}px`,
      )
      assert(
        entry.proxyActionAdjacent === true &&
          typeof entry.proxyGap === 'number' &&
          entry.proxyGap >= 0 &&
          entry.proxyGap <= 40,
        `the MCP proxy action is detached from its label at ${width}px`,
      )
      assert(
        entry.expectedWidth === expectedDialogWidths[width] &&
          Math.abs(entry.width - expectedDialogWidths[width]) <= 1,
        `MCP dialog width does not match the declared 56rem layout at ${width}px`,
      )
    }
  }
  assert(
    report.screenshots?.includes(
      'e2e/foundation/screenshots/mcp-dialog-en-light-1440.png',
    ) === true,
    'the MCP dialog screenshot evidence is missing',
  )
  assert(
    checks.localeThemeRendering?.lang === 'zh-CN' &&
      checks.localeThemeRendering?.dark === true &&
      checks.localeThemeRendering?.colorScheme === 'dark',
    'MCP locale/theme rendering evidence is incomplete',
  )
  assert(
    checks.apiFailures?.length === 0 &&
      checks.consoleErrors?.length === 0 &&
      checks.pageErrors?.length === 0,
    'current MCP run has an unexpected API, console, or page failure',
  )
  const expectedFailures = checks.expectedApiFailures ?? []
  const hasExpected = (method: string, path: string, status: number) =>
    expectedFailures.some(
      (item: { method: string; path: string; status: number }) =>
        item.method === method && item.path === path && item.status === status,
    )
  assert(
    hasExpected('GET', '/api/v1/admin/mcp-servers', 401) &&
      hasExpected(
        'GET',
        '/api/v1/admin/mcp-servers/mcp-context7/catalog',
        404,
      ) &&
      hasExpected('GET', '/api/v1/admin/endpoints', 403) &&
      hasExpected('GET', '/api/v1/admin/relays', 403) &&
      hasExpected('PATCH', '/api/v1/admin/mcp-servers/mcp-context7', 503),
    'anonymous, owner-denial, unrelated-admin, or retry fault evidence is missing',
  )
  assert(
    checks.secretSafety?.fixtureCredentialValuesExcluded === true &&
      checks.secretSafety?.requestBodiesNotSerialized === true &&
      checks.secretSafety?.inputPasswordsMasked === true,
    'MCP report secret-safety ledger failed',
  )

  const reportText = read(reportPath)
  for (const value of [
    'fixture-context7-token',
    'fixture-hosted-token-primary',
    'fixture-hosted-token-secondary',
    'fixture-basic-password',
    'fixture-basic-replacement',
    'FIXTURE_NEW_WORKER_VALUE',
    'http://proxy.local:7890',
    'round-trip',
  ]) {
    assert(
      !reportText.includes(value),
      `MCP report contains a secret or serialized request-body value: ${value}`,
    )
  }
  assert(
    report.screenshots?.length >= 5 &&
      report.screenshots.every((path) => {
        const absolute = join(
          root,
          '..',
          path.replace(/^e2e\/foundation\//, 'foundation/'),
        )
        return existsSync(absolute) && statSync(absolute).size > 0
      }),
    'MCP current-run screenshots are missing or empty',
  )

  const route = ROUTES.find((row) => row.id === 'R-MCP')
  assert(
    route?.foundationTest === 'e2e/foundation/mcp.playwright' &&
      JSON.stringify(route?.mutationKeys) ===
        JSON.stringify(['mcp:create', 'mcp:<id>', 'mcptest:<id>']) &&
      !route?.reads.some((read) => read.includes('request-records')) &&
      !route?.mutationKeys.includes('usage:maintenance') &&
      !route?.legacyTests.includes('mcp-record-scope.test.ts'),
    'R-MCP matrix mapping includes an out-of-scope records/maintenance surface or stale key',
  )
  const authShell = read(join(root, 'auth-shell.playwright'))
  const pendingRoutes =
    authShell.match(/const pendingRoutes = \[([\s\S]*?)\n  \]/)?.[1] ?? ''
  const pendingIds = [
    ...pendingRoutes.matchAll(/\['\/[^']+', '([^']+)'\]/g),
  ].map((match) => match[1])
  assert(
    pendingIds.length === 0 &&
      !pendingIds.includes('R-MCP') &&
      !pendingIds.includes('R-RELAYS') &&
      !pendingIds.includes('R-REDACTION-RULES') &&
      !pendingIds.includes('R-REDACTION-PREVIEW'),
    'shared route-placeholder count is not 2 after the settings ports',
  )
}

if (failures.length > 0) {
  console.error(failures.map((failure) => `FAIL ${failure}`).join('\n'))
  process.exitCode = 1
} else {
  console.log(
    'MCP current-run UI, role, mutation, secret-safety, and matrix evidence is valid.',
  )
}
