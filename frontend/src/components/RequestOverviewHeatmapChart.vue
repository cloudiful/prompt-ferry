<script setup lang="ts">
import { HeatmapChart } from 'echarts/charts'
import {
  GridComponent,
  TooltipComponent,
  VisualMapComponent,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { themeMode } from '@/theme/appTheme'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type {
  RequestOverviewDrilldown,
  RequestOverviewHeatmapCell,
  RequestOverviewResponse,
} from '../request-overview'
import { createHeatmapOption } from '../request-overview-charts'

echarts.use([
  HeatmapChart,
  GridComponent,
  TooltipComponent,
  VisualMapComponent,
  CanvasRenderer,
])

const props = defineProps<{
  overview: RequestOverviewResponse
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const emit = defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()

const heatmapEl = ref<HTMLDivElement | null>(null)
let heatmapChart: echarts.ECharts | null = null
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
    renderChart()
  }, 140)
}

function bindHeatmapClick(): void {
  const cells = props.overview.heatmap.cells
  heatmapChart?.off('click')
  heatmapChart?.on('click', (params) => {
    const [xIndex, yIndex] = params.data as [number, number, number]
    const cell =
      cells[
        yIndex * Math.max(1, props.overview.heatmap.x_labels.length) + xIndex
      ] ??
      cells.find((item) => item.x_index === xIndex && item.y_index === yIndex)
    if (!cell) return
    emit('drilldown', drilldownFromCell(cell))
  })
}

function drilldownFromCell(
  cell: RequestOverviewHeatmapCell,
): RequestOverviewDrilldown {
  return {
    endpoint_id: cell.endpoint_id ?? null,
    model: cell.model ?? null,
    mcp_server_id: cell.mcp_server_id ?? null,
    mcp_bearer_token_slot: cell.mcp_bearer_token_slot ?? null,
  }
}

function renderChart(): void {
  if (!canInitChart(heatmapEl.value)) {
    if (heatmapEl.value) scheduleRenderRetry()
    return
  }
  heatmapChart ??= echarts.init(heatmapEl.value)
  heatmapChart.setOption(
    createHeatmapOption({
      heatmap: props.overview.heatmap,
      formatCount: props.formatting.formatCount,
      formatPercent: props.formatting.formatPercent,
      formatMs: props.formatting.formatMs,
    }),
    true,
  )
  bindHeatmapClick()
}

onMounted(() => {
  void nextTick(renderChart)
  resizeObserver = new ResizeObserver(() => {
    renderChart()
    heatmapChart?.resize()
  })
  if (heatmapEl.value) resizeObserver.observe(heatmapEl.value)
})

watch(
  () => props.overview,
  () => nextTick(renderChart),
  { deep: true },
)

watch(themeMode, () => nextTick(renderChart))

onBeforeUnmount(() => {
  if (renderRetryTimer != null) {
    clearTimeout(renderRetryTimer)
    renderRetryTimer = null
  }
  resizeObserver?.disconnect()
  heatmapChart?.dispose()
})
</script>

<template>
  <section class="rounded-xl border border-default bg-default p-4">
    <div
      class="mb-3 flex items-center gap-2 text-sm font-semibold text-highlighted"
    >
      {{ t('overviewHeatmap') }}
    </div>
    <div
      ref="heatmapEl"
      class="h-[22rem] min-h-[14rem] w-full md:h-[34rem] md:min-h-[18rem]"
    ></div>
  </section>
</template>
