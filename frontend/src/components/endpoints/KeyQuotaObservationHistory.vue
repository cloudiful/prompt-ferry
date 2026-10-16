<script setup lang="ts">
import { computed, defineAsyncComponent, ref, watch } from 'vue'
import type {
  QuotaKeyObservationHistoryItem,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'
import TokenPlanQuotaWindowRow from '@/components/endpoints/TokenPlanQuotaWindowRow.vue'
import { toCanonicalQuotaRows } from '@/composables/useCanonicalQuotaWindows'
import {
  KEY_QUOTA_TREND_RANGES,
  useKeyQuotaObservationTrend,
  type KeyQuotaTrendRange,
} from '@/composables/useKeyQuotaObservationTrend'
import { useKeyQuotaObservationHistory } from '@/composables/useKeyQuotaObservationHistory'
import { formatSubscriptionQuotaDateTime } from '@/composables/subscriptionQuotaWindows'
import {
  fetchKeyQuotaObservationHistory,
  fetchKeyQuotaObservationTrend,
} from '@/stores/endpoints-api'

// Issue #911 P2: chart-first history. The chart is the default view and the
// list keeps its own database-keyset state, so a mode switch never loses the
// loaded pages or the selected range. The chart module is only imported when
// chart mode renders.
const KeyQuotaObservationChart = defineAsyncComponent(
  () => import('@/components/endpoints/KeyQuotaObservationChart.vue'),
)

const props = defineProps<{
  endpointId: string
  t: TranslateFn
}>()

const expanded = ref(false)
const mode = ref<'chart' | 'list'>('chart')
const range = ref<KeyQuotaTrendRange>('24h')
const listActive = computed(() => expanded.value && mode.value === 'list')
const chartActive = computed(() => expanded.value && mode.value === 'chart')

// Atomic request scope (issue #911 P3): endpoint and selected key change
// together as one value, so a new endpoint can never be combined with the
// previous endpoint's key in a single dispatched request. This is the only
// consumer of `props.endpointId`; the request-owning composables watch the
// derived scope instead, so their very first request for a new endpoint
// already carries a null key.
const scope = ref<{ endpointId: string; keyId: string | null }>({
  endpointId: props.endpointId,
  keyId: null,
})
const scopedEndpointId = computed(() => scope.value.endpointId)
const selectedKeyId = computed<string | null>({
  get: () => scope.value.keyId,
  set: (value) => {
    scope.value = { ...scope.value, keyId: value }
  },
})

// Key options are shared by both views and keep every key already seen, so a
// selection can never make its own row disappear from the selector. Removed
// keys stay selectable while they still have history.
const knownKeys = ref<Map<string, string>>(new Map())

// Endpoint change: reset the selected key and the known-keys map atomically
// before either view can request, and invalidate any in-flight generation so a
// late completion from the previous endpoint is ignored.
watch(
  () => props.endpointId,
  (endpointId) => {
    scope.value = { endpointId, keyId: null }
    knownKeys.value = new Map()
  },
  { flush: 'sync' },
)

const history = useKeyQuotaObservationHistory(
  scopedEndpointId,
  selectedKeyId,
  fetchKeyQuotaObservationHistory,
)
const trend = useKeyQuotaObservationTrend(
  scopedEndpointId,
  selectedKeyId,
  range,
  fetchKeyQuotaObservationTrend,
  chartActive,
)

const rangeOptions = computed(() =>
  KEY_QUOTA_TREND_RANGES.map((value) => ({
    value,
    label: props.t(
      value === '24h'
        ? 'quotaObservationRange24h'
        : value === '7d'
          ? 'quotaObservationRange7d'
          : 'quotaObservationRange30d',
    ),
  })),
)

// The list is driven manually instead of through the composable's own
// `expanded` watcher: that watcher clears on collapse, but a mere mode switch
// must keep the loaded pages and cursor intact. It loads lazily on its first
// activation and only follows key/endpoint changes while it is the live view.
watch(
  listActive,
  (active) => {
    if (active && !history.loaded.value && !history.loading.value) {
      void history.loadFirstPage()
    }
  },
  { flush: 'sync', immediate: true },
)
watch(
  [scopedEndpointId, selectedKeyId],
  () => {
    if (listActive.value) void history.loadFirstPage()
    else history.clear()
  },
  { flush: 'sync' },
)
watch(expanded, (open) => {
  if (!open) {
    history.clear()
    trend.clear()
  }
})

// Repopulate the shared key list from whichever view produced data.
watch(
  [trend.trend, history.items],
  () => {
    const next = new Map(knownKeys.value)
    for (const series of trend.trend.value?.series ?? []) {
      if (!next.has(series.key_id)) next.set(series.key_id, series.key_label)
    }
    for (const item of history.items.value) {
      if (!next.has(item.key_id)) next.set(item.key_id, item.key_label)
    }
    knownKeys.value = next
  },
  { flush: 'post' },
)

const keyOptions = computed(() => [
  { value: null as string | null, label: props.t('quotaObservationAllKeys') },
  ...[...knownKeys.value].map(([value, label]) => ({ value, label })),
])

// Window rows render at the observation's own timestamp, never against the
// current clock, so an expired-as-of-now window still shows what was
// observed rather than a current-quota claim.
function observedMs(item: QuotaKeyObservationHistoryItem): number {
  const parsed = Date.parse(item.observed_at)
  return Number.isFinite(parsed) ? parsed : Date.now()
}

function rowsFor(item: QuotaKeyObservationHistoryItem) {
  const key = {
    key_id: item.key_id,
    key_label: item.key_label,
    ok: item.ok,
    model_remains: [],
    quota_windows: item.windows,
  } as TokenPlanKeyUsage
  return toCanonicalQuotaRows(key, observedMs(item))
}

function sourceLabel(item: QuotaKeyObservationHistoryItem): string {
  switch (item.source) {
    case 'manual':
      return props.t('quotaSourceManual')
    case 'request':
      return props.t('quotaSourceRequest')
    case 'periodic':
      return props.t('quotaSourcePeriodic')
    default:
      return props.t('quotaSourceUnknown')
  }
}
</script>

<template>
  <UCollapsible
    v-model:open="expanded"
    class="rounded-md border border-default"
  >
    <template #default="{ open }">
      <UButton
        color="neutral"
        variant="ghost"
        block
        class="justify-start px-3 py-2 text-left"
        :trailing-icon="open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'"
      >
        {{ t('quotaObservationHistory') }}
      </UButton>
    </template>
    <template #content>
      <div class="grid gap-3 border-t border-default p-3">
        <div class="flex flex-wrap items-center gap-2">
          <div
            class="inline-flex gap-1"
            role="group"
            :aria-label="t('quotaObservationModeLabel')"
          >
            <UButton
              size="xs"
              color="neutral"
              :variant="mode === 'chart' ? 'solid' : 'soft'"
              :aria-pressed="mode === 'chart'"
              @click="mode = 'chart'"
            >
              {{ t('quotaObservationModeChart') }}
            </UButton>
            <UButton
              size="xs"
              color="neutral"
              :variant="mode === 'list' ? 'solid' : 'soft'"
              :aria-pressed="mode === 'list'"
              @click="mode = 'list'"
            >
              {{ t('quotaObservationModeList') }}
            </UButton>
          </div>
          <USelect
            v-model="selectedKeyId"
            class="min-w-40 flex-1"
            :items="keyOptions"
            label-key="label"
            value-key="value"
            :aria-label="t('quotaObservationKeyLabel')"
          />
          <USelect
            v-if="mode === 'chart'"
            v-model="range"
            class="w-36"
            :items="rangeOptions"
            label-key="label"
            value-key="value"
            :aria-label="t('quotaObservationRangeLabel')"
          />
        </div>

        <KeyQuotaObservationChart
          v-if="mode === 'chart'"
          :trend="trend.trend.value"
          :loading="trend.loading.value"
          :failed="trend.failed.value"
          :range="range"
          :t="t"
          @retry="trend.retry"
        />

        <template v-else>
          <div v-if="history.loading.value" class="grid gap-2">
            <UProgress animation="carousel" />
            <span class="text-muted">{{ t('quotaHistoryLoading') }}</span>
          </div>
          <template v-else>
            <p v-if="history.failed.value" class="text-warning">
              {{ t('quotaHistoryError') }}
              <UButton
                size="xs"
                color="neutral"
                variant="link"
                @click="history.retry"
              >
                {{ t('quotaHistoryRetry') }}
              </UButton>
            </p>
            <p
              v-if="
                history.loaded.value &&
                history.items.value.length === 0 &&
                !history.failed.value
              "
              class="text-dimmed"
            >
              {{
                t('quotaHistoryEmpty', { days: history.retentionDays.value })
              }}
            </p>
            <div v-if="history.items.value.length > 0" class="grid gap-2">
              <article
                v-for="item in history.items.value"
                :key="item.observation_id"
                class="grid gap-2 rounded-md border border-default p-3"
              >
                <header
                  class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
                >
                  <time class="font-medium text-highlighted">
                    {{ formatSubscriptionQuotaDateTime(item.observed_at) }}
                  </time>
                  <span class="text-dimmed">
                    {{ item.key_label }}
                  </span>
                  <UBadge
                    :label="sourceLabel(item)"
                    color="neutral"
                    variant="subtle"
                  />
                  <span v-if="item.limit_reached" class="text-warning">
                    {{ t('quotaHistoryLimitReached') }}
                  </span>
                  <span v-if="!item.ok && item.error_code" class="text-warning">
                    {{ item.error_code }}
                  </span>
                </header>
                <div v-if="rowsFor(item).length > 0" class="grid gap-2">
                  <TokenPlanQuotaWindowRow
                    v-for="row in rowsFor(item)"
                    :key="`${row.sourceId}-${row.sourceModel ?? ''}`"
                    :row="row"
                    :label="row.sourceModel ?? row.sourceId"
                    :t="t"
                    :now-ms="observedMs(item)"
                    density="compact"
                  />
                </div>
                <span v-else class="text-dimmed">
                  {{ t('quotaHistoryNoWindowDetails') }}
                </span>
              </article>
            </div>
            <UButton
              v-if="history.nextCursor.value"
              :loading="history.loadingMore.value"
              :disabled="history.loadingMore.value"
              color="neutral"
              variant="soft"
              size="sm"
              @click="history.loadMore"
            >
              {{ t('quotaHistoryLoadMore') }}
            </UButton>
          </template>
        </template>
      </div>
    </template>
  </UCollapsible>
</template>
