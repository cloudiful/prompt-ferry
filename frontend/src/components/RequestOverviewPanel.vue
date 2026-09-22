<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import type { TableColumn, TableRow } from '@nuxt/ui'
import type {
  RequestRecordOverviewResponse,
  RequestRecordOverviewBreakdownRow,
} from '@/generated/admin-api'
import { useLocale } from '@/composables/useLocale'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type { RequestOverviewDrilldown } from '../request-overview'
import {
  createErrorOption,
  createTrendOption,
} from '../request-overview-charts'

const UsageChart = defineAsyncComponent(() => import('./usage/UsageChart.vue'))
const BreakdownUpstreamPopover = defineAsyncComponent(
  () => import('./BreakdownUpstreamPopover.vue'),
)

const props = defineProps<{
  overview: RequestRecordOverviewResponse | null
  loading: boolean
  category: 'ai' | 'mcp'
  t: TranslateFn
  formatting: RequestRecordFormatting
}>()

const emit = defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()

const { t } = useLocale()
const hasTraffic = computed(
  () => (props.overview?.summary.request_count ?? 0) > 0,
)
const breakdownRows = computed(() => props.overview?.breakdown ?? [])
const errorRows = computed(() => props.overview?.error_breakdown ?? [])

const chartLabels = computed(() => ({
  cacheRead: t('overviewCacheRead'),
  cacheRate: t('overviewCacheRate'),
  cacheWrite: t('overviewCacheWrite'),
  error: t('overviewError'),
  input: t('overviewInputTokens'),
  output: t('overviewOutputTokens'),
  requests: t('requests'),
  success: t('overviewSuccess'),
}))

const metricCards = computed(() => {
  const summary = props.overview?.summary
  if (!summary) return []
  const common = [
    metric(t('requests'), summary.request_count, 'count'),
    metric(t('overviewSuccessRate'), summary.success_rate, 'ratio'),
    metric(t('overviewErrors'), summary.error_count, 'count'),
    metric(t('overviewP95Latency'), summary.p95_total_ms, 'ms'),
  ]
  if (props.category === 'mcp') {
    return [
      ...common,
      metric(t('overviewMcpMethods'), summary.method_count, 'count'),
    ]
  }
  return [
    metric(t('overviewTotalTokens'), summary.tokens.total_tokens, 'tokens'),
    metric(t('overviewInputTokens'), summary.tokens.input_tokens, 'tokens'),
    metric(t('overviewOutputTokens'), summary.tokens.output_tokens, 'tokens'),
    metric(t('overviewCacheRate'), summary.tokens.cache_rate, 'ratio'),
    metric(t('overviewCacheHitRate'), summary.tokens.cache_hit_rate, 'ratio'),
    metric(
      t('overviewAvgOutputRate'),
      summary.avg_output_tokens_per_second,
      'tokensPerSecond',
    ),
    ...common,
  ]
})

const trendOption = computed(() =>
  createTrendOption({
    category: props.category,
    labels: chartLabels.value,
    trend: props.overview?.trend ?? [],
    formatTime: formatBucket,
    formatCompact: props.formatting.formatTokenQuantity,
  }),
)

const errorOption = computed(() =>
  createErrorOption({
    rows: errorRows.value,
    formatCompact: props.formatting.formatTokenQuantity,
    formatPercent: props.formatting.formatPercent,
  }),
)

function metric(
  label: string,
  value: number | null | undefined,
  kind: 'count' | 'ms' | 'ratio' | 'tokens' | 'tokensPerSecond',
) {
  return { label, value, kind }
}

function formatMetricValue(
  value: number | null | undefined,
  kind: 'count' | 'ms' | 'ratio' | 'tokens' | 'tokensPerSecond',
): string {
  if (kind === 'ratio') return props.formatting.formatPercent(value)
  if (kind === 'ms') return props.formatting.formatMs(value)
  if (kind === 'tokens') return props.formatting.formatTokenQuantity(value)
  if (kind === 'tokensPerSecond')
    return props.formatting.formatTokensPerSecond(value)
  return props.formatting.formatCount(value)
}

function formatBucket(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    month: '2-digit',
  }).format(new Date(value))
}

function emitBreakdownDrilldown(row: RequestRecordOverviewBreakdownRow): void {
  emit('drilldown', {
    model: props.category === 'ai' ? row.model : null,
    mcp_server_id: props.category === 'mcp' ? row.mcp_server_id : null,
  })
}

function onBreakdownSelect(
  _event: Event,
  row: TableRow<RequestRecordOverviewBreakdownRow>,
): void {
  emitBreakdownDrilldown(row.original)
}

const aiColumns = computed<TableColumn<RequestRecordOverviewBreakdownRow>[]>(
  () => [
    { accessorKey: 'label', header: t('overviewObject') },
    { accessorKey: 'request_count', header: t('requests') },
    { accessorKey: 'request_share', header: t('overviewRequestShare') },
    { id: 'tokens', header: t('overviewTotalTokens') },
    { accessorKey: 'token_share', header: t('overviewTokenShare') },
    { id: 'cache_rate', header: t('overviewCacheRate') },
    { accessorKey: 'error_rate', header: t('overviewErrorRate') },
    {
      accessorKey: 'avg_output_tokens_per_second',
      header: t('overviewAvgOutputRate'),
    },
  ],
)

const mcpColumns = computed<TableColumn<RequestRecordOverviewBreakdownRow>[]>(
  () => [
    { accessorKey: 'label', header: t('overviewObject') },
    { accessorKey: 'request_count', header: t('requests') },
    { accessorKey: 'request_share', header: t('overviewRequestShare') },
    { accessorKey: 'success_rate', header: t('overviewSuccessRate') },
  ],
)

function providerBadge(row: RequestRecordOverviewBreakdownRow): string {
  const provider = row.server_provider_kind ?? 'generic'
  return row.usage_unit ? `${provider} · ${row.usage_unit}` : provider
}
</script>

<template>
  <div class="grid gap-3">
    <div v-if="overview && hasTraffic" class="grid gap-3">
      <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4 xl:grid-cols-6">
        <article
          v-for="card in metricCards"
          :key="card.label"
          class="grid gap-1 rounded-lg border border-default bg-default px-3 py-2.5"
        >
          <div
            class="text-[0.68rem] font-bold tracking-wide text-dimmed uppercase"
          >
            {{ card.label }}
          </div>
          <div class="text-base leading-none font-bold text-highlighted">
            {{ formatMetricValue(card.value, card.kind) }}
          </div>
        </article>
      </div>

      <section class="rounded-lg border border-default bg-default p-4">
        <div class="mb-2 text-sm font-semibold text-highlighted">
          {{ t('overviewTrend') }}
        </div>
        <UsageChart :option="trendOption" />
      </section>

      <section class="rounded-lg border border-default bg-default p-4">
        <div class="mb-2 text-sm font-semibold text-highlighted">
          {{ t('overviewErrorBreakdown') }}
        </div>
        <UsageChart :option="errorOption" />
      </section>

      <section
        class="overflow-hidden rounded-lg border border-default bg-default"
      >
        <div
          class="border-b border-default px-4 py-3 text-sm font-semibold text-highlighted"
        >
          {{
            category === 'ai'
              ? t('overviewModelDistribution')
              : t('overviewMcpServerDistribution')
          }}
        </div>
        <div class="overflow-x-auto">
          <UTable
            :data="breakdownRows"
            :columns="category === 'ai' ? aiColumns : mcpColumns"
            class="min-w-[38rem]"
            :ui="{
              thead: 'bg-muted',
              th: 'whitespace-nowrap px-4 py-2 text-muted',
              td: 'px-4 py-2 text-default',
              tr: 'cursor-pointer',
            }"
            @select="onBreakdownSelect"
          >
            <template #empty>-</template>
            <template #label-cell="{ row }">
              <span class="inline-flex items-center gap-1">
                <span class="font-medium text-highlighted">{{
                  row.original.label
                }}</span>
                <BreakdownUpstreamPopover
                  v-if="category === 'ai'"
                  :row="row.original"
                  :formatting="formatting"
                />
                <UBadge
                  v-if="category === 'mcp'"
                  :label="providerBadge(row.original)"
                  color="neutral"
                  size="sm"
                />
              </span>
            </template>
            <template #request_count-cell="{ row }">{{
              formatting.formatCount(row.original.request_count)
            }}</template>
            <template #request_share-cell="{ row }">{{
              formatting.formatPercent(row.original.request_share)
            }}</template>
            <template #tokens-cell="{ row }">{{
              formatting.formatTokenQuantity(row.original.tokens.total_tokens)
            }}</template>
            <template #token_share-cell="{ row }">{{
              formatting.formatPercent(row.original.token_share)
            }}</template>
            <template #cache_rate-cell="{ row }">{{
              formatting.formatPercent(row.original.tokens.cache_rate)
            }}</template>
            <template #error_rate-cell="{ row }">{{
              formatting.formatPercent(row.original.error_rate)
            }}</template>
            <template #avg_output_tokens_per_second-cell="{ row }">{{
              formatting.formatTokensPerSecond(
                row.original.avg_output_tokens_per_second,
              )
            }}</template>
            <template #success_rate-cell="{ row }">{{
              formatting.formatPercent(row.original.success_rate)
            }}</template>
          </UTable>
        </div>
      </section>
    </div>

    <div
      v-else
      class="rounded-lg border border-default bg-default px-4 py-8 text-sm text-dimmed"
    >
      {{
        loading
          ? t('loading')
          : category === 'ai'
            ? t('noAiRequestRecords')
            : t('noMcpCallRecords')
      }}
    </div>
  </div>
</template>
