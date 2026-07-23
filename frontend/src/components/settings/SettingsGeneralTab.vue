<script setup lang="ts">
import { computed } from 'vue'
import { localeOptions, type Locale } from '@/i18n'
import type { ThemeMode } from '@/theme/appTheme'

const props = defineProps<{
  t: TranslateFn
}>()

const locale = defineModel<Locale>('locale', { required: true })
const themeMode = defineModel<ThemeMode>('themeMode', { required: true })

const themeModeOptions = computed<Array<{ label: string; value: ThemeMode }>>(
  () => [
    { label: props.t('darkMode'), value: 'dark' },
    { label: props.t('lightMode'), value: 'light' },
  ],
)

defineEmits<{
  setLocale: [locale: Locale]
  setThemeMode: [mode: ThemeMode]
}>()
</script>

<template>
  <section class="grid gap-4">
    <div class="grid items-start gap-3 md:grid-cols-2">
      <article
        class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-default bg-default px-4 py-3"
      >
        <h3
          class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          {{ t('language') }}
        </h3>
        <div
          class="w-[min(100%,11rem)] min-w-[9.5rem] flex-none max-[767px]:w-[min(100%,10rem)] max-[767px]:min-w-[8.75rem]"
        >
          <USelect
            v-model="locale"
            id="settings-locale"
            class="w-full sm:max-w-44"
            name="settings-locale"
            size="sm"
            :items="localeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setLocale', $event)"
          />
        </div>
      </article>

      <article
        class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-default bg-default px-4 py-3"
      >
        <h3
          class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          {{ t('theme') }}
        </h3>
        <div
          class="w-[min(100%,11rem)] min-w-[9.5rem] flex-none max-[767px]:w-[min(100%,10rem)] max-[767px]:min-w-[8.75rem]"
        >
          <USelect
            v-model="themeMode"
            id="settings-theme-mode"
            class="w-full sm:max-w-44"
            name="settings-theme-mode"
            size="sm"
            :items="themeModeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setThemeMode', $event)"
          />
        </div>
      </article>
    </div>
  </section>
</template>
