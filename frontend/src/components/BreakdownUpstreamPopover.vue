<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
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

// The rendered widths, the TanStack size and the table sum are the same
// value: the table is laid out with fixed columns, so the declared size and
// the rendered width must agree for the sticky offsets to stay aligned. The
// metric columns are as wide as their formatted value needs, and the
// identifier columns follow their content.
const METRIC_COLUMN_WIDTHS: Record<string, number> = {
  request_count: 88,
  request_share: 80,
  total_tokens: 96,
  token_share: 80,
  cache_rate: 80,
  error_rate: 80,
  avg_output_tokens_per_second: 112,
}

// The identifier columns are content aware: the width follows the longest
// displayed identifier and the header, so a short name does not waste space
// and a long one is bounded instead of pushing the metrics out of the
// popover. A truncated identifier keeps its full value in the title.
const IDENTIFIER_MIN_WIDTH = 96
const IDENTIFIER_MAX_WIDTH = 220
const IDENTIFIER_CHAR_WIDTH = 6.5
const IDENTIFIER_CELL_PADDING = 16

// The popover padding on both sides; the popover is as wide as its table.
const POPOVER_PADDING = 24

// Below this width the popover cannot show two identifiers and the metrics
// at the same time, so the pinned set shrinks to one identifier and the
// metrics stay reachable.
const NARROW_VIEWPORT = '(max-width: 640px)'

// A pinned cell must hide the metric columns that scroll behind it, so it
// carries the popover background instead of the translucent table default.
const PINNED_BACKGROUND = 'var(--ui-bg)'

function textWidth(value: string): number {
  let width = IDENTIFIER_CELL_PADDING
  for (const character of value) {
    // A wide character takes about twice the space of a latin one.
    width +=
      character.codePointAt(0)! > 0x2e80
        ? IDENTIFIER_CHAR_WIDTH * 2
        : IDENTIFIER_CHAR_WIDTH
  }
  return Math.round(width)
}

function boundedIdentifierWidth(values: string[]): number {
  const width = values.reduce(
    (widest, value) => Math.max(widest, textWidth(value)),
    0,
  )
  return Math.min(Math.max(width, IDENTIFIER_MIN_WIDTH), IDENTIFIER_MAX_WIDTH)
}

function detailColumn(
  accessorKey: string,
  header: string,
  pinned: boolean,
  width: number,
): TableColumn<DetailEntry> {
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

// The identifier widths follow the identifiers this table shows, so the
// metric columns keep the space they need. The hidden failed-only rows are
// part of the set, so a revealed row never overflows its column.
const identifierWidths = computed(() => ({
  endpoint: boundedIdentifierWidth([
    t('overviewUpstreamEndpoint'),
    ...entries.value.map((entry) => entry.endpoint),
  ]),
  model: boundedIdentifierWidth([
    t('model'),
    ...entries.value.map((entry) => entry.model),
  ]),
}))

const columns = computed<TableColumn<DetailEntry>[]>(() => {
  const headers: TableColumn<DetailEntry>[] = []
  if (!isUpstream.value) {
    headers.push(
      detailColumn(
        'endpoint',
        t('overviewUpstreamEndpoint'),
        true,
        identifierWidths.value.endpoint,
      ),
    )
  }
  headers.push(
    detailColumn('model', t('model'), true, identifierWidths.value.model),
    detailColumn(
      'request_count',
      t('requests'),
      false,
      METRIC_COLUMN_WIDTHS.request_count,
    ),
    detailColumn(
      'request_share',
      t('overviewRequestShare'),
      false,
      METRIC_COLUMN_WIDTHS.request_share,
    ),
    detailColumn(
      'total_tokens',
      t('overviewTotalTokens'),
      false,
      METRIC_COLUMN_WIDTHS.total_tokens,
    ),
    detailColumn(
      'token_share',
      t('overviewTokenShare'),
      false,
      METRIC_COLUMN_WIDTHS.token_share,
    ),
    detailColumn(
      'cache_rate',
      t('overviewCacheRate'),
      false,
      METRIC_COLUMN_WIDTHS.cache_rate,
    ),
    detailColumn(
      'error_rate',
      t('overviewErrorRate'),
      false,
      METRIC_COLUMN_WIDTHS.error_rate,
    ),
    detailColumn(
      'avg_output_tokens_per_second',
      t('overviewAvgOutputRate'),
      false,
      METRIC_COLUMN_WIDTHS.avg_output_tokens_per_second,
    ),
  )
  return headers
})

// The identifier columns stay visible while the metric columns scroll behind
// them. The model perspective keeps the endpoint and the model; the upstream
// perspective only knows the model. A narrow viewport keeps one identifier
// pinned, so the metrics stay readable.
function narrowViewport(): boolean {
  return (
    typeof globalThis.matchMedia === 'function' &&
    globalThis.matchMedia(NARROW_VIEWPORT).matches
  )
}

const isNarrow = ref(narrowViewport())
let narrowQuery: MediaQueryList | undefined

function onNarrowChange(event: MediaQueryListEvent): void {
  isNarrow.value = event.matches
}

onMounted(() => {
  if (typeof globalThis.matchMedia !== 'function') {
    return
  }
  narrowQuery = globalThis.matchMedia(NARROW_VIEWPORT)
  isNarrow.value = narrowQuery.matches
  narrowQuery.addEventListener('change', onNarrowChange)
})

onBeforeUnmount(() => {
  narrowQuery?.removeEventListener('change', onNarrowChange)
})

const columnPinning = computed(() => ({
  left: isUpstream.value || isNarrow.value ? ['model'] : ['endpoint', 'model'],
  right: [] as string[],
}))

// The table is wider than the popover on a narrow viewport, so the metric
// columns scroll. The popover itself follows the table and never grows past
// the viewport.
const tableWidth = computed(() =>
  columns.value.reduce((sum, column) => sum + (column.size ?? 0), 0),
)

const popoverWidth = computed(
  () => `min(${tableWidth.value + POPOVER_PADDING}px, calc(100vw - 2rem))`,
)
</script>

<template>
  <UPopover
    v-if="visible"
    mode="hover"
    :enable-touch="true"
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
        class="max-h-[50vh] overflow-auto p-3"
        :style="{
          '--breakdown-table-width': `${tableWidth}px`,
          width: popoverWidth,
        }"
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
              :title="row.original.endpoint"
              >{{ row.original.endpoint }}</span
            >
          </template>
          <template #model-cell="{ row }">
            <span
              class="block max-w-full truncate font-medium text-highlighted"
              :title="row.original.model"
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
