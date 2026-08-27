<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import type {
  RequestRecordOverviewResponse,
  RequestRecordOverviewBreakdownRow,
} from '@/generated/admin-api'
import { useLocale } from '@/composables/useLocale'
import type { RequestRecordFormatting } from '../models/request-record-formatting'
import type { RequestOverviewDrilldown } from '../request-overview'
import {
  createBreakdownOption,
  createErrorOption,
  createTrendOption,
} from '../request-overview-charts'

const UsageChart = defineAsyncComponent(() => import('./usage/UsageChart.vue'))

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
  }),
)

const breakdownOption = computed(() =>
  createBreakdownOption({
    category: props.category,
    labels: chartLabels.value,
    rows: breakdownRows.value,
    formatCount: props.formatting.formatCount,
    formatPercent: props.formatting.formatPercent,
  }),
)

const errorOption = computed(() =>
  createErrorOption({
    rows: errorRows.value,
    formatCount: props.formatting.formatCount,
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

      <div class="grid gap-3 xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        <section class="rounded-lg border border-default bg-default p-4">
          <div class="mb-2 text-sm font-semibold text-highlighted">
            {{
              category === 'ai'
                ? t('overviewModelDistribution')
                : t('overviewMcpServerDistribution')
            }}
          </div>
          <UsageChart :option="breakdownOption" />
        </section>
        <section class="rounded-lg border border-default bg-default p-4">
          <div class="mb-2 text-sm font-semibold text-highlighted">
            {{ t('overviewErrorBreakdown') }}
          </div>
          <UsageChart :option="errorOption" />
        </section>
      </div>

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
          <table class="w-full min-w-[38rem] text-left text-sm">
            <thead class="bg-muted text-muted">
              <tr>
                <th class="px-4 py-2">{{ t('overviewObject') }}</th>
                <th class="px-4 py-2">{{ t('requests') }}</th>
                <th class="px-4 py-2">{{ t('overviewRequestShare') }}</th>
                <th class="px-4 py-2">
                  {{
                    category === 'ai'
                      ? t('overviewTotalTokens')
                      : t('overviewSuccessRate')
                  }}
                </th>
                <th v-if="category === 'ai'" class="px-4 py-2">
                  {{ t('overviewTokenShare') }}
                </th>
                <th v-if="category === 'ai'" class="px-4 py-2">
                  {{ t('overviewCacheRate') }}
                </th>
                <th v-if="category === 'ai'" class="px-4 py-2">
                  {{ t('overviewAvgOutputRate') }}
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in breakdownRows"
                :key="`${row.label}:${row.model ?? row.mcp_server_id ?? ''}`"
                class="cursor-pointer border-t border-default transition hover:bg-muted"
                @click="emitBreakdownDrilldown(row)"
              >
                <td class="px-4 py-2 font-medium text-highlighted">
                  {{ row.label }}
                </td>
                <td class="px-4 py-2">
                  {{ formatting.formatCount(row.request_count) }}
                </td>
                <td class="px-4 py-2">
                  {{ formatting.formatPercent(row.request_share) }}
                </td>
                <td class="px-4 py-2">
                  {{
                    category === 'ai'
                      ? formatting.formatTokenQuantity(row.tokens.total_tokens)
                      : formatting.formatPercent(row.success_rate)
                  }}
                </td>
                <td v-if="category === 'ai'" class="px-4 py-2">
                  {{ formatting.formatPercent(row.token_share) }}
                </td>
                <td v-if="category === 'ai'" class="px-4 py-2">
                  {{ formatting.formatPercent(row.tokens.cache_rate) }}
                </td>
                <td v-if="category === 'ai'" class="px-4 py-2">
                  {{
                    formatting.formatTokensPerSecond(
                      row.avg_output_tokens_per_second,
                    )
                  }}
                </td>
              </tr>
            </tbody>
          </table>
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
