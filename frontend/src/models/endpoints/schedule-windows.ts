import type { TranslateFn } from '@/i18n'

// Issue #1045: a schedule row with both time fields empty means "all day on
// the selected weekdays". The API stores it as the canonical `00:00`-`24:00`
// window; only a partially filled row is invalid. The shared schedule editor,
// the endpoint dialog and the model-route target validation share these rules
// so a blank row never passes one save gate while blocking another.

export type ScheduleWindowRow = {
  start?: string | null
  end?: string | null
  days?: number[] | null
}

export type ScheduleWindowTimes = { start: string; end: string }

// Issue #457: `end` may additionally be `24:00` (exclusive midnight);
// `start` stays within `00:00-23:59`.
const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/
const HHMM_END_RE = /^(([01]\d|2[0-3]):[0-5]\d|24:00)$/

// The canonical all-day window the API accepts.
export const SCHEDULE_ALL_DAY_START = '00:00'
export const SCHEDULE_ALL_DAY_END = '24:00'

export function isBlankScheduleRow(
  row: ScheduleWindowRow | null | undefined,
): boolean {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  return start === '' && end === ''
}

export function scheduleRowError(
  row: ScheduleWindowRow | null | undefined,
  t: TranslateFn,
): string {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  // A row with both times empty is the all-day form of that row, not an error.
  if (start === '' && end === '') return ''
  if (start === '' || end === '') return t('scheduleRequired')
  if (!HHMM_RE.test(start) || !HHMM_END_RE.test(end))
    return t('scheduleInvalid')
  if (start === end) return t('scheduleEqual')
  return ''
}

export function isScheduleRowValid(
  row: ScheduleWindowRow | null | undefined,
): boolean {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  if (start === '' && end === '') return true
  if (start === '' || end === '') return false
  if (!HHMM_RE.test(start) || !HHMM_END_RE.test(end)) return false
  return start !== end
}

// A blank row is serialized as the canonical all-day window; every other row
// keeps its own times, so a malformed value is never turned into all-day.
export function normalizeScheduleWindowTimes(
  row: ScheduleWindowRow | null | undefined,
): ScheduleWindowTimes {
  const start = (row?.start ?? '').trim()
  const end = (row?.end ?? '').trim()
  if (start === '' && end === '') {
    return { start: SCHEDULE_ALL_DAY_START, end: SCHEDULE_ALL_DAY_END }
  }
  return { start, end }
}

// The visible schedule rows. A blank row shows as the canonical all-day window
// so an explicit all-day override is never hidden; a partially filled row is
// invalid and stays out of the summary.
export function scheduleWindowsForDisplay(
  rows: ReadonlyArray<ScheduleWindowRow> | null | undefined,
): Array<{ start: string; end: string; days?: number[] }> {
  const display: Array<{ start: string; end: string; days?: number[] }> = []
  for (const row of rows ?? []) {
    const start = (row?.start ?? '').trim()
    const end = (row?.end ?? '').trim()
    if (start === '' && end === '') {
      display.push({
        start: SCHEDULE_ALL_DAY_START,
        end: SCHEDULE_ALL_DAY_END,
        ...(Array.isArray(row?.days) ? { days: [...row.days] } : {}),
      })
      continue
    }
    if (start === '' || end === '') continue
    display.push({
      start,
      end,
      ...(Array.isArray(row?.days) ? { days: [...row.days] } : {}),
    })
  }
  return display
}
