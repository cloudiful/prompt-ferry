<script setup lang="ts">
import { computed, useSlots } from 'vue'

const props = defineProps<{
  eyebrow?: string
  title?: string
  subtitle?: string
  status?: string
}>()

const slots = useSlots()
const hasToolbar = computed(() => Boolean(props.status || slots.actions))
</script>

<template>
  <Teleport v-if="hasToolbar" to="#dashboard-navbar-actions" defer>
    <div
      class="flex min-w-0 items-center justify-end gap-1.5 whitespace-nowrap"
    >
      <UBadge v-if="status" :label="status" />
      <slot v-if="$slots.actions" name="actions" />
    </div>
  </Teleport>
</template>
