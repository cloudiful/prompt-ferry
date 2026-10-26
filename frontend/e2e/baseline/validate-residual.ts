import { strict as assert } from 'node:assert'
import { readFileSync } from 'node:fs'

const read = (name: string) =>
  JSON.parse(readFileSync(new URL(name, import.meta.url), 'utf8'))
const mcp = read('./mcp-maintenance.json')
assert.deepEqual(mcp.failures, [])
assert.equal(mcp.scenario, 'R-USAGE-MCP:detail-maintenance')
assert.ok(mcp.checks.initialRows > 0)
assert.ok(mcp.checks.detail.includes('请求载荷'))
assert.ok(mcp.checks.detail.includes('响应载荷'))
assert.equal(mcp.checks.cancelWrites, 0)
assert.equal(mcp.checks.writes.length, 1)
assert.equal(mcp.checks.writes[0].status, 200)
assert.ok(
  (mcp.checks.emptyStateObserved ?? mcp.checks.after).includes(
    '暂无 MCP 调用记录',
  ),
)
const retries = read('./domain-retries.json')
assert.deepEqual(retries.failures, [])
for (const id of ['R-USERS', 'R-MCP', 'R-RELAYS']) {
  const check = retries.checks[id]
  assert.equal(check.draftRetained, true)
  assert.equal(check.closedOnlyOnSuccess, true)
  assert.deepEqual(
    check.writes.map((write: { status: number }) => write.status),
    [503, 200],
  )
  assert.equal(check.writes[0].path, check.writes[1].path)
}
console.log(
  'MCP detail/maintenance and three domain create retries verified; other residual scenarios remain pending.',
)
