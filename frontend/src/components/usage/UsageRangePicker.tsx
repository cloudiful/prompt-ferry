import { useEffect, useState } from 'react'
import { Calendar } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useLocale } from '@/lib/i18n'
import type { UsageRange } from '@/lib/usage-model'

export type UsageRangeApply = {
  range: UsageRange
  start?: string
  end?: string
}

export const USAGE_RANGE_PRESETS: readonly UsageRange[] = [
  'month',
  '24h',
  '7d',
  '30d',
  'custom',
]

const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/

function dayParts(day: string): [number, number, number] | null {
  if (!ISO_DAY.test(day)) return null
  const [year, month, date] = day.split('-').map(Number)
  const utc = new Date(Date.UTC(year, month - 1, date))
  // Reject a day the calendar would normalize, such as 2026-02-30.
  if (
    utc.getUTCFullYear() !== year ||
    utc.getUTCMonth() !== month - 1 ||
    utc.getUTCDate() !== date
  ) {
    return null
  }
  return [year, month, date]
}

export function isUsageDay(value: string | null | undefined): boolean {
  return typeof value === 'string' && dayParts(value) !== null
}

function formatUtcDay(date: Date): string {
  const year = String(date.getUTCFullYear()).padStart(4, '0')
  const month = String(date.getUTCMonth() + 1).padStart(2, '0')
  const day = String(date.getUTCDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

function shiftDay(day: string, days: number): string {
  const parts = dayParts(day)
  if (!parts) return day
  const [year, month, date] = parts
  return formatUtcDay(new Date(Date.UTC(year, month - 1, date + days)))
}

/** The stored calendar day of a UTC range bound. */
export function usageRangeDay(value: string | null | undefined): string | null {
  if (!value) return null
  const day = value.slice(0, 10)
  return isUsageDay(day) ? day : null
}

/** The visible last day: the stored end is the exclusive next-day bound. */
export function usageRangeVisibleEnd(
  value: string | null | undefined,
): string | null {
  const day = usageRangeDay(value)
  return day ? shiftDay(day, -1) : null
}

/**
 * The half-open UTC window a pair of selected days stands for.
 *
 * Only two real calendar days produce a window; `parseDay` rejects a normalized
 * impossible date and the arithmetic stays in UTC, so a DST transition or a
 * leap February never moves a bound.
 */
export function usageCustomRangeInput(
  startDay: string | null | undefined,
  endDay: string | null | undefined,
): UsageRangeApply | null {
  if (!startDay || !endDay) return null
  if (!isUsageDay(startDay) || !isUsageDay(endDay)) return null
  if (startDay > endDay) return null
  return {
    range: 'custom',
    start: `${startDay}T00:00:00.000Z`,
    end: `${shiftDay(endDay, 1)}T00:00:00.000Z`,
  }
}

/** True when two custom windows cover the same half-open interval. */
export function usageRangeEquals(
  a: Pick<UsageRangeApply, 'start' | 'end'>,
  b: Pick<UsageRangeApply, 'start' | 'end'>,
): boolean {
  return (a.start ?? '') === (b.start ?? '') && (a.end ?? '') === (b.end ?? '')
}

/** The label of a preset range, or the stored custom window. */
export function usageRangeLabel(
  range: UsageRange,
  start: string,
  end: string,
  labels: Record<UsageRange, string>,
): string {
  if (range === 'custom') {
    const first = usageRangeDay(start)
    const last = usageRangeVisibleEnd(end)
    if (first && last) return `${first} – ${last}`
  }
  return labels[range]
}

type UsageRangePickerProps = {
  range: UsageRange
  start: string
  end: string
  onApply: (input: UsageRangeApply) => void
}

/**
 * The time-range control: five presets plus a native date window.
 *
 * A preset applies immediately and carries no bounds. The custom window applies
 * only when both ends are real calendar days; an unchanged window reports
 * nothing. The arithmetic stays in plain UTC dates, so the control never needs
 * the transitive calendar dependency the Vue picker uses.
 */
export function UsageRangePicker({
  range,
  start,
  end,
  onApply,
}: UsageRangePickerProps) {
  const { t } = useLocale()
  const [open, setOpen] = useState(false)
  const [draftStart, setDraftStart] = useState(() => usageRangeDay(start) ?? '')
  const [draftEnd, setDraftEnd] = useState(
    () => usageRangeVisibleEnd(end) ?? '',
  )

  useEffect(() => {
    setDraftStart(usageRangeDay(start) ?? '')
    setDraftEnd(usageRangeVisibleEnd(end) ?? '')
  }, [start, end, range])

  const labels: Record<UsageRange, string> = {
    '24h': t('last24Hours'),
    '30d': t('last30Days'),
    '7d': t('last7Days'),
    custom: t('customRange'),
    month: t('thisMonth'),
  }
  const triggerLabel = usageRangeLabel(range, start, end, labels)

  function choosePreset(next: UsageRange): void {
    if (next === 'custom') return
    setOpen(false)
    onApply({ range: next })
  }

  function changeDraft(nextStart: string, nextEnd: string): void {
    setDraftStart(nextStart)
    setDraftEnd(nextEnd)
    const pending = usageCustomRangeInput(nextStart, nextEnd)
    // A half-filled or reversed window has no meaning and reports nothing.
    if (!pending) return
    setOpen(false)
    if (range === 'custom' && usageRangeEquals(pending, { start, end })) return
    onApply(pending)
  }

  return (
    <Popover onOpenChange={setOpen} open={open}>
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
      <PopoverContent align="end" className="grid gap-3">
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
          <div className="flex items-center gap-2">
            <Input
              aria-label={`${t('customRange')} 1`}
              className="w-36"
              onChange={(event) => changeDraft(event.target.value, draftEnd)}
              type="date"
              value={draftStart}
            />
            <span aria-hidden="true" className="text-muted-foreground">
              –
            </span>
            <Input
              aria-label={`${t('customRange')} 2`}
              className="w-36"
              onChange={(event) => changeDraft(draftStart, event.target.value)}
              type="date"
              value={draftEnd}
            />
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
