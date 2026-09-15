<script setup lang="ts">
import ScheduleWindowsForm from '@/components/shared/ScheduleWindowsForm.vue'

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

function onFormSave(value: ScheduleWindow[]): void {
  emit('save', value)
  visible.value = false
}

function onFormCancel(): void {
  visible.value = false
}
</script>

<template>
  <UModal v-model:open="visible" :title="t('scheduleSettings')">
    <template #body>
      <ScheduleWindowsForm
        v-if="visible"
        :hint="props.hint"
        :initial-value="props.initialValue"
        :t="props.t"
        @save="onFormSave"
        @cancel="onFormCancel"
      />
    </template>
  </UModal>
</template>
