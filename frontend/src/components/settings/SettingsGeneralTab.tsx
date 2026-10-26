import { Clock, Languages, Palette } from 'lucide-react'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SettingsCard } from '@/components/settings/SettingsCard'
import { SettingsTimeZoneSelect } from '@/components/settings/SettingsTimeZoneSelect'
import { localeOptions, type Locale } from '@/i18n'
import {
  AUTO_DISPLAY_TIME_ZONE,
  browserTimeZone,
  displayTimeZoneOptions,
} from '@/format/timezone-core'
import { useDisplayTimeZone, setDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { useThemeMode, type ThemeMode } from '@/lib/theme'

/**
 * The three local preferences: language, theme and display time zone.
 *
 * Each control is bound to the provider that already owns the value, so a
 * change applies immediately and persists through the existing storage keys
 * without a server write. A save or refresh control is deliberately absent:
 * these preferences have no server state and no pending state to report.
 */
export function SettingsGeneralTab() {
  const { locale, setLocale, t } = useLocale()
  const { mode, setMode } = useThemeMode()
  const timeZone = useDisplayTimeZone()

  const themeOptions: Array<{ label: string; value: ThemeMode }> = [
    { label: t('darkMode'), value: 'dark' },
    { label: t('lightMode'), value: 'light' },
  ]
  const zoneOptions = displayTimeZoneOptions(
    t('displayTimeZoneAuto', { zone: browserTimeZone() }),
  )

  return (
    <div className="grid min-w-0 gap-3 sm:grid-cols-2">
      <SettingsCard icon={Languages} title={t('language')}>
        <div className="grid min-w-0 gap-1">
          <Label className="sr-only" htmlFor="settings-locale">
            {t('language')}
          </Label>
          <Select
            onValueChange={(value) => setLocale(value as Locale)}
            value={locale}
          >
            <SelectTrigger
              aria-label={t('language')}
              className="w-full min-w-0"
              id="settings-locale"
              name="settings-locale"
              size="sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {localeOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsCard>

      <SettingsCard icon={Palette} title={t('theme')}>
        <div className="grid min-w-0 gap-1">
          <Label className="sr-only" htmlFor="settings-theme-mode">
            {t('theme')}
          </Label>
          <Select
            onValueChange={(value) => setMode(value as ThemeMode)}
            value={mode}
          >
            <SelectTrigger
              aria-label={t('theme')}
              className="w-full min-w-0"
              id="settings-theme-mode"
              name="settings-theme-mode"
              size="sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {themeOptions.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </SettingsCard>

      <SettingsCard icon={Clock} title={t('displayTimeZone')}>
        <div className="grid min-w-0 gap-1">
          <Label className="sr-only" htmlFor="settings-display-time-zone">
            {t('displayTimeZone')}
          </Label>
          <SettingsTimeZoneSelect
            autoValue={t('displayTimeZoneAuto', { zone: browserTimeZone() })}
            filterLabel={t('filter')}
            id="settings-display-time-zone"
            label={t('displayTimeZone')}
            onChange={(zone) =>
              setDisplayTimeZone(zone || AUTO_DISPLAY_TIME_ZONE)
            }
            options={zoneOptions}
            value={timeZone.selected}
          />
          <p className="m-0 text-xs text-muted-foreground">
            {t('displayTimeZoneEffect')}
          </p>
        </div>
      </SettingsCard>
    </div>
  )
}
