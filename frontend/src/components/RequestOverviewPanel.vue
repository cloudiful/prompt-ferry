<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type {
  RequestOverviewDrilldown,
  RequestOverviewResponse,
} from '../request-overview'
import { useLocale } from '@/composables/useLocale'
const RequestOverviewTrafficPanel = defineAsyncComponent(
  () => import('./RequestOverviewTrafficPanel.vue'),
)

const props = defineProps<{
  overview: RequestOverviewResponse | null
  loading: boolean
  category: 'ai' | 'mcp'
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const emit = defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()
const { t } = useLocale()
const visibleSummaryCards = computed(
  () => props.overview?.summary_cards.slice(0, 6) ?? [],
)
const hasOverviewTraffic = computed(() => {
  if (!props.overview) return false
  const hasTrendRequests = props.overview.trend.some(
    (item) => item.request_count > 0,
  )
  const hasRankingRows = props.overview.top_rankings.some(
    (group) => group.rows.length > 0,
  )
  const hasHeatmapTraffic = props.overview.heatmap.cells.some(
    (cell) => cell.request_count > 0,
  )
  return hasTrendRequests || hasRankingRows || hasHeatmapTraffic
})
const emptyLabel = computed(() =>
  props.loading
    ? t('loading')
    : props.category === 'ai'
      ? t('noAiRequestRecords')
      : t('noMcpCallRecords'),
)

function formatCardValue(unit: string, value: number): string {
  if (unit === 'ratio') return props.formatting.formatPercent(value)
  if (unit === 'ms') return props.formatting.formatMs(value)
  if (unit === 'score') return `${Math.round(value)}`
  return props.formatting.formatCount(value)
}
</script>

<template>
  <div class="grid gap-3">
    <template v-if="overview && hasOverviewTraffic">
      <div class="grid gap-2 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-6">
        <article
          v-for="card in visibleSummaryCards"
          :key="card.key"
          class="grid gap-1 rounded-xl border border-default bg-default px-3 py-2.5"
        >
          <div
            class="text-[0.68rem] font-bold tracking-wide text-dimmed uppercase"
          >
            {{ card.label }}
          </div>
          <div
            class="text-[0.98rem] leading-none font-bold text-highlighted md:text-[1.02rem]"
          >
            {{ formatCardValue(card.unit, card.value) }}
          </div>
        </article>
      </div>

      <RequestOverviewTrafficPanel
        :overview="overview"
        :category="category"
        :formatting="formatting"
        :t="t"
        @drilldown="emit('drilldown', $event)"
      />
    </template>
    <div
      v-else
      class="rounded-xl border border-default bg-default px-4 py-6 text-sm text-dimmed"
    >
      {{ emptyLabel }}
    </div>
  </div>
</template>
