<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch } from 'vue'
import type {
  CanonicalQuotaWindowKind,
  QuotaKeyObservationTrendResponse,
} from '@/generated/admin-api'
import {
  createQuotaTrendPanelOption,
  groupQuotaTrendSeries,
  quotaTrendLegendEntries,
} from '@/quota-observation-charts'
import { currentDisplayTimeZone, formatAppDateTime } from '@/format/datetime'
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
  // The axis labels are formatted when the chart draws them, so the display
  // time zone is read here to rebuild the options when the preference
  // changes.
  void currentDisplayTimeZone()
  const kinds: Record<CanonicalQuotaWindowKind, string> = {
    five_hour: props.t('tokenPlanInterval'),
    rolling: props.t('tokenPlanRolling'),
    weekly: props.t('tokenPlanWeekly'),
    monthly: props.t('tokenPlanMonthly'),
    balance: props.t('quotaTrendKindBalance'),
    unlimited: props.t('tokenPlanNoLimit'),
    custom: props.t('quotaTrendKindCustom'),
  }
  return {
    range: props.range,
    formatAxisDate: (value: string) =>
      formatAppDateTime(value, { month: 'numeric', day: 'numeric' }),
    formatAxisTime: (value: string) =>
      formatAppDateTime(value, { hour: '2-digit', minute: '2-digit' }),
    formatTooltipTime: (value: string) => formatAppDateTime(value),
    formatAmount: (value: number, unit: string) =>
      `${value.toFixed(2)}${unit ? ` ${unit}` : ''}`,
    labels: {
      percentSuffix: '%',
      unavailable: props.t('quotaTrendUnavailable'),
      kinds,
    },
  }
})

// One panel per (metric, unit) group: each group gets its own single-axis
// chart so currencies and percentages never share a scale. Panels without a
// single numeric point are dropped instead of drawing a blank axis.
const hiddenSeries = ref<Record<string, string[]>>({})

// A new trend payload means a different endpoint/key/range selection, so stale
// hidden names are dropped instead of hiding a same-named series from another
// selection.
watch(
  () => props.trend,
  () => {
    hiddenSeries.value = {}
  },
)

function hiddenFor(groupKey: string): Set<string> {
  return new Set(hiddenSeries.value[groupKey] ?? [])
}

function toggleSeries(groupKey: string, name: string): void {
  const next = hiddenFor(groupKey)
  if (next.has(name)) next.delete(name)
  else next.add(name)
  hiddenSeries.value = { ...hiddenSeries.value, [groupKey]: [...next] }
}

const panels = computed(() =>
  groupQuotaTrendSeries(props.trend?.series ?? [])
    .map((group) => {
      const hidden = hiddenFor(group.key)
      return {
        group,
        entries: quotaTrendLegendEntries(group, optionInput.value),
        hidden,
        option: createQuotaTrendPanelOption(group, {
          ...optionInput.value,
          hiddenSeries: hidden,
        }),
      }
    })
    .filter((panel) =>
      panel.group.series.some((series) =>
        series.points.some((point) => point.value != null),
      ),
    ),
)

const hasPlottableData = computed(() => panels.value.length > 0)

// Panel heading: localized meaning, not the bare unit. Amount panels still
// name their unit so CNY/USD/credits never look interchangeable.
function panelLabel(unit: string, metric: string): string {
  if (metric === 'remaining_percent') return props.t('quotaTrendRemainingQuota')
  return unit
    ? props.t('quotaTrendBalance', { unit })
    : props.t('quotaTrendKindBalance')
}
</script>

<template>
  <div class="grid min-w-0 gap-3">
    <div
      v-if="
        trend &&
        (trend.failed_observations > 0 ||
          trend.keys_truncated ||
          trend.series_truncated)
      "
      class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-dimmed"
    >
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
      <div v-else class="grid min-w-0 gap-3">
        <section
          v-for="panel in panels"
          :key="panel.group.key"
          class="grid min-w-0 gap-1 rounded-md border border-default p-2"
        >
          <h4 class="text-xs font-medium text-highlighted">
            {{ panelLabel(panel.group.unit, panel.group.metric) }}
          </h4>
          <div
            class="max-h-24 overflow-y-auto"
            role="group"
            :aria-label="t('quotaTrendLegendLabel')"
          >
            <div class="flex min-w-0 flex-wrap gap-x-1 gap-y-1">
              <button
                v-for="entry in panel.entries"
                :key="entry.name"
                type="button"
                class="inline-flex min-w-0 max-w-full items-start gap-1.5 rounded px-1.5 py-0.5 text-left text-xs hover:bg-elevated/60 focus-visible:outline-2 focus-visible:outline-primary"
                :aria-pressed="!panel.hidden.has(entry.name)"
                :class="panel.hidden.has(entry.name) ? 'text-dimmed' : ''"
                @click="toggleSeries(panel.group.key, entry.name)"
              >
                <span
                  class="mt-1 inline-block h-2 w-2 shrink-0 rounded-full"
                  :style="{ backgroundColor: entry.color }"
                  aria-hidden="true"
                />
                <span
                  class="min-w-0 whitespace-normal break-words"
                  :class="panel.hidden.has(entry.name) ? 'line-through' : ''"
                  >{{ entry.name }}</span
                >
              </button>
            </div>
          </div>
          <UsageChart :option="panel.option" />
        </section>
      </div>
    </template>
  </div>
</template>
