<script setup lang="ts">
import { computed } from 'vue'
import type { RequestRecordOverviewBreakdownRow } from '@/generated/admin-api'
import { useLocale } from '@/composables/useLocale'
import type { RequestRecordFormatting } from '../models/request-record-formatting'

const props = defineProps<{
  row: RequestRecordOverviewBreakdownRow
  formatting: RequestRecordFormatting
}>()

const { t } = useLocale()

const visible = computed(() => (props.row.upstream_count ?? 0) > 1)
const entries = computed(() => props.row.upstream_breakdown ?? [])
const hasEntries = computed(() => entries.value.length > 0)

function endpointLabel(
  endpointName: string | null | undefined,
  endpointId: string | null | undefined,
): string {
  if (endpointName && endpointName.length > 0) return endpointName
  if (endpointId && endpointId.length > 0) return endpointId.slice(0, 8)
  return '-'
}
</script>

<template>
  <UPopover
    v-if="visible"
    mode="hover"
    :content="{
      side: 'bottom',
      align: 'start',
      sideOffset: 6,
      collisionPadding: 8,
    }"
  >
    <UButton
      type="button"
      size="xs"
      color="neutral"
      variant="ghost"
      icon="i-lucide-info"
      :aria-label="t('overviewUpstreamBreakdown')"
      @click.stop
    />
    <template #content>
      <div
        class="max-h-[50vh] w-[min(28rem,calc(100vw-2rem))] overflow-auto p-3"
      >
        <div class="mb-2 text-xs font-semibold text-highlighted">
          {{ t('overviewUpstreamBreakdown') }}
        </div>
        <table v-if="hasEntries" class="w-full text-left text-xs">
          <thead class="text-muted">
            <tr>
              <th class="px-2 py-1">{{ t('overviewUpstreamEndpoint') }}</th>
              <th class="px-2 py-1">{{ t('overviewErrorRate') }}</th>
              <th class="px-2 py-1">{{ t('overviewTotalTokens') }}</th>
              <th class="px-2 py-1">{{ t('overviewAvgOutputRate') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="entry in entries"
              :key="entry.endpoint_id ?? entry.endpoint_name ?? ''"
              class="border-t border-default"
            >
              <td class="px-2 py-1 font-medium text-highlighted">
                {{ endpointLabel(entry.endpoint_name, entry.endpoint_id) }}
              </td>
              <td class="px-2 py-1">
                {{ formatting.formatPercent(entry.error_rate) }}
              </td>
              <td class="px-2 py-1">
                {{ formatting.formatTokenQuantity(entry.total_tokens) }}
              </td>
              <td class="px-2 py-1">
                {{
                  formatting.formatTokensPerSecond(
                    entry.avg_output_tokens_per_second,
                  )
                }}
              </td>
            </tr>
          </tbody>
        </table>
        <div v-else class="text-xs text-dimmed">
          {{ t('overviewUpstreamEmpty') }}
        </div>
      </div>
    </template>
  </UPopover>
</template>
