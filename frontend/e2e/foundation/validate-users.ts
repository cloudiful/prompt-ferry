import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type UserReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, unknown>
  screenshots?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'users-result.json')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

assert(
  existsSync(reportPath),
  'focused Users browser result is missing; run run-users.sh',
)

if (existsSync(reportPath)) {
  const report = JSON.parse(read(reportPath)) as UserReport
  const checks = report.checks ?? {}
  const requiredChecks = [
    'routeAndNavigation',
    'listReadWithoutOptions',
    'dialogOpen',
    'createDialogFailureDraftAndRetry',
    'updateFullRow',
    'resetFailureDraftRetryAndPasswordMasking',
    'deleteCancelNoWrite',
    'paginationDistinctRowsAndQuery',
    'pageSizePersistence',
    'deletePageCorrection',
    'longValuesResponsive',
    'localeThemeRendering',
    'adminDenial',
  ]
  const expectedScenarios = [
    'route-render',
    'admin-denial',
    'pagination',
    'dialog-open',
    'mutation-create',
    'mutation-update',
    'mutation-delete',
    'mutation-failure-draft',
    'long-values',
  ]

  assert(
    typeof report.runId === 'string' && report.runId.startsWith('users-'),
    'focused report is not from the Users runner',
  )
  assert(
    typeof report.startedAt === 'string' &&
      typeof report.finishedAt === 'string',
    'focused report is incomplete',
  )
  const runTimestamp = Number(report.runId?.slice('users-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'focused report runId is missing or inconsistent with its start time',
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
      `current-run Users assertion is missing or false: ${check}`,
    )
  }

  const scenarios = checks.scenarioCoverage as
    Record<string, boolean> | undefined
  for (const scenario of expectedScenarios) {
    assert(
      scenarios?.[scenario] === true,
      `current-run R-USERS scenario is missing or false: ${scenario}`,
    )
  }

  const adminDenial = checks.adminDenial as
    | {
        redirectedToApiKeys?: boolean
        hiddenNavigation?: boolean
        adminApiCalls?: number
      }
    | undefined
  assert(
    adminDenial?.redirectedToApiKeys === true &&
      adminDenial.hiddenNavigation === true &&
      adminDenial.adminApiCalls === 0,
    'non-admin Users route denial or API isolation evidence is missing',
  )
  assert(
    JSON.stringify(checks.expectedApiFailures) ===
      JSON.stringify([
        '503 POST /api/v1/admin/users',
        '503 POST /api/v1/admin/users/2/reset-password',
      ]),
    'expected failure/retry set is missing or unexpected',
  )
  assert(
    Array.isArray(checks.apiFailures) && checks.apiFailures.length === 0,
    'unexpected Users API failures were reported',
  )
  assert(
    Array.isArray(checks.consoleErrors) && checks.consoleErrors.length === 0,
    'browser console errors were reported',
  )
  assert(
    Array.isArray(checks.pageErrors) && checks.pageErrors.length === 0,
    'browser page errors were reported',
  )

  const createPayloads = checks.createPayloads as
    Array<Record<string, unknown>> | undefined
  assert(
    JSON.stringify(createPayloads) ===
      JSON.stringify([
        {
          login_name: ' fixture-created-user ',
          display_name: ' Fixture Created ',
          is_admin: true,
          passwordIsRaw: true,
        },
        {
          login_name: ' fixture-created-user ',
          display_name: ' Fixture Created ',
          is_admin: true,
          passwordIsRaw: true,
        },
      ]),
    'create request payloads do not prove raw fields and failure retry',
  )
  assert(
    JSON.stringify(checks.updatePayloads) ===
      JSON.stringify([
        {
          user_id: 2,
          display_name: 'Operator Updated',
          is_admin: true,
          is_active: false,
        },
      ]),
    'full-row UserUpdate payload evidence is missing or malformed',
  )
  assert(
    JSON.stringify(checks.resetPayloads) ===
      JSON.stringify([{ passwordIsRaw: true }, { passwordIsRaw: true }]),
    'reset-password request payload evidence is missing or malformed',
  )
  assert(
    !JSON.stringify(report).includes(' fixture-password ') &&
      !JSON.stringify(report).includes(' fixture-reset-password '),
    'password plaintext leaked into the persisted Users report',
  )

  const responsive = checks.longValuesResponsive as
    | Array<{
        width?: number
        loginInside?: boolean
        inputInside?: boolean
        actionsInside?: boolean
        documentWidth?: number
        viewportWidth?: number
      }>
    | undefined
  assert(
    JSON.stringify(responsive?.map((check) => check.width)) ===
      JSON.stringify([1440, 1024, 768, 390]),
    'long-value responsive checks did not cover every required viewport',
  )
  for (const check of responsive ?? []) {
    assert(
      check.loginInside === true &&
        check.inputInside === true &&
        check.actionsInside === true &&
        typeof check.documentWidth === 'number' &&
        check.documentWidth <= (check.viewportWidth ?? 0),
      `long-value layout containment failed at ${check.width}px`,
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
    'localized light/dark Users rendering evidence is missing',
  )

  const expectedScreenshots = [
    'e2e/foundation/screenshots/users-admin-en-light-1440.png',
    'e2e/foundation/screenshots/users-admin-zh-dark-1440.png',
    'e2e/foundation/screenshots/users-admin-en-light-1440-final.png',
  ]
  for (const screenshot of expectedScreenshots) {
    assert(
      report.screenshots?.includes(screenshot),
      `screenshot is not in the current Users report: ${screenshot}`,
    )
    assert(
      existsSync(join(root, '..', '..', screenshot)),
      `Users screenshot is missing: ${screenshot}`,
    )
  }
}

const row = ROUTES.find((candidate) => candidate.id === 'R-USERS')
const requiredScenarios = [
  'route-render',
  'admin-denial',
  'pagination',
  'dialog-open',
  'mutation-create',
  'mutation-update',
  'mutation-delete',
  'mutation-failure-draft',
  'long-values',
]
assert(
  row?.foundationTest === 'e2e/foundation/users.playwright',
  'R-USERS does not map to its focused React browser run',
)
for (const scenario of requiredScenarios) {
  assert(
    row?.requiredScenarios.includes(
      scenario as (typeof row.requiredScenarios)[number],
    ),
    `R-USERS matrix is missing ${scenario}`,
  )
}
assert(
  row?.status === 'baseline-verified',
  'R-USERS historical baseline status changed',
)
assert(
  row?.evidence.includes(
    'e2e/baseline/screenshots/admin_zh_dark_1440_users.png',
  ) &&
    row.evidence.includes(
      'e2e/baseline/screenshots/admin_en_dark_1440_users.png',
    ),
  'R-USERS historical P0 screenshot evidence changed',
)
assert(
  JSON.stringify(row?.legacyTests) ===
    JSON.stringify([
      'users-store.test.ts',
      'record-actions.gates.test.ts',
      'record-actions.lifecycle.test.ts',
      'record-actions.sfc.test.ts',
      'row-switch-labels.test.ts',
    ]),
  'R-USERS legacy test mapping does not match the existing test modules',
)

const authCallback = read(join(root, 'auth-shell.playwright'))
assert(
  !authCallback.includes("['/users', 'R-USERS']"),
  'ported Users route remains in the common placeholder list',
)
assert(
  (authCallback.match(/\['\/[^']+', 'R-[^']+'\]/g) ?? []).length === 0,
  'the shared shell must not retain a route placeholder',
)

const componentTests = read(join(root, '../../src/react-tests/users.test.tsx'))
const modelTests = read(join(root, '../../src/react-tests/users-model.test.ts'))
for (const scenario of [
  'redirects non-admins before mounting the route',
  'shows the existing empty-users state and refreshes the same page',
  'saves the exact full-row update',
  'keeps create drafts after failure',
  'corrects the last page after deleting its final user',
  'shares each row guard across reset/save/delete while different users remain independent',
  'persists the selected user page size',
]) {
  assert(
    componentTests.includes(scenario),
    `Users component regression test is missing: ${scenario}`,
  )
}
assert(
  modelTests.includes(
    'uses one create key and one shared write key per account',
  ),
  'Users action-key model test is missing',
)

if (failures.length > 0) {
  console.error(`Users focused validation failed:\n- ${failures.join('\n- ')}`)
  process.exitCode = 1
} else {
  console.log(
    'Users focused report, historical mapping and current-run evidence are valid.',
  )
}
