import { expect, test } from 'bun:test'
import {
  DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE,
  breakdownDrilldownForPerspective,
  isFailedOnlyZeroTokenEntry,
  splitFailedOnlyDetailEntries,
} from '../src/request-overview'
import { messages } from '../src/i18n'
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

test('only a fully failed zero-token detail row is failed-only', () => {
  // Collapsed: every request failed or aborted and no token was produced.
  expect(
    isFailedOnlyZeroTokenEntry({
      error_rate: 1,
      request_count: 4,
      total_tokens: 0,
    }),
  ).toBe(true)
  // Successful but empty response.
  expect(
    isFailedOnlyZeroTokenEntry({
      error_rate: 0,
      request_count: 4,
      total_tokens: 0,
    }),
  ).toBe(false)
  // Mixed failures and successes with real tokens.
  expect(
    isFailedOnlyZeroTokenEntry({
      error_rate: 0.5,
      request_count: 4,
      total_tokens: 12,
    }),
  ).toBe(false)
  // A failed row that still produced tokens keeps its token accounting visible.
  expect(
    isFailedOnlyZeroTokenEntry({
      error_rate: 1,
      request_count: 4,
      total_tokens: 1,
    }),
  ).toBe(false)
  // In-flight and rows without an error rate are never collapsed.
  expect(
    isFailedOnlyZeroTokenEntry({
      error_rate: null,
      request_count: 0,
      total_tokens: 0,
    }),
  ).toBe(false)
  expect(
    isFailedOnlyZeroTokenEntry({ request_count: 0, total_tokens: 0 }),
  ).toBe(false)
})

test('the detail split keeps the order and reports the hidden requests', () => {
  const split = splitFailedOnlyDetailEntries([
    { model: 'gpt-4o', error_rate: 0.2, request_count: 8, total_tokens: 900 },
    { model: 'gpt-4o-mini', error_rate: 1, request_count: 3, total_tokens: 0 },
    { model: 'gpt-4.1', error_rate: 0, request_count: 2, total_tokens: 0 },
    { model: 'o3-mini', error_rate: 1, request_count: 1, total_tokens: 0 },
  ])

  expect(split.visible.map((entry) => entry.model)).toEqual([
    'gpt-4o',
    'gpt-4.1',
  ])
  expect(split.failedOnly.map((entry) => entry.model)).toEqual([
    'gpt-4o-mini',
    'o3-mini',
  ])
  // The folded rows stay countable, so no request disappears from the detail.
  expect(split.failedRequestCount).toBe(4)
  // Revealing keeps the informative rows first and the failed ones last.
  expect([...split.visible, ...split.failedOnly].map((e) => e.model)).toEqual([
    'gpt-4o',
    'gpt-4.1',
    'gpt-4o-mini',
    'o3-mini',
  ])
  // Nothing to fold is still an empty fold, not a missing list.
  const untouched = splitFailedOnlyDetailEntries([
    { model: 'gpt-4o', error_rate: 0, request_count: 1, total_tokens: 5 },
  ])
  expect(untouched.failedOnly).toEqual([])
  expect(untouched.failedRequestCount).toBe(0)
  expect(untouched.visible).toHaveLength(1)
})

test('the hidden-failed copy exists in both locales', () => {
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const localeMessages = messages[locale] as Record<string, string>
    for (const key of [
      'overviewFailedOnlyHidden',
      'overviewFailedOnlyHide',
      'overviewFailedOnlyReveal',
    ]) {
      expect(localeMessages[key].length).toBeGreaterThan(0)
    }
    expect(localeMessages.overviewFailedOnlyHidden).toContain('{count}')
  }
})
