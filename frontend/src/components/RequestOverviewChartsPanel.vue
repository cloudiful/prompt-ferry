<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref } from 'vue'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type {
  RequestOverviewDrilldown,
  RequestOverviewResponse,
} from '../request-overview'

const RequestOverviewTrendErrorCharts = defineAsyncComponent(
  () => import('./RequestOverviewTrendErrorCharts.vue'),
)
const RequestOverviewHeatmapChart = defineAsyncComponent(
  () => import('./RequestOverviewHeatmapChart.vue'),
)

defineProps<{
  overview: RequestOverviewResponse
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const emit = defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()

const showHeatmap = ref(false)

onMounted(() => {
  showHeatmap.value = window.innerWidth >= 980
})
</script>

<template>
  <div class="grid gap-4">
    <RequestOverviewTrendErrorCharts
      :overview="overview"
      :formatting="formatting"
      :t="t"
    />

    <section
      v-if="!showHeatmap"
      class="rounded-xl border border-default bg-default p-4"
    >
      <div
        class="mb-3 flex flex-wrap items-center justify-between gap-2 text-sm font-semibold text-highlighted"
      >
        <div>{{ t('overviewHeatmap') }}</div>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          @click="
            () => {
              showHeatmap = true
            }
          "
          >{{ t('showMoreContent') }}</UButton
        >
      </div>
    </section>

    <RequestOverviewHeatmapChart
      v-else
      :overview="overview"
      :formatting="formatting"
      :t="t"
      @drilldown="emit('drilldown', $event)"
    />
  </div>
</template>
