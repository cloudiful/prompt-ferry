import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { THEME_MODE_STORAGE_KEY, readStorage, writeStorage } from '@/storage'

export type ThemeMode = 'dark' | 'light'

type ThemeContextValue = {
  mode: ThemeMode
  setMode: (next: ThemeMode) => void
}

const ThemeContext = createContext<ThemeContextValue | null>(null)

export function resolveThemeMode(stored: string | null): ThemeMode {
  return stored === 'light' ? 'light' : 'dark'
}

export function applyThemeToDocument(mode: ThemeMode): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', mode === 'dark')
  root.style.colorScheme = mode
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(() =>
    resolveThemeMode(readStorage(THEME_MODE_STORAGE_KEY)),
  )

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next)
    applyThemeToDocument(next)
    writeStorage(THEME_MODE_STORAGE_KEY, next)
  }, [])

  useEffect(() => applyThemeToDocument(mode), [mode])

  const value = useMemo(() => ({ mode, setMode }), [mode, setMode])
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
}

export function useThemeMode(): ThemeContextValue {
  const value = useContext(ThemeContext)
  if (!value) throw new Error('useThemeMode must be used within ThemeProvider')
  return value
}
