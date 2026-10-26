import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type RedactionReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  screenshots?: string[]
  expectedApiFailures?: string[]
  apiFailures?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
}

const root = import.meta.dir
const reportPath = join(root, 'redaction-rules-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

assert(
  existsSync(reportPath),
  'focused Redaction Rules report is missing; run run-redaction-rules.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as RedactionReport
  const checks = report.checks ?? {}
  const row = ROUTES.find((route) => route.id === 'R-REDACTION-RULES')
  const requiredScenarios = [
    'route-render',
    'pagination',
    'dialog-open',
    'mutation-toggle',
    'mutation-update',
    'mutation-delete',
    'mutation-failure-draft',
  ]

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('redaction-rules-'),
    'report is not from the Redaction Rules runner',
  )
  const runTimestamp = Number(report.runId?.slice('redaction-rules-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'Redaction Rules report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'Redaction Rules report is stale or has invalid run timestamps',
  )
  assert(
    row?.foundationTest === 'e2e/foundation/redaction-rules.playwright' &&
      requiredScenarios.every((scenario) =>
        row.requiredScenarios.includes(scenario as never),
      ),
    'R-REDACTION-RULES is not mapped to its current focused route and scenario contract',
  )

  assert(
    checks.routeAndInitialReads === true && checks.initialPage === true,
    'authenticated route or initial settings/page reads were not observed',
  )
  assert(
    checks.validationSearchPayload?.serverSearch === true &&
      checks.validationSearchPayload?.exactPageQuery === true &&
      checks.validationSearchPayload?.distinctRows === true,
    'server search, query payload, or distinct paginated rows were not verified',
  )
  assert(
    checks.pagination?.secondPageRead === true &&
      checks.pagination?.distinctVisibleRows === true &&
      checks.pagination?.emptyOutOfRangeCorrected === true &&
      checks.pageSizePersistence === true,
    'pagination, page-size persistence, or empty out-of-range correction is incomplete',
  )
  assert(
    checks.dirtyLocalSearch === true &&
      checks.warningPriority === true &&
      checks.builtinToggleCustomDirty === true,
    'dirty local search, built-in/custom dirtiness, or duplicate/conflict warnings are missing',
  )

  const normalization = checks.normalization
  assert(
    normalization?.trimmed === true &&
      normalization?.emptyDropped === true &&
      normalization?.exactSignatureDeduped === true &&
      normalization?.conflictRetained === true &&
      checks.builtinAndEnabledPayload === true,
    'normalized save payload or built-in/enabled payload contract is incomplete',
  )
  assert(
    checks.failedSaveRetainsDraft === true && checks.retrySuccess === true,
    '503 retry did not preserve the normalized draft and complete a successful retry',
  )
  assert(
    checks.saveGuardAndNewerEdit?.onePatch === true &&
      checks.saveGuardAndNewerEdit?.saveDisabledInFlight === true &&
      checks.saveGuardAndNewerEdit?.newerEditPreserved === true &&
      checks.saveGuardAndNewerEdit?.requestSnapshotWasOlder === true,
    'save guard or in-flight newer-edit preservation is incomplete',
  )
  assert(
    checks.deleteConfirmation?.cancelNoWrite === true &&
      checks.deleteConfirmation?.confirmedLocalDelete === true &&
      checks.deleteConfirmation?.revealIndexShift === true &&
      checks.deleteConfirmation?.nativeConfirmUsed === false,
    'custom delete confirmation, index-shift reveal, or local-only delete evidence is missing',
  )
  assert(
    checks.invalidRegex?.localErrorNoPatch === true &&
      checks.invalidRegex?.correctedPatch === true,
    'invalid regex was not blocked locally before a corrected PATCH',
  )
  assert(
    checks.adminScopeIsolation?.userTargetRead === true &&
      checks.adminScopeIsolation?.userScopedPatch === true &&
      checks.adminScopeIsolation?.globalUnchanged === true,
    'admin target-user configuration was not isolated from global configuration',
  )
  assert(
    checks.roleAuthorization?.nonAdminSelfOnly === true &&
      checks.roleAuthorization?.selfScopedPatch === true &&
      checks.roleAuthorization?.optionsNotRequested === true &&
      checks.roleAuthorization?.anonymous401 === true &&
      checks.roleAuthorization?.anonymousRequestRecords401 === true &&
      checks.roleAuthorization?.globalAndForeign403 === true &&
      checks.roleAuthorization?.unrelatedRequestRecordsStatus === 200 &&
      checks.roleAuthorization?.requestRecordsSelfScoped === true &&
      checks.roleAuthorization?.requestRecordsForgedUserNoWiden === true &&
      checks.roleAuthorization?.unknownRedaction404 === true,
    'authenticated self scope, admin option isolation, or fixture authorization checks failed',
  )

  const responsive = checks.responsiveContainment as
    | Array<{
        locale?: string
        theme?: string
        width?: number
        documentWidth?: number
        viewportWidth?: number
        tableScrolls?: boolean
        actionsReachable?: boolean
        hasRows?: boolean
      }>
    | undefined
  const expectedProfiles = ['en-US:light', 'zh-CN:dark'].flatMap((profile) =>
    [1440, 1024, 768, 390].map((width) => ({ profile, width })),
  )
  assert(
    responsive?.length === expectedProfiles.length,
    'responsive report does not cover both locale/theme profiles at four widths',
  )
  for (const { profile, width } of expectedProfiles) {
    const [locale, theme] = profile.split(':')
    const result = responsive?.find(
      (item) =>
        item.locale === locale && item.theme === theme && item.width === width,
    )
    assert(
      result?.documentWidth === width &&
        result.viewportWidth === width &&
        result.actionsReachable === true &&
        result.hasRows === true &&
        (width !== 390 || result.tableScrolls === true),
      `${profile} responsive containment or action reachability failed at ${width}px`,
    )
  }
  assert(
    checks.deleteDialogResponsive?.fitsViewport === true &&
      checks.deleteDialogResponsive?.actionCount >= 2,
    'responsive delete dialog did not keep both actions reachable',
  )

  assert(
    Array.isArray(report.expectedApiFailures) &&
      report.expectedApiFailures.includes(
        '503 PATCH /api/v1/settings/redaction',
      ) &&
      report.expectedApiFailures.some((failure) =>
        failure.startsWith('401 GET /api/v1/settings/redaction'),
      ) &&
      report.expectedApiFailures.filter((failure) =>
        failure.startsWith('403 GET /api/v1/settings/redaction'),
      ).length >= 2 &&
      report.expectedApiFailures.includes(
        '401 GET /api/v1/admin/request-records',
      ) &&
      report.expectedApiFailures.includes(
        '404 GET /api/v1/settings/redaction/unknown',
      ) &&
      !report.expectedApiFailures.includes(
        '403 GET /api/v1/admin/request-records',
      ) &&
      Array.isArray(report.apiFailures) &&
      report.apiFailures.length === 0,
    'expected save/auth failures or unexpected API responses were not recorded accurately',
  )
  assert(report.consoleErrorCount === 0, 'browser console errors were reported')
  assert(report.pageErrorCount === 0, 'browser page errors were reported')

  for (const screenshot of report.screenshots ?? []) {
    assert(
      existsSync(join(root, screenshot.replace(/^e2e\/foundation\//, ''))),
      `missing screenshot: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.screenshots) && report.screenshots.length >= 2,
    'Redaction Rules screenshots are missing',
  )
  const serialized = readFileSync(reportPath, 'utf8')
  for (const value of [
    'fixture-secret-10',
    'redaction-e2e-normalized',
    'redaction-e2e-newer-draft',
    'private-fixture-3',
  ]) {
    assert(
      !serialized.includes(value),
      'report contains a redaction pattern or search snippet',
    )
  }
}

if (failures.length > 0) {
  console.error('Redaction Rules verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Redaction Rules focused browser verification passed.')
