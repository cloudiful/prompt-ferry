<script setup lang="ts">
import { BarChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { themeMode } from '@/theme/appTheme'
import { useChartTheme } from '@/theme/chartTheme'

echarts.use([
  BarChart,
  LineChart,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  CanvasRenderer,
])

export type UsageSeriesPoint = {
  bucket_at: string
  request_count: number
  error_count: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  cached_tokens: number
  cache_rate?: number | null
  error_rate?: number | null
  avg_duration_ms?: number | null
  avg_first_chunk_ms?: number | null
}

const props = defineProps<{
  points: UsageSeriesPoint[]
  labels: {
    inputTokens: string
    outputTokens: string
    cachedTokens: string
    errorRate: string
    requests: string
    errors: string
    cacheRate: string
    firstTokenLatency: string
    totalLatency: string
  }
  formatBucketTime: (value: string) => string
  formatCount: (value?: number | null) => string
  formatPercent: (value?: number | null) => string
  formatMs: (value?: number | null) => string
}>()

const chartEl = ref<HTMLDivElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null
const theme = useChartTheme()

const option = computed(() => ({
  backgroundColor: 'transparent',
  color: [
    theme.value.input,
    theme.value.output,
    theme.value.cached,
    theme.value.error,
  ],
  animationDuration: 220,
  grid: { left: 48, right: 40, top: 36, bottom: 38 },
  legend: {
    top: 0,
    icon: 'roundRect',
    itemWidth: 18,
    itemHeight: 8,
    itemGap: 14,
    textStyle: {
      color: theme.value.text,
      fontSize: 11,
      fontFamily: 'Monaco, "Lucida Console", monospace',
    },
  },
  tooltip: {
    trigger: 'axis',
    backgroundColor: theme.value.bg,
    borderColor: theme.value.border,
    borderWidth: 1,
    padding: [8, 10],
    textStyle: {
      color: theme.value.text,
      fontSize: 11,
      fontFamily: 'Monaco, "Lucida Console", monospace',
    },
    extraCssText: 'border-radius:4px;',
    axisPointer: {
      type: 'shadow',
      shadowStyle:
        theme.value.input === '#2563eb'
          ? { color: 'rgba(37, 99, 235, 0.08)' }
          : { color: 'rgba(88, 232, 121, 0.07)' },
      lineStyle: { color: theme.value.axis },
    },
    confine: true,
    formatter: (params: unknown) => tooltipHtml(params),
  },
  xAxis: {
    type: 'category',
    data: props.points.map((point) => props.formatBucketTime(point.bucket_at)),
    axisTick: { show: false },
    axisLabel: {
      color: theme.value.muted,
      fontSize: 10,
      fontFamily: 'Monaco, "Lucida Console", monospace',
      margin: 10,
    },
    axisLine: { lineStyle: { color: theme.value.axis } },
  },
  yAxis: [
    {
      type: 'value',
      axisLabel: {
        color: theme.value.muted,
        fontSize: 10,
        fontFamily: 'Monaco, "Lucida Console", monospace',
      },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { lineStyle: { color: theme.value.grid, type: 'dashed' } },
    },
    {
      type: 'value',
      min: 0,
      max: 100,
      axisLabel: {
        color: theme.value.subtle,
        fontSize: 10,
        fontFamily: 'Monaco, "Lucida Console", monospace',
        formatter: '{value}%',
      },
      axisLine: { show: false },
      axisTick: { show: false },
      splitLine: { show: false },
    },
  ],
  series: [
    {
      name: props.labels.inputTokens,
      type: 'bar',
      stack: 'tokens',
      data: props.points.map((point) => point.input_tokens),
      barMaxWidth: 16,
      itemStyle: { borderRadius: [0, 0, 2, 2], opacity: 0.86 },
      emphasis: { itemStyle: { opacity: 1 } },
    },
    {
      name: props.labels.outputTokens,
      type: 'bar',
      stack: 'tokens',
      data: props.points.map((point) => point.output_tokens),
      barMaxWidth: 16,
      itemStyle: { opacity: 0.78 },
      emphasis: { itemStyle: { opacity: 1 } },
    },
    {
      name: props.labels.cachedTokens,
      type: 'bar',
      stack: 'tokens',
      data: props.points.map((point) => point.cached_tokens),
      barMaxWidth: 16,
      itemStyle: { borderRadius: [2, 2, 0, 0], opacity: 0.72 },
      emphasis: { itemStyle: { opacity: 0.94 } },
    },
    {
      name: props.labels.errorRate,
      type: 'line',
      yAxisIndex: 1,
      smooth: true,
      symbol: 'circle',
      symbolSize: 5,
      lineStyle: { width: 2, color: theme.value.error },
      itemStyle: {
        color: theme.value.bg,
        borderColor: theme.value.error,
        borderWidth: 2,
      },
      emphasis: { scale: 1.4 },
      data: props.points.map(
        (point) => Math.round((point.error_rate ?? 0) * 1000) / 10,
      ),
    },
  ],
}))

function tooltipHtml(params: unknown): string {
  const items = Array.isArray(params) ? params : [params]
  const first = items[0] as { dataIndex?: number } | undefined
  const index = Number(first?.dataIndex ?? 0)
  const point = props.points[index]
  if (!point) return ''
  return [
    `<strong style="color:${theme.value.text}">${props.formatBucketTime(point.bucket_at)}</strong>`,
    `${props.labels.requests}: ${props.formatCount(point.request_count)}`,
    `${props.labels.errors}: ${props.formatCount(point.error_count)} / ${props.formatPercent(point.error_rate)}`,
    `${props.labels.inputTokens}: ${props.formatCount(point.input_tokens)}`,
    `${props.labels.outputTokens}: ${props.formatCount(point.output_tokens)}`,
    `${props.labels.cachedTokens}: ${props.formatCount(point.cached_tokens)} / ${props.formatPercent(point.cache_rate)}`,
    `${props.labels.totalLatency}: ${props.formatMs(point.avg_duration_ms)}`,
    `${props.labels.firstTokenLatency}: ${props.formatMs(point.avg_first_chunk_ms)}`,
  ].join('<br/>')
}

function render(): void {
  if (!chartEl.value) return
  chart ??= echarts.init(chartEl.value)
  chart.setOption(option.value, true)
}

onMounted(() => {
  render()
  resizeObserver = new ResizeObserver(() => chart?.resize())
  if (chartEl.value) resizeObserver.observe(chartEl.value)
})

watch(option, () => nextTick(render), { deep: true })
watch(themeMode, () => nextTick(render))

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  chart?.dispose()
  chart = null
})
</script>

<template>
  <div ref="chartEl" class="h-72 w-full"></div>
</template>
