<script setup lang="ts">
import { computed } from 'vue'
import { useLocale } from '../composables/useLocale'
import type { RequestOverviewMode } from '../request-overview'

defineProps<{
  activeMode: RequestOverviewMode
}>()

defineEmits<{
  change: [mode: RequestOverviewMode]
}>()

const { t } = useLocale()
const shortOverviewLabel = computed(() =>
  t('overviewMode').length > 4
    ? t('overviewMode').slice(0, 2)
    : t('overviewMode'),
)
const shortRecordsLabel = computed(() =>
  t('recordsMode').length > 4 ? t('recordsMode').slice(0, 2) : t('recordsMode'),
)
const switchAriaLabel = computed(
  () => `${t('currentSlice')}: ${t('overviewMode')} / ${t('recordsMode')}`,
)
</script>

<template>
  <div
    class="flex w-full min-w-0 flex-nowrap gap-1 md:w-auto md:flex-wrap md:gap-2"
    role="group"
    :aria-label="switchAriaLabel"
  >
    <UButton
      class="flex-1 justify-center px-1.5 py-1 md:flex-none md:px-3 md:py-2"
      size="sm"
      :color="activeMode === 'overview' ? 'primary' : 'neutral'"
      :variant="activeMode === 'overview' ? 'solid' : 'outline'"
      :aria-label="t('overviewMode')"
      @click="$emit('change', 'overview')"
    >
      <span aria-hidden="true" class="text-[0.67rem] md:hidden">{{
        shortOverviewLabel
      }}</span>
      <span aria-hidden="true" class="hidden md:inline">{{
        t('overviewMode')
      }}</span>
    </UButton>
    <UButton
      class="flex-1 justify-center px-1.5 py-1 md:flex-none md:px-3 md:py-2"
      size="sm"
      :color="activeMode === 'records' ? 'primary' : 'neutral'"
      :variant="activeMode === 'records' ? 'solid' : 'outline'"
      :aria-label="t('recordsMode')"
      @click="$emit('change', 'records')"
    >
      <span aria-hidden="true" class="text-[0.67rem] md:hidden">{{
        shortRecordsLabel
      }}</span>
      <span aria-hidden="true" class="hidden md:inline">{{
        t('recordsMode')
      }}</span>
    </UButton>
  </div>
</template>
