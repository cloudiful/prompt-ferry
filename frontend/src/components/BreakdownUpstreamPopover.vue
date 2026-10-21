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

// Explicit widths keep the pinned columns aligned with their sticky offsets:
// the table is laid out with fixed columns, so the declared size and the
// rendered width are the same value.
const COLUMN_WIDTHS: Record<string, number> = {
  endpoint: 160,
  model: 200,
  request_count: 96,
  request_share: 96,
  total_tokens: 104,
  token_share: 96,
  cache_rate: 88,
  error_rate: 88,
  avg_output_tokens_per_second: 140,
}

// A pinned cell must hide the metric columns that scroll behind it, so it
// carries the popover background instead of the translucent table default.
const PINNED_BACKGROUND = 'var(--ui-bg)'

function detailColumn(
  accessorKey: string,
  header: string,
  pinned: boolean,
): TableColumn<DetailEntry> {
  const width = COLUMN_WIDTHS[accessorKey] ?? 120
  const size = `${width}px`
  const style: Record<string, Record<string, string>> = {
    th: { width: size, minWidth: size, maxWidth: size },
    td: { width: size, minWidth: size, maxWidth: size },
  }
  if (pinned) {
    style.th.backgroundColor = PINNED_BACKGROUND
    style.td.backgroundColor = PINNED_BACKGROUND
  }
  return {
    accessorKey,
    header,
    size: width,
    meta: { style },
  } as TableColumn<DetailEntry>
}

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
    headers.push(detailColumn('endpoint', t('overviewUpstreamEndpoint'), true))
  }
  headers.push(
    detailColumn('model', t('model'), true),
    detailColumn('request_count', t('requests'), false),
    detailColumn('request_share', t('overviewRequestShare'), false),
    detailColumn('total_tokens', t('overviewTotalTokens'), false),
    detailColumn('token_share', t('overviewTokenShare'), false),
    detailColumn('cache_rate', t('overviewCacheRate'), false),
    detailColumn('error_rate', t('overviewErrorRate'), false),
    detailColumn(
      'avg_output_tokens_per_second',
      t('overviewAvgOutputRate'),
      false,
    ),
  )
  return headers
})

// The identifier columns stay visible while the metric columns scroll behind
// them. The model perspective keeps the endpoint and the model; the upstream
// perspective only knows the model.
const columnPinning = computed(() => ({
  left: isUpstream.value ? ['model'] : ['endpoint', 'model'],
  right: [] as string[],
}))

// The table is wider than the popover, so the metric columns scroll.
const tableWidth = computed(() =>
  columns.value.reduce((sum, column) => sum + (column.size ?? 0), 0),
)
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
        :style="{ '--breakdown-table-width': `${tableWidth}px` }"
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
          :column-pinning="columnPinning"
          class="w-full"
          :ui="{
            base: 'table-fixed w-[var(--breakdown-table-width)]',
            th: 'whitespace-nowrap px-2 py-1 text-xs',
            td: 'px-2 py-1 text-xs',
          }"
        >
          <template #endpoint-cell="{ row }">
            <span
              class="block max-w-full truncate font-medium text-highlighted"
              >{{ row.original.endpoint }}</span
            >
          </template>
          <template #model-cell="{ row }">
            <span
              class="block max-w-full truncate font-medium text-highlighted"
              >{{ row.original.model }}</span
            >
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
