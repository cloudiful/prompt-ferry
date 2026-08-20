<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { RequestRecordOverviewRange } from '@/generated/admin-api'

type UsageRangePreset = RequestRecordOverviewRange

const props = defineProps<{
  end: string
  start: string
  t: TranslateFn
  value: UsageRangePreset
}>()

const emit = defineEmits<{
  apply: [input: { range: UsageRangePreset; start?: string; end?: string }]
}>()

const selectedValue = ref(props.value)
const customStart = ref(props.start.slice(0, 10))
const customEnd = ref(props.end.slice(0, 10))

const options = computed(() => [
  { label: props.t('thisMonth'), value: 'month' as const },
  { label: props.t('last24Hours'), value: '24h' as const },
  { label: props.t('last7Days'), value: '7d' as const },
  { label: props.t('last30Days'), value: '30d' as const },
  { label: props.t('customRange'), value: 'custom' as const },
])

watch(
  () => [props.start, props.end, props.value],
  ([start, end]) => {
    customStart.value = start.slice(0, 10)
    customEnd.value = end.slice(0, 10)
    selectedValue.value = props.value
  },
)

function selectPreset(value: UsageRangePreset): void {
  selectedValue.value = value
  if (value === 'custom') return
  emit('apply', { range: value })
}

function applyCustomRange(): void {
  if (!customStart.value || !customEnd.value) return
  const end = new Date(`${customEnd.value}T00:00:00.000Z`)
  end.setUTCDate(end.getUTCDate() + 1)
  emit('apply', {
    range: 'custom',
    start: new Date(`${customStart.value}T00:00:00.000Z`).toISOString(),
    end: end.toISOString(),
  })
}
</script>

<template>
  <div class="flex min-w-0 flex-wrap items-center gap-1.5">
    <USelectMenu
      :model-value="selectedValue"
      :items="options"
      value-key="value"
      label-key="label"
      class="w-32 sm:w-36"
      :aria-label="t('timeRange')"
      @update:model-value="selectPreset"
    />
    <template v-if="selectedValue === 'custom'">
      <UInput
        v-model="customStart"
        type="date"
        class="w-32"
        :aria-label="t('startTime')"
        @update:model-value="applyCustomRange"
      />
      <UInput
        v-model="customEnd"
        type="date"
        class="w-32"
        :aria-label="t('endTime')"
        @update:model-value="applyCustomRange"
      />
    </template>
  </div>
</template>
