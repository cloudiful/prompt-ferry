<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  message: string
  severity: 'success' | 'error' | null
}>()

const interactive = computed(() => props.severity !== null)
</script>

<template>
  <UPopover
    v-if="interactive"
    :content="{
      side: 'bottom',
      align: 'start',
      sideOffset: 6,
      collisionPadding: 8,
    }"
  >
    <UBadge
      as="button"
      :label="message"
      :color="severity ?? undefined"
      variant="subtle"
      type="button"
      class="max-w-40 cursor-pointer truncate text-left sm:max-w-56 lg:max-w-72"
      :ui="{ base: 'min-w-0', label: 'min-w-0 truncate text-left' }"
      :aria-label="message"
      :title="message"
    />
    <template #content>
      <div
        class="max-h-[50vh] max-w-[min(22rem,calc(100vw-2rem))] overflow-auto break-words whitespace-pre-wrap p-3 text-sm leading-relaxed [overflow-wrap:anywhere]"
      >
        {{ message }}
      </div>
    </template>
  </UPopover>
  <span
    v-else
    class="block max-w-40 truncate text-xs text-dimmed sm:max-w-56"
    :title="message"
  >
    {{ message }}
  </span>
</template>
