<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  value: unknown
}>()

const TOKEN_REGEX =
  /("(?:\\.|[^"\\])*")(\s*:)?|\btrue\b|\bfalse\b|\bnull\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g

const jsonText = computed(
  () => JSON.stringify(props.value ?? null, null, 2) ?? 'null',
)

const lines = computed(() =>
  jsonText.value.split('\n').map((line, index) => ({
    number: index + 1,
    html: highlightJsonLine(line),
  })),
)

function highlightJsonLine(line: string): string {
  let html = ''
  let lastIndex = 0
  TOKEN_REGEX.lastIndex = 0

  for (
    let match = TOKEN_REGEX.exec(line);
    match;
    match = TOKEN_REGEX.exec(line)
  ) {
    html += escapeHtml(line.slice(lastIndex, match.index))
    const token = match[0]
    const isString = token.startsWith('"')
    const isKey = isString && Boolean(match[2])
    const isBoolean = token === 'true' || token === 'false'
    const className = isKey
      ? 'ms-json-key'
      : isString
        ? 'ms-json-string'
        : token === 'null'
          ? 'ms-json-null'
          : isBoolean
            ? 'ms-json-boolean'
            : 'ms-json-number'
    html += `<span class="${className}">${escapeHtml(token)}</span>`
    lastIndex = match.index + token.length
  }

  html += escapeHtml(line.slice(lastIndex))
  return html || '&nbsp;'
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}
</script>

<template>
  <div
    class="overflow-auto rounded-md border border-default bg-[#0a0d12] px-3 py-2.5 font-mono text-[0.78rem] leading-6 text-[#d7e0ea]"
  >
    <div
      v-for="line in lines"
      :key="line.number"
      class="grid grid-cols-[2rem_minmax(0,1fr)] gap-4"
    >
      <div class="text-right text-[#5f6b7a] select-none">
        {{ line.number }}
      </div>
      <code class="block whitespace-pre-wrap break-words" v-html="line.html" />
    </div>
  </div>
</template>

<style scoped>
:deep(.ms-json-key) {
  color: #7dd3fc;
}

:deep(.ms-json-string) {
  color: #86efac;
}

:deep(.ms-json-number) {
  color: #c4b5fd;
}

:deep(.ms-json-boolean) {
  color: #f9a8d4;
}

:deep(.ms-json-null) {
  color: #fda4af;
}
</style>
