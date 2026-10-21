import { expect, test } from 'bun:test'
import {
  createRequestRecordRowView,
  upstreamLabel,
} from '../src/admin-mappers/request-records'
import { displayTimeZone, setDisplayTimeZone } from '../src/format/timezone'

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

test('the mapped record date follows the display time zone', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('UTC')
    const utc = createRequestRecordRowView({
      created_at: '2026-03-04T23:30:00Z',
    } as never)
    expect(utc.request_date).toBe('2026-03-04')

    setDisplayTimeZone('Asia/Shanghai')
    const shanghai = createRequestRecordRowView({
      created_at: '2026-03-04T23:30:00Z',
    } as never)
    expect(shanghai.request_date).toBe('2026-03-05')
  } finally {
    setDisplayTimeZone(previous)
  }
})
