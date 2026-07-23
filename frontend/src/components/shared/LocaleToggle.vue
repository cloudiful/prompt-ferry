<script setup lang="ts">
import { locale, setLocale } from '@/composables/useLocale'
import type { Locale } from '@/i18n'

const options: Array<{ label: string; title: string; value: Locale }> = [
  { label: '中', title: '中文', value: 'zh-CN' },
  { label: 'EN', title: 'English', value: 'en-US' },
]
</script>

<template>
  <div
    class="inline-flex items-center gap-px rounded-full border border-default bg-muted p-px"
    role="group"
    aria-label="Language"
  >
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      class="min-w-8 cursor-pointer rounded-full border-0 bg-transparent px-2.5 py-1.5 text-[0.72rem] leading-none font-bold text-muted"
      :class="locale === option.value ? 'bg-default text-primary' : ''"
      :title="option.title"
      @click="setLocale(option.value)"
    >
      {{ option.label }}
    </button>
  </div>
</template>
