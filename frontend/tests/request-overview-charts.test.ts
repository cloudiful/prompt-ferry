import { expect, test } from 'bun:test'
import type { RequestRecordOverviewTrendBucket } from '../src/generated/admin-api'
import { formatTokenQuantity } from '../src/composables/useUsageFormatting'
import { usageMessages } from '../src/i18n/modules/usage'

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

const { createTrendOption } = await import('../src/request-overview-charts')

const labels = {
  cacheRead: 'Input (cache hit)',
  cacheRate: 'Cache rate',
  cacheWrite: 'Cache write',
  error: 'Error',
  input: 'Input (cache miss)',
  output: 'Output',
  requests: 'Requests',
  success: 'Success',
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

type ChartSeries = {
  name?: string
  stack?: string
  data: unknown[]
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function seriesOf(option: any): ChartSeries[] {
  return option.series as ChartSeries[]
}

test('overview trend labels use the confirmed input hit/miss wording', () => {
  expect(usageMessages['zh-CN'].overviewCacheRead).toBe('输入（命中缓存）')
  expect(usageMessages['zh-CN'].overviewInputTokens).toBe('输入（未命中缓存）')
  expect(usageMessages['en-US'].overviewCacheRead).toBe('Input (cache hit)')
  expect(usageMessages['en-US'].overviewInputTokens).toBe('Input (cache miss)')
})

test('trend AI stacks cache hit below cache miss and maps each token meter once', () => {
  const option = createTrendOption({
    category: 'ai',
    labels,
    trend: [
      trendBucket({
        tokens: {
          cache_read_tokens: 2_200,
          cache_write_tokens: 3_300,
          input_tokens: 1_100,
          output_tokens: 4_400,
          total_tokens: 11_000,
          cache_rate: 0.25,
        },
      }),
    ],
    formatTime: (value) => value,
    formatCompact: formatTokenQuantity,
  })
  const series = seriesOf(option)
  expect(series.map((item) => item.name)).toEqual([
    labels.cacheRead,
    labels.input,
    labels.cacheWrite,
    labels.output,
    labels.cacheRate,
  ])
  const stackedTokens = series.slice(0, 4).map((item) => item.stack)
  expect(stackedTokens).toEqual(['tokens', 'tokens', 'tokens', 'tokens'])
  expect(
    series.findIndex((item) => item.name === labels.cacheRead),
  ).toBeLessThan(series.findIndex((item) => item.name === labels.input))
  expect(series[0]?.data).toEqual([2_200])
  expect(series[1]?.data).toEqual([1_100])
  expect(series[2]?.data).toEqual([3_300])
  expect(series[3]?.data).toEqual([4_400])
  expect(series[4]?.data).toEqual([25])
})

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
      seriesName: labels.input,
      value: 1_234_567,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: labels.cacheRead,
      value: 2_000,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: 'Cache rate',
      value: 42.3,
    },
  ])
  expect(text).toContain('Input (cache miss): 1.2M')
  expect(text).toContain('Input (cache hit): 2K')
  expect(text).not.toContain('1,234,567')
  expect(text).toContain('42.3%')
})

test('trend AI tooltip adapts hit/miss magnitudes across compact units', () => {
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
      seriesName: labels.cacheRead,
      value: 2_400_000_000_000,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: labels.input,
      value: 1_500_000_000,
    },
    {
      axisValue: 't',
      dataIndex: 0,
      marker: '',
      seriesName: labels.cacheWrite,
      value: 500,
    },
  ])
  expect(text).toContain('Input (cache hit): 2.4T')
  expect(text).toContain('Input (cache miss): 1.5B')
  expect(text).toContain('Cache write: 500')
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
      seriesName: labels.input,
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
  expect(seriesOf(option)[0]?.data).toEqual([2_500_000])
})
