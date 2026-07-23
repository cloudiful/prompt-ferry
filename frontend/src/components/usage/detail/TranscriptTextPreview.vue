<script setup lang="ts">
import TranscriptManagedPreview from '@/components/usage/detail/TranscriptManagedPreview.vue'

const TEXT_PREVIEW_STEP_LINES = 160
const TEXT_PREVIEW_STEP_CHARS = 12_000

defineProps<{
  allLabel: string
  collapseLabel: string
  emptyText: string
  level: number
  maxHeight: string
  mode: 'json' | 'markdown'
  moreLabel: string
  text: string
  truncatedLabel: string
}>()

const emit = defineEmits<{
  'update:level': [value: number]
}>()
</script>

<template>
  <TranscriptManagedPreview
    :level="level"
    :all-label="allLabel"
    :collapse-label="collapseLabel"
    :empty-text="emptyText"
    :max-height="maxHeight"
    :more-label="moreLabel"
    :mode="mode"
    :step-chars="TEXT_PREVIEW_STEP_CHARS"
    :step-lines="TEXT_PREVIEW_STEP_LINES"
    :text="text"
    :truncated-label="truncatedLabel"
    @update:level="emit('update:level', $event)"
  />
</template>
