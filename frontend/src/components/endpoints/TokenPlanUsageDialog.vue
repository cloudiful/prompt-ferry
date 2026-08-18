<script setup lang="ts">
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

function remainingPercent(window: TokenPlanWindowUsage): number {
  return Math.max(0, Math.min(100, window.remaining_percent ?? 0))
}

function usedPercent(window: TokenPlanWindowUsage): number {
  return 100 - remainingPercent(window)
}

function progressColor(
  window: TokenPlanWindowUsage,
): 'success' | 'warning' | 'error' {
  const remaining = remainingPercent(window)
  if (remaining <= 10) return 'error'
  if (remaining <= 30) return 'warning'
  return 'success'
}

function formatReset(value: string | null | undefined): string {
  if (!value) return '-'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '-' : date.toLocaleString()
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
            class="grid gap-3 rounded-lg border border-default p-3"
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
            <div v-else class="grid gap-3 md:grid-cols-2">
              <template
                v-for="model in key.model_remains"
                :key="model.model_name"
              >
                <div
                  v-if="model.interval"
                  class="grid gap-2 rounded-md bg-elevated p-3"
                >
                  <div class="flex items-center justify-between gap-2">
                    <span class="font-medium text-highlighted">{{
                      windowLabel(model, 'interval')
                    }}</span>
                    <span class="font-semibold"
                      >{{ remainingPercent(model.interval).toFixed(1) }}%</span
                    >
                  </div>
                  <UProgress
                    :model-value="usedPercent(model.interval)"
                    :color="progressColor(model.interval)"
                  />
                  <div class="flex justify-between gap-2 text-dimmed">
                    <span>{{ t('tokenPlanRemaining') }}</span>
                    <span
                      >{{ t('tokenPlanResetAt') }}
                      {{ formatReset(model.interval.end_at) }}</span
                    >
                  </div>
                </div>
                <div
                  v-if="model.weekly"
                  class="grid gap-2 rounded-md bg-elevated p-3"
                >
                  <div class="flex items-center justify-between gap-2">
                    <span class="font-medium text-highlighted">{{
                      windowLabel(model, 'weekly')
                    }}</span>
                    <span class="font-semibold"
                      >{{ remainingPercent(model.weekly).toFixed(1) }}%</span
                    >
                  </div>
                  <UProgress
                    :model-value="usedPercent(model.weekly)"
                    :color="progressColor(model.weekly)"
                  />
                  <div class="flex justify-between gap-2 text-dimmed">
                    <span>{{ t('tokenPlanRemaining') }}</span>
                    <span
                      >{{ t('tokenPlanResetAt') }}
                      {{ formatReset(model.weekly.end_at) }}</span
                    >
                  </div>
                </div>
              </template>
            </div>
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
