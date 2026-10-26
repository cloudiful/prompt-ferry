import { readFileSync } from 'node:fs'
const r = JSON.parse(
  readFileSync(new URL('./advanced.json', import.meta.url), 'utf8'),
)
function check(ok: unknown, message: string) {
  if (!ok) throw new Error(message)
}
check(r.failures.length === 0, 'advanced callback failed')
const o = r.checks.oauth
check(
  o.deviceComplete && o.browserComplete,
  'OAuth did not complete through UI',
)
check(
  o.device.filter((q: any) => q.path.endsWith('/device/poll')).length === 2,
  'device pending/complete polling not exercised',
)
check(
  o.browser.some(
    (q: any) => q.path.endsWith('/browser/complete') && q.status === 200,
  ),
  'browser completion absent',
)
check(
  o.refresh.some((q: any) => q.path.endsWith('/refresh') && q.status === 200),
  'refresh absent',
)
check(
  r.checks.oauthFailure.dialogRetained &&
    r.checks.oauthFailure.requests.some((q: any) => q.status === 500),
  'OAuth failure not retryable',
)
check(
  r.checks.quota.text.includes('62.0%') &&
    r.checks.quota.history.requests.some((q: any) =>
      q.path.endsWith('/quota-observations/trend'),
    ),
  'quota history missing',
)
check(r.checks.emptyModels.text.includes('没有数据'), 'empty model UI missing')
console.log(
  'Advanced OAuth/quota/empty-model recorded assertions OK; other advanced/domain scenarios remain pending',
)
