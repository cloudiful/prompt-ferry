/**
 * Matrix validator for the issue #1060 baseline.
 *
 * Run from `frontend/`:
 *   bun run e2e/baseline/validate-matrix.ts
 *
 * It fails when the executable contract, the live app's route table, the
 * browser callback, the captured evidence, or the legacy test catalogue drift.
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import {
  PLAN_CONSTANTS,
  REDIRECTS,
  RETAINED_LEGACY_MODULES,
  RETAINED_LEGACY_MODULE_NAMES,
  ROUTES,
  SCENARIOS,
  SRC_TEST_MODULES,
  FOCUSED_SCENARIO_EVIDENCE,
  LEGACY_MODULE_DISPOSITIONS,
  LEGACY_SUPPORT_FILES,
  classifyLegacyTest,
  rowsForScenario,
  validateLegacyModuleDispositions,
  type ScenarioId,
} from '../parity.matrix.ts'
import { resolveCoverageEvidence } from './scenario-coverage.ts'

const E2E_ROOT = join(import.meta.dir, '..')
const APP_ROOT = join(E2E_ROOT, '..')
const failures: string[] = []
const notes: string[] = []

function fail(message: string): void {
  failures.push(message)
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

function assert(condition: unknown, message: string): void {
  if (!condition) fail(message)
}

const GLOBAL_SCENARIOS: ScenarioId[] = ['redirect']

const LEGACY_DISPOSITIONS = [
  'preserved-framework-neutral',
  'preserved-form-mapper',
  'pending-react-replacement',
  'retire-source-string',
  'support-fixture',
]
const retainedModules = RETAINED_LEGACY_MODULES
const retainedNames = new Set(retainedModules.map((module) => module.file))
assert(
  retainedModules.length === 134,
  `the retained legacy inventory holds ${retainedModules.length} modules instead of 134`,
)
assert(
  retainedNames.size === retainedModules.length,
  'the retained legacy inventory has duplicate names',
)
for (const module of retainedModules) {
  assert(
    LEGACY_DISPOSITIONS.includes(module.disposition),
    `unknown disposition for retained module ${module.file}`,
  )
}

// ------------------------------------------------------------------ matrix
const ids = new Set<string>()
for (const row of ROUTES) {
  assert(!ids.has(row.id), `duplicate row id: ${row.id}`)
  ids.add(row.id)
  assert(row.route.startsWith('/'), `${row.id} route must be absolute`)
  assert(row.requiredScenarios.length > 0, `${row.id} has no required scenario`)
  assert(row.gate.length > 0, `${row.id} has no migration gate`)
  for (const scenario of row.requiredScenarios) {
    assert(
      SCENARIOS.includes(scenario),
      `${row.id} uses unknown scenario ${scenario}`,
    )
  }
  for (const file of row.legacyTests) {
    assert(
      retainedNames.has(file),
      `${row.id} references a legacy module outside the retained inventory: ${file}`,
    )
  }
}

const routes = new Set<string>()
for (const row of ROUTES) {
  assert(!routes.has(row.route), `duplicate route row: ${row.route}`)
  routes.add(row.route)
}

for (const scenario of SCENARIOS) {
  if (GLOBAL_SCENARIOS.includes(scenario)) continue
  assert(
    rowsForScenario(scenario).length > 0,
    `scenario ${scenario} is not required by any route row`,
  )
}

assert(
  PLAN_CONSTANTS.routes === ROUTES.length,
  `PLAN_CONSTANTS.routes=${PLAN_CONSTANTS.routes} but there are ${ROUTES.length} rows`,
)
assert(
  PLAN_CONSTANTS.scenarios === SCENARIOS.length,
  'PLAN_CONSTANTS.scenarios does not match SCENARIOS',
)

for (const redirect of REDIRECTS) {
  assert(redirect.from.startsWith('/'), `${redirect.id} from must be absolute`)
  assert(redirect.to.startsWith('/'), `${redirect.id} to must be absolute`)
  assert(
    routes.has(redirect.to) || redirect.to === '/api-keys',
    `${redirect.id} targets an unknown route ${redirect.to}`,
  )
}

// ------------------------------------------------------- react nav + router
type ReactNavRoute = { id: string; path: string; adminOnly: boolean }
type ReactNavSection = { id: string; routeIds: string[] }
type AppRouteTag = { path: string | null; to: string | null }

/** Bounded fallback for a failed `src/lib/nav.ts` import. */
function parseReactNavSource(source: string): {
  sections: ReactNavSection[]
  routes: ReactNavRoute[]
} {
  const sectionMatches = [...source.matchAll(/^ {4}id: '([^']+)',/gm)].map(
    (match) => ({ id: match[1]!, index: match.index ?? 0 }),
  )
  const routeMatches = [
    ...source.matchAll(
      /id: '(R-[^']+)',\s*path: '([^']+)',\s*titleKey: '[^']+',\s*adminOnly: (true|false)/g,
    ),
  ].map((match) => ({
    id: match[1]!,
    path: match[2]!,
    adminOnly: match[3] === 'true',
    index: match.index ?? 0,
  }))
  const sections: ReactNavSection[] = sectionMatches.map((section, index) => {
    const nextSection = sectionMatches[index + 1]
    return {
      id: section.id,
      routeIds: routeMatches
        .filter(
          (route) =>
            route.index > section.index &&
            (!nextSection || route.index < nextSection.index),
        )
        .map((route) => route.id),
    }
  })
  return {
    sections,
    routes: routeMatches.map(({ id, path, adminOnly }) => ({
      id,
      path,
      adminOnly,
    })),
  }
}

async function loadReactNav(): Promise<{
  sections: ReactNavSection[]
  routes: ReactNavRoute[]
}> {
  const navPath = join(APP_ROOT, 'src/lib/nav.ts')
  try {
    const nav = (await import(navPath)) as {
      navSections?: Array<{
        id: string
        routes?: Array<{ id: string; path: string; adminOnly?: boolean }>
      }>
      appRoutes?: Array<{ id: string; path: string; adminOnly?: boolean }>
    }
    if (nav.navSections && nav.appRoutes) {
      return {
        sections: nav.navSections.map((section) => ({
          id: section.id,
          routeIds: (section.routes ?? []).map((route) => route.id),
        })),
        routes: nav.appRoutes.map((route) => ({
          id: route.id,
          path: route.path,
          adminOnly: route.adminOnly === true,
        })),
      }
    }
  } catch {
    // The alias import is unavailable here; fall back to the bounded parse.
  }
  return parseReactNavSource(read(navPath))
}

/** Reads the `path`/`to` attributes of every `<Route>` element in the app. */
function parseAppRouteTags(source: string): AppRouteTag[] {
  const tags: AppRouteTag[] = []
  const pattern = /<Route\b/g
  let match: RegExpExecArray | null
  while ((match = pattern.exec(source)) !== null) {
    const start = match.index
    let depth = 0
    let end = -1
    for (
      let index = start + match[0].length;
      index < source.length;
      index += 1
    ) {
      const character = source[index]
      if (character === '{') depth += 1
      else if (character === '}') depth -= 1
      else if (character === '>' && depth === 0) {
        end = index
        break
      }
    }
    if (end === -1) break
    const tag = source.slice(start, end)
    tags.push({
      path: readJsxAttribute(tag, 'path'),
      to: readJsxAttribute(tag, 'to'),
    })
    pattern.lastIndex = end
  }
  return tags
}

function readJsxAttribute(tag: string, name: string): string | null {
  const match = new RegExp(
    `\\b${name}=(?:"([^"]*)"|'([^']*)'|\\{([^}]*)\\})`,
  ).exec(tag)
  if (!match) return null
  return match[1] ?? match[2] ?? match[3] ?? null
}

const appSource = read(join(APP_ROOT, 'src/App.tsx'))
const reactNav = await loadReactNav()
const navRoutes = reactNav.routes
const navRouteIds = new Set<string>()
const navRoutePaths = new Set<string>()
for (const route of navRoutes) {
  assert(
    !navRouteIds.has(route.id),
    `duplicate React nav route id: ${route.id}`,
  )
  navRouteIds.add(route.id)
  assert(
    !navRoutePaths.has(route.path),
    `duplicate React nav route path: ${route.path}`,
  )
  navRoutePaths.add(route.path)
}
const navSectionIds = new Set(reactNav.sections.map((section) => section.id))
const navSectionOfRoute = new Map<string, string>()
for (const section of reactNav.sections) {
  for (const routeId of section.routeIds) {
    navSectionOfRoute.set(routeId, section.id)
  }
}

const matrixRouteIds = new Set(ROUTES.map((row) => row.id))
for (const row of ROUTES) {
  if (row.id === 'R-LOGIN') continue
  const navRoute = navRoutes.find((route) => route.id === row.id)
  assert(navRoute, `matrix route ${row.id} is missing from the React nav`)
  if (!navRoute) continue
  assert(
    navRoute.path === row.route,
    `matrix route ${row.id} is ${row.route} but the React nav routes ${navRoute.path}`,
  )
  assert(
    navRoute.adminOnly === (row.scope === 'admin'),
    `matrix route ${row.id} role does not match the React nav`,
  )
  assert(
    navSectionOfRoute.get(row.id) === row.navSection,
    `matrix route ${row.id} names section ${row.navSection} but the React nav places it in ${navSectionOfRoute.get(row.id)}`,
  )
  assert(
    navSectionIds.has(row.navSection),
    `matrix route ${row.id} names an unknown nav section ${row.navSection}`,
  )
}
for (const route of navRoutes) {
  assert(
    matrixRouteIds.has(route.id),
    `React nav route ${route.id} is missing from parity.matrix.ts`,
  )
}
for (const section of reactNav.sections) {
  assert(
    ROUTES.some((row) => row.navSection === section.id),
    `React nav section ${section.id} is not represented in parity.matrix.ts`,
  )
}

const appRouteTags = parseAppRouteTags(appSource)
const livePaths = new Set(navRoutePaths)
livePaths.add('/login')
const redirectSources = new Set(
  REDIRECTS.map((row) => row.from).filter((from) => !from.includes('<')),
)
for (const path of livePaths) {
  assert(
    routes.has(path) || redirectSources.has(path),
    `live route ${path} is missing from parity.matrix.ts`,
  )
}
for (const row of ROUTES) {
  assert(
    livePaths.has(row.route) || redirectSources.has(row.route),
    `matrix route ${row.route} is not routed by the React app`,
  )
}
for (const section of reactNav.sections) {
  const root = `/${section.id}`
  if (livePaths.has(root)) continue
  assert(
    redirectSources.has(root),
    `React nav section root ${root} is neither a route nor a redirect source`,
  )
}
assert(
  /appRoutes[\s\S]{0,400}?path=\{route\.path\}/.test(appSource),
  'the React app does not mount the nav routes',
)

for (const redirect of REDIRECTS) {
  if (redirect.from.includes('<')) {
    const catchAll = appRouteTags.filter((tag) => tag.path === '*')
    assert(catchAll.length > 0, 'the React app has no catch-all route')
    assert(
      catchAll.every((tag) => tag.to === redirect.to),
      `the React catch-all route does not redirect to ${redirect.to}`,
    )
    continue
  }
  const tag = appRouteTags.find((entry) => entry.path === redirect.from)
  assert(tag, `the React app does not route ${redirect.from}`)
  assert(
    tag?.to === redirect.to,
    `the React app redirects ${redirect.from} to ${tag?.to ?? 'nothing'} instead of ${redirect.to}`,
  )
}
assert(
  appRouteTags.some((tag) => tag.path === '/login'),
  'the React app does not route /login',
)

// --------------------------------------------------------------- callback
const captureSource = read(join(E2E_ROOT, 'baseline/capture.playwright'))
for (const row of ROUTES) {
  assert(
    captureSource.includes(`'${row.route}'`),
    `baseline capture does not drive ${row.route}`,
  )
}
for (const redirect of REDIRECTS) {
  if (redirect.from.includes('<')) continue
  assert(
    captureSource.includes(`'${redirect.from}'`),
    `baseline capture does not verify redirect ${redirect.from}`,
  )
}
for (const scenario of [
  'zero-token-fold',
  'mutation-conflict-guard',
  'mutation-failure-draft',
]) {
  assert(
    captureSource.includes(scenario.replaceAll('-', '-')) ||
      captureSource.includes(scenario) ||
      (scenario === 'zero-token-fold' &&
        captureSource.includes('regression_fold')) ||
      (scenario === 'mutation-conflict-guard' &&
        captureSource.includes('conflictGuard')) ||
      (scenario === 'mutation-failure-draft' &&
        captureSource.includes('failureDraft')),
    `baseline capture has no ${scenario} evidence hook`,
  )
}

// ---------------------------------------------------------- legacy catalogue
const testFiles = readdirSync(join(APP_ROOT, 'tests')).filter(
  (file) => file.endsWith('.test.ts') || file.endsWith('.fixtures.ts'),
)
const referenced = new Set(ROUTES.flatMap((row) => row.legacyTests))
for (const file of testFiles) {
  assert(referenced.has(file), `legacy test is not mapped to any row: ${file}`)
  assert(
    retainedNames.has(file),
    `legacy test is not in the retained inventory: ${file}`,
  )
  const disposition = classifyLegacyTest(file)
  assert(
    [
      'preserved-framework-neutral',
      'preserved-form-mapper',
      'pending-react-replacement',
      'retire-source-string',
      'support-fixture',
    ].includes(disposition),
    `unknown disposition for ${file}`,
  )
}
for (const file of referenced) {
  assert(
    retainedNames.has(file),
    `row references a legacy module outside the retained inventory: ${file}`,
  )
}
for (const file of SRC_TEST_MODULES) {
  assert(
    existsSync(join(APP_ROOT, file)),
    `form-mapper suite is missing: ${file}`,
  )
}

notes.push(
  `legacy tests mapped: ${testFiles.length + SRC_TEST_MODULES.length} files, ${ROUTES.reduce((sum, row) => sum + row.legacyTests.length, 0)} row references, ${retainedModules.length} retained inventory names`,
)

// ------------------------------------------------------- legacy dispositions
const dispositionChecks = {
  exists: (path: string) => existsSync(join(APP_ROOT, path)),
  read: (path: string) => {
    try {
      return read(join(APP_ROOT, path))
    } catch {
      return ''
    }
  },
}
for (const problem of validateLegacyModuleDispositions(
  LEGACY_MODULE_DISPOSITIONS,
  RETAINED_LEGACY_MODULE_NAMES,
  dispositionChecks,
)) {
  fail(`legacy disposition: ${problem}`)
}
assert(
  LEGACY_SUPPORT_FILES.length === 3,
  `the legacy support file list holds ${LEGACY_SUPPORT_FILES.length} files instead of 3`,
)
for (const problem of validateLegacyModuleDispositions(
  LEGACY_SUPPORT_FILES,
  LEGACY_SUPPORT_FILES.map((entry) => entry.file),
  dispositionChecks,
)) {
  fail(`legacy support file: ${problem}`)
}

// ---------------------------------------------------------------- evidence
const evidencePath = join(E2E_ROOT, 'baseline/evidence.json')
if (!existsSync(evidencePath)) {
  fail('baseline/evidence.json is missing; run e2e/baseline/run-baseline.sh')
} else {
  const evidence = JSON.parse(read(evidencePath)) as {
    failure?: string
    runId?: string
    runManifest?: {
      runId?: string
      startedAt?: string
      finishedAt?: string
      callbacks?: Array<{
        name: string
        runId: string
        startedAt: string
        finishedAt: string
      }>
    }
    scenarioCoverage?: Record<
      string,
      Record<string, { passed: boolean; evidence: string }>
    >
    knownBaselineDefects?: Array<{
      rowId: string
      scenario: string
      disposition: string
      evidence: string
      futureFixId: string
      targetRequired: boolean
    }>
    captures: Array<{
      name: string
      path: string
      url: string
      errors: string[]
    }>
    checks: Record<string, any>
    mutations: Array<{ name: string; error: string | null; requests: string[] }>
    residual?: Array<{
      name: string
      runId?: string
      report: Record<string, any>
    }>
    residualFailures?: string[]
  }
  assert(!evidence.failure, `baseline failed: ${evidence.failure}`)
  assert(
    !evidence.residualFailures?.length,
    `residual callbacks failed: ${evidence.residualFailures?.join('; ')}`,
  )
  const residual = new Map(
    (evidence.residual ?? []).map((item) => [item.name, item.report]),
  )
  for (const name of [
    'four-slices',
    'form-payloads',
    'advanced',
    'auth-quota-negative',
    'upstream-payloads',
    'quota-stale',
    'mcp-maintenance',
    'domain-retries',
    'relay-secrets',
    'mcp-secrets',
    'endpoint-secrets',
    'settings-secrets',
    'billing-filters',
    'attempt11-missing',
    'attempt11-settings',
    'attempt11-long-values',
  ]) {
    const report = residual.get(name)
    assert(Boolean(report), `current combined evidence missing ${name}`)
    assert(report?.failures?.length === 0, `${name} callback has failures`)
  }
  const residualChecks = Object.fromEntries(
    (evidence.residual ?? []).map(({ name, report }) => [
      name,
      report.checks ?? {},
    ]),
  ) as Record<string, Record<string, any>>
  assert(
    residualChecks['four-slices']?.archive?.cancelNoPost &&
      residualChecks['four-slices']?.billing?.filteredModels?.length > 0 &&
      residualChecks['four-slices']?.sort?.futureFixId,
    'current four-slice archive/billing/sort assertions missing',
  )
  assert(
    residualChecks['form-payloads']?.dimensions?.positive &&
      residualChecks['form-payloads']?.chatControls?.saved?.body?.targets,
    'current route payload assertions missing',
  )
  assert(
    residualChecks['advanced']?.oauth?.deviceComplete &&
      residualChecks['advanced']?.oauth?.browserComplete &&
      residualChecks['advanced']?.emptyModels?.text,
    'current advanced OAuth/model assertions missing',
  )
  assert(
    residualChecks['auth-quota-negative']?.oauth?.deviceFailure &&
      residualChecks['auth-quota-negative']?.quota?.retry,
    'current negative OAuth/quota retry assertions missing',
  )
  assert(
    residualChecks['upstream-payloads']?.subscription?.saved?.body?.api_keys &&
      residualChecks['upstream-payloads']?.transition?.protocol?.body
        ?.protocol_mode === 'manual',
    'current upstream payload assertions missing',
  )
  assert(
    residualChecks['mcp-maintenance']?.cancelWrites === 0 &&
      residualChecks['mcp-maintenance']?.writes?.length === 1,
    'MCP history clear cancel/confirm evidence missing',
  )
  assert(
    ['R-USERS', 'R-MCP', 'R-RELAYS'].every(
      (id) =>
        residualChecks['domain-retries']?.[id]?.writes
          ?.map((write: { status: number }) => write.status)
          .join(',') === '503,200',
    ),
    'domain retry evidence incomplete',
  )
  assert(
    residualChecks['relay-secrets']?.replaceExact &&
      residualChecks['relay-secrets']?.clearExact,
    'relay secret replace/clear evidence missing',
  )
  assert(
    residualChecks['mcp-secrets']?.keepExact &&
      residualChecks['mcp-secrets']?.replaceExact &&
      residualChecks['mcp-secrets']?.proxyClear,
    'MCP secret evidence missing',
  )
  assert(
    residualChecks['endpoint-secrets']?.keep &&
      residualChecks['endpoint-secrets']?.replace &&
      residualChecks['endpoint-secrets']?.proxyClear,
    'endpoint secret evidence missing',
  )
  assert(
    residualChecks['settings-secrets']?.storageRetryPayload &&
      residualChecks['settings-secrets']?.cacheRetry,
    'settings storage/cache retry evidence missing',
  )
  assert(
    residualChecks['billing-filters']?.query &&
      residualChecks['billing-filters']?.rows?.some((row: string) =>
        row.includes('gpt-5.1-codex-preview'),
      ),
    'billing filtered rows evidence missing',
  )

  const missingChecks = residualChecks['attempt11-missing'] ?? {}
  const settingsChecks = residualChecks['attempt11-settings'] ?? {}
  const keyToggle = missingChecks['R-API-KEYS:mutation-toggle']
  const keyGuard = missingChecks['R-API-KEYS:mutation-conflict-guard']
  const keyPage = missingChecks['R-API-KEYS:pagination']
  const endpointFailure =
    missingChecks['R-ENDPOINTS-UPSTREAMS:mutation-failure-draft']
  const endpointUpdateFailure =
    missingChecks['R-ENDPOINTS-UPSTREAMS:update-failure-draft']
  const endpointError = missingChecks['R-ENDPOINTS-UPSTREAMS:error-state']
  const routeGuard = missingChecks['R-ENDPOINTS-ROUTES:mutation-conflict-guard']
  const relayGuard = missingChecks['R-RELAYS:mutation-conflict-guard']
  const redaction =
    missingChecks['R-REDACTION-RULES:mutation-toggle+mutation-failure-draft']
  const redactionPage = missingChecks['R-REDACTION-RULES:pagination']
  const previewGuard =
    missingChecks['R-REDACTION-PREVIEW:mutation-conflict-guard']
  const usageLoading = missingChecks['R-USAGE-AI:loading-state']
  const billingToggle =
    missingChecks['R-BILLING:mutation-toggle+mutation-conflict-guard']
  const approvalGuard = missingChecks['R-APPROVALS:mutation-conflict-guard']
  const approvalError = missingChecks['R-APPROVALS:error-state']
  const requestLogFailure =
    settingsChecks['R-SETTINGS-REQUESTS:request-content-logging-failure-retry']
  const usageRetentionFailure =
    settingsChecks['R-SETTINGS-REQUESTS:usage-retention-failure-retry']
  const streamFailure =
    settingsChecks['R-SETTINGS-REQUESTS:stream-delta-batching-failure-retry']
  const settingsGuard =
    settingsChecks['R-SETTINGS-REQUESTS:mutation-conflict-guard']
  const reviewFailure =
    settingsChecks['R-SETTINGS-REVIEW:mutation-failure-draft']
  const localeTheme =
    settingsChecks['R-SETTINGS-GENERAL:locale-switch+theme-switch']
  assert(
    keyToggle?.statuses?.join(',') === '200,200' &&
      keyToggle.payloads?.[0]?.enabled === false &&
      keyToggle.payloads?.[1]?.enabled === true &&
      keyToggle.targetStates?.join(',') === 'false,true',
    'API-key enable/disable payload and refreshed state evidence missing',
  )
  assert(
    keyGuard?.count === 1 &&
      keyGuard.pendingDisabled &&
      keyGuard.status === 503,
    'API-key shared conflict guard evidence missing',
  )
  assert(
    keyPage?.query?.first === '10' &&
      keyPage.query.rows === '10' &&
      keyPage.firstIds?.length === 10 &&
      keyPage.secondIds?.length === 2 &&
      keyPage.changed &&
      !keyPage.secondIds.some((id: string) => keyPage.firstIds.includes(id)),
    'API-key query and distinct page row-set evidence missing',
  )
  assert(
    [endpointFailure, endpointUpdateFailure].every(
      (check) =>
        check?.retained &&
        check.errorVisible &&
        check.samePayloadRetry &&
        check.closedAfterRetry &&
        check.writes?.length === 2 &&
        check.writes[0].status === 503 &&
        check.writes[1].status === 200,
    ),
    'endpoint create/update 503 draft and same-payload retry evidence missing',
  )
  assert(
    endpointError?.failedRead &&
      endpointError.errorVisible &&
      endpointError.noStaleRows &&
      endpointError.successRead &&
      endpointError.rows > 0,
    'endpoint list error and safe retry evidence missing',
  )
  assert(
    routeGuard?.patchCount === 1 &&
      routeGuard.pendingDisabled &&
      routeGuard.status === 503,
    'model-route conflict guard evidence missing',
  )
  assert(
    relayGuard?.writeCount === 1 &&
      relayGuard.pendingDisabled &&
      relayGuard.status === 503,
    'relay conflict guard evidence missing',
  )
  assert(
    redaction?.toggled &&
      redaction.toggleBody?.rules?.secret === false &&
      redaction.retained &&
      redaction.failureVisible &&
      redaction.samePayloadRetry &&
      redaction.writes?.join(',') === '200,503,200' &&
      typeof redaction.dirtyBadge === 'boolean',
    'redaction toggle payload and server-failure draft/retry evidence missing',
  )
  assert(
    redactionPage?.query?.first === '10' &&
      redactionPage.query.rows === '10' &&
      redactionPage.firstPatterns?.length === 10 &&
      redactionPage.secondPatterns?.length === 2 &&
      redactionPage.changed &&
      !redactionPage.secondPatterns.some((pattern: string) =>
        redactionPage.firstPatterns.includes(pattern),
      ),
    'redaction query and distinct page row-set evidence missing',
  )
  assert(
    previewGuard?.count === 1 &&
      previewGuard.status === 503 &&
      previewGuard.busy &&
      previewGuard.failureVisible,
    'redaction preview shared conflict guard evidence missing',
  )
  assert(
    usageLoading?.loadingVisible &&
      usageLoading.loaded &&
      usageLoading.overviewResponses?.includes(503) &&
      usageLoading.overviewResponses?.includes(200),
    'AI usage delayed loading and retry evidence missing',
  )
  assert(
    billingToggle?.firstBurstCount === 1 &&
      billingToggle.pendingDisabled &&
      billingToggle.count === 2 &&
      billingToggle.writes?.[0]?.status === 503 &&
      billingToggle.writes?.[1]?.status === 200 &&
      billingToggle.writes?.every(
        (write: { body?: { enabled?: boolean } }) =>
          write.body?.enabled === false,
      ) &&
      billingToggle.stateChanged,
    'billing rule toggle payload and conflict guard evidence missing',
  )
  assert(
    approvalGuard?.count === 1 &&
      approvalGuard.alternativeDisabled &&
      approvalGuard.status === 503,
    'approval shared conflict guard evidence missing',
  )
  assert(
    approvalError?.failed &&
      approvalError.errorVisible &&
      approvalError.noStaleRows &&
      approvalError.retrySuccess &&
      approvalError.rowCount > 0,
    'approval list error and safe retry evidence missing',
  )
  assert(
    [requestLogFailure, usageRetentionFailure, streamFailure].every(
      (check) =>
        check?.draftRetained &&
        check.failureVisible &&
        check.failureThenRetry &&
        check.statuses?.join(',') === '503,200',
    ),
    'request-settings section failure/retry evidence missing',
  )
  assert(
    settingsGuard?.heldDisabled &&
      settingsGuard.activeBeforeRelease === 1 &&
      settingsGuard.serialized &&
      settingsGuard.statuses?.join(',') === '503,200',
    'shared request-settings retention conflict guard evidence missing',
  )
  assert(
    reviewFailure?.retained &&
      reviewFailure.failureVisible &&
      reviewFailure.failureThenRetry &&
      reviewFailure.statuses?.join(',') === '503,200',
    'LLM review failure-draft and same-payload retry evidence missing',
  )
  assert(
    localeTheme?.localeStored === 'en-US' &&
      localeTheme.settingsEnglish &&
      localeTheme.themeStored === 'light' &&
      localeTheme.lightTheme,
    'current UI locale/theme switch evidence missing',
  )
  const mcpPagination = residualChecks['mcp-maintenance']?.pagination
  assert(
    mcpPagination?.query?.first === '25' &&
      mcpPagination.query.rows === '25' &&
      mcpPagination.before === 25 &&
      mcpPagination.after === 1 &&
      mcpPagination.changed,
    'MCP usage pagination row-set evidence missing',
  )
  const longValueChecks = residualChecks['attempt11-long-values'] ?? {}
  assert(
    [
      'R-API-KEYS',
      'R-USERS',
      'R-ENDPOINTS-UPSTREAMS',
      'R-ENDPOINTS-ROUTES',
      'R-MCP',
      'R-USAGE-AI',
      'R-BILLING',
    ].every((id) => longValueChecks[id]?.passed === true),
    'one or more required long-value containment checks are missing',
  )
  assert(
    !(evidence as { scenarioFailure?: string }).scenarioFailure,
    `extended scenarios failed: ${(evidence as { scenarioFailure?: string }).scenarioFailure}`,
  )
  assert(evidence.captures.length > 0, 'baseline captured no screenshots')

  for (const capture of evidence.captures) {
    assert(
      existsSync(join(APP_ROOT, capture.path)),
      `missing screenshot ${capture.path}`,
    )
    const expectedFaultCapture = [
      'scenario_settings_network_failure',
      'scenario_archive_import',
      'scenario_archive_export_failure',
      'scenario_relay_host_role_failure',
      'scenario_relay_host_settings_failure',
    ].includes(capture.name)
    const expectedFaultErrors =
      expectedFaultCapture &&
      capture.errors.every((message) => message.includes('500'))
    const expectedAuthErrors =
      capture.name === 'scenario_pagination_api-keys' &&
      capture.errors.every((message) => message.includes('401'))
    const allowed403 =
      capture.name === 'operator_zh_dark_1440_mcp' &&
      capture.errors.every((message) => message.includes('403'))
    assert(
      capture.errors.length === 0 ||
        allowed403 ||
        expectedFaultErrors ||
        expectedAuthErrors,
      `${capture.name} logged console errors: ${capture.errors.join(' | ')}`,
    )
  }

  const names = new Set(evidence.captures.map((capture) => capture.name))
  for (const row of ROUTES) {
    if (row.route === '/login' || row.route === '/relay-host') continue
    const slug = row.route.replaceAll('/', '_')
    for (const prefix of ['admin_zh_dark_1440', 'admin_en_dark_1440']) {
      assert(
        names.has(`${prefix}${slug}`),
        `baseline is missing ${prefix}${slug}`,
      )
    }
  }

  for (const redirect of evidence.checks.redirects ?? []) {
    assert(
      redirect.actual === redirect.expected,
      `redirect ${redirect.id}: expected ${redirect.expected}, got ${redirect.actual}`,
    )
  }
  assert(
    evidence.checks.login?.landed === '/api-keys',
    'login did not reach the first visible section',
  )
  assert(
    evidence.checks.login?.invalidAlertVisible === true,
    'invalid credentials did not render an error',
  )

  const nonAdmin = evidence.checks.nonAdmin
  assert(nonAdmin, 'non-admin baseline evidence is missing')
  if (nonAdmin) {
    for (const entry of nonAdmin.allowed) {
      assert(
        entry.actual === entry.route,
        `non-admin route ${entry.route} landed on ${entry.actual}`,
      )
    }
    for (const entry of nonAdmin.denied) {
      assert(
        entry.actual === '/api-keys',
        `admin-only route ${entry.route} was not denied for a non-admin`,
      )
    }
  }

  const relay = evidence.checks.relayHost
  assert(
    relay?.relaySession?.authenticated === true,
    'relay session did not authenticate',
  )
  assert(
    relay?.workerAfterRelay?.status === 401,
    'relay session leaked a Worker session',
  )
  assert(
    String(relay?.workerRouteActual ?? '').startsWith('/login'),
    'Worker route stayed open without a Worker session',
  )

  assert(
    evidence.checks.conflictGuard?.dispatched === 1,
    `conflict guard dispatched ${evidence.checks.conflictGuard?.dispatched} writes for two triggers`,
  )
  assert(
    evidence.checks.failureDraft?.dialogStillOpen === true &&
      evidence.checks.failureDraft?.draftValue === 'fixture-retryable-draft',
    'failed mutation did not preserve its dialog or draft',
  )
  assert(
    evidence.checks.configExport?.downloadSuggested ===
      'prompt-ferry-config.pfce',
    'config export did not produce the fixed archive filename',
  )

  for (const fold of evidence.checks.zeroTokenFold ?? []) {
    assert(fold.foldedLabelPresent, `${fold.locale}: fold label is missing`)
    assert(
      fold.expandedRows > fold.collapsedRows,
      `${fold.locale}: expanding did not reveal folded rows`,
    )
    assert(
      fold.mixedZeroTokenVisible,
      `${fold.locale}: the 0-token mixed-failure row is not visible`,
    )
    assert(
      fold.headerOverflow === true,
      `${fold.locale}: the baseline header overlap regression is not recorded`,
    )
  }

  const expectedWrites: Record<string, RegExp> = {
    'api-key-create': /^POST \/api\/v1\/me\/client-keys -> 200$/,
    'api-key-create-failure-draft': /^POST \/api\/v1\/me\/client-keys -> 500$/,
    'endpoint-toggle-conflict-guard':
      /^PATCH \/api\/v1\/admin\/endpoints\/[^ ]+ -> 200$/,
    'model-route-toggle':
      /^PATCH \/api\/v1\/admin\/model-routes\/[^ ]+ -> 200$/,
    'mcp-toggle': /^PATCH \/api\/v1\/admin\/mcp-servers\/[^ ]+ -> 200$/,
    'relay-reconnect':
      /^POST \/api\/v1\/admin\/relays\/[^ ]+\/reconnect -> 200$/,
    'approval-approve':
      /^POST \/api\/v1\/admin\/approvals\/[^ ]+\/approve -> 200$/,
    'redaction-save': /^PATCH \/api\/v1\/settings\/redaction -> 200$/,
    'settings-toggle': /^PATCH \/api\/v1\/settings\/[^ ]+ -> 200$/,
    'config-export': /^POST \/api\/v1\/admin\/config-export -> 200$/,
  }
  for (const mutation of evidence.mutations) {
    assert(!mutation.error, `${mutation.name} failed: ${mutation.error}`)
    const expected = expectedWrites[mutation.name]
    if (expected) {
      assert(
        mutation.requests.some((entry) => expected.test(entry)),
        `${mutation.name} did not issue its expected write`,
      )
    }
  }
  notes.push(
    `captures: ${evidence.captures.length}, mutations: ${evidence.mutations.length}`,
  )
}

const extended = JSON.parse(read(evidencePath)) as {
  startedAt?: string
  finishedAt?: string
  runId?: string
  runManifest?: {
    runId?: string
    startedAt?: string
    finishedAt?: string
    callbacks?: Array<{
      name: string
      runId: string
      startedAt: string
      finishedAt: string
    }>
  }
  checks: {
    csvDownloads?: { details: string; monthly: string }
    [key: string]: any
  }
  captures?: Array<Record<string, any>>
  mutations?: Array<Record<string, any>>
  residual?: Array<{
    name: string
    runId?: string
    report: Record<string, any>
  }>
  scenarioCoverage?: Record<
    string,
    Record<string, { passed: boolean; evidence: string }>
  >
  knownBaselineDefects?: Array<{
    rowId: string
    scenario: string
    disposition: string
    evidence: string
    futureFixId: string
    targetRequired: boolean
  }>
}
const extendedResidualChecks = Object.fromEntries(
  (extended.residual ?? []).map(({ name, report }) => [
    name,
    report.checks ?? {},
  ]),
) as Record<string, Record<string, any>>
if (extended.checks.csvDownloads) {
  for (const kind of ['details', 'monthly'] as const) {
    assert(
      extended.checks.csvDownloads[kind] === `prompt-ferry-billing-${kind}.csv`,
      `billing ${kind} filename mismatch`,
    )
    const csvPath = join(E2E_ROOT, `baseline/downloads/billing-${kind}.csv`)
    assert(existsSync(csvPath), `missing billing ${kind} download`)
    if (existsSync(csvPath)) {
      const csv = read(csvPath).replace(/^\uFEFF/, '')
      assert(
        kind === 'details'
          ? csv.includes('request_id,model,user,amount,currency,status') &&
              csv.includes('req-charge-1')
          : csv.includes('month,model,requests,customer_amount') &&
              csv.includes('2026-10,minimax-m2'),
        `billing ${kind} content mismatch`,
      )
    }
  }
}

// ------------------------------------------------------ current-run evidence
const expectedCallbacks = [
  'capture',
  'scenarios',
  'four-slices',
  'form-payloads',
  'advanced',
  'auth-quota-negative',
  'upstream-payloads',
  'quota-stale',
  'mcp-maintenance',
  'domain-retries',
  'relay-secrets',
  'mcp-secrets',
  'endpoint-secrets',
  'settings-secrets',
  'billing-filters',
  'attempt11-missing',
  'attempt11-settings',
  'attempt11-long-values',
]
const runManifest = extended.runManifest
assert(Boolean(extended.runId), 'baseline run id is missing')
assert(
  runManifest?.runId === extended.runId,
  'baseline run manifest id is stale',
)
assert(
  runManifest?.startedAt === extended.startedAt,
  'baseline start time does not match its run manifest',
)
assert(
  runManifest?.finishedAt === extended.finishedAt,
  'baseline finish time does not match its run manifest',
)
const callbackRuns = runManifest?.callbacks ?? []
assert(
  callbackRuns.map((callback) => callback.name).join(',') ===
    expectedCallbacks.join(','),
  'current run callback inventory is missing, duplicated, unknown, or out of order',
)
let previousFinished = Number.NEGATIVE_INFINITY
for (const callback of callbackRuns) {
  const started = Date.parse(callback.startedAt)
  const finished = Date.parse(callback.finishedAt)
  assert(
    callback.runId === extended.runId,
    `${callback.name} has stale run evidence`,
  )
  assert(
    Number.isFinite(started) &&
      Number.isFinite(finished) &&
      finished >= started,
    `${callback.name} has invalid run timestamps`,
  )
  assert(
    started >= previousFinished,
    `${callback.name} overlaps or predates the previous callback`,
  )
  previousFinished = finished
}
assert(
  callbackRuns.at(-1)?.finishedAt === extended.finishedAt,
  'final callback timestamp is stale',
)
for (const item of extended.residual ?? []) {
  assert(
    item.runId === extended.runId,
    `${item.name} residual has a stale run id`,
  )
  assert(
    item.report.runId === extended.runId,
    `${item.name} report has a stale run id`,
  )
  const manifestEntry = callbackRuns.find(
    (callback) => callback.name === item.name,
  )
  assert(
    manifestEntry?.startedAt === item.report.startedAt &&
      manifestEntry.finishedAt === item.report.finishedAt,
    `${item.name} report timestamps do not match the current run manifest`,
  )
}

// ---------------------------------------------------------- scenario coverage
const coverage = extended.scenarioCoverage ?? {}
const routeIds = ROUTES.map((row) => row.id).sort()
assert(
  Object.keys(coverage).sort().join(',') === routeIds.join(','),
  'scenario coverage has missing or unknown route rows',
)
const defectRows = new Set(['R-USAGE-AI', 'R-BILLING'])
for (const row of ROUTES) {
  const rowCoverage = coverage[row.id] ?? {}
  const required = [...row.requiredScenarios].sort()
  assert(
    Object.keys(rowCoverage).sort().join(',') === required.join(','),
    `${row.id} scenario coverage has a missing or unknown required scenario`,
  )
  for (const scenario of row.requiredScenarios) {
    const entry = rowCoverage[scenario]
    assert(
      entry?.passed === true,
      `${row.id}:${scenario} has no passing current-run assertion`,
    )
    assert(
      Boolean(entry?.evidence),
      `${row.id}:${scenario} has no evidence pointer`,
    )
    assert(
      entry?.evidence &&
        resolveCoverageEvidence(extended as any, entry.evidence) !== undefined,
      `${row.id}:${scenario} points to missing or stale evidence ${entry?.evidence ?? ''}`,
    )
  }
  if (defectRows.has(row.id)) {
    assert(
      row.status === 'baseline-partial',
      `${row.id} approved baseline defect must remain partial`,
    )
  } else {
    assert(
      row.status === 'baseline-verified',
      `${row.id} has complete coverage but was not promoted`,
    )
  }
  if (row.status === 'baseline-verified') {
    assert(row.evidence.length > 0, `${row.id} is verified without evidence`)
    for (const file of row.evidence) {
      assert(
        existsSync(join(APP_ROOT, file)),
        `${row.id} references a missing evidence file ${file}`,
      )
    }
  }
}

const expectedDefects = [
  {
    rowId: 'R-USAGE-AI',
    scenario: 'sorting',
    futureFixId: 'REACT-AI-SORT-ACCESSIBLE',
  },
  {
    rowId: 'R-BILLING',
    scenario: 'rule-pagination',
    futureFixId: 'REACT-BILLING-RULES-PAGINATION',
  },
]
const knownDefects = extended.knownBaselineDefects ?? []
assert(
  knownDefects.length === expectedDefects.length &&
    expectedDefects.every((expected) =>
      knownDefects.some(
        (actual) =>
          actual.rowId === expected.rowId &&
          actual.scenario === expected.scenario &&
          actual.disposition === 'known-baseline-defect' &&
          actual.futureFixId === expected.futureFixId &&
          actual.targetRequired === true &&
          Boolean(actual.evidence) &&
          resolveCoverageEvidence(extended as any, actual.evidence) !==
            undefined,
      ),
    ),
  'approved baseline defects or mandatory React fix references are missing',
)
const sortDefect = extendedResidualChecks['four-slices']?.sort
const billingDefect = extendedResidualChecks['four-slices']?.billing
assert(
  sortDefect?.disposition === 'known-baseline-defect' &&
    sortDefect.before === 'none' &&
    sortDefect.after === 'none' &&
    sortDefect.requests?.length === 0 &&
    sortDefect.buttons === 0 &&
    sortDefect.futureFixId === 'REACT-AI-SORT-ACCESSIBLE',
  'approved AI sorting defect evidence changed or is missing',
)
assert(
  billingDefect?.ruleChanged === false &&
    billingDefect.rulesUnchanged === true &&
    billingDefect.ruleRequests?.length === 0 &&
    FOCUSED_SCENARIO_EVIDENCE['R-BILLING:rule-pagination'].futureFixId ===
      'REACT-BILLING-RULES-PAGINATION',
  'approved billing rule-pagination defect evidence changed or is missing',
)

if (failures.length > 0) {
  console.error(`parity matrix validation failed (${failures.length}):`)
  for (const failure of failures) console.error(` - ${failure}`)
  process.exit(1)
}

console.log('parity matrix OK')
console.log(
  ` routes=${ROUTES.length} redirects=${REDIRECTS.length} scenarios=${SCENARIOS.length}`,
)
for (const note of notes) console.log(` ${note}`)
