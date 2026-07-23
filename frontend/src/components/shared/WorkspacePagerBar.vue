<script setup lang="ts">
defineProps<{
  canNext: boolean
  canPrev: boolean
  end: number
  start: number
  total: number
}>()

defineEmits<{
  next: []
  prev: []
}>()
</script>

<template>
  <section
    class="flex flex-wrap items-start justify-between gap-3 rounded-xl border border-default bg-default px-3 py-3 md:items-center md:px-4"
  >
    <div class="grid min-w-0 gap-px">
      <div class="text-[0.76rem] leading-[1.45] text-muted">
        {{ start }}-{{ end }} / {{ total }}
      </div>
    </div>
    <div
      class="flex w-full flex-wrap items-center justify-start gap-2 md:ml-auto md:w-auto md:justify-end"
    >
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        :disabled="!canPrev"
        @click="$emit('prev')"
      >
        <UIcon name="i-lucide-arrow-left" class="h-4 w-4" />
      </UButton>
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        :disabled="!canNext"
        @click="$emit('next')"
      >
        <UIcon name="i-lucide-arrow-right" class="h-4 w-4" />
      </UButton>
    </div>
  </section>
</template>
