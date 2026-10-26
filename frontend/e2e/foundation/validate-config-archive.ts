import { existsSync, readFileSync, rmSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { ROUTES } from '../parity.matrix.ts'

type ScenarioEntry = { passed?: boolean; evidence?: string }

type ConfigArchiveReport = {
  runId?: string
  startedAt?: string
  finishedAt?: string
  checks?: Record<string, any>
  scenarios?: Record<string, Record<string, ScenarioEntry>>
  screenshots?: string[]
  apiFailures?: string[]
  expectedApiFailures?: string[]
  unexpectedRequests?: string[]
  consoleErrorCount?: number
  pageErrorCount?: number
  limitations?: string[]
}

const root = import.meta.dir
const reportPath = join(root, 'config-archive-result.json')
const authShellPath = join(root, 'auth-shell.playwright')
const playwrightPath = join(root, 'config-archive.playwright')
const reactTestPath = join(root, '../src/react-tests/config-archive.test.tsx')
const downloadPath = join(root, 'config-archive-download.pfce')
const failures: string[] = []

function assert(condition: unknown, message: string): void {
  if (!condition) failures.push(message)
}

const REQUIRED_SCENARIOS = [
  'route-render',
  'admin-denial',
  'pagination',
  'file-export',
  'file-import',
  'destructive-confirm',
  'mutation-failure-draft',
]

const REQUIRED_SCREENSHOTS = [
  'e2e/foundation/screenshots/config-archive-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-export-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-preview-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-confirm-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-applied-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-failure-en-light-1440.png',
  'e2e/foundation/screenshots/config-archive-en-light-390.png',
  'e2e/foundation/screenshots/config-archive-zh-dark-1440.png',
  'e2e/foundation/screenshots/config-archive-zh-dark-390.png',
]

const ALLOWED_FAILURES = [
  /^401 GET \/api\/v1\/auth\/me$/,
  /^400 POST \/api\/v1\/admin\/config-import$/,
  /^400 POST \/api\/v1\/admin\/config-import\/preview$/,
  /^422 POST \/api\/v1\/admin\/config-import\/preview$/,
  /^500 POST \/api\/v1\/admin\/config-import\/preview$/,
  /^501 POST \/api\/v1\/admin\/config-export$/,
  /^501 GET \/api\/v1\/admin\/config-audit$/,
  /^501 POST \/api\/v1\/admin\/config-import\/preview$/,
  /^503 POST \/api\/v1\/admin\/config-import$/,
  /^403 (GET \/api\/v1\/admin\/config-audit|POST \/api\/v1\/admin\/config-(export|import|import\/preview))$/,
  /^401 GET \/api\/v1\/admin\/config-audit$/,
]

assert(
  existsSync(reportPath),
  'focused config-archive report is missing; run run-config-archive.sh',
)

const row = ROUTES.find((route) => route.id === 'R-SETTINGS-CONFIG-ARCHIVE')

assert(
  row?.foundationTest === 'e2e/foundation/config-archive.playwright',
  'R-SETTINGS-CONFIG-ARCHIVE is not mapped to the focused config-archive browser run',
)
assert(
  JSON.stringify([...(row?.requiredScenarios ?? [])].sort()) ===
    JSON.stringify([...REQUIRED_SCENARIOS].sort()),
  'R-SETTINGS-CONFIG-ARCHIVE no longer requires the seven verified scenarios',
)
assert(
  JSON.stringify([...(row?.reads ?? [])]) ===
    JSON.stringify(['GET /api/v1/admin/config-audit']),
  'the archive row does not read exactly the configuration audit trail',
)
assert(
  JSON.stringify([...(row?.mutationKeys ?? [])]) ===
    JSON.stringify(['configArchive:export', 'configArchive:import']),
  'the archive row does not declare the real export and import keys',
)
assert(
  row?.gate === 'P5-writes' && row?.domain === 'settings',
  'the archive row lost its settings/write gate identity',
)
assert(
  JSON.stringify([...(row?.legacyTests ?? [])].sort()) ===
    JSON.stringify(
      [
        'config-archive-actions.fixtures.ts',
        'config-archive-actions.test.ts',
        'config-archive.test.ts',
      ].sort(),
    ),
  'the archive row no longer maps the three legacy archive test files',
)
assert(
  REQUIRED_SCREENSHOTS.every((screenshot) =>
    row?.evidence.includes(screenshot),
  ),
  'the archive row does not cite the current-run screenshots',
)

if (existsSync(authShellPath)) {
  const authShell = readFileSync(authShellPath, 'utf8')
  const pendingRoutes =
    authShell.match(/const pendingRoutes = \[([\s\S]*?)\n  \]/)?.[1] ?? ''
  const pendingIds = [
    ...pendingRoutes.matchAll(/\['\/[^']+', '([^']+)'\]/g),
  ].map((match) => match[1] ?? '')
  assert(
    pendingIds.length === 0,
    'the shared placeholder list still holds a settings route',
  )
}

if (existsSync(playwrightPath)) {
  const source = readFileSync(playwrightPath, 'utf8')
  assert(
    !/body:\s*request\.postData\(\)/.test(source),
    'the archive request ledger records the raw request body',
  )
  assert(
    source.includes('passphrase_present') &&
      source.includes('passphrase_chars') &&
      source.includes('archive_base64_chars') &&
      !/body:\s*request\.postData/.test(source),
    'the ledger does not reduce the archive body to a presence flag and a length',
  )
  assert(
    source.includes('content-disposition') &&
      source.includes('server-chosen-name.pfce'),
    'the run does not prove the fixed download name against a server filename',
  )
}

if (existsSync(reactTestPath)) {
  const test = readFileSync(reactTestPath, 'utf8')
  for (const expected of [
    'configArchivePreview',
    'configArchiveImportRequestEnabled',
    'importBusy',
    'same file twice',
    'keeps the confirmed wizard on a 503',
    'opens the confirmation with zero requests',
  ]) {
    if (expected === 'importBusy') continue
    assert(
      test.includes(expected),
      `the active suite does not cover "${expected}"`,
    )
  }
}

if (existsSync(reportPath)) {
  const report = JSON.parse(
    readFileSync(reportPath, 'utf8'),
  ) as ConfigArchiveReport
  const checks = report.checks ?? {}
  const scenarios = report.scenarios ?? {}
  const serialized = JSON.stringify(report)

  assert(
    typeof report.runId === 'string' &&
      report.runId.startsWith('config-archive-'),
    'report is not from the config-archive runner',
  )
  const runTimestamp = Number(report.runId?.slice('config-archive-'.length))
  const startedAt = Date.parse(report.startedAt ?? '')
  const finishedAt = Date.parse(report.finishedAt ?? '')
  assert(
    Number.isFinite(runTimestamp) &&
      Number.isFinite(startedAt) &&
      Math.abs(runTimestamp - startedAt) <= 1000,
    'the runId is missing, stale, or inconsistent with startedAt',
  )
  assert(
    Number.isFinite(finishedAt) &&
      finishedAt >= startedAt &&
      Date.now() - finishedAt <= 20 * 60 * 1000,
    'the archive report is stale or has invalid run timestamps',
  )

  assert(
    !serialized.includes('synthetic-browser-archive-passphrase') &&
      !serialized.includes('synthetic-forged-passphrase'),
    'the report contains a synthetic passphrase',
  )
  assert(
    !/"[A-Za-z0-9+/]{120,}={0,2}"/.test(serialized),
    'the report contains a long base64 archive body',
  )

  assert(
    JSON.stringify(
      Object.keys(scenarios['R-SETTINGS-CONFIG-ARCHIVE'] ?? {}).sort(),
    ) === JSON.stringify([...REQUIRED_SCENARIOS].sort()),
    'the archive route does not carry exactly the seven required scenarios',
  )
  for (const scenario of REQUIRED_SCENARIOS) {
    const entry = scenarios['R-SETTINGS-CONFIG-ARCHIVE']?.[scenario]
    assert(
      entry?.passed === true && Boolean(entry?.evidence),
      `R-SETTINGS-CONFIG-ARCHIVE:${scenario} has no passing current-run assertion`,
    )
  }

  const routeRender = checks.routeRender ?? {}
  const controls = routeRender.controls ?? {}
  assert(
    routeRender.routeId === 'R-SETTINGS-CONFIG-ARCHIVE' &&
      routeRender.routeTab === 'config-archive',
    'the archive route identity is unverified',
  )
  assert(
    Array.isArray(controls.missing) && controls.missing.length === 0,
    'a required archive control is missing',
  )
  assert(
    controls.exportType === 'password' && controls.importType === 'password',
    'a passphrase field is not a password input',
  )
  assert(
    controls.fileAccept === '.pfce,application/octet-stream',
    'the file input does not accept only the archive type',
  )
  assert(
    controls.exportDisabled === true &&
      controls.previewDisabled === true &&
      controls.applyDisabled === true,
    'an action is enabled without its input',
  )
  assert(
    controls.auditRows > 0 && controls.auditColumns === 9,
    'the audit table rows or nine columns are wrong',
  )
  assert(controls.bodyLeak === false, 'a passphrase reached the document')

  const pagination = checks.pagination ?? {}
  assert(
    pagination.storedPageSize === '20' && pagination.rowsAfterSize > 0,
    'the page size preference is not applied or persisted',
  )
  assert(
    pagination.pageOneFirst !== pagination.pageTwoFirst &&
      pagination.pageTwoRows > 0,
    'the second audit page repeats the first or is empty',
  )

  const exportBytes = checks.exportBytes ?? {}
  assert(
    exportBytes.magic === 'PROMPT_FERRY_SYNTHETIC_ARCHIVE' &&
      exportBytes.length > 0,
    'the exported bytes are not the synthetic archive envelope',
  )
  assert(
    exportBytes.headerBackend === 'postgresql' &&
      exportBytes.headerFormatVersion === '1' &&
      /^[0-9a-f]{64}$/.test(exportBytes.headerFingerprint ?? ''),
    'the export metadata headers are wrong',
  )

  const fileExport = checks.fileExport ?? {}
  assert(
    fileExport.suggested === 'prompt-ferry-config.pfce',
    'the download did not use the fixed file name',
  )

  const fileImport = checks.fileImport ?? {}
  assert(
    Array.isArray(fileImport.domains) && fileImport.domains.length === 3,
    'the preview does not list the three domains',
  )
  assert(
    typeof fileImport.warnings === 'string' &&
      fileImport.warnings.includes('unrecoverable secret'),
    'the preview does not report its warnings',
  )
  assert(
    fileImport.body?.keys?.join(',') === 'archive_base64,passphrase' &&
      fileImport.body?.archive_base64_present === true &&
      fileImport.body?.passphrase_present === true,
    'the preview body is not exactly the archive and the passphrase',
  )
  assert(
    fileImport.auditUnchanged === true,
    'a preview wrote an audit row or refreshed the trail',
  )

  const confirm = checks.destructiveConfirm ?? {}
  assert(
    typeof confirm.dialogBody === 'string' &&
      confirm.dialogBody.includes('cannot be undone'),
    'the destructive warning is not shown',
  )
  assert(
    Array.isArray(confirm.domains) && confirm.domains.length === 3,
    'the applied summary does not list every restored domain',
  )
  assert(
    confirm.body?.keys?.join(',') === 'archive_base64,passphrase',
    'the applied body is not exactly the archive and the passphrase',
  )
  assert(
    typeof confirm.latestAuditRow === 'string' &&
      confirm.latestAuditRow.includes('Import'),
    'the committed import did not add its audit row',
  )

  const failure = checks.failureDraft ?? {}
  assert(
    failure.wrongPassphrase?.error?.includes('archive_decrypt_failed') &&
      failure.wrongPassphrase?.passphrasePresent === false &&
      failure.wrongPassphrase?.file === true &&
      failure.wrongPassphrase?.preview === false,
    'a rejected passphrase did not clear itself and keep the file',
  )
  assert(
    String(failure.corrupt ?? '').includes('archive_invalid_header'),
    'the corrupt archive did not answer archive_invalid_header',
  )
  assert(
    String(failure.version ?? '').includes('archive_unsupported_version'),
    'the future-version archive did not answer archive_unsupported_version',
  )
  assert(
    String(failure.backend ?? '').includes('archive_backend_mismatch'),
    'the foreign-backend archive did not answer archive_backend_mismatch',
  )
  assert(
    failure.failed?.dialog === true &&
      failure.failed?.preview === true &&
      failure.failed?.passphrasePresent === true &&
      typeof failure.failed?.error === 'string' &&
      failure.failed.error !== '',
    'a 503 did not keep the confirmed wizard with its error',
  )
  assert(
    failure.writes === 2,
    `the failed and retried apply sent ${failure.writes} writes`,
  )
  assert(
    failure.doubleExport === 1,
    `a double export sent ${failure.doubleExport} requests`,
  )
  const capabilities = checks.capabilities ?? {}
  assert(
    capabilities.audit === true &&
      capabilities.export === true &&
      capabilities.import === true &&
      capabilities.recovered === true,
    'the three capability codes are not distinct and recoverable',
  )

  const responsive = checks.responsive as
    | Array<{ width?: number; documentWidth?: number; headers?: number }>
    | undefined
  assert(
    responsive?.length === 4,
    'the responsive profile does not cover four widths',
  )
  for (const width of [1440, 1024, 768, 390]) {
    const entry = responsive?.find((item) => item.width === width)
    assert(
      (entry?.documentWidth ?? width + 1) <= width,
      `the archive tab overflows horizontally at ${width}px`,
    )
    assert(entry?.headers === 9, `the audit table drops a column at ${width}px`)
  }

  const denial = checks.adminDenial ?? {}
  assert(
    denial.navHidden === true && denial.redirected === true,
    'an ordinary user was not kept out of the archive route',
  )
  assert(
    denial.forged?.audit === 403 &&
      denial.forged?.export === 403 &&
      denial.forged?.preview === 403 &&
      denial.forged?.apply === 403,
    'the fixture did not refuse every forged archive route with 403',
  )
  assert(
    denial.generalIntact === true &&
      denial.redactionIntact === 200 &&
      denial.anonymousRead === 401,
    'the ordinary-user general/redaction behaviour or the anonymous refusal regressed',
  )

  assert(
    checks.officialPendingRoutes === 0,
    'the report does not record that no settings placeholder remains',
  )
  assert(
    Array.isArray(report.unexpectedRequests) &&
      report.unexpectedRequests.length === 0,
    'the archive tab issued an unexpected business request',
  )
  assert(
    Array.isArray(report.apiFailures) && report.apiFailures.length === 0,
    'unexpected API responses were recorded',
  )
  assert(
    (report.expectedApiFailures ?? []).length > 0 &&
      (report.expectedApiFailures ?? []).every((entry) =>
        ALLOWED_FAILURES.some((pattern) => pattern.test(entry)),
      ),
    'an expected API failure outside the allowlist was recorded',
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
      `the required archive screenshot is missing: ${screenshot}`,
    )
  }
  assert(
    Array.isArray(report.limitations) && report.limitations.length === 2,
    'the report does not record its evidence limitations truthfully',
  )

  // The saved download is disposable evidence: check it, then remove it.
  if (existsSync(downloadPath)) {
    const size = statSync(downloadPath).size
    const head = readFileSync(downloadPath).subarray(0, 32).toString('utf8')
    assert(
      size === exportBytes.length,
      `the saved download is ${size} bytes, the response was ${exportBytes.length}`,
    )
    assert(
      head.startsWith('PROMPT_FERRY_SYNTHETIC_ARCHIVE'),
      'the saved download does not carry the synthetic magic',
    )
    rmSync(downloadPath)
  } else {
    failures.push('the saved download evidence is missing')
  }
}

if (failures.length > 0) {
  console.error('Config archive focused browser verification failed:')
  for (const failure of failures) console.error(`- ${failure}`)
  process.exit(1)
}

console.log(
  'Config archive focused browser verification passed; removed the disposable synthetic download copy.',
)
