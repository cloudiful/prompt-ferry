<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import type {
  QuotaKeyObservationHistoryItem,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'
import TokenPlanQuotaWindowRow from '@/components/endpoints/TokenPlanQuotaWindowRow.vue'
import { toCanonicalQuotaRows } from '@/composables/useCanonicalQuotaWindows'
import { useKeyQuotaObservationHistory } from '@/composables/useKeyQuotaObservationHistory'
import { formatSubscriptionQuotaDateTime } from '@/composables/subscriptionQuotaWindows'
import { fetchKeyQuotaObservationHistory } from '@/stores/endpoints-api'

const props = defineProps<{
  endpointId: string
  t: TranslateFn
}>()

const expanded = ref(false)
const selectedKeyId = ref<string | null>(null)
const history = useKeyQuotaObservationHistory(
  toRef(props, 'endpointId'),
  selectedKeyId,
  fetchKeyQuotaObservationHistory,
  expanded,
)

// Key options come from the rows already seen (current config plus any
// historical key labels), never from the live key configuration alone, so
// removed keys stay selectable while they have history.
const keyOptions = computed(() => {
  const seen = new Map<string, string>()
  for (const item of history.items.value) {
    if (!seen.has(item.key_id)) seen.set(item.key_id, item.key_label)
  }
  return [
    { value: null as string | null, label: props.t('quotaObservationAllKeys') },
    ...[...seen].map(([value, label]) => ({ value, label })),
  ]
})

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
        <USelect
          v-model="selectedKeyId"
          class="w-full"
          :items="keyOptions"
          label-key="label"
          value-key="value"
        />
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
            {{ t('quotaHistoryEmpty', { days: history.retentionDays.value }) }}
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
      </div>
    </template>
  </UCollapsible>
</template>
