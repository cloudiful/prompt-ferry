<script setup lang="ts">
import { BarChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { themeMode } from '@/theme/appTheme'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type { RequestOverviewResponse } from '../request-overview'
import {
  createErrorOption,
  createTrendOption,
} from '../request-overview-charts'

echarts.use([
  BarChart,
  LineChart,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  CanvasRenderer,
])

const props = defineProps<{
  overview: RequestOverviewResponse
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const trendEl = ref<HTMLDivElement | null>(null)
const errorEl = ref<HTMLDivElement | null>(null)
let trendChart: echarts.ECharts | null = null
let errorChart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null
let renderRetryTimer: ReturnType<typeof setTimeout> | null = null

function canInitChart(
  element: HTMLDivElement | null,
): element is HTMLDivElement {
  return !!element && element.clientWidth > 0 && element.clientHeight > 0
}

function scheduleRenderRetry(): void {
  if (renderRetryTimer != null) return
  renderRetryTimer = window.setTimeout(() => {
    renderRetryTimer = null
    renderCharts()
  }, 140)
}

function renderCharts(): void {
  let waitingForSize = false

  if (canInitChart(trendEl.value)) {
    trendChart ??= echarts.init(trendEl.value)
    trendChart.setOption(
      createTrendOption({
        trend: props.overview.trend,
        formatTime: props.formatting.formatTime,
      }),
      true,
    )
  } else if (trendEl.value) {
    waitingForSize = true
  }

  if (canInitChart(errorEl.value)) {
    errorChart ??= echarts.init(errorEl.value)
    errorChart.setOption(
      createErrorOption({
        rows: props.overview.error_breakdown,
        formatCount: props.formatting.formatCount,
        formatPercent: props.formatting.formatPercent,
      }),
      true,
    )
  } else if (errorEl.value) {
    waitingForSize = true
  }

  if (waitingForSize) scheduleRenderRetry()
}

onMounted(() => {
  void nextTick(renderCharts)
  resizeObserver = new ResizeObserver(() => {
    renderCharts()
    trendChart?.resize()
    errorChart?.resize()
  })
  if (trendEl.value) resizeObserver.observe(trendEl.value)
  if (errorEl.value) resizeObserver.observe(errorEl.value)
})

watch(
  () => props.overview,
  () => nextTick(renderCharts),
  { deep: true },
)

watch(themeMode, () => nextTick(renderCharts))

onBeforeUnmount(() => {
  if (renderRetryTimer != null) {
    clearTimeout(renderRetryTimer)
    renderRetryTimer = null
  }
  resizeObserver?.disconnect()
  trendChart?.dispose()
  errorChart?.dispose()
})
</script>

<template>
  <div class="grid gap-4 xl:grid-cols-[minmax(0,1.65fr)_minmax(0,1fr)]">
    <section class="rounded-xl border border-default bg-default p-4">
      <div
        class="mb-3 flex items-center gap-2 text-sm font-semibold text-highlighted"
      >
        <UIcon name="i-lucide-activity" class="h-4 w-4" />
        <span>{{ t('overviewTrend') }}</span>
      </div>
      <div
        ref="trendEl"
        class="min-h-[14rem] w-full md:min-h-[18rem] md:h-[18rem]"
      ></div>
    </section>
    <section class="rounded-xl border border-default bg-default p-4">
      <div
        class="mb-3 flex items-center gap-2 text-sm font-semibold text-highlighted"
      >
        {{ t('overviewErrorBreakdown') }}
      </div>
      <div
        ref="errorEl"
        class="min-h-[14rem] w-full md:min-h-[18rem] md:h-[18rem]"
      ></div>
    </section>
  </div>
</template>
