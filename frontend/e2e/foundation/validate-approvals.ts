import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ApprovalsReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  stage?: string
  error?: string
  checks?: Record<string, any>
  screenshots?: string[]
  expectedApiFailures?: string[]
  apiFailures?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
}

const root = import.meta.dir
const reportPath = join(root, 'approvals-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(join(root, '..', '..', path), 'utf8')
}

assert(
  existsSync(reportPath),
  'focused Approvals report is missing; run run-approvals.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as ApprovalsReport
  const checks = report.checks ?? {}
  const row = ROUTES.find((route) => route.id === 'R-APPROVALS')
  const requiredScenarios = [
    'route-render',
    'admin-denial',
    'pagination',
    'dialog-open',
    'mutation-update',
    'mutation-conflict-guard',
    'error-state',
  ]

  assert(
    report.stage === undefined && report.error === undefined,
    `the focused Approvals callback failed at ${report.stage}: ${report.error}`,
  )
  assert(
    typeof report.runId === 'string' && report.runId.startsWith('approvals-'),
    'report is not from the Approvals runner',
  )
  const runTimestamp = Number(report.runId?.slice('approvals-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'Approvals report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'Approvals report is stale or has invalid run timestamps',
  )
  assert(
    row?.foundationTest === 'e2e/foundation/approvals.playwright' &&
      requiredScenarios.every((scenario) =>
        row.requiredScenarios.includes(scenario as never),
      ) &&
      row.mutationKeys.includes('approval:<id>'),
    'R-APPROVALS is not mapped to its current focused route, scenario contract, and shared key',
  )

  assert(
    checks.routeRender?.routeId === true &&
      checks.routeRender?.noPlaceholder === true &&
      checks.routeRender?.pendingDefault === true &&
      checks.routeRender?.singleRead === true &&
      checks.routeRender?.rowCount === 10 &&
      checks.routeRender?.remainingWait === true,
    'the approvals route did not render the pending first page',
  )
  assert(
    checks.pagination?.secondPageRead === true &&
      checks.pagination?.secondPageRows === 1 &&
      checks.pagination?.pageSizeRead === true &&
      checks.pagination?.range === true &&
      checks.pagination?.mobilePagerOnlyWhenOverflowing === true &&
      Array.isArray(checks.pagination?.pageSizeOptions) &&
      checks.pagination.pageSizeOptions.join(',') === '10,25,50',
    'the approvals pager, page sizes, or mobile pager rule are not verified',
  )
  assert(
    checks.filter?.resolvedRead === true &&
      checks.filter?.resolvedRows === 4 &&
      checks.filter?.resolvedHasNoAnswers === true &&
      checks.filter?.resolvedRemainingDash === true &&
      checks.filter?.pendingResetsFirst === true,
    'the pending/resolved filter, its answers, or its first-row reset are not verified',
  )
  assert(
    checks.dialogOpen?.detailRead === true &&
      checks.dialogOpen?.showsUserKeyModel === true &&
      checks.dialogOpen?.showsCategories === true &&
      checks.dialogOpen?.showsPreview === true &&
      checks.dialogOpen?.showsPayload === true &&
      checks.dialogOpen?.hidesForbiddenFields === true &&
      checks.dialogOpen?.hidesEmptyPayload === true,
    'the approval detail fields, its successful read, or its empty payload rule are not verified',
  )
  assert(
    checks.lastPageCorrection?.emptyPageRead === true &&
      checks.lastPageCorrection?.correctedRead === true &&
      checks.lastPageCorrection?.correctedRows === 10,
    'the empty last page was not corrected to the last page with rows',
  )
  assert(
    checks.mutationApprove?.singleWrite === true &&
      checks.mutationApprove?.noBody === true &&
      checks.mutationApprove?.listRefreshed === true &&
      checks.mutationApprove?.detailReloaded === true &&
      checks.mutationApprove?.toast === true &&
      checks.mutationApprove?.leftPendingList === true,
    'the approve path did not write once, refresh the list, reload the detail, and leave the pending page',
  )
  assert(
    checks.mutationReject?.singleWrite === true &&
      checks.mutationReject?.toast === true &&
      checks.mutationReject?.leftPendingList === true &&
      checks.mutationReject?.movedToResolved === true,
    'the reject path did not write once, refresh the list, and move the request to resolved',
  )
  assert(
    checks.conflictGuard?.singleWrite === true &&
      checks.conflictGuard?.ownRowDisabled === true &&
      checks.conflictGuard?.otherRowEnabled === true,
    'the delayed double trigger or the per-row answer gate is not verified',
  )
  assert(
    checks.modalGuard?.cancelDisabled === true &&
      checks.modalGuard?.dismissalBlocked === true,
    'the answered detail dialog could still be dismissed while the answer was in flight',
  )
  assert(
    checks.errorState?.listFailureClearsRows === true &&
      checks.errorState?.listRetryRecovers === true &&
      checks.errorState?.detailFailureKeepsDialogClosed === true &&
      checks.errorState?.conflictRejected === true &&
      checks.errorState?.conflictNoRefresh === true &&
      checks.errorState?.writeFailureNoRefresh === true,
    'the list, detail, conflict, or write error states are not verified',
  )
  assert(
    checks.latestWins?.listKeepsNewest === true &&
      checks.latestWins?.detailKeepsNewest === true,
    'the newer list or detail read did not win over a stale one',
  )
  assert(
    checks.timerCleanup?.tickerRan === true &&
      checks.timerCleanup?.clearedOnLeave === true,
    'the remaining wait ticker did not run or was not cleared on leave',
  )
  assert(
    checks.adminDenial?.redirected === true &&
      checks.adminDenial?.noApprovalReads === true &&
      checks.adminDenial?.forged403 === true &&
      checks.anonymousScope?.list401 === true,
    'the ordinary-user denial or the anonymous rejection is not verified',
  )

  const responsive = Array.isArray(checks.responsive) ? checks.responsive : []
  for (const profile of [
    { locale: 'en-US', theme: 'light' },
    { locale: 'zh-CN', theme: 'dark' },
  ]) {
    for (const width of [1440, 1024, 768, 390]) {
      const result = responsive.find(
        (entry: any) =>
          entry.locale === profile.locale &&
          entry.theme === profile.theme &&
          entry.width === width,
      )
      assert(
        result &&
          result.viewportWidth === width &&
          result.documentWidth <= width &&
          result.desktopVisible === width >= 768 &&
          result.mobileVisible === width < 768 &&
          result.desktopPagerVisible === width >= 768 &&
          result.mobilePagerVisible === width < 768 &&
          (width >= 768 || result.mobileCardHasAnswer === true),
        `${profile.locale}/${profile.theme} approvals containment failed at ${width}px`,
      )
    }
  }

  assert(
    Array.isArray(report.expectedApiFailures) &&
      report.expectedApiFailures.includes('503 GET /api/v1/admin/approvals') &&
      report.expectedApiFailures.includes(
        '404 GET /api/v1/admin/approvals/:id',
      ) &&
      report.expectedApiFailures.includes(
        '409 POST /api/v1/admin/approvals/:id/approve',
      ) &&
      report.expectedApiFailures.includes(
        '503 POST /api/v1/admin/approvals/:id/approve',
      ) &&
      report.expectedApiFailures.includes('403 GET /api/v1/admin/approvals') &&
      report.expectedApiFailures.includes('401 GET /api/v1/admin/approvals') &&
      Array.isArray(report.apiFailures) &&
      report.apiFailures.length === 0,
    'expected approval failures or unexpected API responses were not recorded accurately',
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
    Array.isArray(report.screenshots) && report.screenshots.length >= 4,
    'Approvals screenshots are missing',
  )
}

const app = read('src/App.tsx')
assert(
  app.includes("case 'R-APPROVALS'") &&
    app.includes('<AdminGuard>') &&
    app.includes('<ApprovalsPage />'),
  'React Approvals page is not mounted behind AdminGuard',
)

const authShell = read('e2e/foundation/auth-shell.playwright')
assert(
  !authShell.includes("['/approvals', 'R-APPROVALS']"),
  'the ported approvals route remains in the shared placeholder list',
)

if (failures.length > 0) {
  console.error('Approvals verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Approvals focused browser verification passed.')
