<script setup lang="ts">
import { computed } from 'vue'
import type { TokenPlanUsageResponse } from '@/generated/admin-api'
import {
  useTokenPlanKeyCarousel,
  useTokenPlanTicker,
  useTokenPlanWindowEntries,
} from '@/composables/useTokenPlanWindowEntries'

const props = defineProps<{
  endpointName: string
  loading: boolean
  t: TranslateFn
  usage: TokenPlanUsageResponse | null
}>()

const visible = defineModel<boolean>('visible', { required: true })

const nowMs = useTokenPlanTicker(visible)

const usageKeys = computed(() => props.usage?.keys ?? [])
const carousel = useTokenPlanKeyCarousel(usageKeys, visible)

const {
  remainingPercent,
  usedPercent,
  keyWindowCount,
  minimumRemainingPercent,
  progressColor,
  formatRemaining,
  ccEntries,
  ccMinRemaining,
  progressWindowEntries,
  progressWindowMinRemaining,
  openrouterEntries,
  formatOpenRouterCredits,
} = useTokenPlanWindowEntries(props.t, nowMs)
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
        @mouseenter="carousel.setPaused(true)"
        @mouseleave="carousel.setPaused(false)"
      >
        <div v-if="loading" class="grid gap-2">
          <UProgress animation="carousel" />
          <span class="text-muted">{{ t('loading') }}</span>
        </div>

        <template v-else-if="usage">
          <div
            v-for="(key, index) in usage.keys"
            :key="key.key_id"
            class="col-start-1 row-start-1 border-b border-default transition-opacity duration-500 last:border-b-0"
            :class="[
              index === carousel.index
                ? 'opacity-100'
                : 'pointer-events-none opacity-0',
              { grayscale: usageKeys.length > 1 && !key.ok },
            ]"
            :aria-hidden="index !== carousel.index"
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
                      <UBadge
                        :label="
                          key.ok
                            ? t('tokenPlanAvailable')
                            : t('tokenPlanUnavailable')
                        "
                        :color="key.ok ? 'success' : 'error'"
                        variant="subtle"
                      />
                      <UBadge
                        v-if="key.openrouter_balance?.is_free_tier"
                        :label="t('tokenPlanFreeTier')"
                        color="neutral"
                        variant="subtle"
                      />
                    </span>
                    <span
                      v-if="
                        key.ok &&
                        keyWindowCount(key) +
                          ccEntries(key).length +
                          progressWindowEntries(key).length >
                          0
                      "
                      class="shrink-0 text-xs text-dimmed"
                    >
                      {{
                        t('tokenPlanMinRemaining', {
                          percent: (
                            minimumRemainingPercent(key) ??
                            ccMinRemaining(key) ??
                            progressWindowMinRemaining(key)
                          )?.toFixed(1),
                        })
                      }}
                      ·
                      {{
                        t('tokenPlanWindowCount', {
                          count:
                            keyWindowCount(key) +
                            ccEntries(key).length +
                            progressWindowEntries(key).length,
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
                    <div
                      v-for="(entry, rowIndex) in [
                        ...ccEntries(key),
                        ...progressWindowEntries(key),
                      ]"
                      :key="rowIndex"
                      class="grid gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3"
                    >
                      <span class="text-dimmed">{{ t(entry.labelKey) }}</span>
                      <UProgress
                        class="token-plan-progress h-1.5"
                        :model-value="usedPercent(entry.adapted)"
                        :style="{
                          '--token-plan-progress-color': progressColor(
                            entry.adapted,
                          ),
                        }"
                      />
                      <div
                        class="flex items-center justify-between gap-2 text-xs sm:min-w-[8.5rem] sm:justify-end"
                      >
                        <span class="text-dimmed">{{
                          formatRemaining(entry.adapted)
                        }}</span>
                        <span class="shrink-0 font-semibold"
                          >{{
                            remainingPercent(entry.adapted).toFixed(1)
                          }}%</span
                        >
                      </div>
                      <span
                        v-if="entry.subline"
                        class="text-dimmed sm:col-span-2 sm:col-start-2"
                        >{{ entry.subline }}</span
                      >
                    </div>
                    <p
                      v-if="key.balances && !key.five_hour && !key.weekly"
                      class="text-dimmed"
                    >
                      {{ t('tokenPlanPaygNoWindow') }}
                    </p>
                    <div
                      v-if="key.model_remains.length > 0"
                      class="grid gap-3 sm:grid-cols-2"
                    >
                      <div
                        v-for="model in key.model_remains"
                        :key="model.model_name"
                        class="grid gap-2 rounded-md border border-default p-3"
                      >
                        <div class="break-words font-medium text-highlighted">
                          {{ model.model_name }}
                        </div>
                        <div
                          v-if="model.interval"
                          class="grid gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3"
                        >
                          <span class="text-dimmed">{{
                            t('tokenPlanInterval')
                          }}</span>
                          <UProgress
                            class="token-plan-progress h-1.5"
                            :model-value="usedPercent(model.interval)"
                            :style="{
                              '--token-plan-progress-color': progressColor(
                                model.interval,
                              ),
                            }"
                          />
                          <div
                            class="flex items-center justify-between gap-2 text-xs sm:min-w-[8.5rem] sm:justify-end"
                          >
                            <span class="text-dimmed">{{
                              formatRemaining(model.interval)
                            }}</span>
                            <span class="shrink-0 font-semibold"
                              >{{
                                remainingPercent(model.interval).toFixed(1)
                              }}%</span
                            >
                          </div>
                        </div>
                        <div
                          v-if="model.weekly"
                          class="grid gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3"
                        >
                          <span class="text-dimmed">{{
                            t('tokenPlanWeekly')
                          }}</span>
                          <UProgress
                            class="token-plan-progress h-1.5"
                            :model-value="usedPercent(model.weekly)"
                            :style="{
                              '--token-plan-progress-color': progressColor(
                                model.weekly,
                              ),
                            }"
                          />
                          <div
                            class="flex items-center justify-between gap-2 text-xs sm:min-w-[8.5rem] sm:justify-end"
                          >
                            <span class="text-dimmed">{{
                              formatRemaining(model.weekly)
                            }}</span>
                            <span class="shrink-0 font-semibold"
                              >{{
                                remainingPercent(model.weekly).toFixed(1)
                              }}%</span
                            >
                          </div>
                        </div>
                      </div>
                    </div>
                  </template>
                </div>
              </template>
            </UCollapsible>
          </div>
          <div
            v-if="usageKeys.length > 1"
            class="col-start-1 row-start-2 flex items-center justify-center gap-2"
          >
            <UButton
              color="neutral"
              variant="ghost"
              size="xs"
              icon="i-lucide-chevron-left"
              @click="carousel.go(carousel.index - 1)"
            />
            <button
              v-for="(key, index) in usageKeys"
              :key="key.key_id"
              type="button"
              :aria-label="key.key_label"
              class="h-2 w-2 cursor-pointer rounded-full p-0"
              :class="index === carousel.index ? 'bg-primary' : 'bg-elevated'"
              @click="carousel.go(index)"
            />
            <UButton
              color="neutral"
              variant="ghost"
              size="xs"
              icon="i-lucide-chevron-right"
              @click="carousel.go(carousel.index + 1)"
            />
          </div>
          <p v-if="usage.keys.length === 0" class="text-dimmed">
            {{ t('tokenPlanNoUsage') }}
          </p>
        </template>

        <p v-else class="text-dimmed">{{ t('tokenPlanNoUsage') }}</p>
      </div>
    </template>
  </UModal>
</template>

<style scoped>
.token-plan-progress :deep([data-slot='indicator']) {
  background-color: var(--token-plan-progress-color);
}
</style>
