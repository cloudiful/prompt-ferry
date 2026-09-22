<script setup lang="ts">
import { computed } from 'vue'

type ScheduleWindow = { start: string; end: string; days?: number[] }

const props = defineProps<{
  t: TranslateFn
}>()

const windows = defineModel<ScheduleWindow[]>('windows', { required: true })
const touched = defineModel<boolean>('touched', { required: true })

// Issue #457: `end` may additionally be `24:00` (exclusive midnight);
// `start` stays within `00:00-23:59`.
const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/
const HHMM_END_RE = /^(([01]\d|2[0-3]):[0-5]\d|24:00)$/

function markTouched(): void {
  touched.value = true
}

function rowError(row: ScheduleWindow): string {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  if (!start || !end) return props.t('scheduleRequired')
  if (!HHMM_RE.test(start) || !HHMM_END_RE.test(end))
    return props.t('scheduleInvalid')
  if (start === end) return props.t('scheduleEqual')
  return ''
}

const rowErrors = computed(() => (windows.value ?? []).map(rowError))

function addWindow(): void {
  if (!Array.isArray(windows.value)) windows.value = []
  windows.value.push({ start: '', end: '' })
  markTouched()
}

function removeWindow(index: number): void {
  windows.value.splice(index, 1)
  markTouched()
}

const ALL_DAYS = [1, 2, 3, 4, 5, 6, 7]

function rowDays(index: number): number[] {
  const days = windows.value?.[index]?.days
  if (!days || days.length === 0) return [...ALL_DAYS]
  return [...days]
}

function setRowDays(index: number, days: number[]): void {
  const row = windows.value?.[index]
  if (!row) return
  row.days = [...days]
  markTouched()
}

function toggleRowDay(index: number, day: number): void {
  const current = new Set(rowDays(index))
  if (current.has(day)) current.delete(day)
  else current.add(day)
  setRowDays(
    index,
    [...current].sort((a, b) => a - b),
  )
}

function onTimeUpdate(
  index: number,
  field: 'start' | 'end',
  value: string,
): void {
  const row = windows.value?.[index]
  if (!row) return
  row[field] = value ?? ''
  markTouched()
}
</script>

<template>
  <div class="grid gap-3 text-xs">
    <div v-if="(windows ?? []).length === 0" class="text-xs text-muted">
      {{ t('scheduleEmptyHint') }}
    </div>
    <div v-for="(row, index) in windows" :key="index" class="grid gap-1">
      <div class="flex items-center gap-2">
        <label class="grid flex-1 gap-1">
          <span class="text-xs text-muted">{{ t('scheduleStart') }}</span>
          <UInput
            :model-value="row.start"
            type="time"
            class="w-full"
            @update:model-value="onTimeUpdate(index, 'start', $event as string)"
          />
        </label>
        <label class="grid flex-1 gap-1">
          <span class="text-xs text-muted">{{ t('scheduleEnd') }}</span>
          <UInput
            :model-value="row.end"
            type="text"
            placeholder="HH:MM"
            maxlength="5"
            inputmode="numeric"
            pattern="([01][0-9]|2[0-3]):[0-5][0-9]|24:00"
            class="w-full"
            @update:model-value="onTimeUpdate(index, 'end', $event as string)"
          />
        </label>
        <UButton
          type="button"
          size="sm"
          color="error"
          variant="ghost"
          :aria-label="t('delete')"
          @click="removeWindow(index)"
          ><UIcon name="i-lucide-trash-2" class="h-4 w-4"
        /></UButton>
      </div>
      <p v-if="rowErrors[index]" class="text-xs text-error">
        {{ rowErrors[index] }}
      </p>
      <div class="flex flex-wrap gap-1">
        <UButton
          type="button"
          size="xs"
          variant="ghost"
          @click="setRowDays(index, [1, 2, 3, 4, 5])"
          >{{ t('weekdays') }}</UButton
        >
        <UButton
          type="button"
          size="xs"
          variant="ghost"
          @click="setRowDays(index, [6, 7])"
          >{{ t('weekend') }}</UButton
        >
        <UButton
          type="button"
          size="xs"
          variant="ghost"
          @click="setRowDays(index, [1, 2, 3, 4, 5, 6, 7])"
          >{{ t('everyDay') }}</UButton
        >
        <UButton
          v-for="d in [1, 2, 3, 4, 5, 6, 7]"
          :key="d"
          type="button"
          size="xs"
          :variant="rowDays(index).includes(d) ? 'solid' : 'outline'"
          @click="toggleRowDay(index, d)"
          >{{ t('weekday' + d) }}</UButton
        >
      </div>
    </div>
    <div class="flex flex-wrap gap-2">
      <UButton
        type="button"
        size="sm"
        color="neutral"
        variant="outline"
        @click="addWindow"
        ><UIcon name="i-lucide-plus" class="h-4 w-4" />{{
          t('scheduleAddWindow')
        }}</UButton
      >
    </div>
  </div>
</template>
