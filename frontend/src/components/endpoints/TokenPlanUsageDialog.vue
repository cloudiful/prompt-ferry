<script setup lang="ts">
import { computed } from 'vue'
import type {
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanModelUsage,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import SubscriptionQuotaHistory from '@/components/endpoints/SubscriptionQuotaHistory.vue'
import KeyQuotaObservationHistory from '@/components/endpoints/KeyQuotaObservationHistory.vue'
import TokenPlanKeyQuotaStatus from '@/components/endpoints/TokenPlanKeyQuotaStatus.vue'
import TokenPlanQuotaWindowRow from '@/components/endpoints/TokenPlanQuotaWindowRow.vue'
import { deepseekBalanceRows } from '@/components/endpoints/tokenPlanBadgeRender'
import {
  toCanonicalQuotaRows,
  type CanonicalQuotaRow,
} from '@/composables/useCanonicalQuotaWindows'
import {
  useTokenPlanTicker,
  useTokenPlanWindowEntries,
} from '@/composables/useTokenPlanWindowEntries'
import {
  formatSubscriptionQuotaDateTime,
  subscriptionWindowLabel,
} from '@/composables/subscriptionQuotaWindows'

const props = defineProps<{
  endpointId: string
  endpointName: string
  loading: boolean
  provider: EndpointProvider | null
  t: TranslateFn
  usage: TokenPlanUsageResponse | null
}>()

const visible = defineModel<boolean>('visible', { required: true })

const nowMs = useTokenPlanTicker(visible)

const {
  keyWindowCount,
  minimumRemainingPercent,
  openrouterEntries,
  formatOpenRouterCredits,
} = useTokenPlanWindowEntries(props.t, nowMs)

function isQuotaRow(row: CanonicalQuotaRow): boolean {
  return (
    row.kind === 'five_hour' ||
    row.kind === 'rolling' ||
    row.kind === 'weekly' ||
    row.kind === 'monthly' ||
    row.kind === 'custom'
  )
}

// Provider slots (no source model): CommandCode USD, opencodeGo, GLM.
function quotaRows(key: TokenPlanKeyUsage): CanonicalQuotaRow[] {
  return toCanonicalQuotaRows(key, nowMs.value).filter(
    (row) => row.sourceModel === null && isQuotaRow(row),
  )
}

type ModelQuotaGroup = {
  modelName: string
  rows: CanonicalQuotaRow[]
  observation: TokenPlanModelUsage['observation']
}

// Canonical model/detail rows grouped by source model, preserving payload
// order. Detail rows (is_default_scope false) stay visible here but never
// bind the pool minimum, which comes from default-scope rows only.
// Observation-only models (assessed empty with stale metadata) keep their
// metadata group even when they contribute no rows.
function modelGroups(key: TokenPlanKeyUsage): ModelQuotaGroup[] {
  const rowsByModel = new Map<string, CanonicalQuotaRow[]>()
  for (const row of toCanonicalQuotaRows(key, nowMs.value)) {
    if (row.sourceModel === null || !isQuotaRow(row)) continue
    const list = rowsByModel.get(row.sourceModel)
    if (list) list.push(row)
    else rowsByModel.set(row.sourceModel, [row])
  }
  const groups: ModelQuotaGroup[] = []
  for (const model of key.model_remains) {
    const rows = rowsByModel.get(model.model_name) ?? []
    rowsByModel.delete(model.model_name)
    if (rows.length === 0 && model.observation == null) continue
    groups.push({
      modelName: model.model_name,
      rows,
      observation: model.observation ?? null,
    })
  }
  for (const [modelName, rows] of rowsByModel) {
    groups.push({
      modelName,
      rows,
      observation: null,
    })
  }
  return groups
}

function quotaRowLabelKey(row: CanonicalQuotaRow): string {
  if (row.sourceId.startsWith('command_code:')) {
    return row.kind === 'weekly' ? 'tokenPlanWeeklyUsd' : 'tokenPlanFiveHour'
  }
  if (row.sourceId.startsWith('glm:')) {
    return row.kind === 'weekly' ? 'tokenPlanWeekly' : 'tokenPlanInterval'
  }
  if (row.sourceId.startsWith('opencode_go:')) {
    if (row.kind === 'rolling') return 'tokenPlanRolling'
    if (row.kind === 'weekly') return 'tokenPlanWeekly'
    return 'tokenPlanMonthly'
  }
  return row.kind === 'weekly' || row.kind === 'monthly'
    ? 'tokenPlanWeekly'
    : 'tokenPlanInterval'
}

// Model/detail rows use their actual duration: subscription custom windows
// go through the existing duration helper, MiniMax count slots reuse the
// interval/weekly copy. The row only receives the translated string.
function modelRowLabel(row: CanonicalQuotaRow): string {
  if (row.kind === 'custom') {
    return subscriptionWindowLabel(
      {
        source_window: row.sourceId,
        window_seconds: row.durationSeconds,
        availability: row.availability,
        remaining_percent: row.remainingPercent,
        reset_at: row.resetAt,
      },
      props.t,
    )
  }
  return props.t(quotaRowLabelKey(row))
}

function quotaRowQuantitySuffix(row: CanonicalQuotaRow): string {
  // The USD caption the CommandCode section has always shown; every other
  // slot renders the bare `used / total` caption.
  return row.unit === 'currency' ? ' USD' : ''
}

function minimumRemainingLabel(key: TokenPlanKeyUsage): string {
  // Canonical-only pool minimum: default-scope rows decide, detail rows
  // never leak in (an authoritative empty or all-expired stays unknown).
  const minimum = minimumRemainingPercent(key)
  return minimum === null
    ? props.t('tokenPlanMinRemainingUnknown')
    : props.t('tokenPlanMinRemaining', { percent: minimum.toFixed(1) })
}

function observationTime(
  observation: TokenPlanModelUsage['observation'],
): string | null {
  const value = observation?.observed_at
  return value ? formatSubscriptionQuotaDateTime(value) : null
}

function observationSourceLabel(
  observation: TokenPlanModelUsage['observation'],
): string | null {
  switch (observation?.source) {
    case 'manual':
      return props.t('quotaSourceManual')
    case 'request':
      return props.t('quotaSourceRequest')
    case 'periodic':
      return props.t('quotaSourcePeriodic')
    default:
      return null
  }
}

function observationRetryTime(
  observation: TokenPlanModelUsage['observation'],
): string | null {
  const value = observation?.next_retry_at
  return value ? formatSubscriptionQuotaDateTime(value) : null
}

const historyRefreshKey = computed(() => {
  const observation = props.usage?.keys
    .flatMap((key) => key.model_remains)
    .find((model) => model.observation)?.observation
  if (!observation) return ''
  return [
    observation.observed_at ?? '',
    observation.last_error_code ?? '',
    observation.source ?? '',
  ].join('|')
})
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="`${t('tokenPlanUsage')} / ${endpointName}`"
    :ui="{ content: 'w-[calc(100vw-2rem)] sm:max-w-6xl' }"
  >
    <template #body>
      <div
        class="grid max-h-[min(70vh,42rem)] gap-4 overflow-y-auto pr-1 text-xs"
      >
        <div v-if="loading" class="grid gap-2">
          <UProgress animation="carousel" />
          <span class="text-muted">{{ t('loading') }}</span>
        </div>

        <template v-else-if="usage">
          <div
            v-for="key in usage.keys"
            :key="key.key_id"
            class="border-b border-default last:border-b-0"
          >
            <UCollapsible :default-open="true">
              <template #default="{ open }">
                <UButton
                  color="neutral"
                  variant="ghost"
                  block
                  class="justify-start px-1 py-2 text-left"
                  :trailing-icon="
                    open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
                  "
                >
                  <span
                    class="flex min-w-0 flex-1 items-center justify-between gap-3"
                  >
                    <span class="flex min-w-0 items-center gap-2">
                      <span class="truncate font-semibold text-highlighted">{{
                        key.key_label
                      }}</span>
                      <TokenPlanKeyQuotaStatus
                        :status="key.quota_status ?? null"
                        :t="t"
                      />
                      <UBadge
                        v-if="key.openrouter_balance?.is_free_tier"
                        :label="t('tokenPlanFreeTier')"
                        color="neutral"
                        variant="subtle"
                      />
                    </span>
                    <span
                      v-if="key.ok && keyWindowCount(key) > 0"
                      class="shrink-0 text-xs text-dimmed"
                    >
                      {{ minimumRemainingLabel(key) }}
                      ·
                      {{
                        t('tokenPlanWindowCount', {
                          count: keyWindowCount(key),
                        })
                      }}
                    </span>
                  </span>
                </UButton>
              </template>
              <template #content>
                <div class="grid gap-2 px-1 pb-3">
                  <p v-if="!key.ok" class="break-words text-error">
                    {{ key.error_message ?? t('tokenPlanUsageFailed') }}
                  </p>
                  <template v-else>
                    <div
                      v-if="key.balances"
                      class="grid gap-1 rounded-md border border-default p-3"
                    >
                      <div class="break-words font-medium text-highlighted">
                        {{ t('tokenPlanBalances') }}
                      </div>
                      <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
                        <span
                          >{{ t('tokenPlanMonthlyCredits') }}:
                          {{ key.balances.monthly_credits.toFixed(2) }}</span
                        >
                        <span
                          >{{ t('tokenPlanPurchasedCredits') }}:
                          {{ key.balances.purchased_credits.toFixed(2) }}</span
                        >
                        <span
                          >{{ t('tokenPlanFreeCredits') }}:
                          {{ key.balances.free_credits.toFixed(2) }}</span
                        >
                        <span class="font-semibold"
                          >{{ t('tokenPlanRemainingCredits') }}:
                          {{ key.balances.remaining_credits.toFixed(2) }}</span
                        >
                      </div>
                    </div>
                    <div
                      v-if="key.openrouter_balance"
                      class="grid gap-1 rounded-md border border-default p-3"
                    >
                      <div class="break-words font-medium text-highlighted">
                        {{ t('tokenPlanOpenRouterBalance') }}
                      </div>
                      <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
                        <span
                          >{{ t('tokenPlanLimit') }}:
                          {{
                            formatOpenRouterCredits(
                              key.openrouter_balance.limit,
                            )
                          }}</span
                        >
                        <span
                          >{{ t('tokenPlanLimitRemaining') }}:
                          {{
                            formatOpenRouterCredits(
                              key.openrouter_balance.limit_remaining,
                            )
                          }}</span
                        >
                        <span v-if="key.openrouter_balance.limit_reset"
                          >{{ t('tokenPlanLimitReset') }}:
                          {{ key.openrouter_balance.limit_reset }}</span
                        >
                        <span
                          >{{ t('tokenPlanTotalCredits') }}:
                          {{
                            formatOpenRouterCredits(
                              key.openrouter_balance.total_credits,
                            )
                          }}</span
                        >
                        <span
                          >{{ t('tokenPlanTotalUsage') }}:
                          {{
                            formatOpenRouterCredits(
                              key.openrouter_balance.total_usage,
                            )
                          }}</span
                        >
                      </div>
                      <p
                        v-if="key.openrouter_balance.is_free_tier"
                        class="text-dimmed"
                      >
                        {{ t('tokenPlanFreeTierHint') }}
                      </p>
                    </div>
                    <div
                      v-if="key.deepseek_balance"
                      class="grid gap-1 rounded-md border border-default p-3"
                    >
                      <div class="break-words font-medium text-highlighted">
                        {{ t('tokenPlanDeepSeekBalance') }}
                      </div>
                      <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
                        <span
                          >{{ t('status') }}:
                          {{
                            key.deepseek_balance.is_available
                              ? t('tokenPlanAvailable')
                              : t('tokenPlanUnavailable')
                          }}</span
                        >
                      </div>
                      <div
                        v-for="(entry, entryIndex) in deepseekBalanceRows(
                          key.deepseek_balance,
                          t,
                        )"
                        :key="`${entry.currency}:${entryIndex}`"
                        class="flex flex-wrap gap-x-4 gap-y-1 text-xs"
                      >
                        <span class="font-semibold">{{ entry.currency }}</span>
                        <span
                          >{{ t('tokenPlanTotalBalance') }}:
                          {{ entry.total }}</span
                        >
                        <span
                          >{{ t('tokenPlanGrantedBalance') }}:
                          {{ entry.granted }}</span
                        >
                        <span
                          >{{ t('tokenPlanToppedUpBalance') }}:
                          {{ entry.toppedUp }}</span
                        >
                      </div>
                    </div>
                    <div
                      v-if="key.openrouter_spend"
                      class="grid gap-1 rounded-md border border-default p-3"
                    >
                      <div class="break-words font-medium text-highlighted">
                        {{ t('tokenPlanOpenRouterSpend') }}
                      </div>
                      <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
                        <span
                          v-for="entry in openrouterEntries(key)"
                          :key="entry.labelKey"
                          >{{ t(entry.labelKey) }}:
                          {{ entry.value.toFixed(2) }}</span
                        >
                      </div>
                    </div>
                    <TokenPlanQuotaWindowRow
                      v-for="row in quotaRows(key)"
                      :key="row.sourceId"
                      :row="row"
                      :label="t(quotaRowLabelKey(row))"
                      :t="t"
                      :now-ms="nowMs"
                      density="full"
                      :quantity-suffix="quotaRowQuantitySuffix(row)"
                    />
                    <p
                      v-if="key.balances && !key.five_hour && !key.weekly"
                      class="text-dimmed"
                    >
                      {{ t('tokenPlanPaygNoWindow') }}
                    </p>
                    <div
                      v-if="modelGroups(key).length > 0"
                      class="grid gap-3 sm:grid-cols-2"
                    >
                      <div
                        v-for="group in modelGroups(key)"
                        :key="group.modelName"
                        class="grid gap-2 rounded-md border border-default p-3"
                      >
                        <div class="break-words font-medium text-highlighted">
                          {{ group.modelName }}
                        </div>
                        <div
                          v-if="group.observation"
                          class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
                        >
                          <span
                            v-if="observationTime(group.observation)"
                            class="text-dimmed"
                          >
                            {{
                              t('quotaObservedAt', {
                                time: observationTime(group.observation),
                              })
                            }}
                          </span>
                          <span
                            v-if="observationSourceLabel(group.observation)"
                            class="text-dimmed"
                            >{{
                              observationSourceLabel(group.observation)
                            }}</span
                          >
                          <UBadge
                            v-if="group.observation.refreshing"
                            :label="t('quotaRefreshing')"
                            color="neutral"
                            variant="subtle"
                          />
                          <UBadge
                            v-if="group.observation.stale"
                            :label="t('quotaCachedObservation')"
                            color="warning"
                            variant="subtle"
                          />
                          <span
                            v-if="group.observation.last_error_code"
                            class="text-warning"
                          >
                            {{ t('quotaRefreshFailedCached') }}
                          </span>
                          <span
                            v-if="observationRetryTime(group.observation)"
                            class="text-dimmed"
                          >
                            {{
                              t('quotaRetryAt', {
                                time: observationRetryTime(group.observation),
                              })
                            }}
                          </span>
                        </div>
                        <TokenPlanQuotaWindowRow
                          v-for="row in group.rows"
                          :key="row.sourceId"
                          :row="row"
                          :label="modelRowLabel(row)"
                          :t="t"
                          :now-ms="nowMs"
                          density="full"
                          :quantity-suffix="quotaRowQuantitySuffix(row)"
                        />
                      </div>
                    </div>
                  </template>
                </div>
              </template>
            </UCollapsible>
          </div>
          <p v-if="usage.keys.length === 0" class="text-dimmed">
            {{ t('tokenPlanNoUsage') }}
          </p>
        </template>

        <p v-else class="text-dimmed">{{ t('tokenPlanNoUsage') }}</p>
        <SubscriptionQuotaHistory
          v-if="visible && provider === 'openai' && endpointId"
          :key="endpointId"
          :endpoint-id="endpointId"
          :now-ms="nowMs"
          :refresh-key="historyRefreshKey"
          :t="t"
        />
        <KeyQuotaObservationHistory
          v-if="
            visible && endpointId && provider !== null && provider !== 'generic'
          "
          :key="`observations-${endpointId}`"
          :endpoint-id="endpointId"
          :t="t"
        />
      </div>
    </template>
  </UModal>
</template>
