<script setup lang="ts">
import ProxySettingsForm from '@/components/shared/ProxySettingsForm.vue'

const props = defineProps<{
  hint: string
  initialValue: string
  hasSaved: boolean
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })

const emit = defineEmits<{
  save: [value: string]
  clear: []
}>()

function onFormSave(value: string): void {
  emit('save', value)
  visible.value = false
}

function onFormClear(): void {
  emit('clear')
  visible.value = false
}

function onFormCancel(): void {
  visible.value = false
}
</script>

<template>
  <UModal v-model:open="visible" :title="t('proxySettings')">
    <template #body>
      <ProxySettingsForm
        v-if="visible"
        :hint="props.hint"
        :initial-value="props.initialValue"
        :has-saved="props.hasSaved"
        :t="props.t"
        @save="onFormSave"
        @clear="onFormClear"
        @cancel="onFormCancel"
      />
    </template>
  </UModal>
</template>
