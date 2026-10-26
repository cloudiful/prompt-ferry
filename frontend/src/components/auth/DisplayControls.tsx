import { Moon, Sun } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import { useThemeMode } from '@/lib/theme'

export function LocaleToggle() {
  const { locale, setLocale, t } = useLocale()
  const nextLocale = locale === 'zh-CN' ? 'en-US' : 'zh-CN'

  return (
    <Button
      aria-label={t('language')}
      onClick={() => setLocale(nextLocale)}
      size="sm"
      type="button"
      variant="outline"
    >
      {nextLocale === 'en-US' ? 'English' : '中文'}
    </Button>
  )
}

export function ThemeToggle() {
  const { mode, setMode } = useThemeMode()
  const { t } = useLocale()
  const nextMode = mode === 'dark' ? 'light' : 'dark'
  const Icon = mode === 'dark' ? Sun : Moon

  return (
    <Button
      aria-label={t(nextMode === 'dark' ? 'darkMode' : 'lightMode')}
      onClick={() => setMode(nextMode)}
      size="sm"
      type="button"
      variant="outline"
    >
      <Icon aria-hidden="true" />
      <span>{t(nextMode === 'dark' ? 'darkMode' : 'lightMode')}</span>
    </Button>
  )
}
