import { expect, test } from 'bun:test'
import { upstreamLabel } from '../src/admin-mappers/request-records'

test('upstreamLabel returns endpoint name when upstream_model is empty', () => {
  expect(
    upstreamLabel({ endpoint_name: 'opencode go', upstream_model: null }),
  ).toBe('opencode go')
})

test('upstreamLabel ignores upstream model and returns endpoint name', () => {
  expect(
    upstreamLabel({
      endpoint_name: 'opencode go',
      upstream_model: 'muse-spark-1.3-contri',
    }),
  ).toBe('opencode go')
})

test('upstreamLabel ignores upstream model even when different from endpoint name', () => {
  expect(upstreamLabel({ endpoint_name: 'glm', upstream_model: 'glm' })).toBe(
    'glm',
  )
  expect(upstreamLabel({ endpoint_name: 'glm', upstream_model: ' GLM ' })).toBe(
    'glm',
  )
})

test('upstreamLabel keeps MCP rows untouched', () => {
  expect(
    upstreamLabel({
      mcp_server_name: 'filesystem',
      endpoint_name: 'opencode go',
      upstream_model: 'muse-spark-1.3-contri',
    }),
  ).toBe('filesystem')
})
