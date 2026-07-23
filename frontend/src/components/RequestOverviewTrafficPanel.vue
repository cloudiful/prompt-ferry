<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type {
  RequestOverviewDrilldown,
  RequestOverviewResponse,
} from '../request-overview'

const RequestOverviewChartsPanel = defineAsyncComponent(
  () => import('./RequestOverviewChartsPanel.vue'),
)

const props = defineProps<{
  overview: RequestOverviewResponse
  category: 'ai' | 'mcp'
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const emit = defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()

const rankingGroups = computed(() =>
  props.overview.top_rankings.filter(
    (group) =>
      group.key.toLowerCase().includes('endpoint') &&
      group.key.toLowerCase().includes('model'),
  ),
)

function emitRankingDrilldown(
  row: RequestOverviewResponse['top_rankings'][number]['rows'][number],
): void {
  emit('drilldown', {
    endpoint_id: row.endpoint_id ?? null,
    model: row.model ?? null,
    mcp_server_id: row.mcp_server_id ?? null,
    mcp_bearer_token_slot: row.mcp_bearer_token_slot ?? null,
  })
}
</script>

<template>
  <div class="grid gap-4">
    <div class="grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
      <section
        class="rounded-xl border border-default bg-default p-4 xl:col-span-2"
      >
        <div
          class="mb-3 flex items-center gap-2 text-sm font-semibold text-highlighted"
        >
          {{ t('overviewTopRankings') }}
        </div>
        <div class="grid gap-3">
          <div
            v-for="group in rankingGroups"
            :key="group.key"
            class="grid gap-2"
          >
            <div class="text-xs font-bold tracking-wide text-dimmed uppercase">
              {{ group.title }}
            </div>
            <div class="overflow-hidden rounded-lg border border-default">
              <table class="w-full text-left text-sm">
                <thead class="bg-muted text-muted">
                  <tr>
                    <th class="px-3 py-2">{{ t('overviewObject') }}</th>
                    <th class="px-3 py-2">{{ t('overviewQuality') }}</th>
                    <th class="px-3 py-2">{{ t('requests') }}</th>
                    <th class="px-3 py-2">{{ t('success') }}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="row in group.rows"
                    :key="`${group.key}:${row.label}:${row.secondary_label ?? ''}`"
                    class="cursor-pointer border-t border-default transition hover:bg-muted"
                    @click="emitRankingDrilldown(row)"
                  >
                    <td class="px-3 py-2">
                      <div class="font-medium text-highlighted">
                        {{ row.label }}
                      </div>
                      <div
                        v-if="row.secondary_label"
                        class="text-xs text-muted"
                      >
                        {{ row.secondary_label }}
                      </div>
                    </td>
                    <td class="px-3 py-2 text-highlighted">
                      {{ Math.round(row.quality_score) }}
                    </td>
                    <td class="px-3 py-2">
                      {{ formatting.formatCount(row.request_count) }}
                    </td>
                    <td class="px-3 py-2">
                      {{ formatting.formatPercent(row.success_rate) }}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </section>
    </div>

    <section
      class="rounded-xl border border-default bg-default p-4 text-sm text-muted"
    >
      <div class="mb-2 font-medium text-highlighted">
        {{ t('overviewFormula') }}
      </div>
      <div class="flex flex-wrap gap-x-4 gap-y-2">
        <span
          v-for="item in overview.quality_formula.components"
          :key="item.key"
        >
          {{ item.label }} {{ Math.round(item.weight * 100) }}%:
          {{ item.description }}
        </span>
      </div>
    </section>

    <RequestOverviewChartsPanel
      :overview="overview"
      :formatting="formatting"
      :t="t"
      @drilldown="emit('drilldown', $event)"
    />
  </div>
</template>
