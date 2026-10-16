<script setup lang="ts">
import { computed, defineAsyncComponent } from 'vue'
import type { QuotaKeyObservationTrendResponse } from '@/generated/admin-api'
import {
  createQuotaTrendPanelOption,
  groupQuotaTrendSeries,
} from '@/quota-observation-charts'
import { formatSubscriptionQuotaDateTime } from '@/composables/subscriptionQuotaWindows'
import type { KeyQuotaTrendRange } from '@/composables/useKeyQuotaObservationTrend'
import { themeMode } from '@/theme/appTheme'

// Issue #911 P3: presentational, theme-tokened balance/remaining-quota chart.
// The ECharts bundle is only pulled in when this component mounts (chart mode),
// never on the list-only path.
const UsageChart = defineAsyncComponent(
  () => import('@/components/usage/UsageChart.vue'),
)

const props = defineProps<{
  trend: QuotaKeyObservationTrendResponse | null
  loading: boolean
  failed: boolean
  range: KeyQuotaTrendRange
  t: TranslateFn
}>()

defineEmits<{ retry: [] }>()

const optionInput = computed(() => {
  // `themeMode` is a reactive dependency so a light/dark switch rebuilds the
  // tokenized options, not just re-renders the canvases.
  void themeMode.value
  return {
    formatTime: (value: string) => formatSubscriptionQuotaDateTime(value),
    formatAmount: (value: number, unit: string) =>
      `${value.toFixed(2)}${unit ? ` ${unit}` : ''}`,
    labels: {
      percentSuffix: '%',
      unavailable: props.t('quotaTrendUnavailable'),
    },
  }
})

// One panel per (metric, unit) group: each group gets its own single-axis
// chart so currencies and percentages never share a scale. Panels without a
// single numeric point are dropped instead of drawing a blank axis.
const panels = computed(() =>
  groupQuotaTrendSeries(props.trend?.series ?? [])
    .map((group) => ({
      group,
      option: createQuotaTrendPanelOption(group, optionInput.value),
    }))
    .filter((panel) =>
      panel.group.series.some((series) =>
        series.points.some((point) => point.value != null),
      ),
    ),
)

const hasPlottableData = computed(() => panels.value.length > 0)

const bucketHint = computed(() => {
  const trend = props.trend
  if (!trend) return ''
  return props.t('quotaTrendBucketHint', {
    seconds: trend.bucket_seconds,
    count: trend.bucket_count,
  })
})

const rangeLabel = computed(() =>
  props.t(
    props.range === '24h'
      ? 'quotaObservationRange24h'
      : props.range === '7d'
        ? 'quotaObservationRange7d'
        : 'quotaObservationRange30d',
  ),
)

// Panel heading: the amount unit verbatim, or the percent metric's unit.
// Interpolated through Vue, so untrusted unit text is escaped; the `%` panel
// uses the builder's stable percent suffix.
function panelLabel(unit: string, metric: string): string {
  return metric === 'remaining_percent' ? '%' : unit
}
</script>

<template>
  <div class="grid gap-3">
    <div class="grid gap-1">
      <div
        class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-dimmed"
      >
        <span class="font-medium text-highlighted">
          {{ t('quotaTrendTitle') }}
        </span>
        <span class="text-dimmed">{{ rangeLabel }}</span>
        <span v-if="trend">{{ bucketHint }}</span>
      </div>
      <p class="text-xs text-dimmed">{{ t('quotaTrendUnitHint') }}</p>
      <div
        v-if="trend"
        class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-dimmed"
      >
        <span>
          {{ t('quotaTrendSampled', { count: trend.sampled_observations }) }}
        </span>
        <span v-if="trend.failed_observations > 0" class="text-warning">
          {{ t('quotaTrendFailed', { count: trend.failed_observations }) }}
        </span>
        <span v-if="trend.keys_truncated" class="text-warning">
          {{ t('quotaTrendKeysTruncated') }}
        </span>
        <span v-if="trend.series_truncated" class="text-warning">
          {{ t('quotaTrendSeriesTruncated') }}
        </span>
      </div>
    </div>

    <div v-if="loading" class="grid gap-2">
      <UProgress animation="carousel" />
      <span class="text-muted">{{ t('quotaTrendLoading') }}</span>
    </div>
    <template v-else>
      <p v-if="failed" class="text-warning">
        {{ t('quotaTrendError') }}
        <UButton
          size="xs"
          color="neutral"
          variant="link"
          @click="$emit('retry')"
        >
          {{ t('quotaTrendRetry') }}
        </UButton>
      </p>
      <p v-else-if="!hasPlottableData" class="text-dimmed">
        {{ t('quotaTrendEmpty') }}
      </p>
      <div v-else class="grid gap-3">
        <section
          v-for="panel in panels"
          :key="panel.group.key"
          class="grid gap-1 rounded-md border border-default p-2"
        >
          <h4 class="text-xs font-medium text-highlighted">
            {{ panelLabel(panel.group.unit, panel.group.metric) }}
          </h4>
          <UsageChart :option="panel.option" />
        </section>
      </div>
    </template>
  </div>
</template>
