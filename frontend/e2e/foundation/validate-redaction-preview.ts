import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type PreviewReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  screenshots?: string[]
  expectedApiFailures?: string[]
  apiFailures?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
  previewRequestCount?: number
}

const root = import.meta.dir
const reportPath = join(root, 'redaction-preview-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

assert(
  existsSync(reportPath),
  'focused Redaction Preview report is missing; run run-redaction-preview.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as PreviewReport
  const checks = report.checks ?? {}
  const row = ROUTES.find((route) => route.id === 'R-REDACTION-PREVIEW')
  const requiredScenarios = [
    'route-render',
    'mutation-update',
    'stale-response',
    'mutation-conflict-guard',
  ]

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('redaction-preview-'),
    'report is not from the Redaction Preview runner',
  )
  const runTimestamp = Number(report.runId?.slice('redaction-preview-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'Redaction Preview report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'Redaction Preview report is stale or has invalid run timestamps',
  )
  assert(
    row?.foundationTest === 'e2e/foundation/redaction-preview.playwright' &&
      requiredScenarios.every((scenario) =>
        row.requiredScenarios.includes(scenario as never),
      ),
    'R-REDACTION-PREVIEW is not mapped to its current focused route and scenario contract',
  )

  assert(
    checks.routeRender?.routeId === true &&
      checks.routeRender?.emptyDetails === true &&
      checks.routeRender?.noPlaceholder === true &&
      (checks.routeRender?.inputKindOptions ?? 0) === 1,
    'the preview route, input kind control, or empty details were not rendered',
  )
  assert(
    checks.success?.requestSnapshot === true &&
      checks.success?.outputMaskedEmail === true &&
      checks.success?.outputMaskedSecret === true &&
      checks.success?.statsMatched === true &&
      checks.success?.findingRows === 2,
    'the successful preview request snapshot, masked output, or statistics are incomplete',
  )
  assert(
    checks.selection?.outputHighlighted === true &&
      checks.selection?.inputHighlighted === true &&
      checks.selection?.mobilePaneSwitched === true,
    'finding selection did not highlight the panes or switch the active pane',
  )
  assert(
    checks.copyOutput?.clipboardWritten === true &&
      checks.copyOutput?.copiedFeedback === true,
    'the copy action did not write the redacted output or report the copied state',
  )
  assert(
    checks.staleResponse?.singleRequest === true &&
      checks.staleResponse?.lateResultDiscarded === true &&
      checks.staleResponse?.noHighlight === true &&
      checks.staleRetry?.retrySucceeded === true,
    'the stale preview response was not discarded, guarded, or retried',
  )
  assert(
    checks.conflict?.singleRequest === true &&
      checks.conflict?.errorVisible === true &&
      checks.conflict?.resultCleared === true &&
      checks.conflictRetry?.retrySucceeded === true,
    'the failing preview did not clear the result, guard the request, or allow a retry',
  )
  assert(
    checks.sharedDraft?.draftPreserved === true &&
      checks.sharedDraft?.noRefetch === true &&
      checks.sharedDraft?.customSignatureSent === true &&
      checks.sharedDraft?.builtinToggleSent === true,
    'the unsaved rules draft was not preserved across the preview route',
  )
  assert(
    checks.inputKindBoundary?.gitDiffSent === true &&
      checks.inputKindBoundary?.customSignatureSent === true,
    'the git diff input kind or custom signature was not carried in the request',
  )
  assert(
    checks.disabledPreview?.textUnchanged === true &&
      checks.disabledPreview?.noFindings === true,
    'a disabled preview still redacted the input',
  )

  const responsive = checks.responsivePanes as
    | Array<{
        locale?: string
        theme?: string
        width?: number
        documentWidth?: number
        viewportWidth?: number
        outputVisible?: boolean
        inputVisible?: boolean
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
      result?.documentWidth === width && result.viewportWidth === width,
      `${profile} preview containment failed at ${width}px`,
    )
    if (width >= 768) {
      assert(
        result?.outputVisible === true && result?.inputVisible === true,
        `${profile} did not show both preview panes at ${width}px`,
      )
    }
  }
  assert(
    checks.mobilePaneSwitch?.outputVisible === true &&
      checks.mobilePaneSwitch?.inputHidden === true &&
      checks.mobilePaneSwitch?.inputRestored === true,
    'the mobile preview pane did not switch between the input and the output',
  )

  assert(
    Array.isArray(report.expectedApiFailures) &&
      report.expectedApiFailures.includes(
        '503 POST /api/v1/settings/redaction/preview',
      ) &&
      Array.isArray(report.apiFailures) &&
      report.apiFailures.length === 0,
    'expected preview failures or unexpected API responses were not recorded accurately',
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
    'Redaction Preview screenshots are missing',
  )
  const serialized = readFileSync(reportPath, 'utf8')
  for (const value of [
    'mail@fixture.invalid',
    'sk_fixture_preview0001',
    'redaction-preview-e2e-unsaved',
  ]) {
    assert(
      !serialized.includes(value),
      'report contains raw preview text, a rule pattern, or a secret',
    )
  }
}

if (failures.length > 0) {
  console.error('Redaction Preview verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Redaction Preview focused browser verification passed.')
