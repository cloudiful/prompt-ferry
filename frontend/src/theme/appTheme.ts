import { computed, ref, watch } from 'vue'
import { readStorage, THEME_MODE_STORAGE_KEY, writeStorage } from '@/storage'

export type ThemeMode = 'dark' | 'light'

const DEFAULT_THEME_MODE: ThemeMode = 'dark'

/**
 * The single rule that turns a persisted value into a theme mode. The
 * synchronous first-paint script in index.html mirrors it verbatim, because it
 * cannot import a module; the regression test asserts the two agree.
 */
export function resolveThemeMode(stored: string | null): ThemeMode {
  return stored === 'light' ? 'light' : DEFAULT_THEME_MODE
}

/**
 * The DOM state the first-paint script and the runtime must agree on: the
 * `dark` class the theme tokens key off, plus the browser color-scheme.
 */
export function applyThemeToDocument(mode: ThemeMode): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', mode === 'dark')
  root.style.colorScheme = mode
}

export const themeMode = ref<ThemeMode>(
  resolveThemeMode(readStorage(THEME_MODE_STORAGE_KEY)),
)

watch(themeMode, (value) => {
  applyThemeToDocument(value)
  writeStorage(THEME_MODE_STORAGE_KEY, value)
})

/**
 * Startup sync only. It re-applies the mode resolved from storage, which is
 * the same state the inline first-paint script already set, and deliberately
 * does not write storage: module initialization must never rewrite or undo a
 * correct first paint.
 */
export function initTheme(): void {
  applyThemeToDocument(themeMode.value)
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
