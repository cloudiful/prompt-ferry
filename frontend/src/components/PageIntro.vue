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
  <section
    v-if="hasToolbar"
    class="grid items-start gap-x-4 gap-y-2 md:grid-cols-[minmax(0,1fr)_auto] md:gap-y-3"
  >
    <div
      class="flex min-w-0 w-full flex-nowrap items-center justify-end gap-1 overflow-x-auto overscroll-x-contain pb-px md:w-auto md:justify-self-end md:gap-1.5 md:overflow-visible md:pb-0"
    >
      <UBadge :label="status" />
      <slot v-if="$slots.actions" name="actions" />
    </div>
  </section>
</template>
