import { readFileSync } from 'node:fs'
const r = JSON.parse(
  readFileSync(new URL('./four-slices.json', import.meta.url), 'utf8'),
)
function check(ok: unknown, message: string): void {
  if (!ok) throw new Error(message)
}
check(r.failures.length === 0, 'focused callback failed')
const { archive: a, billing: b, sort: s, redaction: d } = r.checks
check(
  a.cancelNoPost && a.cancelCleared,
  'archive cancel must clear wizard without POST',
)
check(
  a.failed?.status === 500 && a.success?.status === 200,
  'archive failure/retry missing',
)
check(
  a.failed.body.archive_base64 === a.success.body.archive_base64,
  'archive retry changed bytes',
)
check(
  a.success.body.passphrase === 'fixture-passphrase',
  'archive passphrase payload missing',
)
check(
  a.auditRefreshed && a.auditVisible && a.text.includes('配置已导入'),
  'import success/audit missing',
)
check(
  b.chargeChanged && b.rulesUnchanged && b.chargesUnchanged,
  'billing pager independence',
)
check(
  b.chargeRequests.some(
    (r: any) => r.query.first === '25' && r.query.rows === '25',
  ),
  'charge page request',
)
check(
  b.filtered
    .filter(
      (r: any) => r.path.endsWith('/summary') || r.path.endsWith('/charges'),
    )
    .every((r: any) => r.query.requested_model === 'minimax-m2'),
  'filter requests',
)
check(
  b.filteredModels.length === b.summaryCount &&
    b.filteredModels.every((m: string) => m === 'minimax-m2'),
  'honest filter summary',
)
check(
  s.disposition === 'known-baseline-defect' &&
    s.requests.length === 0 &&
    s.before === s.after &&
    s.buttons === 0 &&
    s.source &&
    s.futureFixId,
  'inert sort defect evidence',
)
check(
  d.invalid === 'true' && d.noInvalidPatch && d.corrected.status === 200,
  'regex validation evidence',
)
check(
  d.searchValues.length === 1 && d.searchValues[0] === 'fixture-secret-10',
  'server case-insensitive search',
)
check(
  d.requests.some(
    (r: any) => r.query.search === 'SECRET-10' && r.query.first === '0',
  ),
  'search request',
)
check(
  d.duplicates.signatures.length === 2 &&
    new Set(d.duplicates.signatures.map((r: any) => r.scope)).size === 2,
  'duplicate dedupe/conflict retention',
)
check(
  d.duplicates.warningText.includes('存在完全相同的规则') &&
    d.duplicates.conflictText.includes('同名的模式匹配方式/作用域不同'),
  'warnings missing',
)
console.log(
  'Four-slice recorded assertions OK; billing rule pager remains unresolved (not waived):',
  b.ruleChanged,
  b.ruleRequests,
)
