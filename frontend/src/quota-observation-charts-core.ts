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

export const QUOTA_TREND_KIND_KEYS: Record<CanonicalQuotaWindowKind, string> = {
  five_hour: 'tokenPlanInterval',
  rolling: 'tokenPlanRolling',
  weekly: 'tokenPlanWeekly',
  monthly: 'tokenPlanMonthly',
  balance: 'quotaTrendKindBalance',
  unlimited: 'tokenPlanNoLimit',
  custom: 'quotaTrendKindCustom',
}

export const QUOTA_TREND_KINDS: readonly CanonicalQuotaWindowKind[] = [
  'five_hour',
  'rolling',
  'weekly',
  'monthly',
  'balance',
  'unlimited',
  'custom',
]

export function quotaTrendKindLabels(
  translate: (key: string) => string,
): QuotaTrendKindLabels {
  const labels = {} as QuotaTrendKindLabels
  for (const kind of QUOTA_TREND_KINDS) {
    labels[kind] = translate(QUOTA_TREND_KIND_KEYS[kind])
  }
  return labels
}

export type QuotaTrendCoreOptionInput = {
  range: QuotaTrendRange
  from?: string
  to?: string
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

/**
 * Splits one logical series into non-increasing segments. A finite increase or
 * a null/unknown/gap starts a new segment: the earlier low sample ends the old
 * segment and the high sample begins the new one, so no line is ever drawn
 * upward and no reset is bridged. Original measured points are never copied or
 * synthesized.
 */
function measurementMs(point: QuotaKeyObservationTrendPoint): number | null {
  if (point.value == null || !Number.isFinite(point.value)) return null
  if (point.observed_at == null) return null
  const ms = Date.parse(point.observed_at)
  return Number.isFinite(ms) ? ms : null
}

export function splitQuotaTrendSegments(
  points: readonly QuotaKeyObservationTrendPoint[],
): QuotaKeyObservationTrendPoint[][] {
  const ordered = [...points].sort((a, b) => compareText(a.at, b.at))
  const segments: QuotaKeyObservationTrendPoint[][] = []
  let current: QuotaKeyObservationTrendPoint[] = []
  let previousValue: number | null = null
  for (const point of ordered) {
    const value = measurementMs(point) == null ? null : point.value
    if (value == null) {
      if (current.length > 0) segments.push(current)
      current = []
      previousValue = null
      continue
    }
    if (previousValue != null && value > previousValue) {
      if (current.length > 0) segments.push(current)
      current = []
    }
    current.push(point)
    previousValue = value
  }
  if (current.length > 0) segments.push(current)
  return segments
}

export function groupQuotaTrendSeries(
  series: readonly QuotaKeyObservationTrendSeries[],
): QuotaTrendGroup[] {
  const groups = new Map<string, QuotaTrendGroup>()
  for (const entry of series) {
    if (isHiddenQuotaTrendSource(entry.source_id)) continue
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

// MiniMax video observations are stored, but the shared display projection
// hides them from quota detail, history, and trend charts. Identity lives in
// the canonical source id (`minimax:<model>:<slot>`).
export function isHiddenQuotaTrendSource(sourceId: string): boolean {
  const [provider, model] = sourceId.split(':')
  return (
    provider?.trim().toLowerCase() === 'minimax' &&
    model?.trim().toLowerCase() === 'video'
  )
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
    const denseSymbols = points.length <= 48
    for (const segment of splitQuotaTrendSegments(points)) {
      const data = segment.flatMap((point) => {
        const ms = measurementMs(point)
        return ms == null ? [] : [[ms, point.value]]
      })
      if (data.length === 0) continue
      series.push({
        name,
        type: 'line',
        smooth: 0.2,
        smoothMonotone: 'x',
        connectNulls: false,
        showSymbol: segment.length === 1 || denseSymbols,
        symbol: 'circle',
        symbolSize: 4,
        data,
        itemStyle: { color },
        lineStyle: { color },
      })
    }
  }

  type TooltipParam = {
    axisValue?: string | number
    marker?: string
    seriesName?: string
    value?: number | string | null | Array<number | string | null>
  }
  const measurementOf = (item: TooltipParam): number | null => {
    const value = Array.isArray(item.value) ? item.value[0] : undefined
    const ms = typeof value === 'number' ? value : Number(value)
    if (Number.isFinite(ms)) return ms
    const axis = Number(item.axisValue)
    return Number.isFinite(axis) ? axis : null
  }
  const isoAt = (ms: number): string => new Date(ms).toISOString()
  const formatTooltip = (params: TooltipParam[]): string => {
    const first = params[0]
    if (!first) return ''
    const headerMs = measurementOf(first)
    const header =
      headerMs == null
        ? input.formatTooltipTime(String(first.axisValue ?? ''))
        : input.formatTooltipTime(isoAt(headerMs))
    const seen = new Set<string>()
    const lines: string[] = []
    for (const item of params) {
      const name = item.seriesName ?? ''
      const raw = Array.isArray(item.value) ? item.value[1] : item.value
      const itemMs = measurementOf(item)
      const dedupe = `${name}\u0000${itemMs ?? 'axis'}`
      if (seen.has(dedupe)) continue
      seen.add(dedupe)
      const meta = metaByName.get(name)
      let display = input.labels.unavailable
      if (raw != null) {
        display =
          meta?.metric === PERCENT_METRIC
            ? `${raw}${input.labels.percentSuffix}`
            : input.formatAmount(Number(raw), meta?.unit ?? '')
      }
      const measurement =
        itemMs != null && itemMs !== headerMs
          ? ` · ${input.formatTooltipTime(isoAt(itemMs))}`
          : ''
      lines.push(
        `${item.marker ?? ''}${escapeHtml(name)}: ${escapeHtml(display)}${escapeHtml(measurement)}`,
      )
    }
    return `${escapeHtml(header)}<br/>${lines.join('<br/>')}`
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
  const fromMs = input.from === undefined ? Number.NaN : Date.parse(input.from)
  const toMs = input.to === undefined ? Number.NaN : Date.parse(input.to)
  const rangeBounds = {
    ...(Number.isFinite(fromMs) ? { min: fromMs } : {}),
    ...(Number.isFinite(toMs) ? { max: toMs } : {}),
  }

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
      type: 'time',
      ...rangeBounds,
      axisLabel: {
        color: theme.muted,
        fontSize: 10,
        formatter: (value: number) =>
          input.range === '24h'
            ? input.formatAxisTime(isoAt(value))
            : input.formatAxisDate(isoAt(value)),
      },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    yAxis,
    series,
  }
}
