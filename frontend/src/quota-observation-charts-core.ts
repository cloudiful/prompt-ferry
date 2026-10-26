import type {
  CanonicalQuotaWindowKind,
  QuotaKeyObservationTrendMetric,
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
} from '@/generated/admin-api'
import type { LegendComponentOption } from 'echarts/components'
import type { AppChartTheme } from '@/theme/chart-tokens'

export type QuotaTrendRange = '24h' | '7d' | '30d'

export type QuotaTrendGroup = {
  key: string
  metric: QuotaKeyObservationTrendMetric
  unit: string
  series: QuotaKeyObservationTrendSeries[]
}

export type QuotaTrendKindLabels = Record<CanonicalQuotaWindowKind, string>

export type QuotaTrendCoreOptionInput = {
  range: QuotaTrendRange
  formatAxisDate: (value: string) => string
  formatAxisTime: (value: string) => string
  formatTooltipTime: (value: string) => string
  formatAmount: (value: number, unit: string) => string
  formatDayKey: (value: string | number | Date) => string
  theme: AppChartTheme
  hiddenSeries?: ReadonlySet<string>
  labels: {
    percentSuffix: string
    unavailable: string
    kinds: QuotaTrendKindLabels
  }
}

export type QuotaTrendLegendEntry = {
  name: string
  color: string
  series: QuotaKeyObservationTrendSeries
}

export type QuotaTrendNameContext = {
  duplicateLabels: ReadonlySet<string>
  singleKey: boolean
  kinds: QuotaTrendKindLabels
}

const PERCENT_METRIC: QuotaKeyObservationTrendMetric = 'remaining_percent'

function compareText(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0
}

export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function compareSeries(
  a: QuotaKeyObservationTrendSeries,
  b: QuotaKeyObservationTrendSeries,
): number {
  return (
    compareText(a.key_id, b.key_id) ||
    compareText(a.source_id, b.source_id) ||
    compareText(a.kind, b.kind) ||
    compareText(a.unit, b.unit) ||
    compareText(a.metric, b.metric)
  )
}

function sortedPoints(
  entry: QuotaKeyObservationTrendSeries,
): QuotaKeyObservationTrendPoint[] {
  return [...entry.points].sort((a, b) => compareText(a.at, b.at))
}

export function groupQuotaTrendSeries(
  series: readonly QuotaKeyObservationTrendSeries[],
): QuotaTrendGroup[] {
  const groups = new Map<string, QuotaTrendGroup>()
  for (const entry of series) {
    const key = `${entry.metric}\u0000${entry.unit}`
    let group = groups.get(key)
    if (!group) {
      group = { key, metric: entry.metric, unit: entry.unit, series: [] }
      groups.set(key, group)
    }
    group.series.push(entry)
  }
  const ordered = [...groups.values()]
  ordered.sort((a, b) => {
    if (a.metric !== b.metric) return a.metric === PERCENT_METRIC ? 1 : -1
    return compareText(a.unit, b.unit)
  })
  for (const group of ordered) group.series.sort(compareSeries)
  return ordered
}

export function duplicateKeyLabels(
  series: readonly QuotaKeyObservationTrendSeries[],
): Set<string> {
  const idsByLabel = new Map<string, Set<string>>()
  for (const entry of series) {
    const ids = idsByLabel.get(entry.key_label) ?? new Set<string>()
    ids.add(entry.key_id)
    idsByLabel.set(entry.key_label, ids)
  }
  return new Set(
    [...idsByLabel.entries()]
      .filter(([, ids]) => ids.size > 1)
      .map(([label]) => label),
  )
}

export function shortKeyId(keyId: string): string {
  return keyId.replace(/-/g, '').slice(0, 8)
}

function customSourceIdentity(sourceId: string): string {
  const separator = sourceId.lastIndexOf(':')
  return separator >= 0 ? sourceId.slice(separator + 1) : sourceId
}

export function quotaTrendSeriesName(
  entry: QuotaKeyObservationTrendSeries,
  context: QuotaTrendNameContext,
): string {
  const label = entry.key_label.trim()
  const key = label
    ? context.duplicateLabels.has(entry.key_label)
      ? `${label} #${shortKeyId(entry.key_id)}`
      : label
    : shortKeyId(entry.key_id)
  const kind = context.kinds[entry.kind] ?? entry.kind
  const slot =
    entry.kind === 'custom'
      ? `${kind} (${customSourceIdentity(entry.source_id)})`
      : kind
  return context.singleKey ? slot : `${key} · ${slot}`
}

export function quotaTrendLegendEntries(
  group: QuotaTrendGroup,
  input: QuotaTrendCoreOptionInput,
): QuotaTrendLegendEntry[] {
  const duplicateLabels = duplicateKeyLabels(group.series)
  const singleKey =
    new Set(group.series.map((entry) => entry.key_id)).size === 1
  const palette = input.theme.categorical
  const usedNames = new Set<string>()
  const entries: QuotaTrendLegendEntry[] = []
  let colorIndex = 0
  for (const entry of group.series) {
    let name = quotaTrendSeriesName(entry, {
      duplicateLabels,
      singleKey,
      kinds: input.labels.kinds,
    })
    if (usedNames.has(name)) name = `${name} · ${entry.source_id}`
    usedNames.add(name)
    entries.push({
      name,
      color: palette[colorIndex % palette.length],
      series: entry,
    })
    colorIndex += 1
  }
  return entries
}

export function createQuotaTrendPanelOption(
  group: QuotaTrendGroup,
  input: QuotaTrendCoreOptionInput,
) {
  const theme = input.theme
  const palette = theme.categorical
  const hidden = input.hiddenSeries ?? new Set<string>()
  const metaByName = new Map<
    string,
    { metric: QuotaKeyObservationTrendMetric; unit: string }
  >()
  const series: Record<string, unknown>[] = []

  for (const { name, color, series: entry } of quotaTrendLegendEntries(
    group,
    input,
  )) {
    metaByName.set(name, { metric: group.metric, unit: group.unit })
    if (hidden.has(name)) continue
    const points = sortedPoints(entry)
    series.push({
      name,
      type: 'line',
      step: 'end',
      connectNulls: false,
      showSymbol: points.length <= 48,
      symbol: 'circle',
      symbolSize: 4,
      data: points.map((point) => (point.value == null ? null : point.value)),
      itemStyle: { color },
      lineStyle: { color },
    })
  }

  type TooltipParam = {
    axisValue?: string | number
    marker?: string
    seriesName?: string
    value?: number | string | null
  }
  const formatTooltip = (params: TooltipParam[]): string => {
    const first = params[0]
    if (!first) return ''
    const lines = params.map((item) => {
      const meta = item.seriesName ? metaByName.get(item.seriesName) : undefined
      let display = input.labels.unavailable
      if (item.value != null) {
        display =
          meta?.metric === PERCENT_METRIC
            ? `${item.value}${input.labels.percentSuffix}`
            : input.formatAmount(Number(item.value), meta?.unit ?? '')
      }
      return `${item.marker ?? ''}${escapeHtml(item.seriesName ?? '')}: ${escapeHtml(display)}`
    })
    const axis = input.formatTooltipTime(String(first.axisValue ?? ''))
    return `${escapeHtml(axis)}<br/>${lines.join('<br/>')}`
  }

  const categoryPoints =
    group.series.length > 0 ? sortedPoints(group.series[0]) : []
  const categories = categoryPoints.map((point) => point.at)
  const axisLabels = new Map<string, string>()
  let previousDay: string | null = null
  for (const at of categories) {
    const date = new Date(at)
    const day = Number.isNaN(date.getTime()) ? at : input.formatDayKey(date)
    const label =
      input.range === '24h'
        ? previousDay !== null && previousDay !== day
          ? `${input.formatAxisDate(at)} ${input.formatAxisTime(at)}`
          : input.formatAxisTime(at)
        : input.formatAxisDate(at)
    axisLabels.set(at, label)
    previousDay = day
  }

  const isPercent = group.metric === PERCENT_METRIC
  const yAxis = isPercent
    ? {
        type: 'value',
        position: 'left',
        min: 0,
        max: 100,
        axisLabel: {
          color: theme.muted,
          fontSize: 10,
          formatter: `{value}${input.labels.percentSuffix}`,
        },
        splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
      }
    : {
        type: 'value',
        position: 'left',
        scale: true,
        name: group.unit,
        nameLocation: 'middle',
        nameGap: 48,
        nameRotate: 90,
        nameTextStyle: { color: theme.muted, fontSize: 10 },
        axisLabel: { color: theme.muted, fontSize: 10 },
        splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
      }
  const legend: LegendComponentOption = { show: false }

  return {
    backgroundColor: 'transparent',
    color: palette,
    grid: { left: 56, right: 24, top: 16, bottom: 34 },
    legend,
    tooltip: {
      trigger: 'axis',
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: formatTooltip,
    },
    xAxis: {
      type: 'category',
      data: categories,
      axisLabel: {
        color: theme.muted,
        fontSize: 10,
        formatter: (value: string | number) =>
          axisLabels.get(String(value)) ?? input.formatAxisTime(String(value)),
      },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    yAxis,
    series,
  }
}
