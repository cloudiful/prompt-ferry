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
const { themeMode } = await import('../src/theme/appTheme')

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

type Rgb = readonly [number, number, number]

/** Chart surface behind the transparent option, per app theme mode. */
const CHART_SURFACE: Record<'dark' | 'light', Rgb> = {
  dark: [5, 10, 7],
  light: [255, 255, 255],
}

function channels(color: string): number[] {
  const value = color.trim()
  if (!value.startsWith('#')) {
    return (value.match(/[\d.]+/g) ?? []).map(Number)
  }
  const hex = value.slice(1)
  const full =
    hex.length === 3 ? [...hex].map((char) => char + char).join('') : hex
  return [0, 2, 4].map((at) => parseInt(full.slice(at, at + 2), 16))
}

/** WCAG relative luminance. */
function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map((channel) => {
    const value = channel / 255
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG contrast ratio of a hex or rgba() chart color against the surface. */
function contrastOn(color: string, surface: Rgb): number {
  const parts = channels(color)
  const alpha = parts.length > 3 ? parts[3] : 1
  const flat = [0, 1, 2].map(
    (at) => parts[at] * alpha + surface[at] * (1 - alpha),
  ) as unknown as Rgb
  const values = [luminance(flat), luminance(surface)]
  return (Math.max(...values) + 0.05) / (Math.min(...values) + 0.05)
}

test('trend chart colors stay readable on the surface in both modes', () => {
  const failures: string[] = []
  const firstSeriesColor = new Map<'dark' | 'light', string>()
  for (const mode of ['light', 'dark'] as const) {
    themeMode.value = mode
    const surface = CHART_SURFACE[mode]
    for (const category of ['ai', 'mcp'] as const) {
      const option = createTrendOption({
        category,
        labels,
        trend: [trendBucket()],
        formatTime: (value) => value,
        formatCompact: formatTokenQuantity,
      })
      const splitLine = (
        option as unknown as {
          yAxis: Array<{ splitLine?: { lineStyle?: { color?: string } } }>
        }
      ).yAxis[0]?.splitLine
      // WCAG AA for text, 3:1 for bars/lines/axis, and a visible-but-quiet grid.
      const checks: Array<[string, string, number]> = [
        ['legend', option.legend.textStyle.color, 4.5],
        ['axis label', option.xAxis.axisLabel.color, 4.5],
        ['tooltip text', option.tooltip.textStyle.color, 4.5],
        ['axis line', option.xAxis.axisLine.lineStyle.color, 3],
        ['grid', splitLine?.lineStyle?.color ?? '', 1.4],
        ...seriesOf(option).map((series) => [
          `series ${series.name}`,
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (series as any).itemStyle?.color ?? '',
          3,
        ]),
      ]
      for (const [label, color, floor] of checks) {
        const ratio = contrastOn(color, surface)
        if (ratio < floor) {
          failures.push(
            `${mode}/${category}/${label} ${color} ${ratio.toFixed(2)}:1 < ${floor}:1`,
          )
        }
      }
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      firstSeriesColor.set(mode, (seriesOf(option)[0] as any).itemStyle.color)
    }
  }
  expect(failures).toEqual([])
  // The palette is mode-driven: a theme switch must redraw with other colors.
  expect(firstSeriesColor.get('light')).not.toBe(firstSeriesColor.get('dark'))
})
