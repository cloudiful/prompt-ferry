<script setup lang="ts">
import { computed, ref } from 'vue'

import TranscriptPreviewPane from '@/components/usage/detail/TranscriptPreviewPane.vue'
import {
  collapsePreview,
  previewText,
  showAllPreview,
  showMorePreview,
} from '@/composables/useTextPreview'

const props = withDefaults(
  defineProps<{
    allLabel: string
    collapseLabel: string
    emptyText: string
    level: number
    maxHeight: string
    mode: 'json' | 'markdown'
    moreLabel: string
    stepChars?: number
    stepLines?: number
    text: string
    truncatedLabel: string
  }>(),
  {
    stepChars: 12_000,
    stepLines: 160,
  },
)

const emit = defineEmits<{
  'update:level': [value: number]
}>()

const preview = computed(() =>
  previewText(props.text, props.level, props.stepChars, props.stepLines),
)

function setLevel(value: number): void {
  emit('update:level', value)
}

function handleShowAll(): void {
  const target = ref(props.level)
  showAllPreview(target)
  setLevel(target.value)
}

function handleShowMore(): void {
  const target = ref(props.level)
  showMorePreview(target)
  setLevel(target.value)
}

function handleCollapse(): void {
  const target = ref(props.level)
  collapsePreview(target)
  setLevel(target.value)
}
</script>

<template>
  <TranscriptPreviewPane
    :all-label="allLabel"
    :collapse-label="collapseLabel"
    :empty-text="emptyText"
    :expanded="level > 1"
    :has-more="preview.hasMore"
    :max-height="maxHeight"
    :more-label="moreLabel"
    :mode="mode"
    :text="preview.text"
    :truncated-label="truncatedLabel"
    @all="handleShowAll"
    @collapse="handleCollapse"
    @more="handleShowMore"
  />
</template>
