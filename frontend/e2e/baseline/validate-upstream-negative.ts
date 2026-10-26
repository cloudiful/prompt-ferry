import { readFileSync } from 'node:fs'
const read = (file: string) =>
  JSON.parse(readFileSync(new URL(file, import.meta.url), 'utf8'))
function check(ok: unknown, message: string) {
  if (!ok) throw new Error(message)
}
const u = read('./upstream-payloads.json')
const n = read('./auth-quota-negative.json')
check(!u.failures.length && !n.failures.length, 'callback failure')
const s = u.checks.subscription
check(
  s.keyInputs === 0 &&
    s.saved.body.plan === 'chatgpt_subscription' &&
    s.saved.body.api_keys.length === 0,
  'subscription keyless payload',
)
const p = u.checks.platform
check(
  p.keyEditorVisible && p.saved.body.plan === 'platform_api_key',
  'platform key mode',
)
check(
  p.saved.body.api_keys.length === 1 &&
    p.saved.body.api_keys[0].key_label === 'fixture-platform-row' &&
    p.saved.body.api_keys[0].api_key === '[synthetic-secret-present]',
  'platform key payload redaction',
)
const t = u.checks.transition
check(
  t.protocol.body.protocol_mode === 'manual' &&
    t.protocol.body.native_api_override === 'embeddings' &&
    t.roundtrip === 'Embeddings',
  'protocol roundtrip',
)
check(
  t.providerReset.body.provider === 'generic' &&
    t.providerReset.body.plan === 'platform_api_key' &&
    t.providerReset.body.provider_region === null &&
    !t.providerReset.body.mcp_enabled,
  'provider reset',
)
const o = n.checks.oauth
check(
  o.deviceFailure.some((r: any) => r.status === 500) &&
    o.browserFailure.some((r: any) => r.status === 500),
  'negative OAuth responses',
)
check(
  o.dismissalBlocked && o.closed && o.statusAfterCancel,
  'pending dismissal/navigate cancellation',
)
check(
  o.afterClose.filter((r: any) => r.path.endsWith('/device/poll')).length <=
    1 && !o.afterClose.some((r: any) => r.path.endsWith('/oauth')),
  'stale OAuth poll must not trigger success status refresh',
)
check(
  n.checks.quota.failed.some((r: any) => r.status === 500) &&
    n.checks.quota.retry.some(
      (r: any) => r.status === 200 && r.path.endsWith('/quota-observations'),
    ),
  'quota list error/retry',
)
console.log(
  'Upstream key/plan/provider/protocol and OAuth cancel/negative/quota-list retry assertions OK; stale quota/domain closure pending',
)
