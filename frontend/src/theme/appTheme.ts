import { computed, ref, watch } from 'vue'
import { readStorage, THEME_MODE_STORAGE_KEY, writeStorage } from '@/storage'

export type ThemeMode = 'dark' | 'light'

const DEFAULT_THEME_MODE: ThemeMode = 'dark'

function normalizeThemeMode(value: string | null): ThemeMode {
  return value === 'light' ? 'light' : DEFAULT_THEME_MODE
}

export const themeMode = ref<ThemeMode>(
  normalizeThemeMode(readStorage(THEME_MODE_STORAGE_KEY)),
)

function applyTheme(next: ThemeMode): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', next === 'dark')
  root.style.colorScheme = next
}

watch(
  themeMode,
  (value) => {
    applyTheme(value)
    writeStorage(THEME_MODE_STORAGE_KEY, value)
  },
  { immediate: true },
)

export function initTheme(): void {
  applyTheme(themeMode.value)
}

export function useThemeMode() {
  return computed<ThemeMode>({
    get: () => themeMode.value,
    set: (value) => {
      themeMode.value = value
    },
  })
}

export function setThemeMode(next: ThemeMode): void {
  themeMode.value = next
}
