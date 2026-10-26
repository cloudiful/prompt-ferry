import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type BillingReport = {
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
const reportPath = join(root, 'billing-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

assert(
  existsSync(reportPath),
  'focused Billing report is missing; run run-billing.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as BillingReport
  const checks = report.checks ?? {}
  const row = ROUTES.find((route) => route.id === 'R-BILLING')
  const requiredScenarios = [
    'route-render',
    'pagination',
    'dialog-open',
    'mutation-create',
    'mutation-toggle',
    'mutation-delete',
    'mutation-update',
    'mutation-conflict-guard',
    'file-export',
    'long-values',
    'responsive-width',
  ]

  assert(
    typeof report.runId === 'string' && report.runId.startsWith('billing-'),
    'report is not from the Billing runner',
  )
  const runTimestamp = Number(report.runId?.slice('billing-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'Billing report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'Billing report is stale or has invalid run timestamps',
  )
  assert(
    row?.foundationTest === 'e2e/foundation/billing.playwright' &&
      requiredScenarios.every((scenario) =>
        row.requiredScenarios.includes(scenario as never),
      ),
    'R-BILLING is not mapped to its current focused route and scenario contract',
  )

  assert(
    checks.routeRender?.routeId === true &&
      checks.routeRender?.summary === true &&
      checks.routeRender?.charges === true &&
      checks.routeRender?.priceRules === true &&
      checks.routeRender?.noPlaceholder === true,
    'the billing route, summary, charges, or price rules did not render',
  )
  assert(
    checks.initialReads?.summary === 1 &&
      checks.initialReads?.charges === 1 &&
      checks.initialReads?.priceRules === 1,
    'the initial summary, charges, or price rule reads are incomplete',
  )
  const pagers = checks.independentPagers
  assert(
    pagers?.rulePageChangedQuery === true &&
      pagers?.ruleRowsDistinct === true &&
      pagers?.rulePagerNoChargeFetch === true &&
      pagers?.chargePageChangedQuery === true &&
      pagers?.chargeRowsUnchanged === true &&
      pagers?.chargePagerNoRuleFetch === true &&
      pagers?.pageSizesPersisted === true,
    'the independent charge and price rule pagers are not verified',
  )
  assert(
    checks.filters?.periodAutoApply === true &&
      checks.filters?.periodResetsPage === true &&
      checks.filters?.summaryAndChargesOnly === true &&
      checks.filters?.modelFilter === true &&
      checks.filters?.periodPreserved === true,
    'the billing filter and period behavior is incomplete',
  )
  assert(
    checks.chargeDetail?.opened === true && checks.chargeDetail?.lines === true,
    'the charge detail did not open with its lines',
  )
  assert(
    checks.ruleCreate?.payload === true &&
      checks.ruleCreate?.localEffectiveFrom === true &&
      checks.ruleCreate?.dialogClosed === true,
    'the price rule create payload, local time, or dialog close is incomplete',
  )
  assert(
    checks.ruleFailureRetry?.draftRetained === true &&
      checks.ruleFailureRetry?.errorVisible === true &&
      checks.ruleFailureRetry?.retrySucceeded === true,
    'the failed price rule save did not retain its draft and retry',
  )
  assert(
    checks.ruleConflictGuard?.singleWrite === true,
    'the price rule guard issued more than one write',
  )
  assert(
    checks.ruleToggleAndDelete?.togglePayload === true &&
      checks.ruleToggleAndDelete?.longModelRendered === true &&
      checks.ruleToggleAndDelete?.deleteCancelled === true &&
      checks.ruleToggleAndDelete?.deleteConfirmed === true,
    'the price rule toggle, long value, or delete confirmation is incomplete',
  )
  assert(
    checks.reprice?.requested === true &&
      checks.reprice?.countReported === true,
    'the reprice request or its reported count is missing',
  )
  const exports = checks.csvExports
  assert(
    exports?.detailsFileName === true &&
      exports?.detailsBom === true &&
      exports?.detailsContent === true &&
      exports?.monthlyFileName === true &&
      exports?.monthlyBom === true &&
      exports?.monthlyContent === true,
    'the CSV exports did not verify the file names, byte order marks, or content',
  )
  assert(
    checks.csvFailure?.errorVisible === true &&
      checks.csvFailure?.noDownload === true,
    'a failed export did not report the error without a download',
  )
  assert(
    checks.anonymousScope?.summary401 === true &&
      checks.anonymousScope?.charges401 === true &&
      checks.anonymousScope?.priceRules401 === true,
    'the billing endpoints did not reject an anonymous caller',
  )
  assert(
    checks.ruleEdit?.dialogPopulated === true &&
      checks.ruleEdit?.localEffectiveFrom === true &&
      checks.ruleEdit?.putPath === true &&
      checks.ruleEdit?.payloadKeys === true &&
      checks.ruleEdit?.payloadValues === true &&
      checks.ruleEdit?.refreshRulesAndSummary === true &&
      checks.ruleEdit?.noChargesRefresh === true &&
      checks.ruleEdit?.dialogClosed === true &&
      checks.ruleEdit?.reopenedValues === true,
    'the price rule edit did not use the stored path, payload, or refresh set',
  )
  assert(
    checks.filters?.userFilter === true &&
      checks.filters?.clientKeyFilter === true &&
      checks.filters?.endpointFilter === true &&
      checks.filters?.usageFilter === true &&
      checks.filters?.pricingFilter === true &&
      checks.filters?.clearedFiltersOmitted === true &&
      checks.filters?.truthfulRows === true &&
      checks.filters?.summaryMatchesCharges === true &&
      checks.filters?.exportUsesFilters === true &&
      checks.filters?.routeRequestIdPreserved === true,
    'the billing filter controls, omissions, rows, summary, export, or route request id are incomplete',
  )
  assert(
    checks.detailLatestWins?.newerDetailKept === true &&
      checks.detailLatestWins?.staleDetailNotShown === true &&
      checks.detailLatestWins?.failedDetailVisible === true &&
      checks.detailLatestWins?.failedDetailClosed === true,
    'the charge detail did not keep the latest target or report the failed read',
  )
  assert(
    checks.listLatestWins?.newerRowsKept === true &&
      checks.listLatestWins?.staleRowsRejected === true &&
      checks.listLatestWins?.failureKeepsRows === true &&
      checks.listLatestWins?.loadingCleared === true,
    'the charge list did not keep the latest rows or the failed read state',
  )
  assert(
    checks.emptyTail?.chargeTailCorrected === true &&
      checks.emptyTail?.chargeTailRowsDistinct === true &&
      checks.emptyTail?.chargeTailPagerUnchanged === true &&
      checks.emptyTail?.ruleTailCorrected === true &&
      checks.emptyTail?.ruleTailRowsDistinct === true &&
      checks.emptyTail?.ruleTailPagerUnchanged === true,
    'the empty tail page was not corrected to the last valid page on both pagers',
  )
  assert(
    checks.csvExportFilterState?.statePreserved === true &&
      checks.csvExportFilterState?.bootUnchanged === true &&
      checks.csvExportFilterState?.authMeUnchanged === true &&
      checks.csvExportFilterState?.noBootstrapBurst === true &&
      checks.csvExportFilterState?.secondExportUsesFilters === true,
    'the CSV export did not preserve the applied filters, the document, or the session',
  )
  assert(
    checks.nonAdminScope?.selfSummary === true &&
      checks.nonAdminScope?.noAdminReads === true &&
      checks.nonAdminScope?.noAdminControls === true &&
      checks.nonAdminScope?.foreignDetail404 === true &&
      checks.nonAdminScope?.priceRuleWrite403 === true,
    'the ordinary user billing scope or authorization checks failed',
  )

  const responsive = checks.responsive as
    | Array<{
        locale?: string
        theme?: string
        width?: number
        documentWidth?: number
        viewportWidth?: number
        headerVisible?: boolean
        actionReachable?: boolean
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
        result.headerVisible === true &&
        result.actionReachable === true,
      `${profile} billing containment failed at ${width}px`,
    )
  }

  assert(
    Array.isArray(report.expectedApiFailures) &&
      report.expectedApiFailures.includes(
        '503 POST /api/v1/admin/billing/price-rules',
      ) &&
      report.expectedApiFailures.includes('404 GET billing charge detail') &&
      report.expectedApiFailures.includes('403 GET billing price rules') &&
      report.expectedApiFailures.includes(
        '403 POST /api/v1/admin/billing/reprice-unpriced',
      ) &&
      report.expectedApiFailures.includes('503 GET billing export') &&
      report.expectedApiFailures.includes(
        '500 GET /api/v1/admin/billing/charges/:id',
      ) &&
      report.expectedApiFailures.includes(
        '500 GET /api/v1/admin/billing/charges',
      ) &&
      Array.isArray(report.apiFailures) &&
      report.apiFailures.length === 0,
    'expected billing failures or unexpected API responses were not recorded accurately',
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
    'Billing screenshots are missing',
  )
}

if (failures.length > 0) {
  console.error('Billing verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Billing focused browser verification passed.')
