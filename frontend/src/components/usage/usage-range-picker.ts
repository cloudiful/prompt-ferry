import {
  parseDate,
  type CalendarDate,
  type DateValue,
} from '@internationalized/date'
import type { DateRange } from 'reka-ui'

export type UsageCalendarRange = DateRange

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/

/** Parse the date part of a stored ISO string/instant into a UTC calendar date. */
export function parseCalendarDate(value?: string | null): CalendarDate | null {
  if (!value) return null
  const text = value.slice(0, 10)
  if (!ISO_DATE.test(text)) return null
  try {
    return parseDate(text)
  } catch {
    return null
  }
}

/**
 * The visible range for a stored (start, end) pair; null when either side is
 * missing. Stored `end` is the exclusive next-day bound, so the visible last
 * day is one day earlier and `toCustomRangeInput` adds that day back.
 */
export function parseRange(
  start?: string | null,
  end?: string | null,
): UsageCalendarRange | null {
  const startDate = parseCalendarDate(start)
  const storedEnd = parseCalendarDate(end)
  if (!startDate || !storedEnd) return null
  return { start: startDate, end: storedEnd.subtract({ days: 1 }) }
}

/** UTC instant at the start of a calendar date (`00:00:00.000Z`). */
export function startOfDateIso(date: DateValue): string {
  return new Date(`${date.toString().slice(0, 10)}T00:00:00.000Z`).toISOString()
}

/** Exclusive UTC end of a range: midnight on the day after the last selected date. */
export function exclusiveEndIso(date: DateValue): string {
  return startOfDateIso(date.add({ days: 1 }))
}

/** True when a calendar range has both ends selected. */
export function isCompleteRange(
  range?: UsageCalendarRange | null,
): range is UsageCalendarRange & { start: DateValue; end: DateValue } {
  return Boolean(range?.start && range?.end)
}

/**
 * The custom-range apply payload for a complete calendar range: the API expects
 * a UTC half-open interval `[start, end)`.
 */
export function toCustomRangeInput(range: UsageCalendarRange): {
  range: 'custom'
  start: string
  end: string
} | null {
  if (!isCompleteRange(range)) return null
  return {
    range: 'custom',
    start: startOfDateIso(range.start),
    end: exclusiveEndIso(range.end),
  }
}

/** True when two calendar ranges cover the same two dates. */
export function rangesEqual(
  a?: UsageCalendarRange | null,
  b?: UsageCalendarRange | null,
): boolean {
  if (!isCompleteRange(a) || !isCompleteRange(b)) return false
  return (
    a.start.toString().slice(0, 10) === b.start.toString().slice(0, 10) &&
    a.end.toString().slice(0, 10) === b.end.toString().slice(0, 10)
  )
}

/**
 * The calendar selection a (preset, stored window) pair may display. A preset
 * owns its window, so an earlier custom selection must not stay highlighted
 * over it; a custom selection echoes the stored window so it round-trips.
 */
export function calendarRangeForValue(
  value: string,
  start?: string | null,
  end?: string | null,
): UsageCalendarRange | null {
  return value === 'custom' ? parseRange(start, end) : null
}

/** `YYYY-MM-DD – YYYY-MM-DD` label for a complete calendar range. */
export function formatRangeLabel(range?: UsageCalendarRange | null): string {
  if (!isCompleteRange(range)) return ''
  return `${range.start.toString().slice(0, 10)} – ${range.end.toString().slice(0, 10)}`
}
