import { expect, test } from 'bun:test'
import type {
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
} from '../src/generated/admin-api'

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

const {
  createQuotaTrendPanelOption,
  duplicateKeyLabels,
  escapeHtml,
  groupQuotaTrendSeries,
  quotaTrendSeriesName,
  shortKeyId,
} = await import('../src/quota-observation-charts')
const { getChartTheme } = await import('../src/theme/chartTheme')
const { themeMode } = await import('../src/theme/appTheme')

const KINDS = {
  five_hour: 'Five hour',
  rolling: 'Rolling',
  weekly: 'Weekly',
  monthly: 'Monthly',
  balance: 'Balance',
  unlimited: 'Unlimited',
  custom: 'Custom window',
} as const

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyOption = Record<string, any>

function point(
  at: string,
  value: number | null,
): QuotaKeyObservationTrendPoint {
  return { at, observed_at: value == null ? null : at, value }
}

function series(
  overrides: Partial<QuotaKeyObservationTrendSeries> = {},
): QuotaKeyObservationTrendSeries {
  return {
    key_id: 'key-a',
    key_label: 'Key A',
    source_id: 'minimax:general:interval',
    kind: 'five_hour',
    unit: 'percent',
    metric: 'remaining_percent',
    points: [point('2026-10-16T00:00:00Z', 80)],
    ...overrides,
  }
}

function optionInput(
  overrides: Partial<ReturnType<typeof baseOptionInput>> = {},
) {
  return { ...baseOptionInput(), ...overrides }
}

function baseOptionInput() {
  return {
    range: '24h' as '24h' | '7d' | '30d',
    formatAxisDate: (value: string) => `date:${value}`,
    formatAxisTime: (value: string) => `time:${value}`,
    formatTooltipTime: (value: string) => value,
    formatAmount: (value: number, unit: string) => `${value} ${unit}`,
    labels: {
      percentSuffix: '%',
      unavailable: 'n/a',
      kinds: { ...KINDS },
    },
  }
}

function nameContext(overrides: Record<string, unknown> = {}) {
  return {
    duplicateLabels: new Set<string>(),
    singleKey: false,
    kinds: { ...KINDS },
    ...overrides,
  }
}

function tooltipHtml(
  option: AnyOption,
  params: Partial<Record<string, unknown>>[],
): string {
  const formatter = option.tooltip.formatter as (p: unknown[]) => string
  return formatter(params)
}

test('escapeHtml neutralizes markup, quotes and the ampersand', () => {
  expect(escapeHtml('<img src=x onerror=alert(1)>')).toBe(
    '&lt;img src=x onerror=alert(1)&gt;',
  )
  expect(escapeHtml('</script>')).toBe('&lt;/script&gt;')
  expect(escapeHtml(`"'&`)).toBe('&quot;&#39;&amp;')
})

test('grouping separates metrics and amount units, never merging currencies', () => {
  const groups = groupQuotaTrendSeries([
    series({
      key_id: 'k1',
      source_id: 'deepseek:CNY',
      kind: 'balance',
      unit: 'CNY',
      metric: 'amount',
    }),
    series({
      key_id: 'k1',
      source_id: 'deepseek:USD',
      kind: 'balance',
      unit: 'USD',
      metric: 'amount',
    }),
    series({
      key_id: 'k1',
      source_id: 'command_code:balances',
      kind: 'balance',
      unit: 'credits',
      metric: 'amount',
    }),
    series({ key_id: 'k2', metric: 'remaining_percent', unit: 'percent' }),
  ])

  expect(groups.map((group) => `${group.metric}:${group.unit}`)).toEqual([
    'amount:CNY',
    'amount:USD',
    'amount:credits',
    'remaining_percent:percent',
  ])
})

test('the tooltip escapes a malicious key label and unit, keeping br structure', () => {
  const maliciousLabel = '<img src=x onerror=alert(1)>'
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        key_id: 'key-a',
        key_label: maliciousLabel,
        kind: 'balance',
        unit: '</script>USD',
        metric: 'amount',
        points: [point('2026-10-16T00:00:00Z', 5)],
      }),
      series({
        key_id: 'key-b',
        key_label: 'Other',
        kind: 'balance',
        unit: '</script>USD',
        metric: 'amount',
        points: [point('2026-10-16T00:00:00Z', 6)],
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  const html = tooltipHtml(option, [
    {
      axisValue: '<b>2026-10-16</b>',
      marker: '●',
      seriesName: option.series[0].name,
      value: 5,
    },
  ])

  // No raw markup from any interpolated string survives.
  expect(html).not.toContain('<img')
  expect(html).not.toContain('</script>')
  expect(html).not.toContain('<b>')
  expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;')
  expect(html).toContain('&lt;/script&gt;')
  expect(html).toContain('&lt;b&gt;2026-10-16&lt;/b&gt;')
  // The only literal markup is the internally generated separator.
  expect(html).toContain('<br/>')
  expect(html.match(/<[^>]+>/g)).toEqual(['<br/>'])
})

test('a malicious axis value cannot inject structure across the br separator', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([series()])[0],
    optionInput(),
  ) as AnyOption

  const html = tooltipHtml(option, [
    {
      axisValue: 'x<img src=x onerror=alert(1)>',
      marker: '',
      seriesName: option.series[0].name,
      value: 1,
    },
  ])
  expect(html).not.toContain('<img')
  expect(html).toContain('&lt;img')
  expect(html.match(/<[^>]+>/g)).toEqual(['<br/>'])
})

test('duplicate key labels gain a short key id, unique labels stay clean', () => {
  const duplicate = new Set(['Same label'])
  const first = series({ key_id: 'aaaaaaaa-1111', key_label: 'Same label' })
  const second = series({ key_id: 'bbbbbbbb-2222', key_label: 'Same label' })
  const unique = series({ key_id: 'cccccccc-3333', key_label: 'Other' })

  expect(
    quotaTrendSeriesName(first, nameContext({ duplicateLabels: duplicate })),
  ).toBe(`Same label #${shortKeyId('aaaaaaaa-1111')} · Five hour`)
  expect(
    quotaTrendSeriesName(second, nameContext({ duplicateLabels: duplicate })),
  ).toContain(shortKeyId('bbbbbbbb-2222'))
  expect(
    quotaTrendSeriesName(unique, nameContext({ duplicateLabels: duplicate })),
  ).toBe('Other · Five hour')
})

test('duplicate detection counts distinct keys, not series', () => {
  const sameKey = [
    series({ key_id: 'key-a', key_label: 'One key', kind: 'five_hour' }),
    series({ key_id: 'key-a', key_label: 'One key', kind: 'weekly' }),
  ]
  expect(duplicateKeyLabels(sameKey).size).toBe(0)

  const differentKeys = [
    ...sameKey,
    series({ key_id: 'key-b', key_label: 'One key', kind: 'five_hour' }),
  ]
  expect(duplicateKeyLabels(differentKeys)).toEqual(new Set(['One key']))
})

test('a single-key group uses localized window slots without a key id', () => {
  const group = groupQuotaTrendSeries([
    series({ key_id: 'k1', key_label: 'Only key', kind: 'rolling' }),
    series({ key_id: 'k1', key_label: 'Only key', kind: 'weekly' }),
    series({ key_id: 'k1', key_label: 'Only key', kind: 'monthly' }),
  ])[0]
  const option = createQuotaTrendPanelOption(group, optionInput()) as AnyOption

  expect(option.series.map((entry: { name: string }) => entry.name)).toEqual([
    'Monthly',
    'Rolling',
    'Weekly',
  ])
})

test('custom windows keep their source identity in a single-key legend', () => {
  const group = groupQuotaTrendSeries([
    series({
      key_id: 'k1',
      key_label: 'Only key',
      kind: 'custom',
      source_id: 'subscription:primary',
    }),
    series({
      key_id: 'k1',
      key_label: 'Only key',
      kind: 'custom',
      source_id: 'subscription:secondary',
    }),
  ])[0]
  const option = createQuotaTrendPanelOption(group, optionInput()) as AnyOption

  expect(option.series.map((entry: { name: string }) => entry.name)).toEqual([
    'Custom window (primary)',
    'Custom window (secondary)',
  ])
})

test('a panel has exactly one y-axis, explicitly positioned on the left', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        source_id: 'deepseek:CNY',
        kind: 'balance',
        unit: 'CNY',
        metric: 'amount',
        points: [point('2026-10-16T00:00:00Z', 0)],
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  expect(option.yAxis.type).toBe('value')
  expect(option.yAxis.position).toBe('left')
  expect(option.yAxis.name).toBe('CNY')
  // No zero floor for amounts: real zero and negative balances stay visible.
  expect(option.yAxis.scale).toBe(true)
  expect(option.yAxis.min).toBeUndefined()
})

test('the percent panel is bounded 0..100 with percent tick labels only', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([series()])[0],
    optionInput(),
  ) as AnyOption

  expect(option.yAxis.min).toBe(0)
  expect(option.yAxis.max).toBe(100)
  // The axis no longer carries a bare `%` title; only its ticks keep the unit.
  expect(option.yAxis.name).toBeUndefined()
  expect(option.yAxis.axisLabel.formatter).toContain('%')
})

test('the built-in canvas legend is disabled so no clipped pager is drawn', () => {
  const many = Array.from({ length: 96 }, (_, index) =>
    series({
      key_id: `key-${String(index).padStart(3, '0')}`,
      key_label: `A very long key label number ${index} that must not wrap the canvas legend`,
      source_id: `source-${index}`,
      points: [point('2026-10-16T00:00:00Z', index)],
    }),
  )
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries(many)[0],
    optionInput(),
  ) as AnyOption

  expect(option.legend.show).toBe(false)
  expect(option.legend.type).toBeUndefined()
  expect(option.series).toHaveLength(96)
})

test('hidden series are filtered out while names and colors stay stable', () => {
  const group = groupQuotaTrendSeries([
    series({ key_id: 'a', key_label: 'Key A', kind: 'weekly' }),
    series({ key_id: 'b', key_label: 'Key B', kind: 'weekly' }),
  ])[0]
  const all = createQuotaTrendPanelOption(group, optionInput()) as AnyOption
  const firstColor = all.series[0].itemStyle.color

  const hidden = createQuotaTrendPanelOption(group, {
    ...optionInput(),
    hiddenSeries: new Set([all.series[0].name]),
  }) as AnyOption

  expect(hidden.series.map((entry: { name: string }) => entry.name)).toEqual([
    'Key B · Weekly',
  ])
  // The surviving series keeps the color it had when both were drawn.
  expect(hidden.series[0].itemStyle.color).toBe(all.series[1].itemStyle.color)
  expect(hidden.series[0].itemStyle.color).not.toBe(firstColor)
})

test('hiding every series leaves axes and grid without fabricated data', () => {
  const group = groupQuotaTrendSeries([series()])[0]
  const option = createQuotaTrendPanelOption(group, {
    ...optionInput(),
    hiddenSeries: new Set(['Five hour']),
  }) as AnyOption

  expect(option.series).toEqual([])
  expect(option.xAxis.data.length).toBeGreaterThan(0)
  expect(option.yAxis.type).toBe('value')
})

test('lines are step, gap-honest, and never area-filled', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        points: [
          point('2026-10-16T00:00:00Z', 0),
          point('2026-10-16T00:15:00Z', null),
          point('2026-10-16T00:30:00Z', -3.5),
        ],
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  const entry = option.series[0]
  expect(entry.type).toBe('line')
  expect(entry.connectNulls).toBe(false)
  expect(entry.areaStyle).toBeUndefined()
  expect(entry.step).toBe('end')
  // A real zero, a null gap and a negative value all survive unchanged.
  expect(entry.data).toEqual([0, null, -3.5])
})

test('points are sorted ascending even when the payload is unordered', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        points: [
          point('2026-10-16T00:30:00Z', 30),
          point('2026-10-16T00:00:00Z', 10),
          point('2026-10-16T00:15:00Z', 20),
        ],
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  expect(option.series[0].data).toEqual([10, 20, 30])
  expect(option.xAxis.data).toEqual([
    '2026-10-16T00:00:00Z',
    '2026-10-16T00:15:00Z',
    '2026-10-16T00:30:00Z',
  ])
})

test('24h axis labels stay compact and add a date only across a day boundary', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        points: [
          point('2026-10-15T12:00:00Z', 10),
          point('2026-10-16T12:00:00Z', 20),
        ],
      }),
    ])[0],
    optionInput({ range: '24h' }),
  ) as AnyOption

  const formatter = option.xAxis.axisLabel.formatter as (
    value: string,
  ) => string
  expect(formatter('2026-10-15T12:00:00Z')).toBe('time:2026-10-15T12:00:00Z')
  expect(formatter('2026-10-16T12:00:00Z')).toBe(
    'date:2026-10-16T12:00:00Z time:2026-10-16T12:00:00Z',
  )
})

test('7d and 30d axis labels use compact dates only', () => {
  for (const range of ['7d', '30d'] as const) {
    const option = createQuotaTrendPanelOption(
      groupQuotaTrendSeries([series()])[0],
      optionInput({ range }),
    ) as AnyOption
    const formatter = option.xAxis.axisLabel.formatter as (
      value: string,
    ) => string
    expect(formatter('2026-10-16T00:00:00Z')).toBe('date:2026-10-16T00:00:00Z')
  }
})

test('the tooltip shows the full localized timestamp, not the compact axis label', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([series()])[0],
    optionInput({
      formatTooltipTime: (value) => `full:${value}`,
    }),
  ) as AnyOption

  const html = tooltipHtml(option, [
    {
      axisValue: '2026-10-16T00:00:00Z',
      marker: '',
      seriesName: option.series[0].name,
      value: 5,
    },
  ])
  expect(html).toContain('full:2026-10-16T00:00:00Z')
  expect(html).not.toContain('time:2026-10-16T00:00:00Z')
})

test('series colors come from the quota categorical palette, not semantic tokens', () => {
  const group = groupQuotaTrendSeries([series()])[0]
  themeMode.value = 'dark'
  const dark = createQuotaTrendPanelOption(group, optionInput()) as AnyOption
  const darkTheme = getChartTheme()
  expect(dark.series[0].itemStyle.color).toBe(darkTheme.categorical[0])
  expect(darkTheme.categorical).not.toContain(darkTheme.accent)

  themeMode.value = 'light'
  const light = createQuotaTrendPanelOption(group, optionInput()) as AnyOption
  const lightTheme = getChartTheme()
  expect(light.series[0].itemStyle.color).toBe(lightTheme.categorical[0])
  expect(light.series[0].itemStyle.color).not.toBe(darkTheme.categorical[0])
  themeMode.value = 'dark'
})

test('the tooltip names amounts by unit and never calls them consumption', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({
        key_label: 'DS',
        kind: 'balance',
        unit: 'CNY',
        metric: 'amount',
        points: [point('2026-10-16T00:00:00Z', 5)],
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  const html = tooltipHtml(option, [
    {
      axisValue: '2026-10-16T00:00:00Z',
      marker: '●',
      seriesName: option.series[0].name,
      value: 5,
    },
  ])
  expect(html).toContain('5 CNY')
  expect(html).not.toContain('used')
  expect(html).not.toContain('consumption')
})

test('a null bucket renders as unavailable instead of a fabricated zero', () => {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([series()])[0],
    optionInput(),
  ) as AnyOption

  const html = tooltipHtml(option, [
    {
      axisValue: '2026-10-16T00:00:00Z',
      marker: '●',
      seriesName: option.series[0].name,
      value: null,
    },
  ])
  expect(html).toContain('n/a')
  expect(html).not.toContain('0%')
})

test('an empty group still yields a well-formed empty option', () => {
  const option = createQuotaTrendPanelOption(
    {
      key: 'remaining_percent\0percent',
      metric: 'remaining_percent',
      unit: 'percent',
      series: [],
    },
    optionInput(),
  ) as AnyOption

  expect(option.series).toEqual([])
  expect(option.xAxis.data).toEqual([])
})
