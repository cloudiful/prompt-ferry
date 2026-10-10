<script setup lang="ts">
import { computed } from 'vue'
import type {
  SubscriptionQuotaObservation,
  SubscriptionWindowUsage,
} from '@/generated/admin-api'
import {
  formatSubscriptionQuotaDateTime,
  subscriptionWindowLabel,
  subscriptionWindowProgressColor,
  subscriptionWindowRemainingPercent,
  subscriptionWindowResetMs,
} from '@/composables/subscriptionQuotaWindows'
import { formatResetDuration } from '@/composables/useTokenPlanWindowEntries'

const props = defineProps<{
  nowMs: number
  observation?: SubscriptionQuotaObservation | null
  resetBaseMs?: number
  hideMetadata?: boolean
  t: TranslateFn
  windows: SubscriptionWindowUsage[]
}>()

const observationTime = computed(() => {
  const value = props.observation?.observed_at
  return value ? formatSubscriptionQuotaDateTime(value) : null
})
const retryTime = computed(() => {
  const value = props.observation?.next_retry_at
  return value ? formatSubscriptionQuotaDateTime(value) : null
})
const relativeResetBaseMs = computed(() => {
  if (props.resetBaseMs !== undefined) return props.resetBaseMs
  const observedAt = props.observation?.observed_at
  if (!observedAt) return undefined
  const timestamp = Date.parse(observedAt)
  return Number.isFinite(timestamp) ? timestamp : undefined
})
const sourceLabel = computed(() => {
  switch (props.observation?.source) {
    case 'manual':
      return props.t('quotaSourceManual')
    case 'request':
      return props.t('quotaSourceRequest')
    case 'periodic':
      return props.t('quotaSourcePeriodic')
    default:
      return null
  }
})

const rows = computed(() =>
  props.windows.map((window, index) => {
    const remaining = subscriptionWindowRemainingPercent(window)
    return {
      key: `${window.source_window}-${window.window_seconds ?? 'unknown'}-${index}`,
      label: subscriptionWindowLabel(window, props.t),
      remaining,
      reset: formatResetDuration(
        subscriptionWindowResetMs(
          window,
          props.nowMs,
          relativeResetBaseMs.value,
        ),
        props.t,
      ),
      color:
        remaining === null
          ? undefined
          : subscriptionWindowProgressColor(remaining),
    }
  }),
)
</script>

<template>
  <div
    v-if="!hideMetadata && observation"
    class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
  >
    <span v-if="observationTime" class="text-dimmed">
      {{ t('quotaObservedAt', { time: observationTime }) }}
    </span>
    <span v-if="sourceLabel" class="text-dimmed">{{ sourceLabel }}</span>
    <UBadge
      v-if="observation.refreshing"
      :label="t('quotaRefreshing')"
      color="neutral"
      variant="subtle"
    />
    <UBadge
      v-if="observation.stale"
      :label="t('quotaCachedObservation')"
      color="warning"
      variant="subtle"
    />
    <span v-if="observation.last_error_code" class="text-warning">
      {{ t('quotaRefreshFailedCached') }}
    </span>
    <span v-if="retryTime" class="text-dimmed">
      {{ t('quotaRetryAt', { time: retryTime }) }}
    </span>
  </div>
  <div v-if="rows.length > 0" class="grid gap-1.5">
    <div
      v-for="row in rows"
      :key="row.key"
      class="grid gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3"
    >
      <span class="text-dimmed">{{ row.label }}</span>
      <UProgress
        v-if="row.remaining !== null"
        class="token-plan-progress h-1.5"
        :model-value="100 - row.remaining"
        :style="{ '--token-plan-progress-color': row.color }"
      />
      <span v-else class="text-dimmed">{{ t('tokenPlanUsageUnknown') }}</span>
      <div
        class="flex items-center justify-between gap-2 text-xs sm:min-w-[8.5rem] sm:justify-end"
      >
        <span class="text-dimmed">{{ row.reset }}</span>
        <span v-if="row.remaining !== null" class="shrink-0 font-semibold">
          {{ row.remaining.toFixed(1) }}%
        </span>
        <span v-else class="shrink-0 font-semibold">-</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.token-plan-progress :deep([data-slot='indicator']) {
  background-color: var(--token-plan-progress-color);
}
</style>
