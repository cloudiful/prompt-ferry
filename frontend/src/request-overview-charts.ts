import type { RequestRecordOverviewTrendBucket } from './generated/admin-api'
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
  formatCompact: (value?: number | null) => string
}) {
  const theme = getChartTheme()
  const isAi = input.category === 'ai'
  const formatAxisValue = (value: string | number): string => {
    const numeric = typeof value === 'string' ? Number(value) : value
    if (Number.isNaN(numeric)) return '-'
    return input.formatCompact(numeric)
  }
  type TrendTooltipParam = {
    axisValue?: string | number
    dataIndex: number
    marker?: string
    seriesName?: string
    value?: number | string | null
  }
  const formatTooltip = (params: TrendTooltipParam[]): string => {
    const first = params[0]
    if (!first) return ''
    const lines = params.map((item) => {
      let display: string
      if (item.value == null) {
        display = '-'
      } else if (item.seriesName === input.labels.cacheRate) {
        display = `${item.value}%`
      } else {
        const numeric =
          typeof item.value === 'string' ? Number(item.value) : item.value
        display =
          typeof numeric === 'number' && Number.isNaN(numeric)
            ? '-'
            : input.formatCompact(numeric)
      }
      return `${item.marker ?? ''}${item.seriesName ?? ''}: ${display}`
    })
    return `${first.axisValue ?? ''}<br/>${lines.join('<br/>')}`
  }
  const series = isAi
    ? [
        {
          name: input.labels.cacheRead,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.cache_read_tokens),
          itemStyle: { color: theme.cached },
        },
        {
          name: input.labels.input,
          type: 'bar',
          stack: 'tokens',
          data: input.trend.map((item) => item.tokens.input_tokens),
          itemStyle: { color: theme.input },
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
      formatter: formatTooltip,
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
        axisLabel: {
          color: theme.muted,
          fontSize: 10,
          formatter: formatAxisValue,
        },
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
