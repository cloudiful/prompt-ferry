import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { messages, normalizeLocale, type Locale, type MessageKey } from '@/i18n'
import { LOCALE_STORAGE_KEY, readStorage, writeStorage } from '@/storage'

export type MessageValues = Record<string, string | number>

export type Translate = (key: MessageKey, values?: MessageValues) => string

type LocaleContextValue = {
  locale: Locale
  setLocale: (next: Locale) => void
  t: Translate
}

const LocaleContext = createContext<LocaleContextValue | null>(null)

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() =>
    normalizeLocale(readStorage(LOCALE_STORAGE_KEY)),
  )

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next)
    writeStorage(LOCALE_STORAGE_KEY, next)
    if (typeof document !== 'undefined') document.documentElement.lang = next
  }, [])

  const t = useCallback<Translate>(
    (key, values) => {
      const message: string = messages[locale][key]
      if (!values) return message
      return message.replace(/\{([^}]+)\}/g, (match, name: string) =>
        Object.hasOwn(values, name) ? String(values[name]) : match,
      )
    },
    [locale],
  )

  useEffect(() => {
    if (typeof document !== 'undefined') document.documentElement.lang = locale
  }, [locale])

  const value = useMemo(
    () => ({ locale, setLocale, t }),
    [locale, setLocale, t],
  )
  return (
    <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>
  )
}

export function useLocale(): LocaleContextValue {
  const value = useContext(LocaleContext)
  if (!value) throw new Error('useLocale must be used within LocaleProvider')
  return value
}
