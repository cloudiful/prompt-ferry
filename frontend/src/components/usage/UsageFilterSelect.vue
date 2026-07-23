<script setup lang="ts">
import type { Option } from '@/models'

defineProps<{
  modelValue: string | boolean | null
  options: Array<Option<string | boolean>>
  placeholder: string
}>()

const emit = defineEmits<{
  change: []
  'update:modelValue': [value: string | boolean | null]
}>()

function handleUpdate(value: string | boolean | null): void {
  emit('update:modelValue', value)
  emit('change')
}
</script>

<template>
  <USelectMenu
    :model-value="modelValue"
    class="w-44"
    :items="options"
    label-key="label"
    value-key="value"
    :placeholder="placeholder"
    clear
    @update:model-value="handleUpdate"
  />
</template>
