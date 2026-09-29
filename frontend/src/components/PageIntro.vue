<script setup lang="ts">
import { computed, useSlots } from 'vue'

const props = defineProps<{
  status?: string
}>()

const slots = useSlots()
const hasToolbar = computed(() => Boolean(props.status || slots.actions))
</script>

<template>
  <div
    v-if="hasToolbar"
    class="flex min-w-0 max-w-full items-center justify-end gap-1.5 overflow-x-auto overscroll-x-contain whitespace-nowrap"
  >
    <UBadge v-if="status" :label="status" />
    <slot v-if="$slots.actions" name="actions" />
  </div>
</template>
