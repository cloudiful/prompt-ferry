import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewErrorRow,
  RequestRecordOverviewTrendBucket,
} from './generated/admin-api'
import { getChartTheme } from './theme/chartTheme'

type ChartLabels = {
  cacheRead: string
  cacheRate: string
  cacheWrite: string
  error: string
  input: string
  output: string
  requests: string
  success: string
}

export function createTrendOption(input: {
  category: 'ai' | 'mcp'
  labels: ChartLabels
  trend: RequestRecordOverviewTrendBucket[]
  formatTime: (value: string) => string
}) {
  const theme = getChartTheme()
  const isAi = input.category === 'ai'
  const series = isAi
    ? [
        {
          name: input.labels.input,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.input_tokens),
          itemStyle: { color: theme.input },
        },
        {
          name: input.labels.cacheRead,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.cache_read_tokens),
          itemStyle: { color: theme.cached },
        },
        {
          name: input.labels.cacheWrite,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.cache_write_tokens),
          itemStyle: { color: theme.warn },
        },
        {
          name: input.labels.output,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.output_tokens),
          itemStyle: { color: theme.output },
        },
        {
          name: input.labels.cacheRate,
          type: 'line',
          yAxisIndex: 1,
          smooth: true,
          connectNulls: false,
          data: input.trend.map((item) =>
            item.tokens.cache_rate == null
              ? null
              : Math.round(item.tokens.cache_rate * 1000) / 10,
          ),
          itemStyle: { color: theme.cached },
        },
      ]
    : [
        {
          name: input.labels.success,
          type: 'bar',
          stack: 'requests',
          data: input.trend.map((item) => item.success_count),
          itemStyle: { color: theme.accent },
        },
        {
          name: input.labels.error,
          type: 'bar',
          stack: 'requests',
          data: input.trend.map((item) => item.error_count),
          itemStyle: { color: theme.error },
        },
      ]

  return {
    backgroundColor: 'transparent',
    color: [theme.accent, theme.cached, theme.warn, theme.output, theme.info],
    grid: { left: 52, right: isAi ? 52 : 24, top: 42, bottom: 34 },
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
      ...(isAi
        ? [
            {
              type: 'value',
              min: 0,
              max: 100,
              axisLabel: {
                color: theme.muted,
                fontSize: 10,
                formatter: '{value}%',
              },
              splitLine: { show: false },
            },
          ]
        : []),
    ],
    series,
  }
}

export function createBreakdownOption(input: {
  category: 'ai' | 'mcp'
  labels: ChartLabels
  rows: RequestRecordOverviewBreakdownRow[]
  formatCount: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
}) {
  const theme = getChartTheme()
  const isAi = input.category === 'ai'
  const rows = input.rows.slice(0, 12)
  return {
    backgroundColor: 'transparent',
    grid: { left: 112, right: 32, top: 12, bottom: 24 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: (params: Array<{ dataIndex: number }>) => {
        const row = rows[params[0]?.dataIndex ?? 0]
        if (!row) return ''
        const value = isAi ? row.tokens.total_tokens : row.request_count
        const share = isAi ? row.token_share : row.request_share
        return `${row.label}<br/>${input.formatCount(value)} / ${input.formatPercent(share)}`
      },
    },
    xAxis: {
      type: 'value',
      axisLabel: { color: theme.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: theme.grid, type: 'dashed' } },
    },
    yAxis: {
      type: 'category',
      data: rows.map((row) => row.label),
      axisLabel: { color: theme.text, fontSize: 11 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    series: [
      {
        type: 'bar',
        data: rows.map((row) =>
          isAi ? row.tokens.total_tokens : row.request_count,
        ),
        itemStyle: { color: isAi ? theme.accent : theme.info, borderRadius: 3 },
        barMaxWidth: 20,
      },
    ],
  }
}

export function createErrorOption(input: {
  rows: RequestRecordOverviewErrorRow[]
  formatCount: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
}) {
  const theme = getChartTheme()
  const rows = input.rows.slice(0, 10)
  return {
    backgroundColor: 'transparent',
    grid: { left: 112, right: 24, top: 12, bottom: 20 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      backgroundColor: theme.bg,
      borderColor: theme.border,
      textStyle: { color: theme.text },
      formatter: (params: Array<{ dataIndex: number }>) => {
        const row = rows[params[0]?.dataIndex ?? 0]
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
      data: rows.map((row) => row.label),
      axisLabel: { color: theme.text, fontSize: 11 },
      axisLine: { lineStyle: { color: theme.axis } },
    },
    series: [
      {
        type: 'bar',
        data: rows.map((row) => row.count),
        itemStyle: { color: theme.error, borderRadius: 3 },
        barMaxWidth: 20,
      },
    ],
  }
}
