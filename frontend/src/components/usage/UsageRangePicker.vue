<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue'
import { CalendarDate, parseDate } from '@internationalized/date'
import type { DateRange } from 'reka-ui'
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

function toCalendarDate(iso: string): CalendarDate | null {
  const text = iso.slice(0, 10)
  if (!/^\d{4}-\d{2}-\d{2}$/.test(text)) return null
  try {
    return parseDate(text)
  } catch {
    return null
  }
}

function toCustomRange(start: string, end: string): DateRange | null {
  const startDate = toCalendarDate(start)
  const endDate = toCalendarDate(end)
  if (!startDate || !endDate) return null
  return { start: startDate, end: endDate }
}

const customRange = shallowRef<DateRange | null>(
  toCustomRange(props.start, props.end),
)

const options = computed(() => [
  { label: props.t('thisMonth'), value: 'month' as const },
  { label: props.t('last24Hours'), value: '24h' as const },
  { label: props.t('last7Days'), value: '7d' as const },
  { label: props.t('last30Days'), value: '30d' as const },
  { label: props.t('customRange'), value: 'custom' as const },
])

watch(
  () => [props.start, props.end, props.value],
  () => {
    customRange.value = toCustomRange(props.start, props.end)
    selectedValue.value = props.value
  },
)

function selectPreset(value: UsageRangePreset): void {
  selectedValue.value = value
  if (value === 'custom') return
  emit('apply', { range: value })
}

function applyCustomRange(value?: DateRange | null): void {
  const range = value ?? customRange.value
  if (!range?.start || !range?.end) return
  customRange.value = range
  const startText = range.start.toString().slice(0, 10)
  const endText = range.end.toString().slice(0, 10)
  const end = new Date(`${endText}T00:00:00.000Z`)
  end.setUTCDate(end.getUTCDate() + 1)
  emit('apply', {
    range: 'custom',
    start: new Date(`${startText}T00:00:00.000Z`).toISOString(),
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
      <UInputDate
        v-model="customRange"
        range
        size="sm"
        :aria-label="t('customRange')"
        @update:model-value="applyCustomRange"
      />
    </template>
  </div>
</template>
