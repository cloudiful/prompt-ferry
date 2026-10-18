<script setup lang="ts">
import { computed } from 'vue'
import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'
import { canonicalQuantityCaption } from '@/composables/useCanonicalQuotaWindows'
import {
  formatResetDuration,
  knownRemainingPercent,
  progressColor,
  remainingPercent,
} from '@/composables/useTokenPlanWindowEntries'

const props = defineProps<{
  row: CanonicalQuotaRow
  label: string
  t: TranslateFn
  nowMs: number
  density: 'compact' | 'full'
  quantitySuffix?: string
}>()

// The shared progress/countdown shape every quota row renders through. The
// row itself only carries the freshness-applied share, so an unknown or
// expired window degrades to the same dash the dialog has always shown.
const adapted = computed(() => ({
  remaining_percent: props.row.remainingPercent,
}))

const caption = computed(() => {
  const base = canonicalQuantityCaption(props.row)
  if (base === null) return null
  return props.quantitySuffix ? `${base}${props.quantitySuffix}` : base
})

const resetMs = computed(() => {
  if (props.row.resetAt == null) return null
  const ts = Date.parse(props.row.resetAt)
  return Number.isNaN(ts) ? null : ts - props.nowMs
})

// Time windows render the progress/countdown row; balance/unlimited rows
// are text-only and never grow a synthetic progress bar.
const isTimeWindow = computed(
  () => props.row.kind !== 'balance' && props.row.kind !== 'unlimited',
)
</script>

<template>
  <div
    :class="
      density === 'compact'
        ? 'grid gap-1.5 sm:grid-cols-[7rem_minmax(0,1fr)_9rem] sm:items-center sm:gap-3'
        : 'grid gap-1.5 sm:grid-cols-[minmax(7rem,auto)_minmax(0,1fr)_minmax(8.5rem,auto)] sm:items-center sm:gap-3'
    "
  >
    <span class="text-dimmed">{{ label }}</span>
    <template v-if="isTimeWindow">
      <UProgress
        v-if="knownRemainingPercent(adapted) !== null"
        class="token-plan-progress h-1.5"
        :model-value="remainingPercent(adapted)"
        :style="{
          '--token-plan-progress-color': progressColor(adapted),
        }"
      />
      <div
        class="flex items-center justify-between gap-2 text-xs"
        :class="{ 'sm:min-w-[8.5rem] sm:justify-end': density === 'full' }"
      >
        <span class="text-dimmed">{{ formatResetDuration(resetMs, t) }}</span>
        <span class="shrink-0 font-semibold">
          {{
            knownRemainingPercent(adapted) === null
              ? '—'
              : `${t('quotaRemainingPercent')} ${remainingPercent(adapted).toFixed(1)}%`
          }}
        </span>
      </div>
      <span v-if="caption" class="text-dimmed sm:col-span-2 sm:col-start-2">{{
        caption
      }}</span>
    </template>
    <template v-else>
      <span class="text-dimmed sm:col-span-2">{{ caption ?? '—' }}</span>
    </template>
  </div>
</template>

<style scoped>
.token-plan-progress :deep([data-slot='indicator']) {
  background-color: var(--token-plan-progress-color);
}
</style>
