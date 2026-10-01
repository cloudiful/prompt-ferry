import { expect, test } from 'bun:test'
import {
  DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE,
  breakdownDrilldownForPerspective,
} from '../src/request-overview'
import type { RequestRecordOverviewBreakdownRow } from '../src/generated/admin-api'

function breakdownRow(
  overrides: Partial<RequestRecordOverviewBreakdownRow> = {},
): RequestRecordOverviewBreakdownRow {
  return {
    label: 'row',
    request_count: 1,
    request_share: 1,
    success_count: 1,
    success_rate: 1,
    tokens: {
      input_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    ...overrides,
  }
}

test('the default perspective stays the model view', () => {
  expect(DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE).toBe('model')
})

test('an AI model row drills down to the model filter', () => {
  const filter = breakdownDrilldownForPerspective(
    'model',
    'ai',
    breakdownRow({ model: 'gpt-4o', mcp_server_id: 'server-1' }),
  )

  expect(filter).toEqual({ model: 'gpt-4o', mcp_server_id: null })
})

test('an AI upstream row drills down to the endpoint identity', () => {
  const filter = breakdownDrilldownForPerspective(
    'upstream',
    'ai',
    breakdownRow({ endpoint_id: 'endpoint-1', model: 'gpt-4o' }),
  )

  expect(filter).toEqual({ endpoint_id: 'endpoint-1' })
})

test('an AI direct upstream row yields no drilldown', () => {
  const filter = breakdownDrilldownForPerspective(
    'upstream',
    'ai',
    breakdownRow({ endpoint_id: null, model: 'gpt-4o' }),
  )

  expect(filter).toBeNull()
})

test('an MCP row keeps the MCP server filter under the upstream perspective', () => {
  const filter = breakdownDrilldownForPerspective(
    'upstream',
    'mcp',
    breakdownRow({ endpoint_id: null, mcp_server_id: 'mcp-uuid-1' }),
  )

  expect(filter).toEqual({ model: null, mcp_server_id: 'mcp-uuid-1' })
})

test('an MCP row keeps the MCP server filter under the model perspective', () => {
  const filter = breakdownDrilldownForPerspective(
    'model',
    'mcp',
    breakdownRow({ endpoint_id: null, mcp_server_id: 'mcp-uuid-1' }),
  )

  expect(filter).toEqual({ model: null, mcp_server_id: 'mcp-uuid-1' })
})
