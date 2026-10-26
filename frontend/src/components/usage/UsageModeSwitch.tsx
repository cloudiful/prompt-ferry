import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import type { UsageMode } from '@/lib/usage-model'

type UsageModeSwitchProps = {
  value: UsageMode
  onChange: (mode: UsageMode) => void
}

/** The visible short label used when the localized name does not fit. */
export function usageModeShortLabel(label: string): string {
  return label.length > 4 ? label.slice(0, 2) : label
}

/**
 * The overview/records switch.
 *
 * The mode itself belongs to the store and the parent owns persistence, so the
 * switch only reports the chosen mode and issues no read. Each button keeps its
 * full localized accessible name while showing a short label on small screens.
 */
export function UsageModeSwitch({ value, onChange }: UsageModeSwitchProps) {
  const { t } = useLocale()
  const modes: { mode: UsageMode; label: string }[] = [
    { mode: 'overview', label: t('overviewMode') },
    { mode: 'records', label: t('recordsMode') },
  ]

  return (
    <div
      aria-label={`${t('currentSlice')}: ${t('overviewMode')} / ${t('recordsMode')}`}
      className="inline-flex items-center rounded-md border border-input p-0.5"
      role="group"
    >
      {modes.map((entry) => (
        <Button
          aria-label={entry.label}
          aria-pressed={value === entry.mode}
          key={entry.mode}
          onClick={() => onChange(entry.mode)}
          size="sm"
          type="button"
          variant={value === entry.mode ? 'secondary' : 'ghost'}
        >
          <span aria-hidden="true" className="sm:hidden">
            {usageModeShortLabel(entry.label)}
          </span>
          <span className="hidden sm:inline">{entry.label}</span>
        </Button>
      ))}
    </div>
  )
}
