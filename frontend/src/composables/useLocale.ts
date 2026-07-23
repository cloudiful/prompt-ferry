import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { normalizeLocale, type Locale } from '../i18n'
import { i18n } from '../i18n/plugin'
import { LOCALE_STORAGE_KEY, readStorage, writeStorage } from '../storage'

export const locale = ref<Locale>(
  normalizeLocale(readStorage(LOCALE_STORAGE_KEY)),
)

i18n.global.locale.value = locale.value

watch(
  locale,
  (value) => {
    i18n.global.locale.value = value
    writeStorage(LOCALE_STORAGE_KEY, value)
  },
  { immediate: true },
)

export function useLocale() {
  const composer = useI18n({ useScope: 'global' })

  return {
    locale: computed<Locale>({
      get: () => composer.locale.value as Locale,
      set: (value) => {
        locale.value = value
      },
    }),
    t: composer.t,
  }
}

export function setLocale(next: Locale): void {
  locale.value = next
}
