<script setup lang="ts" generic="T extends string | boolean | number">
import type { Option } from '@/models'

defineProps<{
  modelValue: T | null
  options: Array<Option<T>>
  placeholder: string
}>()

const emit = defineEmits<{
  change: []
  'update:modelValue': [value: T | null]
}>()

function handleUpdate(value: T | null): void {
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
