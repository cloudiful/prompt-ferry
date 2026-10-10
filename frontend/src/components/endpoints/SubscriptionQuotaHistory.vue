<script setup lang="ts">
import { ref, toRef } from 'vue'
import type { QuotaSnapshotHistoryItem } from '@/generated/admin-api'
import SubscriptionQuotaWindows from '@/components/endpoints/SubscriptionQuotaWindows.vue'
import { formatSubscriptionQuotaDateTime } from '@/composables/subscriptionQuotaWindows'
import { useSubscriptionQuotaHistory } from '@/composables/useSubscriptionQuotaHistory'
import { fetchQuotaSnapshotHistory } from '@/stores/endpoints-api'

const props = defineProps<{
  endpointId: string
  nowMs: number
  refreshKey: string
  t: TranslateFn
}>()

const expanded = ref(false)
const history = useSubscriptionQuotaHistory(
  toRef(props, 'endpointId'),
  fetchQuotaSnapshotHistory,
  expanded,
  toRef(props, 'refreshKey'),
)

function snapshotTimeMs(item: QuotaSnapshotHistoryItem): number {
  return Date.parse(item.observed_at)
}

function sourceLabel(item: QuotaSnapshotHistoryItem): string {
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
        {{ t('quotaSnapshotHistory') }}
      </UButton>
    </template>
    <template #content>
      <div class="grid gap-3 border-t border-default p-3">
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
              :key="item.snapshot_id"
              class="grid gap-2 rounded-md border border-default p-3"
            >
              <header
                class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
              >
                <time class="font-medium text-highlighted">
                  {{ formatSubscriptionQuotaDateTime(item.observed_at) }}
                </time>
                <span v-if="item.plan_type" class="text-dimmed">
                  {{ item.plan_type }}
                </span>
                <UBadge
                  :label="sourceLabel(item)"
                  color="neutral"
                  variant="subtle"
                />
                <span v-if="item.limit_reached" class="text-warning">
                  {{ t('quotaHistoryLimitReached') }}
                </span>
              </header>
              <SubscriptionQuotaWindows
                v-if="item.windows.length > 0"
                :now-ms="nowMs"
                :reset-base-ms="snapshotTimeMs(item)"
                :hide-metadata="true"
                :t="t"
                :windows="item.windows"
              />
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
