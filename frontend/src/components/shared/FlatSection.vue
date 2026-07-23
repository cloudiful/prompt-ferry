<script setup lang="ts">
defineProps<{
  bodyClass?: string
  title?: string
}>()
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <div
      v-if="$slots.header || title || $slots.actions"
      class="flex flex-wrap items-center justify-between gap-3"
    >
      <slot name="header" />
      <h2
        v-if="!$slots.header && title"
        class="m-0 text-[0.98rem] leading-[1.3] font-semibold text-highlighted"
      >
        {{ title }}
      </h2>
      <slot name="actions" />
    </div>
    <div v-if="$slots.default" class="grid gap-3" :class="bodyClass">
      <slot />
    </div>
  </section>
</template>
