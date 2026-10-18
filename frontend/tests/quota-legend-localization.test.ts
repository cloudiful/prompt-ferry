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

const { createQuotaTrendPanelOption, groupQuotaTrendSeries } =
  await import('../src/quota-observation-charts')
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
    source_id: 'glm:five_hour',
    kind: 'five_hour',
    unit: 'percent',
    metric: 'remaining_percent',
    points: [point('2026-10-16T00:00:00Z', 80)],
    ...overrides,
  }
}

function optionInput() {
  return {
    range: '24h' as const,
    formatAxisDate: (value: string) => value,
    formatAxisTime: (value: string) => value,
    formatTooltipTime: (value: string) => value,
    formatAmount: (value: number, unit: string) => `${value} ${unit}`,
    labels: { percentSuffix: '%', unavailable: 'n/a', kinds: { ...KINDS } },
  }
}

function names(entries: QuotaKeyObservationTrendSeries[]): string[] {
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries(entries)[0],
    optionInput(),
  ) as AnyOption
  return option.series.map((entry: { name: string }) => entry.name)
}

test('one key with three windows uses friendly localized slot names only', () => {
  const entries = [
    series({ kind: 'rolling' }),
    series({ kind: 'weekly' }),
    series({ kind: 'monthly' }),
  ]
  expect(names(entries)).toEqual(['Monthly', 'Rolling', 'Weekly'])
})

test('multiple keys prefix the key label and the localized slot', () => {
  const entries = [
    series({ key_id: 'a', key_label: 'Key A', kind: 'weekly' }),
    series({ key_id: 'b', key_label: 'Key B', kind: 'weekly' }),
  ]
  expect(names(entries)).toEqual(['Key A · Weekly', 'Key B · Weekly'])
})

test('a genuine same-label collision adds only the short key id', () => {
  const entries = [
    series({ key_id: 'aaaaaaaa-1111', key_label: 'Same', kind: 'weekly' }),
    series({ key_id: 'bbbbbbbb-2222', key_label: 'Same', kind: 'weekly' }),
  ]
  const rendered = names(entries)
  expect(rendered).toHaveLength(2)
  expect(rendered[0]).toContain('Same #aaaaaaaa')
  expect(rendered[1]).toContain('Same #bbbbbbbb')
  expect(new Set(rendered).size).toBe(2)
})

test('custom windows keep source identity instead of collapsing models', () => {
  const entries = [
    series({
      kind: 'custom',
      source_id: 'subscription:primary',
    }),
    series({
      kind: 'custom',
      source_id: 'subscription:secondary',
    }),
  ]
  expect(names(entries)).toEqual([
    'Custom window (primary)',
    'Custom window (secondary)',
  ])
})

type Rgb = readonly [number, number, number]

const CHART_SURFACE: Record<'light' | 'dark', Rgb> = {
  dark: [5, 10, 7],
  light: [255, 255, 255],
}

function channels(color: string): number[] {
  const hex = color.slice(1)
  const full =
    hex.length === 3 ? [...hex].map((char) => char + char).join('') : hex
  return [0, 2, 4].map((at) => parseInt(full.slice(at, at + 2), 16))
}

function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map((channel) => {
    const value = channel / 255
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrastOn(color: string, surface: Rgb): number {
  const [red, green, blue] = channels(color)
  const values = [luminance([red, green, blue]), luminance(surface)]
  return (Math.max(...values) + 0.05) / (Math.min(...values) + 0.05)
}

test('quota categorical colors are distinct and readable in both modes', () => {
  const failures: string[] = []
  for (const mode of ['light', 'dark'] as const) {
    themeMode.value = mode
    const theme = getChartTheme()
    expect(theme.categorical).toHaveLength(5)
    expect(new Set(theme.categorical).size).toBe(5)
    for (const color of theme.categorical) {
      const ratio = contrastOn(color, CHART_SURFACE[mode])
      if (ratio < 3) {
        failures.push(`${mode} ${color} ${ratio.toFixed(2)}:1 < 3:1`)
      }
    }
  }
  themeMode.value = 'dark'
  expect(failures).toEqual([])
})

test('quota series use the categorical palette, not semantic chart tokens', () => {
  themeMode.value = 'dark'
  const theme = getChartTheme()
  const option = createQuotaTrendPanelOption(
    groupQuotaTrendSeries([
      series({ key_id: 'a', key_label: 'Key A', kind: 'weekly' }),
      series({ key_id: 'b', key_label: 'Key B', kind: 'weekly' }),
    ])[0],
    optionInput(),
  ) as AnyOption

  const colors = option.series.map(
    (entry: { itemStyle: { color: string } }) => entry.itemStyle.color,
  )
  expect(colors).toEqual(theme.categorical.slice(0, 2))
  for (const token of [
    theme.accent,
    theme.info,
    theme.warn,
    theme.input,
    theme.output,
    theme.cached,
    theme.error,
  ]) {
    expect(colors).not.toContain(token)
  }
})
