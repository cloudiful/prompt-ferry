<script setup lang="ts">
import { computed } from 'vue'
import { localeOptions, type Locale } from '@/i18n'
import type { ThemeMode } from '@/theme/appTheme'
import {
  AUTO_DISPLAY_TIME_ZONE,
  browserTimeZone,
  displayTimeZoneOptions,
} from '@/format/timezone'
import SettingsCard from './SettingsCard.vue'

const props = defineProps<{
  t: TranslateFn
}>()

const locale = defineModel<Locale>('locale', { required: true })
const themeMode = defineModel<ThemeMode>('themeMode', { required: true })
const displayTimeZone = defineModel<string>('displayTimeZone', {
  required: true,
})

const themeModeOptions = computed<Array<{ label: string; value: ThemeMode }>>(
  () => [
    { label: props.t('darkMode'), value: 'dark' },
    { label: props.t('lightMode'), value: 'light' },
  ],
)

const timeZoneOptions = computed(() =>
  displayTimeZoneOptions(
    props.t('displayTimeZoneAuto', { zone: browserTimeZone() }),
  ),
)

defineEmits<{
  setLocale: [locale: Locale]
  setThemeMode: [mode: ThemeMode]
  setDisplayTimeZone: [zone: string]
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
        <div class="grid gap-1">
          <USelect
            v-model="locale"
            id="settings-locale"
            class="w-full"
            name="settings-locale"
            size="sm"
            :aria-label="t('language')"
            :items="localeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setLocale', $event)"
          />
        </div>
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
        <div class="grid gap-1">
          <USelect
            v-model="themeMode"
            id="settings-theme-mode"
            class="w-full"
            name="settings-theme-mode"
            size="sm"
            :aria-label="t('theme')"
            :items="themeModeOptions"
            label-key="label"
            value-key="value"
            @update:model-value="$emit('setThemeMode', $event)"
          />
        </div>
      </SettingsCard>

      <SettingsCard>
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon name="i-lucide-clock" class="h-3.5 w-3.5 text-muted" />
            {{ t('displayTimeZone') }}
          </h3>
        </template>
        <div class="grid gap-1">
          <USelectMenu
            v-model="displayTimeZone"
            id="settings-display-time-zone"
            class="w-full"
            name="settings-display-time-zone"
            size="sm"
            :aria-label="t('displayTimeZone')"
            :items="timeZoneOptions"
            label-key="label"
            value-key="value"
            @update:model-value="
              $emit('setDisplayTimeZone', $event ?? AUTO_DISPLAY_TIME_ZONE)
            "
          />
          <p class="m-0 text-xs text-dimmed">
            {{ t('displayTimeZoneEffect') }}
          </p>
        </div>
      </SettingsCard>
    </div>
  </section>
</template>
