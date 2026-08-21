<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import type {
  TokenPlanModelUsage,
  TokenPlanUsageResponse,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'

const props = defineProps<{
  endpointName: string
  loading: boolean
  t: TranslateFn
  usage: TokenPlanUsageResponse | null
}>()

const visible = defineModel<boolean>('visible', { required: true })

// Live "now" anchor used by the reset countdown. It only ticks while the
// dialog is open so the rendered text stays fresh without leaking background
// timers after the user closes the modal.
const nowMs = ref<number>(Date.now())
let ticker: ReturnType<typeof setInterval> | null = null

function startTicker(): void {
  if (ticker !== null) return
  nowMs.value = Date.now()
  ticker = setInterval(() => {
    nowMs.value = Date.now()
  }, 1000)
}

function stopTicker(): void {
  if (ticker === null) return
  clearInterval(ticker)
  ticker = null
}

watch(
  () => visible.value,
  (open) => {
    if (open) startTicker()
    else stopTicker()
  },
  { immediate: true },
)

onBeforeUnmount(stopTicker)

function remainingPercent(window: TokenPlanWindowUsage): number {
  const raw = window.remaining_percent
  if (raw == null || !Number.isFinite(raw)) return 0
  return Math.max(0, Math.min(100, raw))
}

function usedPercent(window: TokenPlanWindowUsage): number {
  return 100 - remainingPercent(window)
}

function progressColor(window: TokenPlanWindowUsage): string {
  const used = usedPercent(window)
  const hue = 120 - used * 1.2
  return `hsl(${hue} 80% 45%)`
}

function endTimeMs(window: TokenPlanWindowUsage): number | null {
  const value = window.end_at
  if (typeof value !== 'string' || value.length === 0) return null
  const ts = Date.parse(value)
  return Number.isNaN(ts) ? null : ts
}

function remainingMs(window: TokenPlanWindowUsage): number | null {
  // Prefer end_at so the countdown tracks wall-clock time; fall back to the
  // snapshot value when no parseable end_at is available.
  const end = endTimeMs(window)
  if (end !== null) return end - nowMs.value
  const snapshot = window.remains_time_ms
  if (typeof snapshot === 'number' && Number.isFinite(snapshot)) {
    return snapshot
  }
  return null
}

function formatRemaining(window: TokenPlanWindowUsage): string {
  const ms = remainingMs(window)
  if (ms === null) return '-'
  if (ms <= 0) return props.t('tokenPlanExpired')
  const totalSeconds = Math.floor(ms / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  if (hours > 0) {
    return props.t('tokenPlanResetExpiresHoursMinutes', { hours, minutes })
  }
  if (minutes > 0) {
    return props.t('tokenPlanResetExpiresMinutes', { minutes })
  }
  return props.t('tokenPlanResetExpiresSeconds', { seconds: totalSeconds })
}

function windowLabel(
  model: TokenPlanModelUsage,
  kind: 'interval' | 'weekly',
): string {
  return kind === 'interval'
    ? `${model.model_name} / ${props.t('tokenPlanInterval')}`
    : `${model.model_name} / ${props.t('tokenPlanWeekly')}`
}
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="`${t('tokenPlanUsage')} / ${endpointName}`"
  >
    <template #body>
      <div class="grid gap-4 text-xs">
        <div v-if="loading" class="grid gap-2">
          <UProgress animation="carousel" />
          <span class="text-muted">{{ t('loading') }}</span>
        </div>

        <template v-else-if="usage">
          <div
            v-for="key in usage.keys"
            :key="key.key_id"
            class="grid gap-3 border-t border-default pt-3 first:border-t-0 first:pt-0"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="font-semibold text-highlighted">{{
                key.key_label
              }}</span>
              <UBadge
                :label="
                  key.ok ? t('tokenPlanAvailable') : t('tokenPlanUnavailable')
                "
                :color="key.ok ? 'success' : 'error'"
                variant="subtle"
              />
            </div>
            <p v-if="!key.ok" class="break-words text-error">
              {{ key.error_message ?? t('tokenPlanUsageFailed') }}
            </p>
            <template v-else>
              <template
                v-for="model in key.model_remains"
                :key="model.model_name"
              >
                <div v-if="model.interval" class="grid gap-1.5">
                  <div class="flex items-center justify-between gap-2">
                    <span class="font-medium text-highlighted">{{
                      windowLabel(model, 'interval')
                    }}</span>
                    <span class="font-semibold"
                      >{{ remainingPercent(model.interval).toFixed(1) }}%</span
                    >
                  </div>
                  <UProgress
                    class="token-plan-progress"
                    :model-value="usedPercent(model.interval)"
                    :style="{
                      '--token-plan-progress-color': progressColor(
                        model.interval,
                      ),
                    }"
                  />
                  <div class="flex justify-end gap-2 text-dimmed">
                    <span>{{ formatRemaining(model.interval) }}</span>
                  </div>
                </div>
                <div v-if="model.weekly" class="grid gap-1.5">
                  <div class="flex items-center justify-between gap-2">
                    <span class="font-medium text-highlighted">{{
                      windowLabel(model, 'weekly')
                    }}</span>
                    <span class="font-semibold"
                      >{{ remainingPercent(model.weekly).toFixed(1) }}%</span
                    >
                  </div>
                  <UProgress
                    class="token-plan-progress"
                    :model-value="usedPercent(model.weekly)"
                    :style="{
                      '--token-plan-progress-color': progressColor(
                        model.weekly,
                      ),
                    }"
                  />
                  <div class="flex justify-end gap-2 text-dimmed">
                    <span>{{ formatRemaining(model.weekly) }}</span>
                  </div>
                </div>
              </template>
            </template>
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
