import { useState, type CSSProperties } from 'react'
import { DayPicker, TZDate, type DateRange } from '@daypicker/react'
import { enUS, zhCN } from '@daypicker/react/locale'
import '@daypicker/react/style.css'
import { Calendar } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useLocale } from '@/lib/i18n'
import {
  usageInstantToWallTime,
  usageWallTimeToInstant,
  type LocalUsageRange as UsageRange,
} from '@/lib/usage-range'
import { useDisplayTimeZone } from '@/lib/display-timezone'

export type UsageRangeApply = {
  range: UsageRange
  start?: string
  end?: string
}

export const USAGE_RANGE_PRESETS: readonly UsageRange[] = [
  '1h',
  '5h',
  '24h',
  '7d',
  '30d',
  'month',
  'custom',
]

type UsageRangePickerProps = {
  range: UsageRange
  start: string
  end: string
  onApply: (input: UsageRangeApply) => void
}

export function UsageRangePicker({
  range,
  start,
  end,
  onApply,
}: UsageRangePickerProps) {
  const { t, locale } = useLocale()
  const { effective: timeZone } = useDisplayTimeZone()
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<DateRange | undefined>()
  const [startTime, setStartTime] = useState('00:00')
  const [endTime, setEndTime] = useState('00:00')
  const [invalid, setInvalid] = useState(false)

  function openChange(next: boolean): void {
    if (next) {
      const from =
        range === 'custom' ? usageInstantToWallTime(start, timeZone) : null
      const to =
        range === 'custom' ? usageInstantToWallTime(end, timeZone) : null
      const toDate = (day: string) => {
        const [year, month, date] = day.split('-').map(Number)
        return TZDate.tz(timeZone, year!, month! - 1, date!, 12)
      }
      setSelected(
        from && to ? { from: toDate(from.day), to: toDate(to.day) } : undefined,
      )
      setStartTime(from?.time ?? '00:00')
      setEndTime(to?.time ?? '00:00')
      setInvalid(false)
    }
    setOpen(next)
  }

  const labels: Record<UsageRange, string> = {
    '1h': t('last1Hour'),
    '5h': t('last5Hours'),
    '24h': t('last24Hours'),
    '30d': t('last30Days'),
    '7d': t('last7Days'),
    custom: t('customRange'),
    month: t('thisMonth'),
  }
  const displayedStart = usageInstantToWallTime(start, timeZone)
  const displayedEnd = usageInstantToWallTime(end, timeZone)
  const triggerLabel =
    range === 'custom' && displayedStart && displayedEnd
      ? `${displayedStart.day} ${displayedStart.time} – ${displayedEnd.day} ${displayedEnd.time}`
      : labels[range]

  function choosePreset(next: UsageRange): void {
    if (next === 'custom') return
    setOpen(false)
    onApply({ range: next })
  }

  function applyCustom(): void {
    const day = (date: Date | undefined) =>
      date
        ? usageInstantToWallTime(date.toISOString(), timeZone)?.day
        : undefined
    const first = day(selected?.from)
    const last = day(selected?.to)
    const firstInstant = first
      ? usageWallTimeToInstant(first, startTime, timeZone)
      : null
    const lastInstant = last
      ? usageWallTimeToInstant(last, endTime, timeZone)
      : null
    if (
      !firstInstant ||
      !lastInstant ||
      Date.parse(firstInstant) >= Date.parse(lastInstant)
    ) {
      setInvalid(true)
      return
    }
    setInvalid(false)
    setOpen(false)
    if (range === 'custom' && firstInstant === start && lastInstant === end)
      return
    onApply({ range: 'custom', start: firstInstant, end: lastInstant })
  }

  return (
    <Popover onOpenChange={openChange} open={open}>
      <PopoverTrigger asChild>
        <Button
          aria-label={t('timeRange')}
          size="sm"
          type="button"
          variant="outline"
        >
          <Calendar aria-hidden="true" />
          <span className="max-w-40 truncate">{triggerLabel}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="grid max-h-[min(85vh,50rem)] w-[min(24rem,calc(100vw-2rem))] gap-3 overflow-y-auto"
      >
        <div className="grid gap-1">
          {USAGE_RANGE_PRESETS.map((preset) => (
            <Button
              aria-pressed={range === preset}
              className="justify-start"
              key={preset}
              onClick={() => choosePreset(preset)}
              size="sm"
              type="button"
              variant={range === preset ? 'secondary' : 'ghost'}
            >
              {labels[preset]}
            </Button>
          ))}
        </div>
        <div
          aria-label={t('customRange')}
          className="grid gap-2 border-t pt-3"
          role="group"
        >
          <div className="text-sm text-muted-foreground">
            {t('rangeTimeZone', { zone: timeZone })}
          </div>
          <DayPicker
            mode="range"
            locale={locale === 'zh-CN' ? zhCN : enUS}
            timeZone={timeZone}
            selected={selected}
            defaultMonth={selected?.from}
            onSelect={(value) => {
              setSelected(value)
              setInvalid(false)
            }}
            style={
              {
                '--rdp-accent-color': 'var(--primary)',
                '--rdp-accent-background-color': 'var(--accent)',
              } as CSSProperties
            }
          />
          <div className="grid grid-cols-2 gap-2">
            <label className="grid min-w-0 gap-1 text-sm">
              {t('startTime')}
              <Input
                type="time"
                value={startTime}
                onChange={(event) => {
                  setStartTime(event.target.value)
                  setInvalid(false)
                }}
              />
            </label>
            <label className="grid min-w-0 gap-1 text-sm">
              {t('endTime')}
              <Input
                type="time"
                value={endTime}
                onChange={(event) => {
                  setEndTime(event.target.value)
                  setInvalid(false)
                }}
              />
            </label>
          </div>
          {invalid && (
            <div role="alert" className="text-sm text-destructive">
              {t('invalidTimeRange')}
            </div>
          )}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setOpen(false)}
            >
              {t('cancel')}
            </Button>
            <Button type="button" size="sm" onClick={applyCustom}>
              {t('applyTimeRange')}
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
