import { readFileSync } from 'node:fs'
const r = JSON.parse(
  readFileSync(new URL('./quota-stale.json', import.meta.url), 'utf8'),
)
if (r.failures.length || r.scenario !== 'R-ENDPOINTS-UPSTREAMS:quota-stale')
  throw new Error('quota callback failed')
for (const label of [
  '缓存观测',
  '刷新失败，仍显示最近一次成功观测。',
  '下次重试：',
]) {
  if (!r.checks.staleText.includes(label) || r.checks.freshText.includes(label))
    throw new Error(`stale/fresh label mismatch: ${label}`)
}
for (const text of [r.checks.staleText, r.checks.freshText]) {
  if (!text.includes('剩余 62.0%') || !text.includes('38.00 / 100.00'))
    throw new Error('retained observation values missing')
}
console.log('Quota stale retained values and fresh-read label reset OK')
