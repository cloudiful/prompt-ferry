<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { TableColumn } from '@nuxt/ui'
import type { RequestRecordOverviewBreakdownRow } from '@/generated/admin-api'
import { useLocale } from '@/composables/useLocale'
import {
  splitFailedOnlyDetailEntries,
  type RequestOverviewPerspective,
} from '@/request-overview'
import type { RequestRecordFormatting } from '../models/request-record-formatting'

/**
 * Normalized hover row so the model and upstream perspectives share one table.
 * `endpoint` is only rendered in the model perspective's upstream hover.
 */
type DetailEntry = {
  key: string
  endpoint: string
  model: string
  request_count: number
  request_share: number
  total_tokens: number
  token_share: number | null
  cache_rate: number | null
  error_rate: number
  avg_output_tokens_per_second: number | null
}

const props = defineProps<{
  row: RequestRecordOverviewBreakdownRow
  perspective: RequestOverviewPerspective
  formatting: RequestRecordFormatting
}>()

const { t } = useLocale()

const isUpstream = computed(() => props.perspective === 'upstream')

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

const entries = computed<DetailEntry[]>(() => {
  if (isUpstream.value) {
    return (props.row.model_breakdown ?? []).map((entry) => ({
      key: entry.model,
      endpoint: '-',
      model: entry.model,
      request_count: entry.request_count,
      request_share: entry.request_share,
      total_tokens: entry.total_tokens,
      token_share: entry.token_share ?? null,
      cache_rate: entry.cache_rate ?? null,
      error_rate: entry.error_rate,
      avg_output_tokens_per_second: entry.avg_output_tokens_per_second ?? null,
    }))
  }
  return (props.row.upstream_breakdown ?? []).map((entry) => ({
    key: `${entry.endpoint_id ?? 'direct'}:${entry.upstream_model ?? ''}`,
    endpoint: endpointLabel(entry.endpoint_name, entry.endpoint_id),
    model: modelLabel(entry.upstream_model),
    request_count: entry.request_count,
    request_share: entry.request_share,
    total_tokens: entry.total_tokens,
    token_share: entry.token_share ?? null,
    cache_rate: entry.cache_rate ?? null,
    error_rate: entry.error_rate,
    avg_output_tokens_per_second: entry.avg_output_tokens_per_second ?? null,
  }))
})

const visible = computed(() =>
  isUpstream.value
    ? entries.value.length > 1
    : (props.row.upstream_count ?? 0) > 1,
)

// Issue #34 P3: fully failed zero-token rows are collapsed by default; the
// hidden request count stays on screen with a control that reveals them.
const revealFailedOnly = ref(false)
const split = computed(() => splitFailedOnlyDetailEntries(entries.value))
const shownEntries = computed(() =>
  revealFailedOnly.value
    ? [...split.value.visible, ...split.value.failedOnly]
    : split.value.visible,
)

watch(
  () => props.row,
  () => {
    revealFailedOnly.value = false
  },
)

const columns = computed<TableColumn<DetailEntry>[]>(() => {
  const headers: TableColumn<DetailEntry>[] = []
  if (!isUpstream.value) {
    headers.push({
      accessorKey: 'endpoint',
      header: t('overviewUpstreamEndpoint'),
    })
  }
  headers.push(
    { accessorKey: 'model', header: t('model') },
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
  )
  return headers
})
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
      :aria-label="
        isUpstream
          ? t('overviewModelBreakdown')
          : t('overviewUpstreamBreakdown')
      "
      @click.stop
    />
    <template #content>
      <div
        class="max-h-[50vh] w-[min(38rem,calc(100vw-2rem))] overflow-auto p-3"
      >
        <div class="mb-2 text-xs font-semibold text-highlighted">
          {{
            isUpstream
              ? t('overviewModelBreakdown')
              : t('overviewUpstreamBreakdown')
          }}
        </div>
        <div
          v-if="split.failedOnly.length"
          class="mb-2 flex items-center justify-between gap-2 text-xs text-muted"
        >
          <span>{{
            t('overviewFailedOnlyHidden', {
              count: formatting.formatCount(split.failedRequestCount),
            })
          }}</span>
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            :label="
              revealFailedOnly
                ? t('overviewFailedOnlyHide')
                : t('overviewFailedOnlyReveal')
            "
            @click.stop="revealFailedOnly = !revealFailedOnly"
          />
        </div>
        <UTable
          v-if="shownEntries.length"
          :data="shownEntries"
          :columns="columns"
          class="w-full"
          :ui="{
            th: 'whitespace-nowrap px-2 py-1 text-xs',
            td: 'px-2 py-1 text-xs',
          }"
        >
          <template #endpoint-cell="{ row }">
            <span class="font-medium text-highlighted">{{
              row.original.endpoint
            }}</span>
          </template>
          <template #model-cell="{ row }">
            <span class="font-medium text-highlighted">{{
              row.original.model
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
        <div v-else-if="!split.failedOnly.length" class="text-xs text-dimmed">
          {{ t('overviewUpstreamEmpty') }}
        </div>
      </div>
    </template>
  </UPopover>
</template>
