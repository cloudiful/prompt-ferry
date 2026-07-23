<script setup lang="ts">
defineProps<{
  label: string
}>()
</script>

<template>
  <div class="grid content-start gap-px">
    <span class="text-[0.72rem] text-dimmed">{{ label }}</span>
    <strong class="text-[0.8rem] leading-[1.45] font-semibold text-highlighted">
      <slot />
    </strong>
  </div>
</template>
