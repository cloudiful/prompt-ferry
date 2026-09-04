import { expect, test } from 'bun:test'
import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewTrendBucket,
} from '../src/generated/admin-api'
import { formatTokenQuantity } from '../src/composables/useUsageFormatting'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => {
      storage.set(key, value)
    },
    removeItem: (key: string) => {
      storage.delete(key)
    },
    clear: () => {
      storage.clear()
    },
  },
  configurable: true,
})

const { createBreakdownOption, createErrorOption, createTrendOption } =
  await import('../src/request-overview-charts')

const labels = {
  cacheRead: 'Cache read',
  cacheRate: 'Cache rate',
  cacheWrite: 'Cache write',
  error: 'Error',
  input: 'Input',
  output: 'Output',
  requests: 'Requests',
  success: 'Success',
}

function formatPercent(value?: number | null): string {
  if (value == null) return '-'
  return `${Math.round(value * 100)}%`
}

function trendBucket(
  overrides: Partial<RequestRecordOverviewTrendBucket> = {},
): RequestRecordOverviewTrendBucket {
  return {
    bucket_at: '2026-09-01T00:00:00.000Z',
    error_count: 0,
    error_rate: 0,
    request_count: 0,
    success_count: 0,
    success_rate: 1,
    tokens: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    ...overrides,
  }
}

function breakdownRow(
  overrides: Partial<RequestRecordOverviewBreakdownRow> = {},
): RequestRecordOverviewBreakdownRow {
  return {
    label: 'gpt-4o',
    model: 'gpt-4o',
    mcp_server_id: null,
    request_count: 0,
    request_share: 0,
    success_count: 0,
    success_rate: 1,
    token_share: 0,
    tokens: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    ...overrides,
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function tooltipOf(option: any): (params: any) => string {
  return option.tooltip.formatter as (params: any) => string
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function axisFormatterOf(option: any, axis: 'x' | 'y'): (value: any) => string {
  if (axis === 'x') {
    return option.xAxis.axisLabel.formatter as (value: any) => string
  }
  const yAxis = Array.isArray(option.yAxis) ? option.yAxis[0] : option.yAxis
  return yAxis.axisLabel.formatter as (value: any) => string
}

test('trend AI primary axis compacts large token values and keeps cache-rate axis as percent', () => {
  const option = createTrendOption({
    category: 'ai',
    labels,
    trend: [],
    formatTime: (value) => value,
    formatCompact: formatTokenQuantity,
  })
  const primary = axisFormatterOf(option, 'y')
  expect(primary(999)).toBe('999')
  expect(primary(1_234_567)).toBe('1.2M')
  const yAxis = (
    option as unknown as {
      yAxis: Array<{ axisLabel?: { formatter?: unknown } }>
    }
  ).yAxis
  expect(yAxis).toHaveLength(2)
  expect(yAxis[1]?.axisLabel?.formatter).toBe('{value}%')
})

test('trend AI tooltip compacts token bars and keeps cache rate as percent', () => {
  const option = createTrendOption({
    category: 'ai',
    labels,
    trend: [
      trendBucket({
        tokens: {
          cache_read_tokens: 2_000,
          cache_write_tokens: 0,
          input_tokens: 1_234_567,
          output_tokens: 500,
          total_tokens: 1_237_067,
          cache_rate: 0.423,
        },
      }),
    ],
    formatTime: (value) => value,
    formatCompact: formatTokenQuantity,
  })
  const text = tooltipOf(option)([
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Input',
      value: 1_234_567,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Cache rate',
      value: 42.3,
    },
  ])
  expect(text).toContain('1.2M')
  expect(text).not.toContain('1,234,567')
  expect(text).toContain('42.3%')
})

test('trend AI tooltip renders dash for null cache-rate gaps', () => {
  const option = createTrendOption({
    category: 'ai',
    labels,
    trend: [trendBucket()],
    formatTime: (value) => value,
    formatCompact: formatTokenQuantity,
  })
  const text = tooltipOf(option)([
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Input',
      value: 500,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Cache rate',
      value: null,
    },
  ])
  expect(text).toContain('Cache rate: -')
})

test('trend MCP axis and tooltip compact request counts', () => {
  const option = createTrendOption({
    category: 'mcp',
    labels,
    trend: [trendBucket({ success_count: 2_500_000, error_count: 1_500 })],
    formatTime: (value) => value,
    formatCompact: formatTokenQuantity,
  })
  expect(axisFormatterOf(option, 'y')(2_500_000)).toBe('2.5M')
  const text = tooltipOf(option)([
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Success',
      value: 2_500_000,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Error',
      value: 1500,
    },
  ])
  expect(text).toContain('2.5M')
  expect(text).toContain('1.5K')
  // Raw series values stay numeric for ECharts stacking.
  const series = (option as unknown as { series: Array<{ data: unknown[] }> })
    .series
  expect(series[0]?.data).toEqual([2_500_000])
})

test('model breakdown axis and tooltip compact tokens while share stays percent', () => {
  const option = createBreakdownOption({
    category: 'ai',
    labels,
    rows: [
      breakdownRow({
        tokens: {
          cache_read_tokens: 0,
          cache_write_tokens: 0,
          input_tokens: 10_000_000,
          output_tokens: 2_345_678,
          total_tokens: 12_345_678,
        },
        token_share: 0.42,
      }),
    ],
    formatCompact: formatTokenQuantity,
    formatPercent,
  })
  expect(axisFormatterOf(option, 'x')(12_345_678)).toBe('12.3M')
  const text = tooltipOf(option)([{ dataIndex: 0 }])
  expect(text).toContain('12.3M')
  expect(text).toContain('42%')
  const series = (option as unknown as { series: Array<{ data: unknown[] }> })
    .series
  expect(series[0]?.data).toEqual([12_345_678])
})

test('mcp breakdown tooltip compacts request counts', () => {
  const option = createBreakdownOption({
    category: 'mcp',
    labels,
    rows: [
      breakdownRow({
        label: 'server-a',
        request_count: 1_500_000,
        request_share: 0.75,
      }),
    ],
    formatCompact: formatTokenQuantity,
    formatPercent,
  })
  const text = tooltipOf(option)([{ dataIndex: 0 }])
  expect(text).toContain('1.5M')
  expect(text).toContain('75%')
})

test('error breakdown axis and tooltip compact counts while rate stays percent', () => {
  const option = createErrorOption({
    rows: [{ count: 2_500_000, key: 'boom', label: 'boom', rate: 0.125 }],
    formatCompact: formatTokenQuantity,
    formatPercent,
  })
  expect(axisFormatterOf(option, 'x')(2_500_000)).toBe('2.5M')
  const text = tooltipOf(option)([{ dataIndex: 0 }])
  expect(text).toContain('2.5M')
  expect(text).not.toContain('2,500,000')
  expect(text).toContain('13%')
})
