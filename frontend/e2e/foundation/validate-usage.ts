import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type UsageReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  scenarios?: Record<string, Record<string, ScenarioEntry>>
  screenshots?: string[]
  expectedApiFailures?: string[]
  apiFailures?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
  widthChecks?: Array<{
    locale?: string
    viewport?: number
    clientWidth?: number
    expectedWidth?: number
    intrinsicWidth?: number
    rectLeft?: number
    rectRight?: number
    scrollerClientWidth?: number
    scrollerScrollWidth?: number
    documentOverflow?: boolean
  }>
  headerChecks?: Array<{
    locale?: string
    viewport?: number
    headers?: number
    overlaps?: unknown[]
    pinned?: boolean
    scrollWidth?: number
  }>
  foldChecks?: Record<string, unknown> | null
  sortChecks?: Record<string, unknown> | null
  detailChecks?: Record<string, unknown> | null
  sessionChecks?: Record<string, unknown> | null
  clearChecks?: Record<string, unknown> | null
  roleChecks?: Record<string, unknown> | null
  limitations?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'usage-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

const AI_SCENARIOS = [
  'route-render',
  'empty-state',
  'loading-state',
  'error-state',
  'pagination',
  'stale-response',
  'dialog-open',
  'mutation-update',
  'destructive-confirm',
  'zero-token-fold',
  'long-values',
  'locale-switch',
  'responsive-width',
]

// The parity matrix still declares the route scenarios above; the browser run
// additionally verifies the AI request column geometry.
const AI_RUN_SCENARIOS = [...AI_SCENARIOS, 'column-geometry']

const MCP_SCENARIOS = [
  'route-render',
  'empty-state',
  'pagination',
  'dialog-open',
  'mutation-update',
  'destructive-confirm',
]

const EXPECTED_API_FAILURES = [
  '503 GET /api/v1/admin/request-records/overview',
  '404 GET /api/v1/admin/request-records/10',
  '403 PUT /api/v1/admin/conversations/conversation-1/endpoint-override',
  '403 DELETE /api/v1/admin/conversations/conversation-1/endpoint-override',
  '403 POST /api/v1/admin/request-records/1/reset-session-affinity',
  '400 PUT /api/v1/admin/conversations/conversation-1/endpoint-override',
  '400 POST /api/v1/admin/request-records/14/reset-session-affinity',
  '503 POST /api/v1/admin/request-records/clear',
  '400 POST /api/v1/admin/request-records/clear',
]

const REQUIRED_SCREENSHOTS = [
  'e2e/foundation/screenshots/usage-ai-overview-en-light-1440.png',
  'e2e/foundation/screenshots/usage-fold-en-light-1440-collapsed.png',
  'e2e/foundation/screenshots/usage-fold-en-light-1440-expanded.png',
  'e2e/foundation/screenshots/usage-fold-upstream-en-light-1440-expanded.png',
  'e2e/foundation/screenshots/usage-fold-unknown-probe-en-light-1440.png',
  'e2e/foundation/screenshots/usage-fold-en-light-390.png',
  'e2e/foundation/screenshots/usage-fold-zh-dark-1440-collapsed.png',
  'e2e/foundation/screenshots/usage-fold-zh-dark-1440.png',
  'e2e/foundation/screenshots/usage-fold-zh-dark-1440-expanded.png',
  'e2e/foundation/screenshots/usage-fold-zh-dark-390.png',
  'e2e/foundation/screenshots/usage-detail-messages-en-light-1440.png',
  'e2e/foundation/screenshots/usage-detail-parent-session-en-light-1440.png',
  'e2e/foundation/screenshots/usage-detail-error-en-light-1440.png',
  'e2e/foundation/screenshots/usage-mcp-records-en-light-1440.png',
  'e2e/foundation/screenshots/usage-mcp-detail-en-light-1440.png',
]

assert(
  existsSync(reportPath),
  'focused usage report is missing; run run-usage.sh',
)

const aiRow = ROUTES.find((route) => route.id === 'R-USAGE-AI')
const mcpRow = ROUTES.find((route) => route.id === 'R-USAGE-MCP')

assert(
  aiRow?.foundationTest === 'e2e/foundation/usage.playwright' &&
    mcpRow?.foundationTest === 'e2e/foundation/usage.playwright',
  'the usage routes are not mapped to the focused usage browser run',
)
assert(
  JSON.stringify([...(aiRow?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...AI_SCENARIOS].sort()),
  'R-USAGE-AI no longer requires the verified scenario set',
)
assert(
  JSON.stringify([...(mcpRow?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...MCP_SCENARIOS].sort()),
  'R-USAGE-MCP no longer requires the verified scenario set',
)
assert(
  aiRow?.status === 'baseline-partial',
  'R-USAGE-AI must stay partial while the fixture gaps remain open',
)
assert(
  !aiRow?.reads.includes(
    'GET /api/v1/admin/conversations/{id}/endpoint-override',
  ),
  'R-USAGE-AI still claims the unused conversation override read',
)
assert(
  aiRow?.reads.includes('GET /api/v1/admin/users/options') &&
    mcpRow?.reads.includes('GET /api/v1/admin/users/options'),
  'the admin user-options read is missing from the usage rows',
)
assert(
  aiRow?.mutationKeys.includes('conversation:<id>') &&
    aiRow?.mutationKeys.includes('record:<id>') &&
    aiRow?.mutationKeys.includes('usage:maintenance') &&
    aiRow?.mutationKeys.includes('usage:perspective'),
  'the AI usage write keys are incomplete',
)
assert(
  mcpRow?.mutationKeys.includes('usage:maintenance'),
  'the MCP usage write keys are incomplete',
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
      !pendingIds.includes('R-USAGE-AI') &&
      !pendingIds.includes('R-USAGE-MCP') &&
      pendingIds.every((id) => id.startsWith('R-SETTINGS-')) &&
      !pendingIds.includes('R-SETTINGS-GENERAL'),
    'the shared placeholder list does not hold exactly the 0 remaining settings routes',
  )
}

if (existsSync(reportPath)) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as UsageReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}

  assert(
    typeof report.runId === 'string' && report.runId.startsWith('usage-'),
    'report is not from the usage runner',
  )
  const runTimestamp = Number(report.runId?.slice('usage-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'usage report runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'usage report is stale or has invalid run timestamps',
  )

  assert(
    JSON.stringify(Object.keys(scenarios['R-USAGE-AI'] ?? {}).sort()) ===
      JSON.stringify([...AI_RUN_SCENARIOS].sort()),
    'the AI route does not carry exactly the 14 verified scenarios',
  )
  assert(
    JSON.stringify(Object.keys(scenarios['R-USAGE-MCP'] ?? {}).sort()) ===
      JSON.stringify([...MCP_SCENARIOS].sort()),
    'the MCP route does not carry exactly the 6 verified scenarios',
  )
  for (const [rowId, expected] of [
    ['R-USAGE-AI', AI_RUN_SCENARIOS],
    ['R-USAGE-MCP', MCP_SCENARIOS],
  ] as const) {
    for (const scenario of expected) {
      const entry = scenarios[rowId]?.[scenario]
      assert(
        entry?.passed === true && Boolean(entry?.evidence),
        `${rowId}:${scenario} has no passing current-run assertion`,
      )
    }
  }

  assert(
    checks.aiRouteRender?.cards === 10 &&
      checks.aiRouteRender?.chartCanvas === true &&
      checks.aiRouteRender?.distributionRows === true,
    'the AI overview did not render its cards, chart, and distribution',
  )
  assert(
    checks.drilldown?.mode === 'records' &&
      typeof checks.drilldown?.endpoint === 'string' &&
      checks.drilldown?.first === '0',
    'the distribution drilldown did not open the records table with the endpoint filter',
  )
  assert(
    checks.modeLocal?.requests === 0,
    'switching the view mode issued a request',
  )
  assert(
    checks.pagination?.pageTwo === 10 &&
      checks.pagination?.persistedRows === 10,
    'the AI pagination did not change the row set or persist the page size',
  )
  assert(
    checks.filters?.searchEnterOnly === true &&
      checks.filters?.clientKey === true &&
      checks.filters?.state === true,
    'the AI search and filter behavior is incomplete',
  )
  assert(
    checks.advancedFiltersDefault?.collapsed === true &&
      checks.advancedFiltersDefault?.mounted === true,
    'the advanced filters are not mounted and collapsed by default',
  )
  assert(
    checks.advancedFilters?.collapsed === true &&
      checks.advancedFilters?.mounted === true &&
      checks.advancedFilters?.retainedDraft === true &&
      checks.advancedFilters?.retainedFilter === true &&
      checks.advancedFilters?.activeCount >= 2 &&
      checks.advancedFilters?.noRequest === true,
    'collapsing the advanced filters did not keep the draft, filter, or active indicator',
  )
  assert(
    checks.navigation?.desktop?.minLinkHeight >= 36 &&
      checks.navigation?.mobile?.minLinkHeight >= 36 &&
      checks.navigation?.desktop?.nestedRowGap <= 4 &&
      checks.navigation?.mobile?.nestedRowGap <= 4,
    'the navigation is not compact while keeping its touch height',
  )
  assert(
    checks.range?.preset === '7d' && checks.range?.custom === 'custom',
    'the AI range presets and custom window are incomplete',
  )
  assert(
    checks.loadingState?.busy >= 1,
    'no loading state was observed during a slow read',
  )
  assert(
    checks.errorRetry === true && checks.staleRead === true,
    'the error retry or the stale read suppression was not verified',
  )
  assert(checks.aiEmpty === true, 'the AI empty state was not verified')

  const fold = (report.foldChecks ?? {}) as Record<string, any>
  const foldVariants = [
    'positive',
    'success-zero',
    'mixed-60%-zero',
    'all-failed-zero',
  ]
  for (const perspective of ['model', 'upstream']) {
    const matrix = fold[perspective] ?? {}
    assert(
      matrix.collapsedRows === 1 &&
        matrix.expandedRows === 4 &&
        matrix.hiddenRequestCount === 8 &&
        JSON.stringify(matrix.variants) === JSON.stringify(foldVariants) &&
        matrix.visibleFirst === true &&
        matrix.literalZero === true &&
        matrix.noRequestOnExpand === true &&
        matrix.totalsUnchanged === true,
      `the ${perspective}-perspective zero-token fold matrix is incomplete or false`,
    )
  }
  for (const locale of ['zh', 'zhUpstream']) {
    const matrix = fold[locale] ?? {}
    assert(
      matrix.collapsedRows === 1 &&
        matrix.expandedRows === 4 &&
        matrix.hiddenRequestCount === 8 &&
        matrix.literalZero === true &&
        JSON.stringify(matrix.rates) === JSON.stringify(['0%', '60%', '100%']),
      `the Chinese ${locale} fold matrix is incomplete or false`,
    )
  }
  assert(
    checks.foldTriggerKeyboard?.focused === true &&
      checks.foldTriggerKeyboard?.opened === true &&
      checks.foldTriggerKeyboard?.noDrilldown === true,
    'the fold trigger was not opened from the keyboard without a drilldown',
  )
  const unknown = fold.unknown ?? {}
  assert(
    unknown.source === 'browser-interception-client-robustness' &&
      unknown.visibleRows === 2 &&
      unknown.dash === true &&
      unknown.coercedToZero === false &&
      unknown.hiddenRequestCount === 7 &&
      unknown.foldedRequestSum === 7 &&
      unknown.positivePreserved === true,
    'the unknown-token popover robustness probe is missing or misreported',
  )

  const variants = (report as Record<string, any>).detailVariants ?? {}
  assert(
    variants.messages === 'low' &&
      variants.responses === 'low → high' &&
      variants.parentSession === 'medium → high',
    'the reasoning caller/override and sub-session detail variants are incomplete',
  )

  const nullable = checks.nullableCells
  assert(
    nullable?.listTokenCell === '- · -' &&
      nullable?.listOutputRate === '-' &&
      nullable?.detailNoFabricatedZero === true,
    'the actual nullable-token list cells and detail are not verified as dashes',
  )
  assert(
    nullable?.populatedTokenCell?.text === '1.1K · 40%' &&
      nullable?.populatedTokenCell?.whiteSpace === 'nowrap' &&
      !nullable?.populatedTokenCell?.text?.includes('/') &&
      (!nullable?.populatedTokenCell?.lineHeight ||
        nullable?.populatedTokenCell?.height <=
          nullable?.populatedTokenCell?.lineHeight * 1.5),
    'the token cell is not a single line of total tokens and cache rate',
  )

  const widths = report.widthChecks ?? []
  assert(
    widths.length === 8,
    'the popover width measurement does not cover both locales at four widths',
  )
  for (const locale of ['en', 'zh']) {
    for (const width of [1440, 1024, 768, 390]) {
      const entry = widths.find(
        (item) => item.locale === locale && item.viewport === width,
      )
      const expected = Math.min(entry?.intrinsicWidth ?? 0, width - 32)
      assert(
        Math.abs((entry?.clientWidth ?? 0) - expected) <= 6,
        `the ${locale} popover at ${width}px is ${entry?.clientWidth} instead of the intrinsic-or-viewport ${expected}`,
      )
      assert(
        (entry?.rectLeft ?? -1) >= -1 &&
          (entry?.rectRight ?? width + 1) <= width + 1,
        `the ${locale} popover at ${width}px escapes the viewport`,
      )
      assert(
        entry?.documentOverflow === false,
        `the ${locale} popover at ${width}px overflows the document`,
      )
      const fits = (entry?.intrinsicWidth ?? 0) + 32 <= width
      assert(
        fits
          ? (entry?.scrollerScrollWidth ?? 1) <=
              (entry?.scrollerClientWidth ?? 0) + 1
          : (entry?.scrollerScrollWidth ?? 0) >
              (entry?.scrollerClientWidth ?? 0),
        `the ${locale} popover at ${width}px does not show every metric or keep a controlled scroll`,
      )
    }
  }
  for (const width of [1440, 1024]) {
    assert(
      [1440, 1024].length > 0 &&
        widths.some(
          (item) =>
            item.viewport === width &&
            (item.scrollerScrollWidth ?? 1) <=
              (item.scrollerClientWidth ?? 0) + 1,
        ),
      `no locale shows every popover metric without horizontal pan at ${width}px`,
    )
  }
  for (const perspective of ['model', 'upstream']) {
    const matrix = fold[perspective] ?? {}
    assert(
      matrix.collapsedFooterHidden === true &&
        matrix.expandedCopyNeutral === true,
      `the ${perspective} fold footer does not describe the current state truthfully`,
    )
  }
  for (const locale of ['zh', 'zhUpstream']) {
    const matrix = fold[locale] ?? {}
    assert(
      matrix.collapsedFooterHidden === true &&
        matrix.expandedCopyNeutral === true,
      `the Chinese ${locale} fold footer does not describe the current state truthfully`,
    )
  }

  const headers = report.headerChecks ?? []
  assert(
    headers.length === 8,
    'the header measurement does not cover both locales at four widths',
  )
  for (const locale of ['en', 'zh']) {
    for (const width of [1440, 1024, 768, 390]) {
      const entry = headers.find(
        (item) => item.locale === locale && item.viewport === width,
      )
      assert(
        entry?.headers === 9 &&
          (entry.overlaps?.length ?? 1) === 0 &&
          entry.pinned === true &&
          (entry.scrollWidth ?? 0) <= width,
        `the ${locale} headers at ${width}px are not measured as wrapped, unpinned, and contained`,
      )
    }
  }

  const sort = report.sortChecks ?? {}
  assert(
    sort.ascending === true &&
      sort.descending === true &&
      sort.unsorted === true &&
      sort.usageDate === true &&
      sort.createdAt === true &&
      sort.rowOrderChanged === true &&
      sort.keptFirst === 10 &&
      sort.tabReachable === true,
    'the accessible sorting cycle is not fully verified',
  )

  const detail = report.detailChecks ?? {}
  assert(
    detail.transcriptFirst === 11 &&
      detail.transcriptAll === 13 &&
      detail.cursor === '10' &&
      detail.error === true &&
      detail.abort === true,
    'the AI detail, transcript paging, error, and abort blocks are incomplete',
  )

  const session = report.sessionChecks ?? {}
  assert(
    session.save === true &&
      session.clear === true &&
      session.confirmCancel === true &&
      session.confirm === true &&
      session.pendingGuard === true &&
      session.errors === true,
    'the admin session routing writes, confirmation, guard, or errors are incomplete',
  )

  const clear = report.clearChecks ?? {}
  assert(
    clear.row === true &&
      clear.clearRequest === '/api/v1/admin/request-records/clear' &&
      clear.failedRetry === true &&
      clear.singleWrite === true &&
      clear.errors === true,
    'the history clear row, request, single write, retry, or validation errors are incomplete',
  )

  const role = report.roleChecks ?? {}
  assert(
    role.selfScope === true &&
      role.foreign404 === true &&
      role.writes403 === true &&
      role.sessionReadOnly === true,
    'the ordinary-user scope, foreign read, or write denial is incomplete',
  )

  assert(
    checks.mcpSort?.ascending === true &&
      checks.mcpSort?.descending === true &&
      checks.mcpSort?.unsorted === true &&
      checks.mcpSort?.field === 'created_at',
    'the MCP sortable column cycle is not verified',
  )
  assert(
    checks.mcpRoute?.cards === 5 &&
      Array.isArray(checks.mcpRoute?.columns) &&
      checks.mcpScope === 'gateway' &&
      checks.mcpDetail?.details === true &&
      checks.mcpDetail?.errorLink === true,
    'the MCP route, scope, detail, or error link is incomplete',
  )
  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.limitations) &&
      report.limitations.length === 3 &&
      report.limitations.some((item) => item.includes('browser-interception')),
    'the report does not record its evidence limitations truthfully',
  )

  assert(
    Array.isArray(report.expectedApiFailures) &&
      EXPECTED_API_FAILURES.every((failure) =>
        report.expectedApiFailures?.includes(failure),
      ) &&
      Array.isArray(report.apiFailures) &&
      report.apiFailures.length === 0,
    'expected usage failures or unexpected API responses were not recorded accurately',
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
      `the required usage screenshot is missing: ${screenshot}`,
    )
  }

  const columnGeometryScenario = scenarios['R-USAGE-AI']?.['column-geometry']
  assert(
    columnGeometryScenario?.passed === true &&
      Boolean(columnGeometryScenario?.evidence),
    'R-USAGE-AI:column-geometry has no passing current-run assertion',
  )
  assert(
    checks.requestTableNoWrap?.headers === true &&
      checks.requestTableNoWrap?.dateHeader === true &&
      checks.requestTableNoWrap?.dateTimeCells === true,
    'the request table headers or the date/time cells were not measured as non-wrapping',
  )
  const columnGeometry = checks.aiRecordsZhDark ?? {}
  for (const viewport of ['desktop', 'mobile'] as const) {
    const measurement = columnGeometry[viewport] ?? {}
    const rem = measurement.rem
    const modelWidth = measurement.modelWidth
    assert(
      typeof rem === 'number' &&
        Number.isFinite(rem) &&
        rem > 0 &&
        typeof modelWidth === 'number' &&
        Number.isFinite(modelWidth) &&
        modelWidth >= 14 * rem,
      `the AI model column is not at least 14rem of measured width in the zh-dark ${viewport} records`,
    )
    assert(
      measurement.headerNoWrap === true && measurement.dateTimeNoWrap === true,
      `the zh-dark ${viewport} records headers or date/time cells wrap`,
    )
    assert(
      measurement.headerOverlap === false,
      `the zh-dark ${viewport} records headers overlap`,
    )
    assert(
      measurement.documentNoOverflow === true,
      `the zh-dark ${viewport} records overflow the document horizontally`,
    )
    assert(
      measurement.containerScrolls === true,
      `the zh-dark ${viewport} records table container does not own the scroll`,
    )
  }
  for (const screenshot of [
    'e2e/foundation/screenshots/usage-ai-records-zh-dark-1440.png',
    'e2e/foundation/screenshots/usage-ai-records-zh-dark-390.png',
    'e2e/foundation/screenshots/usage-ai-records-token-zh-dark-1440.png',
  ]) {
    assert(
      screenshots.includes(screenshot) &&
        existsSync(join(root, screenshot.replace(/^e2e\/foundation\//, ''))),
      `the zh-dark AI records screenshot evidence is missing: ${screenshot}`,
    )
  }

  const tokenColumn = checks.aiRecordsTokenColumn ?? {}
  assert(
    tokenColumn.found === true &&
      typeof tokenColumn.scrollLeft === 'number' &&
      typeof tokenColumn.maxScrollLeft === 'number' &&
      tokenColumn.scrollLeft >= tokenColumn.maxScrollLeft - 1,
    'the zh-dark token-column capture did not scroll the request table to its end',
  )
  assert(
    typeof tokenColumn.visibleCount === 'number' &&
      tokenColumn.visibleCount > 0 &&
      Array.isArray(tokenColumn.cells) &&
      tokenColumn.cells.length === tokenColumn.visibleCount,
    'the zh-dark token column is not visible in the captured evidence',
  )
  const tokenShape = /^[\d.,KMB-]+ · [\d.%-]+$/
  const tokenCells = Array.isArray(tokenColumn.cells) ? tokenColumn.cells : []
  assert(
    tokenCells.length > 0 &&
      tokenCells.every(
        (cell: Record<string, any>) =>
          cell.whiteSpace === 'nowrap' &&
          cell.lineCount === 1 &&
          typeof cell.height === 'number' &&
          typeof cell.lineHeight === 'number' &&
          cell.height <= cell.lineHeight * 1.5 &&
          tokenShape.test(String(cell.text ?? '')),
      ),
    'a zh-dark token cell is not one line of total tokens and cache rate',
  )
  const tokenTexts = tokenCells.map((cell: Record<string, any>) => cell.text)
  assert(
    tokenTexts.includes('- · -') &&
      tokenTexts.some((text: string) => text !== '- · -'),
    'the zh-dark token evidence does not show both an empty and a populated cell',
  )
}

if (failures.length > 0) {
  console.error('Usage focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log('Usage focused browser verification passed.')
