<script setup lang="ts">
import { computed } from 'vue'

type ScheduleWindow = { start: string; end: string }

const props = defineProps<{
  t: TranslateFn
}>()

const windows = defineModel<ScheduleWindow[]>('windows', { required: true })
const touched = defineModel<boolean>('touched', { required: true })

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/

function markTouched(): void {
  touched.value = true
}

function rowError(row: ScheduleWindow): string {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  if (!start || !end) return props.t('scheduleRequired')
  if (!HHMM_RE.test(start) || !HHMM_RE.test(end))
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

function restoreAllDay(): void {
  windows.value = []
  markTouched()
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
      {{ t('scheduleAllDay') }}
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
            type="time"
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
      <UButton
        v-if="(windows ?? []).length > 0"
        type="button"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="restoreAllDay"
        >{{ t('scheduleRestoreAllDay') }}</UButton
      >
    </div>
  </div>
</template>
