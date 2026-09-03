<script setup lang="ts">
defineProps<{
  bodyClass?: string
  headerClass?: string
}>()
</script>

<template>
  <article
    class="min-w-0 overflow-hidden rounded-lg border border-default bg-default"
  >
    <div
      v-if="$slots.header"
      class="flex flex-wrap items-start justify-between gap-2 border-b border-default px-3 py-2.5"
      :class="headerClass"
    >
      <slot name="header" />
    </div>
    <div v-if="$slots.default" class="grid gap-3 p-3" :class="bodyClass">
      <slot />
    </div>
  </article>
</template>
