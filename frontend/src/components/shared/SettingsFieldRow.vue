<script setup lang="ts">
defineProps<{
  label: string
  hint?: string
  forId?: string
}>()
</script>

<template>
  <div class="grid gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center">
    <div class="flex min-w-0 items-center gap-1">
      <label v-if="forId" :for="forId" class="text-xs font-medium text-default">
        {{ label }}
      </label>
      <span v-else class="text-xs font-medium text-default">{{ label }}</span>
      <UTooltip v-if="hint" :text="hint">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="hint"
        />
      </UTooltip>
    </div>
    <slot />
  </div>
</template>
