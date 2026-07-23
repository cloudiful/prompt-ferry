<script setup lang="ts">
defineProps<{
  label: string
}>()
</script>

<template>
  <div
    class="grid items-center gap-x-2 gap-y-px rounded border border-default bg-default p-2.5 text-[0.75rem] text-muted"
    style="grid-template-columns: auto 1fr"
  >
    <span>{{ label }}</span>
    <strong
      class="col-[1/-1] text-base leading-[1.25] font-semibold text-highlighted"
    >
      <slot />
    </strong>
  </div>
</template>
