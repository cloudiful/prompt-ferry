<script setup lang="ts">
import MarkdownLog from '@/components/shared/MarkdownLog.vue'
import PreviewExpansionActions from '@/components/shared/PreviewExpansionActions.vue'

defineProps<{
  allLabel: string
  collapseLabel: string
  emptyText: string
  expanded: boolean
  hasMore: boolean
  maxHeight: string
  moreLabel: string
  mode: 'json' | 'markdown'
  text: string
  truncatedLabel: string
}>()

defineEmits<{
  all: []
  collapse: []
  more: []
}>()
</script>

<template>
  <div class="grid gap-2">
    <pre
      v-if="mode === 'json'"
      class="ms-code ms-code-flat overflow-auto"
      :style="{ maxHeight }"
      >{{ text }}</pre>
    <MarkdownLog
      v-else
      :text="text"
      :empty-text="emptyText"
      :max-height="maxHeight"
    />
    <PreviewExpansionActions
      :all-label="allLabel"
      :collapse-label="collapseLabel"
      :expanded="expanded"
      :has-more="hasMore"
      :more-label="moreLabel"
      :truncated-label="truncatedLabel"
      @all="$emit('all')"
      @collapse="$emit('collapse')"
      @more="$emit('more')"
    />
  </div>
</template>
