import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type FocusedReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, unknown>
  screenshots?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'api-keys-models-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

assert(
  existsSync(reportPath),
  'focused browser result is missing; run run-api-keys-models.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(read(reportPath)) as FocusedReport
  const checks = report.checks ?? {}
  const requiredChecks = [
    'apiKeysRouteAndNavigation',
    'apiKeysPagination',
    'staleOwnerAndPageResponses',
    'createFailureDraftAndDismissGuard',
    'flatCreateSecretDisplayedAndCopied',
    'keyRenameAndToggle',
    'deleteConfirmation',
    'adminOwnerReadAndRename',
    'localeThemeRendering',
    'apiKeysResponsive',
    'modelsReadRefreshAndPagination',
    'modelsRenderedEmptyState',
    'staleModelPageResponse',
    'availableModelsResponsive',
  ]

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('api-keys-models-'),
    'focused report is not from the client-key/models runner',
  )
  assert(
    typeof report.startedAt === 'string' &&
      typeof report.finishedAt === 'string',
    'focused report is incomplete',
  )
  const runTimestamp = Number(report.runId?.slice('api-keys-models-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'focused report runId is missing, stale, or inconsistent with its start time',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 15 * 60 * 1000,
    'focused report is stale or has invalid run timestamps',
  )
  for (const check of requiredChecks) {
    assert(
      Boolean(checks[check]),
      `focused current-run assertion is missing or false: ${check}`,
    )
  }

  const ownerReads = checks.ownerReads as
    { self?: boolean; users?: boolean } | undefined
  assert(ownerReads?.self === true, 'self client-key endpoint was not observed')
  assert(
    ownerReads?.users === true,
    'admin user-options endpoint was not observed',
  )
  const emptyState = checks.modelsRenderedEmptyState as
    | {
        en?: {
          tableCount?: number
          tbodyCount?: number
          rowCount?: number
          cellText?: string
          columnSpan?: number
        }
        zh?: {
          tableCount?: number
          tbodyCount?: number
          rowCount?: number
          cellText?: string
          columnSpan?: number
        }
      }
    | undefined
  for (const [locale, expectedText] of [
    ['en', 'No data'],
    ['zh', '没有数据'],
  ] as const) {
    const state = emptyState?.[locale]
    assert(
      state?.tableCount === 1 &&
        state.tbodyCount === 1 &&
        state.rowCount === 1 &&
        state.cellText === expectedText &&
        state.columnSpan === 2,
      `${locale} actual empty-table rendering evidence is missing or malformed`,
    )
  }
  assert(
    JSON.stringify(checks.createPayloads) ===
      JSON.stringify(['fixture-created-key', 'fixture-created-key']),
    'create request payload evidence is missing or not trimmed',
  )
  assert(
    JSON.stringify(checks.createRequestStatuses) === JSON.stringify([503, 200]),
    'create did not show one failed request followed by a successful retry',
  )
  assert(
    JSON.stringify(checks.expectedApiFailures) ===
      JSON.stringify(['503 /api/v1/me/client-keys']),
    'unexpected API failure set was reported',
  )
  assert(
    (checks.consoleErrors as unknown[] | undefined)?.length === 0,
    'browser console errors were reported',
  )
  assert(
    (checks.pageErrors as unknown[] | undefined)?.length === 0,
    'browser page errors were reported',
  )
  assert(
    (checks.apiFailures as unknown[] | undefined)?.length === 0,
    'unexpected API failures were reported',
  )
  assert(
    !JSON.stringify(report).includes('pf_fixture_secret_revealed_once'),
    'one-time secret leaked into the persisted browser result',
  )

  const expectedScreenshots = [
    'e2e/foundation/screenshots/api-keys-zh-dark-1440.png',
    'e2e/foundation/screenshots/api-key-create-dialog-zh-dark-1440.png',
    'e2e/foundation/screenshots/available-models-en-light-1440.png',
    'e2e/foundation/screenshots/available-models-empty-en-light-1440.png',
    'e2e/foundation/screenshots/available-models-empty-zh-light-1440.png',
  ]
  for (const screenshot of expectedScreenshots) {
    assert(
      report.screenshots?.includes(screenshot),
      `screenshot is not in the current report: ${screenshot}`,
    )
    assert(
      existsSync(join(root, '..', '..', screenshot)),
      `focused screenshot is missing: ${screenshot}`,
    )
  }
}

for (const [id, expectedTest, required] of [
  [
    'R-API-KEYS',
    'e2e/foundation/api-keys-models.playwright',
    [
      'route-render',
      'nav-visibility',
      'pagination',
      'dialog-open',
      'mutation-create',
      'mutation-toggle',
      'mutation-delete',
      'mutation-conflict-guard',
      'mutation-failure-draft',
      'long-values',
    ],
  ],
  [
    'R-AVAILABLE-MODELS',
    'e2e/foundation/api-keys-models.playwright',
    ['route-render', 'nav-visibility', 'pagination', 'empty-state'],
  ],
] as const) {
  const row = ROUTES.find((candidate) => candidate.id === id)
  assert(
    row?.foundationTest === expectedTest,
    `${id} does not map to its focused React run`,
  )
  for (const scenario of required) {
    assert(
      row?.requiredScenarios.includes(scenario),
      `${id} is missing mapped scenario ${scenario}`,
    )
  }
  assert(row?.status === 'baseline-verified', `${id} baseline status changed`)
}

const callbackPath = join(root, 'api-keys-models.playwright')
assert(existsSync(callbackPath), 'focused Playwright callback is missing')
if (existsSync(callbackPath)) {
  const callback = read(callbackPath)
  for (const routeId of ['R-API-KEYS', 'R-AVAILABLE-MODELS']) {
    assert(
      callback.includes(routeId),
      `focused callback does not assert ${routeId}`,
    )
  }
}

const authCallback = read(join(root, 'auth-shell.playwright'))
assert(
  !authCallback.includes("['/api-keys', 'R-API-KEYS']") &&
    !authCallback.includes("['/available-models', 'R-AVAILABLE-MODELS']"),
  'ported routes remain in the common placeholder list',
)
assert(
  (authCallback.match(/\['\/[^']+', 'R-[^']+'\]/g) ?? []).length === 0,
  'the shared shell must not retain a route placeholder',
)

const apiTests = read(join(root, '../../src/react-tests/api-keys.test.tsx'))
const modelTests = read(
  join(root, '../../src/react-tests/available-models.test.tsx'),
)
assert(
  apiTests.includes('owner-scoped admin create, update, and delete paths'),
  'API key owner-write test is missing',
)
assert(
  apiTests.includes('suppresses an error from a stale owner read'),
  'API key stale-owner error-suppression test is missing',
)
assert(
  apiTests.includes('failed rename draft retryable'),
  'API key failure-draft test is missing',
)
assert(
  apiTests.includes('target-owner refresh'),
  'API key refresh-lock test is missing',
)
assert(
  modelTests.includes('localized empty tbody row'),
  'available-model empty-state test is missing',
)
assert(
  modelTests.includes('out-of-range response'),
  'available-model out-of-range test is missing',
)

if (failures.length > 0) {
  console.error(
    `API keys/models focused validation failed:\n- ${failures.join('\n- ')}`,
  )
  process.exitCode = 1
} else {
  console.log('API keys/models focused report and matrix mappings are valid.')
}
