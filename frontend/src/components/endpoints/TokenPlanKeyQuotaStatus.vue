<script setup lang="ts">
import { computed } from 'vue'
import type { TokenPlanQuotaStatus } from '@/generated/admin-api'

const props = defineProps<{
  status: TokenPlanQuotaStatus | null
  t: TranslateFn
}>()

const state = computed(() => props.status?.state ?? 'unknown')
const label = computed(() => {
  switch (state.value) {
    case 'available':
      return props.t('tokenPlanAvailable')
    case 'exhausted':
      return props.t('tokenPlanUnavailable')
    case 'cooling':
      return props.t('tokenPlanQuotaCooling')
    default:
      return props.t('tokenPlanQuotaUnknown')
  }
})
const color = computed(() => {
  if (state.value === 'available') return 'success'
  if (state.value === 'exhausted') return 'error'
  return 'neutral'
})
const title = computed(() => {
  const hints = [props.t('tokenPlanDefaultTextScope')]
  const observedAt = props.status?.observed_at
  if (observedAt) {
    hints.push(
      props.t('quotaObservedAt', {
        time: new Date(observedAt).toLocaleString(),
      }),
    )
  }
  if (props.status?.refresh_failed) hints.push(props.t('quotaRefreshFailedCached'))
  const cooldownUntil = props.status?.cooldown_until
  if (cooldownUntil) {
    hints.push(
      props.t('quotaRetryAt', {
        time: new Date(cooldownUntil).toLocaleString(),
      }),
    )
  }
  return hints.join(' · ')
})
</script>

<template>
  <UBadge :label="label" :color="color" variant="subtle" :title="title" />
</template>
