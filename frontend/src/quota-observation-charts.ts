import type {
  QuotaKeyObservationTrendMetric,
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
} from '@/generated/admin-api'
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

export type QuotaTrendOptionInput = {
  formatTime: (value: string) => string
  formatAmount: (value: number, unit: string) => string
  labels: {
    percentSuffix: string
    unavailable: string
  }
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

function duplicateKeyLabels(
  series: readonly QuotaKeyObservationTrendSeries[],
): Set<string> {
  const counts = new Map<string, number>()
  for (const entry of series) {
    counts.set(entry.key_label, (counts.get(entry.key_label) ?? 0) + 1)
  }
  return new Set(
    [...counts.entries()]
      .filter(([, count]) => count > 1)
      .map(([label]) => label),
  )
}

export function shortKeyId(keyId: string): string {
  return keyId.replace(/-/g, '').slice(0, 8)
}

// Legend/tooltip name: the key plus its window slot, with the amount unit
// appended so two currencies never look interchangeable. Duplicate key labels
// gain a short key id so two keys with the same display label stay distinct.
export function quotaTrendSeriesName(
  entry: QuotaKeyObservationTrendSeries,
  duplicateLabels: ReadonlySet<string>,
): string {
  const base = entry.key_label.trim() || shortKeyId(entry.key_id)
  const key = duplicateLabels.has(entry.key_label)
    ? `${base} #${shortKeyId(entry.key_id)}`
    : base
  const slot =
    entry.metric === PERCENT_METRIC
      ? entry.kind
      : `${entry.kind} (${entry.unit})`
  return `${key} · ${slot}`
}

// One panel per `(metric, unit)` group: exactly one y-axis and a scroll legend,
// so many series (up to the backend cap) never wrap a legend over the plot and
// never place two axes on one side.
export function createQuotaTrendPanelOption(
  group: QuotaTrendGroup,
  input: QuotaTrendOptionInput,
) {
  const theme = getChartTheme()
  const duplicateLabels = duplicateKeyLabels(group.series)
  const palette = [
    theme.accent,
    theme.info,
    theme.cached,
    theme.warn,
    theme.output,
    theme.input,
    theme.error,
  ]

  const metaByName = new Map<
    string,
    { metric: QuotaKeyObservationTrendMetric; unit: string }
  >()
  const usedNames = new Set<string>()
  const series: Record<string, unknown>[] = []
  let colorIndex = 0
  for (const entry of group.series) {
    let name = quotaTrendSeriesName(entry, duplicateLabels)
    if (usedNames.has(name)) name = `${name} · ${entry.source_id}`
    usedNames.add(name)
    metaByName.set(name, { metric: group.metric, unit: group.unit })
    const points = sortedPoints(entry)
    const color = palette[colorIndex % palette.length]
    colorIndex += 1
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
    return `${escapeHtml(String(first.axisValue ?? ''))}<br/>${lines.join('<br/>')}`
  }

  const category =
    group.series.length > 0
      ? sortedPoints(group.series[0]).map((point) => input.formatTime(point.at))
      : []

  const isPercent = group.metric === PERCENT_METRIC
  const yAxis = isPercent
    ? {
        type: 'value',
        position: 'left',
        min: 0,
        max: 100,
        name: input.labels.percentSuffix,
        nameLocation: 'middle',
        nameGap: 40,
        nameRotate: 90,
        nameTextStyle: { color: theme.muted, fontSize: 10 },
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

  return {
    backgroundColor: 'transparent',
    color: palette,
    grid: { left: 56, right: 24, top: 44, bottom: 34 },
    legend: {
      type: 'scroll',
      top: 0,
      width: '96%',
      textStyle: { color: theme.text, fontSize: 11 },
    },
    tooltip: {
      trigger: 'axis',
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: formatTooltip,
    },
    xAxis: {
      type: 'category',
      data: category,
      axisLabel: { color: theme.muted, fontSize: 10 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    yAxis,
    series,
  }
}
