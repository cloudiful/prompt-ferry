import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'
import { evaluateTargetRowLayout } from './validate-target-row.ts'

type RouteReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  screenshots?: string[]
}

const foundationDir = import.meta.dir
const frontendRoot = join(foundationDir, '../..')
const reportPath = join(foundationDir, 'endpoints-routes-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

assert(
  existsSync(reportPath),
  'current model-route browser report is missing; run run-endpoints-routes.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(read(reportPath)) as RouteReport
  const checks = report.checks ?? {}
  const scenarios = [
    'route-render',
    'admin-denial',
    'dialog-open',
    'mutation-create',
    'mutation-toggle',
    'mutation-update',
    'mutation-delete',
    'mutation-conflict-guard',
    'long-values',
  ]
  const requiredChecks = [
    'adminDenial',
    'routeAndNavigation',
    'allEndpointResolution',
    'catalogBehavior',
    'pagination',
    'targetSettingsAndOrdering',
    'targetSettingsRoundtrip',
    'embeddingDimensionNullRoundtrip',
    'catalogRace',
    'probeDetails',
    'mutationCreate',
    'mutationToggle',
    'mutationUpdate',
    'mutationDelete',
    'mutationConflictGuard',
    'longValuesResponsive',
    'localeThemeRendering',
    'apiIsolation',
    'apiLedger',
    'consoleErrors',
    'pageErrors',
    'scenarioCoverage',
  ]

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('endpoints-routes-'),
    'report runId is missing or malformed',
  )
  assert(
    typeof report.startedAt === 'string' &&
      typeof report.finishedAt === 'string',
    'report timestamps are incomplete',
  )
  const runTimestamp = Number(report.runId?.slice('endpoints-routes-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'report runId is not current to its start time',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'report is stale or has invalid finish time',
  )

  for (const name of requiredChecks)
    assert(
      checks[name] !== undefined,
      `current-run route check is missing: ${name}`,
    )
  assert(
    checks.callbackFailure === undefined,
    'current-run browser callback reported a failure',
  )
  for (const scenario of scenarios)
    assert(
      checks.scenarioCoverage?.[scenario] === true,
      `current-run route scenario is missing or false: ${scenario}`,
    )

  const route = ROUTES.find((entry) => entry.id === 'R-ENDPOINTS-ROUTES')
  assert(
    route?.route === '/endpoints/routes' &&
      route.scope === 'admin' &&
      route.domain === 'model-routes',
    'route matrix identity or admin scope changed',
  )
  assert(
    route?.foundationTest === 'e2e/foundation/endpoints-routes.playwright',
    'route matrix does not map the focused React callback',
  )
  assert(
    JSON.stringify(route?.mutationKeys) ===
      JSON.stringify([
        'modelroute:<id>',
        'modelroute:create',
        'networktest:modelroute:<id>',
      ]),
    'route matrix mutation keys drifted',
  )
  assert(
    scenarios.every((scenario) =>
      route?.requiredScenarios.includes(scenario as never),
    ),
    'route matrix omitted a required browser scenario',
  )

  assert(
    checks.adminDenial?.redirectedToApiKeys === true &&
      checks.adminDenial?.hiddenNavigation === true &&
      checks.adminDenial?.adminApiCalls === 0,
    'non-admin route isolation failed',
  )
  assert(
    checks.routeAndNavigation?.routeId === 'R-ENDPOINTS-ROUTES' &&
      checks.routeAndNavigation?.navigationVisible === true &&
      checks.routeAndNavigation?.rowCount === 10,
    'route render or first-page row evidence is incomplete',
  )
  assert(
    checks.allEndpointResolution?.query?.first === '0' &&
      checks.allEndpointResolution?.query?.rows === '200' &&
      checks.allEndpointResolution?.offPageTargetId === 'ep-fixture-8' &&
      checks.allEndpointResolution?.targetSelectable === true,
    'all-page endpoint/protocol resolution was not verified',
  )
  assert(
    checks.catalogBehavior?.offPageCatalogFetched === true &&
      checks.catalogBehavior?.endpointId === 'ep-fixture-8' &&
      checks.catalogBehavior?.concurrentIdenticalRequests === true &&
      checks.catalogBehavior?.successfulCacheHit === true,
    'catalog protocol, coalescing, or cache behavior was not verified',
  )
  assert(
    checks.pagination?.initialRows === 10 &&
      checks.pagination?.pageSize20Rows === 12 &&
      checks.pagination?.independentQueryRows === '20' &&
      checks.pagination?.pageSizePersistence === true,
    'independent route pagination or page-size persistence failed',
  )

  assert(
    checks.targetSettingsAndOrdering?.keyboardOrderMoved === true &&
      checks.targetSettingsAndOrdering?.dragOrderRestoredAssociation === true &&
      checks.targetSettingsAndOrdering?.targetCount === 5 &&
      checks.targetSettingsAndOrdering?.duplicateEndpointIdsPreserved ===
        true &&
      checks.targetSettingsAndOrdering?.dimensionValid === true &&
      checks.targetSettingsAndOrdering?.scheduleDays?.join(',') === '1,2,3,4,5',
    'target reorder or settings payload evidence is incomplete',
  )
  assert(
    checks.targetSettingsRoundtrip?.pattern === true &&
      checks.targetSettingsRoundtrip?.orderedModels === true &&
      checks.targetSettingsRoundtrip?.proxyKeep === true &&
      checks.targetSettingsRoundtrip?.overnightWeekdays === true &&
      checks.targetSettingsRoundtrip?.chatSettings === true &&
      checks.targetSettingsRoundtrip?.embeddingDimensions === 1536 &&
      checks.targetSettingsRoundtrip?.chatHiddenForEmbeddings === true,
    'target settings did not reopen with the submitted values',
  )
  assert(
    checks.embeddingDimensionNullRoundtrip?.requestStatus === 200 &&
      checks.embeddingDimensionNullRoundtrip?.targetIndex === 2 &&
      checks.embeddingDimensionNullRoundtrip?.inputBeforeSave === '' &&
      checks.embeddingDimensionNullRoundtrip?.payloadValue === null &&
      checks.embeddingDimensionNullRoundtrip?.reopenedInputValue === '',
    'cleared embedding dimensions were not sent as null and reopened blank',
  )
  assert(
    JSON.stringify(checks.catalogRace?.requestQueries) ===
      JSON.stringify([
        '?native_api=anthropic_messages',
        '?native_api=responses',
      ]) &&
      JSON.stringify(checks.catalogRace?.requestProtocols) ===
        JSON.stringify(['anthropic_messages', 'responses']) &&
      JSON.stringify(checks.catalogRace?.responseOrder) ===
        JSON.stringify(['responses', 'anthropic_messages']),
    'catalog race did not record the two protocol queries and out-of-order responses',
  )
  assert(
    checks.catalogRace?.oldResponsePendingWhenCurrentRendered === true &&
      JSON.stringify(checks.catalogRace?.suggestionsAfterCurrentResponse) ===
        JSON.stringify(['catalog-race-current-model']) &&
      JSON.stringify(checks.catalogRace?.suggestionsAfterOldResponse) ===
        JSON.stringify(['catalog-race-current-model']) &&
      checks.catalogRace?.currentModelVisibleAfterCurrentResponse === true &&
      checks.catalogRace?.oldModelHiddenAfterCurrentResponse === true &&
      checks.catalogRace?.currentModelVisibleAfterOldResponse === true &&
      checks.catalogRace?.oldModelHiddenAfterOldResponse === true,
    'late response changed suggestions for the current protocol',
  )
  assert(
    checks.catalogRace?.modelAfterCurrentResponse ===
      'catalog-race-custom-draft' &&
      checks.catalogRace?.modelAfterOldResponse ===
        'catalog-race-custom-draft' &&
      checks.catalogRace?.draftPreservedBeforeProtocolChange === true &&
      checks.catalogRace?.draftPreservedAfterCurrentResponse === true &&
      checks.catalogRace?.draftPreservedAfterOldResponse === true,
    'protocol catalog race changed the controlled custom model text',
  )
  assert(
    checks.probeDetails?.resultCount === 5 &&
      checks.probeDetails?.orderedModels?.length === 5 &&
      checks.probeDetails?.preferredFirst === true &&
      checks.probeDetails?.includesHttpAndMessage === true &&
      checks.probeDetails?.noToast === true,
    'ordered probe result details or mobile access failed',
  )

  assert(
    checks.mutationCreate?.status === 200 &&
      checks.mutationCreate?.selectedOffPageEndpoint === 'ep-fixture-10' &&
      checks.mutationCreate?.proxyPayloadSafe === true &&
      checks.mutationCreate?.newRuleVisible === true,
    'route create payload or result failed',
  )
  assert(
    checks.mutationToggle?.probeRanDuringToggle === true &&
      checks.mutationToggle?.conflictingControlsDisabled === true &&
      checks.mutationToggle?.routeListReadsBefore ===
        checks.mutationToggle?.routeListReadsAfter,
    'toggle guard or independent probe behavior failed',
  )
  assert(
    checks.mutationUpdate?.status === 200 &&
      checks.mutationUpdate?.preservedLongModel === true &&
      checks.mutationUpdate?.orderedTargets?.length === 5,
    'route update did not persist ordered target data',
  )
  assert(
    checks.mutationDelete?.cancelledWrites === 0 &&
      checks.mutationDelete?.successfulDelete === 204 &&
      checks.mutationDelete?.routeRemoved === true,
    'route delete cancel/confirm behavior failed',
  )
  assert(
    checks.mutationConflictGuard?.oneFailedWrite === true &&
      checks.mutationConflictGuard?.retryStatus === 200 &&
      checks.mutationConflictGuard?.dismissalDisabled === true &&
      checks.mutationConflictGuard?.draftRetained === true,
    'retryable route failure or pending exclusion failed',
  )

  const responsive = checks.longValuesResponsive?.responsive as
    Array<Record<string, any>> | undefined
  assert(
    checks.longValuesResponsive?.longPattern > 40 &&
      checks.longValuesResponsive?.longModel > 40,
    'long controlled model/pattern evidence is missing',
  )
  assert(
    JSON.stringify(responsive?.map((item) => item.width)) ===
      JSON.stringify([1440, 1024, 768, 390]) &&
      responsive?.every((item) => item.noHorizontalOverflow === true),
    'route responsive widths or overflow checks are incomplete',
  )
  assert(
    responsive?.find((item) => item.width === 390)?.cardVisible === true &&
      responsive.find((item) => item.width === 390)?.tableVisible === false &&
      responsive.find((item) => item.width === 768)?.tableVisible === true,
    'table/card breakpoint was not verified',
  )
  assert(
    checks.longValuesResponsive?.dialogContainment?.length === 2 &&
      checks.longValuesResponsive.dialogContainment.every(
        (item: Record<string, any>) =>
          item.withinViewport && item.saveReachable,
      ),
    'dialog containment or action reachability was not verified',
  )
  assert(
    checks.localeThemeRendering?.chineseActionVisible === true &&
      checks.localeThemeRendering?.darkTheme === true &&
      checks.localeThemeRendering?.longTargetDialog === true,
    'Chinese/dark route rendering was not verified',
  )
  failures.push(...evaluateTargetRowLayout(checks.targetRowLayout))
  assert(
    checks.apiIsolation?.unexpectedAdminRequests?.length === 0 &&
      checks.apiIsolation?.quotaCalls === 0,
    'unexpected admin traffic or quota requests were observed',
  )
  assert(
    checks.consoleErrors?.length === 0 && checks.pageErrors?.length === 0,
    'browser console or page errors remain',
  )
  assert(
    Array.isArray(checks.apiLedger) &&
      checks.apiLedger.every(
        (request: Record<string, any>) =>
          typeof request.method === 'string' &&
          typeof request.path === 'string' &&
          Number.isInteger(request.status),
      ),
    'sanitized current-run API ledger is malformed',
  )

  const requiredScreenshots = [
    'e2e/foundation/screenshots/endpoints-routes-en-light-1440.png',
    'e2e/foundation/screenshots/endpoints-routes-en-light-390.png',
    'e2e/foundation/screenshots/endpoints-routes-dialog-390.png',
    'e2e/foundation/screenshots/endpoints-routes-zh-dark-1440.png',
    'e2e/foundation/screenshots/endpoints-routes-target-row-1440.png',
    'e2e/foundation/screenshots/endpoints-routes-target-row-390.png',
    'e2e/foundation/screenshots/endpoints-routes-target-row-zh-dark-1440.png',
  ]
  for (const screenshot of requiredScreenshots)
    assert(
      report.screenshots?.includes(screenshot) &&
        existsSync(join(frontendRoot, screenshot)),
      `current-run screenshot is missing: ${screenshot}`,
    )
  assert(
    read(join(frontendRoot, 'src/App.tsx')).includes(
      "case 'R-ENDPOINTS-ROUTES'",
    ),
    'React app graph does not mount the route page',
  )
}

if (failures.length) {
  console.error(failures.join('\n'))
  process.exitCode = 1
} else {
  console.log('Model-route React browser acceptance report passed.')
}
