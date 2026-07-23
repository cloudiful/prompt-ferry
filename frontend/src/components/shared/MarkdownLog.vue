<script setup lang="ts">
import MarkdownIt from 'markdown-it'
import { computed } from 'vue'

const props = defineProps<{
  text?: string | null
  emptyText: string
  maxHeight?: string
}>()

const markdown = new MarkdownIt({
  html: false,
  breaks: true,
  linkify: true,
})

const html = computed(() => markdown.render(props.text || props.emptyText))
const style = computed(() =>
  props.maxHeight ? { maxHeight: props.maxHeight } : undefined,
)
</script>

<template>
  <div
    class="ms-code ms-markdown-log"
    :class="{ 'ms-markdown-log-scroll': Boolean(maxHeight) }"
    :style="style"
    v-html="html"
  />
</template>
