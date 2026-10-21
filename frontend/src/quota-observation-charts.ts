import type {
  CanonicalQuotaWindowKind,
  QuotaKeyObservationTrendMetric,
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
} from '@/generated/admin-api'
import type { LegendComponentOption } from 'echarts/components'
import type { KeyQuotaTrendRange } from '@/composables/useKeyQuotaObservationTrend'
import { formatAppDateOnly } from '@/format/datetime'
import { getChartTheme } from '@/theme/chartTheme'

// Issue #911 P3: pure trend-option builder. The backend already emitted one
// fixed bucket grid per range, so the builder only projects series into
// theme-tokened ECharts options. It never fabricates a point: a null bucket
// stays a gap, and no series is area-filled or interpolated across it.
//
// Security: the trend tooltip is an ECharts HTML formatter, so every string it
// interpolates — axis values, series names, units and formatted amounts — is
// untrusted operator/upstream text and must be escaped. Only the structural
// `<br/>` separators and the ECharts-provided color `marker` are literal.
export type QuotaTrendGroup = {
  key: string
  metric: QuotaKeyObservationTrendMetric
  unit: string
  series: QuotaKeyObservationTrendSeries[]
}

export type QuotaTrendKindLabels = Record<CanonicalQuotaWindowKind, string>

export type QuotaTrendOptionInput = {
  range: KeyQuotaTrendRange
  formatAxisDate: (value: string) => string
  formatAxisTime: (value: string) => string
  formatTooltipTime: (value: string) => string
  formatAmount: (value: number, unit: string) => string
  // Series names the native legend currently hides. Names, colors and series
  // identity are unchanged; only visibility differs.
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

// Deterministic code-unit comparison: locale collation orders `USD` after
// `credits`, which would make panel order environment-dependent.
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

// Amounts and percentages are separate metrics, and every amount unit keeps
// its own group: CNY, USD and credits are never stacked on one scale, and a
// percent axis is never merged with an amount axis. Each group renders as its
// own single-axis panel.
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

// A label needs a key-id suffix only when it identifies more than one
// distinct key. Counting series would falsely disambiguate one key that has
// several windows (five-hour + weekly) under the same display label.
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

// Legend/tooltip name. A single key needs only the localized window slot; a
// multi-key group prefixes the key label, adding a short id only for a genuine
// same-label collision. Custom windows keep their source identity so two
// subscription windows from different sources never collapse into one name.
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

// Single authority for a group's series identity, legend name and color. The
// option builder and the native legend both read it, so a name or color can
// never drift between the chart and its controls.
export function quotaTrendLegendEntries(
  group: QuotaTrendGroup,
  input: QuotaTrendOptionInput,
): QuotaTrendLegendEntry[] {
  const duplicateLabels = duplicateKeyLabels(group.series)
  const singleKey =
    new Set(group.series.map((entry) => entry.key_id)).size === 1
  const palette = getChartTheme().categorical
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

// One panel per `(metric, unit)` group: exactly one y-axis and one series set,
// so two units never share a scale. The built-in canvas legend is disabled;
// the component renders an accessible native legend from the same entries.
export function createQuotaTrendPanelOption(
  group: QuotaTrendGroup,
  input: QuotaTrendOptionInput,
) {
  const theme = getChartTheme()
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
      // A step line keeps the observed level between samples instead of
      // implying a measured slope, and there is no area fill.
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
        if (meta?.metric === PERCENT_METRIC) {
          display = `${item.value}${input.labels.percentSuffix}`
        } else {
          display = input.formatAmount(Number(item.value), meta?.unit ?? '')
        }
      }
      // `marker` is ECharts-generated structural markup; everything derived
      // from data or labels is escaped.
      return `${item.marker ?? ''}${escapeHtml(item.seriesName ?? '')}: ${escapeHtml(display)}`
    })
    const axis = input.formatTooltipTime(String(first.axisValue ?? ''))
    return `${escapeHtml(axis)}<br/>${lines.join('<br/>')}`
  }

  // The category data stays raw so the tooltip can show the full localized
  // timestamp. Only the axis labels compress it: 24h keeps the time and adds a
  // date at a day boundary, while 7d/30d use compact dates.
  const categoryPoints =
    group.series.length > 0 ? sortedPoints(group.series[0]) : []
  const categories = categoryPoints.map((point) => point.at)
  const axisLabels = new Map<string, string>()
  let previousDay: string | null = null
  for (const at of categories) {
    const date = new Date(at)
    // The day boundary is the calendar day in the display time zone, not the
    // browser's own day, so the axis does not jump a day when the preference
    // differs from the browser zone.
    const day = Number.isNaN(date.getTime()) ? at : formatAppDateOnly(date)
    let label: string
    if (input.range === '24h') {
      label =
        previousDay !== null && previousDay !== day
          ? `${input.formatAxisDate(at)} ${input.formatAxisTime(at)}`
          : input.formatAxisTime(at)
    } else {
      label = input.formatAxisDate(at)
    }
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
        // No zero floor: a real zero and a negative balance must both stay
        // inside the axis instead of being clipped or hidden.
        scale: true,
        name: group.unit,
        nameLocation: 'middle',
        nameGap: 48,
        nameRotate: 90,
        nameTextStyle: { color: theme.muted, fontSize: 10 },
        axisLabel: { color: theme.muted, fontSize: 10 },
        splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
      }

  // The built-in canvas legend is disabled: at narrow widths its pager is
  // clipped, so the component renders a native, keyboard-accessible legend.
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
