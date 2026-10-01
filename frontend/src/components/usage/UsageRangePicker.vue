<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue'
import type { DateRange } from 'reka-ui'
import type { RequestRecordOverviewRange } from '@/generated/admin-api'
import {
  calendarRangeForValue,
  formatRangeLabel,
  isCompleteRange,
  parseRange,
  rangesEqual,
  toCustomRangeInput,
} from './usage-range-picker'

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

const open = ref(false)
const selectedValue = ref(props.value)
const calendarRange = shallowRef<DateRange | null>(
  calendarRangeForValue(props.value, props.start, props.end),
)

const options = computed(() => [
  { label: props.t('thisMonth'), value: 'month' as const },
  { label: props.t('last24Hours'), value: '24h' as const },
  { label: props.t('last7Days'), value: '7d' as const },
  { label: props.t('last30Days'), value: '30d' as const },
  { label: props.t('customRange'), value: 'custom' as const },
])

const triggerLabel = computed(() => {
  if (selectedValue.value === 'custom') {
    return formatRangeLabel(calendarRange.value) || props.t('customRange')
  }
  return (
    options.value.find((option) => option.value === selectedValue.value)
      ?.label ?? props.t('timeRange')
  )
})

watch(
  () => [props.start, props.end, props.value] as const,
  () => {
    selectedValue.value = props.value
    // Issue #34 P2: a preset owns the window it just applied, so the calendar
    // must not keep a custom selection from an earlier range highlighted.
    calendarRange.value = calendarRangeForValue(
      props.value,
      props.start,
      props.end,
    )
  },
)

function selectPreset(value: UsageRangePreset): void {
  selectedValue.value = value
  if (value === 'custom') return
  calendarRange.value = calendarRangeForValue(value, props.start, props.end)
  open.value = false
  emit('apply', { range: value })
}

function applyCustomRange(range?: DateRange | null): void {
  if (!isCompleteRange(range)) return
  if (rangesEqual(parseRange(props.start, props.end), range)) {
    // Nothing to apply: keep the stored selection so the trigger label and the
    // calendar cannot disagree about the active range.
    selectedValue.value = props.value
    calendarRange.value = calendarRangeForValue(
      props.value,
      props.start,
      props.end,
    )
    return
  }
  const input = toCustomRangeInput(range)
  if (!input) return
  selectedValue.value = 'custom'
  open.value = false
  emit('apply', input)
}
</script>

<template>
  <UPopover v-model:open="open" :content="{ align: 'end' }">
    <UButton
      color="neutral"
      variant="outline"
      size="sm"
      icon="i-lucide-calendar"
      trailing-icon="i-lucide-chevron-down"
      :aria-label="t('timeRange')"
    >
      <span class="max-w-40 truncate">{{ triggerLabel }}</span>
    </UButton>

    <template #content>
      <div class="flex gap-3 p-3">
        <div class="flex flex-col gap-1">
          <UButton
            v-for="option in options"
            :key="option.value"
            size="sm"
            block
            class="justify-start"
            :color="selectedValue === option.value ? 'primary' : 'neutral'"
            :variant="selectedValue === option.value ? 'soft' : 'ghost'"
            :label="option.label"
            @click="selectPreset(option.value)"
          />
        </div>

        <div class="grid content-start gap-2">
          <UInputDate
            v-model="calendarRange"
            range
            size="sm"
            :aria-label="t('customRange')"
            @update:model-value="applyCustomRange"
          />
          <UCalendar
            v-model="calendarRange"
            range
            size="sm"
            :aria-label="t('timeRange')"
            @update:valid-model-value="applyCustomRange"
          />
        </div>
      </div>
    </template>
  </UPopover>
</template>
