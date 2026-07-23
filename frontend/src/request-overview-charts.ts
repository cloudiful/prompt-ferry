import type {
  RequestOverviewErrorBreakdownRow,
  RequestOverviewHeatmap,
  RequestOverviewTrendBucket,
} from './request-overview'
import { getChartTheme } from './theme/chartTheme'

export function createTrendOption(input: {
  trend: RequestOverviewTrendBucket[]
  formatTime: (value: string) => string
}) {
  const theme = getChartTheme()
  return {
    backgroundColor: 'transparent',
    color: [theme.accent, theme.info, theme.warn],
    grid: { left: 44, right: 46, top: 28, bottom: 34 },
    legend: {
      top: 0,
      textStyle: { color: theme.text, fontSize: 11 },
    },
    tooltip: {
      trigger: 'axis',
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
    },
    xAxis: {
      type: 'category',
      data: input.trend.map((item) => input.formatTime(item.bucket_at)),
      axisLabel: { color: theme.muted, fontSize: 10 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    yAxis: [
      {
        type: 'value',
        axisLabel: { color: theme.muted, fontSize: 10 },
        splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
      },
      {
        type: 'value',
        min: 0,
        max: 100,
        axisLabel: { color: theme.muted, fontSize: 10, formatter: '{value}%' },
        splitLine: { show: false },
      },
    ],
    series: [
      {
        name: '请求数',
        type: 'bar',
        data: input.trend.map((item) => item.request_count),
        barMaxWidth: 16,
      },
      {
        name: '成功率',
        type: 'line',
        yAxisIndex: 1,
        smooth: true,
        data: input.trend.map(
          (item) => Math.round(item.success_rate * 1000) / 10,
        ),
      },
      {
        name: '质量分',
        type: 'line',
        yAxisIndex: 1,
        smooth: true,
        data: input.trend.map((item) => Math.round(item.quality_score)),
      },
    ],
  }
}

export function createErrorOption(input: {
  rows: RequestOverviewErrorBreakdownRow[]
  formatCount: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
}) {
  const theme = getChartTheme()
  return {
    backgroundColor: 'transparent',
    grid: { left: 110, right: 24, top: 12, bottom: 20 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: (items: Array<{ dataIndex: number }>) => {
        const row = input.rows[items[0]?.dataIndex ?? 0]
        return row
          ? `${row.label}<br/>${input.formatCount(row.count)} / ${input.formatPercent(row.rate)}`
          : ''
      },
    },
    xAxis: {
      type: 'value',
      axisLabel: { color: theme.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
    },
    yAxis: {
      type: 'category',
      data: input.rows.map((row) => row.label),
      axisLabel: { color: theme.text, fontSize: 11 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    series: [
      {
        type: 'bar',
        data: input.rows.map((row) => row.count),
        itemStyle: { color: theme.warn, borderRadius: 4 },
      },
    ],
  }
}

export function createHeatmapOption(input: {
  heatmap: RequestOverviewHeatmap
  formatCount: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
  formatMs: (value?: number | null) => string
}) {
  const theme = getChartTheme()
  return {
    backgroundColor: 'transparent',
    tooltip: {
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: (params: {
        data: [number, number, number, number, number, number]
      }) => {
        const [xIndex, yIndex, quality, requests, successRate, p95] =
          params.data
        return [
          `${input.heatmap.y_labels[yIndex]} / ${input.heatmap.x_labels[xIndex]}`,
          `质量分: ${Math.round(quality)}`,
          `请求数: ${input.formatCount(requests)}`,
          `成功率: ${input.formatPercent(successRate)}`,
          `P95: ${input.formatMs(p95)}`,
        ].join('<br/>')
      },
    },
    grid: { left: 112, right: 24, top: 20, bottom: 56 },
    xAxis: {
      type: 'category',
      data: input.heatmap.x_labels,
      axisLabel: { color: theme.muted, fontSize: 10, interval: 0, rotate: 20 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    yAxis: {
      type: 'category',
      data: input.heatmap.y_labels,
      axisLabel: { color: theme.text, fontSize: 10 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    visualMap: {
      min: 0,
      max: 100,
      calculable: false,
      orient: 'horizontal',
      left: 'center',
      bottom: 4,
      textStyle: { color: theme.muted },
      inRange: { color: [theme.heatLow, theme.heatHigh] },
    },
    series: [
      {
        type: 'heatmap',
        data: input.heatmap.cells.map((cell) => [
          cell.x_index,
          cell.y_index,
          Math.round(cell.quality_score * 10) / 10,
          cell.request_count,
          cell.success_rate,
          cell.p95_total_ms ?? 0,
        ]),
        label: {
          show: true,
          color: theme.labelStrong,
          formatter: (params: { data: [number, number, number] }) =>
            String(Math.round(params.data[2])),
        },
        emphasis: {
          itemStyle: { borderColor: theme.emphasisBorder, borderWidth: 1 },
        },
      },
    ],
  }
}
