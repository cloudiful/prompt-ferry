<script setup lang="ts">
import { computed } from 'vue'
import type { TableColumn } from '@nuxt/ui'
import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewUpstreamBreakdown,
} from '@/generated/admin-api'
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

const upstreamColumns = computed<
  TableColumn<RequestRecordOverviewUpstreamBreakdown>[]
>(() => [
  { accessorKey: 'endpoint_name', header: t('overviewUpstreamEndpoint') },
  { id: 'model', header: t('model') },
  { accessorKey: 'request_count', header: t('requests') },
  { accessorKey: 'request_share', header: t('overviewRequestShare') },
  { accessorKey: 'total_tokens', header: t('overviewTotalTokens') },
  { accessorKey: 'token_share', header: t('overviewTokenShare') },
  { accessorKey: 'cache_rate', header: t('overviewCacheRate') },
  { accessorKey: 'error_rate', header: t('overviewErrorRate') },
  {
    accessorKey: 'avg_output_tokens_per_second',
    header: t('overviewAvgOutputRate'),
  },
])

function endpointLabel(
  endpointName: string | null | undefined,
  endpointId: string | null | undefined,
): string {
  if (endpointName && endpointName.length > 0) return endpointName
  if (endpointId && endpointId.length > 0) return endpointId.slice(0, 8)
  return '-'
}

function modelLabel(upstreamModel: string | null | undefined): string {
  if (upstreamModel && upstreamModel.length > 0) return upstreamModel
  return props.row.label
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
        class="max-h-[50vh] w-[min(38rem,calc(100vw-2rem))] overflow-auto p-3"
      >
        <div class="mb-2 text-xs font-semibold text-highlighted">
          {{ t('overviewUpstreamBreakdown') }}
        </div>
        <UTable
          v-if="hasEntries"
          :data="entries"
          :columns="upstreamColumns"
          class="w-full"
          :ui="{
            th: 'whitespace-nowrap px-2 py-1 text-xs',
            td: 'px-2 py-1 text-xs',
          }"
        >
          <template #endpoint_name-cell="{ row }">
            <span class="font-medium text-highlighted">{{
              endpointLabel(
                row.original.endpoint_name,
                row.original.endpoint_id,
              )
            }}</span>
          </template>
          <template #model-cell="{ row }">
            <span class="font-medium text-highlighted">{{
              modelLabel(row.original.upstream_model)
            }}</span>
          </template>
          <template #request_count-cell="{ row }">{{
            formatting.formatCount(row.original.request_count)
          }}</template>
          <template #request_share-cell="{ row }">{{
            formatting.formatPercent(row.original.request_share)
          }}</template>
          <template #total_tokens-cell="{ row }">{{
            formatting.formatTokenQuantity(row.original.total_tokens)
          }}</template>
          <template #token_share-cell="{ row }">{{
            formatting.formatPercent(row.original.token_share)
          }}</template>
          <template #cache_rate-cell="{ row }">{{
            formatting.formatPercent(row.original.cache_rate)
          }}</template>
          <template #error_rate-cell="{ row }">{{
            formatting.formatPercent(row.original.error_rate)
          }}</template>
          <template #avg_output_tokens_per_second-cell="{ row }">{{
            formatting.formatTokensPerSecond(
              row.original.avg_output_tokens_per_second,
            )
          }}</template>
        </UTable>
        <div v-else class="text-xs text-dimmed">
          {{ t('overviewUpstreamEmpty') }}
        </div>
      </div>
    </template>
  </UPopover>
</template>
