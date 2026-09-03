<script setup lang="ts">
import { computed } from 'vue'
import { localeOptions, type Locale } from '@/i18n'
import type { ThemeMode } from '@/theme/appTheme'
import SettingsCard from './SettingsCard.vue'

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
  <section class="grid gap-3">
    <div class="grid items-start gap-3 sm:grid-cols-2">
      <SettingsCard>
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon name="i-lucide-languages" class="h-3.5 w-3.5 text-muted" />
            {{ t('language') }}
          </h3>
        </template>
        <label class="grid gap-1" for="settings-locale">
          <span class="text-xs text-muted">{{ t('language') }}</span>
          <USelect
            v-model="locale"
            id="settings-locale"
            class="w-full"
            name="settings-locale"
            size="sm"
            :items="localeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setLocale', $event)"
          />
        </label>
      </SettingsCard>

      <SettingsCard>
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon name="i-lucide-palette" class="h-3.5 w-3.5 text-muted" />
            {{ t('theme') }}
          </h3>
        </template>
        <label class="grid gap-1" for="settings-theme-mode">
          <span class="text-xs text-muted">{{ t('theme') }}</span>
          <USelect
            v-model="themeMode"
            id="settings-theme-mode"
            class="w-full"
            name="settings-theme-mode"
            size="sm"
            :items="themeModeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setThemeMode', $event)"
          />
        </label>
      </SettingsCard>
    </div>
  </section>
</template>
