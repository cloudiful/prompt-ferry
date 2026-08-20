<script setup lang="ts">
import { BarChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import type { EChartsOption } from 'echarts'
import { CanvasRenderer } from 'echarts/renderers'
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { themeMode } from '@/theme/appTheme'

echarts.use([
  BarChart,
  CanvasRenderer,
  GridComponent,
  LegendComponent,
  LineChart,
  TooltipComponent,
])

const props = defineProps<{
  option: EChartsOption
}>()

const chartEl = ref<HTMLDivElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null

function renderChart(): void {
  const element = chartEl.value
  if (!element || element.clientWidth <= 0 || element.clientHeight <= 0) return
  chart ??= echarts.init(element)
  chart.setOption(props.option, true)
}

onMounted(() => {
  void nextTick(renderChart)
  resizeObserver = new ResizeObserver(() => chart?.resize())
  if (chartEl.value) resizeObserver.observe(chartEl.value)
})

watch(
  () => props.option,
  () => nextTick(renderChart),
  { deep: true },
)
watch(themeMode, () => nextTick(renderChart))

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
  chart?.dispose()
})
</script>

<template>
  <div ref="chartEl" class="h-[20rem] min-h-[16rem] w-full" />
</template>
