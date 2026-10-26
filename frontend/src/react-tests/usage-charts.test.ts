import { describe, expect, it } from 'vitest'
import type { RequestRecordOverviewTrendBucket } from '@/generated/admin-api'
import { createUsageTrendOption } from '@/request-overview-charts-core'
import type { AppChartTheme } from '@/theme/chart-tokens'

const theme: AppChartTheme = {
  accent: '#acc',
  axis: '#ax1',
  bg: '#bg0',
  border: '#bd0',
  cached: '#cac',
  categorical: ['#c1'],
  error: '#err',
  grid: '#grd',
  info: '#inf',
  input: '#inp',
  muted: '#mut',
  output: '#out',
  text: '#txt',
  warn: '#wrn',
}

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

function bucket(
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

function option(
  category: 'ai' | 'mcp',
  trend: RequestRecordOverviewTrendBucket[],
  overrides: {
    labels?: typeof labels
    formatCompact?: (value?: number | null) => string
  } = {},
) {
  return createUsageTrendOption({
    category,
    labels: overrides.labels ?? labels,
    theme,
    trend,
    formatTime: (value) => `t:${value.slice(11, 16)}`,
    formatCompact:
      overrides.formatCompact ??
      ((value) => (value == null ? '-' : String(value))),
  })
}

type AnyOption = {
  backgroundColor: string
  color: string[]
  grid: { left: number; right: number; top: number; bottom: number }
  legend: { top: number; textStyle: { color: string; fontSize: number } }
  tooltip: { formatter: (params: unknown[]) => string; backgroundColor: string }
  xAxis: { data: string[]; axisLabel: { color: string; fontSize: number } }
  yAxis: Array<{
    max?: number
    min?: number
    axisLabel?: { formatter?: unknown }
  }>
  series: Array<{
    name: string
    type: string
    stack?: string
    yAxisIndex?: number
    connectNulls?: boolean
    data: unknown[]
    itemStyle: { color: string }
  }>
}

describe('usage trend option: AI', () => {
  const trend = option('ai', [
    bucket({
      tokens: {
        cache_read_tokens: 200,
        cache_rate: 0.456,
        cache_write_tokens: 80,
        input_tokens: 700,
        output_tokens: 460,
        total_tokens: 1440,
      },
    }),
    bucket({
      tokens: {
        cache_read_tokens: 0,
        cache_rate: null,
        cache_write_tokens: 0,
        input_tokens: 10,
        output_tokens: 20,
        total_tokens: 30,
      },
    }),
  ]) as AnyOption

  it('renders the four stacked token series and the cache-rate line', () => {
    expect(trend.series.map((series) => series.name)).toEqual([
      labels.cacheRead,
      labels.input,
      labels.cacheWrite,
      labels.output,
      labels.cacheRate,
    ])
    expect(
      trend.series.slice(0, 4).every((series) => series.stack === 'tokens'),
    ).toBe(true)
    expect(trend.series[4].type).toBe('line')
    expect(trend.series[4].yAxisIndex).toBe(1)
    expect(trend.series[4].connectNulls).toBe(false)
    expect(trend.series[0].data).toEqual([200, 0])
    expect(trend.series[1].data).toEqual([700, 10])
    expect(trend.series[2].data).toEqual([80, 0])
    expect(trend.series[3].data).toEqual([460, 20])
  })

  it('rounds the cache rate to one decimal percent and keeps the null gap', () => {
    expect(trend.series[4].data).toEqual([45.6, null])
  })

  it('uses the injected theme for the palette and the grid', () => {
    expect(trend.color).toEqual([
      theme.accent,
      theme.cached,
      theme.warn,
      theme.output,
      theme.info,
    ])
    expect(trend.backgroundColor).toBe('transparent')
    expect(trend.grid).toEqual({ left: 52, right: 52, top: 42, bottom: 34 })
    expect(trend.legend).toEqual({
      top: 0,
      textStyle: { color: theme.text, fontSize: 11 },
    })
    expect(trend.xAxis.axisLabel).toEqual({ color: theme.muted, fontSize: 10 })
  })

  it('keeps a percent second axis bounded to 0..100', () => {
    expect(trend.yAxis).toHaveLength(2)
    expect(trend.yAxis[1]).toMatchObject({ min: 0, max: 100 })
    expect(trend.yAxis[1]?.axisLabel?.formatter).toBe('{value}%')
  })

  it('labels the buckets through the supplied formatter', () => {
    expect(trend.xAxis.data).toEqual(['t:00:00', 't:00:00'])
  })
})

describe('usage trend option: MCP', () => {
  const trend = option('mcp', [
    bucket({ success_count: 9, error_count: 1, request_count: 10 }),
  ]) as AnyOption

  it('renders only the stacked success and error bars', () => {
    expect(trend.series.map((series) => series.name)).toEqual([
      labels.success,
      labels.error,
    ])
    expect(trend.series.every((series) => series.stack === 'requests')).toBe(
      true,
    )
    expect(trend.series[0].itemStyle.color).toBe(theme.accent)
    expect(trend.series[1].itemStyle.color).toBe(theme.error)
    expect(trend.series[0].data).toEqual([9])
    expect(trend.series[1].data).toEqual([1])
  })

  it('has no percent axis and a narrower right grid', () => {
    expect(trend.yAxis).toHaveLength(1)
    expect(trend.grid.right).toBe(24)
  })
})

describe('usage trend option: tooltip', () => {
  it('compacts token bars, keeps the cache rate as a percent and dashes a gap', () => {
    const chart = option('ai', [bucket()], {
      formatCompact: (value) => (value == null ? '-' : `${value}K`),
    }) as AnyOption
    const text = chart.tooltip.formatter([
      {
        axisValue: '12:00',
        dataIndex: 0,
        marker: '<i></i>',
        seriesName: labels.input,
        value: 5,
      },
      {
        axisValue: '12:00',
        dataIndex: 0,
        marker: '<i></i>',
        seriesName: labels.cacheRate,
        value: 46.2,
      },
      {
        axisValue: '12:00',
        dataIndex: 0,
        marker: '<i></i>',
        seriesName: labels.cacheRead,
        value: null,
      },
    ])
    expect(text).toContain('12:00')
    expect(text).toContain(`${labels.input}: 5K`)
    expect(text).toContain(`${labels.cacheRate}: 46.2%`)
    expect(text).toContain(`${labels.cacheRead}: -`)
    expect(text).toContain('<i></i>')
  })

  it('escapes labels and the axis value instead of injecting markup', () => {
    const malicious = {
      ...labels,
      input: '<img src=x onerror=alert(1)>',
    }
    const chart = option('ai', [bucket()], { labels: malicious }) as AnyOption
    const text = chart.tooltip.formatter([
      {
        axisValue: '<script>alert(1)</script>',
        dataIndex: 0,
        marker: '',
        seriesName: malicious.input,
        value: 1,
      },
    ])
    expect(text).not.toContain('<img')
    expect(text).not.toContain('<script>')
    expect(text).toContain('&lt;img src=x onerror=alert(1)&gt;')
    expect(text).toContain('&lt;script&gt;alert(1)&lt;/script&gt;')
  })

  it('returns an empty string when there is no axis point', () => {
    const chart = option('ai', [bucket()]) as AnyOption
    expect(chart.tooltip.formatter([])).toBe('')
  })

  it('dashes a non-numeric axis value', () => {
    const chart = option('mcp', [bucket()]) as AnyOption
    const axisFormatter = chart.yAxis[0]?.axisLabel?.formatter as (
      value: string | number,
    ) => string
    expect(axisFormatter('not-a-number')).toBe('-')
    expect(axisFormatter(1200)).toBe('1200')
  })
})
