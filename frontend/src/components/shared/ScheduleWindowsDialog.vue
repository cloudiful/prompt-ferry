<script setup lang="ts">
import { computed, ref, watch } from 'vue'

type ScheduleWindow = { start: string; end: string }

const props = defineProps<{
  hint: string
  initialValue: ScheduleWindow[]
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })

const emit = defineEmits<{
  save: [value: ScheduleWindow[]]
}>()

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/

function cloneWindows(value: ScheduleWindow[]): ScheduleWindow[] {
  return (value ?? []).map((window) => ({
    start: (window?.start ?? '').trim(),
    end: (window?.end ?? '').trim(),
  }))
}

function sortWindows(value: ScheduleWindow[]): ScheduleWindow[] {
  return [...value].sort((a, b) =>
    a.start === b.start
      ? a.end.localeCompare(b.end)
      : a.start.localeCompare(b.start),
  )
}

const rows = ref<ScheduleWindow[]>([])
const initialRows = ref<ScheduleWindow[]>([])

watch(
  () => visible.value,
  (open) => {
    if (!open) return
    rows.value = cloneWindows(props.initialValue)
    initialRows.value = cloneWindows(props.initialValue)
  },
  { immediate: true },
)

function rowError(row: ScheduleWindow): string {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  if (!start || !end) return props.t('scheduleRequired')
  if (!HHMM_RE.test(start) || !HHMM_RE.test(end))
    return props.t('scheduleInvalid')
  if (start === end) return props.t('scheduleEqual')
  return ''
}

const rowErrors = computed(() => rows.value.map(rowError))

const hasErrors = computed(() => rowErrors.value.some((error) => error !== ''))

const isDirty = computed(
  () => JSON.stringify(rows.value) !== JSON.stringify(initialRows.value),
)

const canSave = computed(() => !hasErrors.value)

function addWindow(): void {
  rows.value.push({ start: '', end: '' })
}

function removeWindow(index: number): void {
  rows.value.splice(index, 1)
}

function restoreAllDay(): void {
  rows.value = []
}

function onSave(): void {
  // Untouched save keeps the stored schedule: close without emitting so the
  // outer form keeps `active_windows_touched=false` (request omits the key).
  if (!isDirty.value) {
    visible.value = false
    return
  }
  if (hasErrors.value) return
  emit('save', sortWindows(cloneWindows(rows.value)))
  visible.value = false
}
</script>

<template>
  <UModal v-model:open="visible" :title="t('scheduleSettings')">
    <template #body>
      <div class="grid gap-3 text-xs">
        <p class="text-xs leading-snug text-muted">{{ hint }}</p>
        <div v-if="rows.length === 0" class="text-xs text-muted">
          {{ t('scheduleAllDay') }}
        </div>
        <div v-for="(row, index) in rows" :key="index" class="grid gap-1">
          <div class="flex items-center gap-2">
            <label class="grid flex-1 gap-1">
              <span class="text-xs text-muted">{{ t('scheduleStart') }}</span>
              <UInput v-model="row.start" type="time" class="w-full" />
            </label>
            <label class="grid flex-1 gap-1">
              <span class="text-xs text-muted">{{ t('scheduleEnd') }}</span>
              <UInput v-model="row.end" type="time" class="w-full" />
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
            v-if="rows.length > 0"
            type="button"
            size="sm"
            color="neutral"
            variant="ghost"
            @click="restoreAllDay"
            >{{ t('scheduleRestoreAllDay') }}</UButton
          >
        </div>
        <div class="flex justify-end gap-2 pt-1">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            variant="outline"
            @click="visible = false"
            >{{ t('cancel') }}</UButton
          >
          <UButton
            type="button"
            size="sm"
            :disabled="!canSave"
            @click="onSave"
            >{{ t('save') }}</UButton
          >
        </div>
      </div>
    </template>
  </UModal>
</template>
