import { Temporal } from '@js-temporal/polyfill'
import type { UsageRange } from '@/lib/usage-model'

export type LocalUsageRange = UsageRange | '1h' | '5h'

export function slidingUsageWindow(
  range: '1h' | '5h',
  now: number,
): { start: string; end: string } {
  const hours = range === '1h' ? 1 : 5
  return {
    start: new Date(now - hours * 60 * 60 * 1000).toISOString(),
    end: new Date(now).toISOString(),
  }
}

export function usageWallTimeToInstant(
  day: string,
  time: string,
  timeZone: string,
): string | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day) || !/^\d{2}:\d{2}$/.test(time))
    return null
  try {
    return Temporal.PlainDateTime.from(`${day}T${time}`)
      .toZonedDateTime(timeZone, { disambiguation: 'reject' })
      .toInstant()
      .toString({ fractionalSecondDigits: 3 })
  } catch {
    return null
  }
}

export function usageInstantToWallTime(
  instant: string,
  timeZone: string,
): { day: string; time: string } | null {
  try {
    const local = Temporal.Instant.from(instant).toZonedDateTimeISO(timeZone)
    return {
      day: local.toPlainDate().toString(),
      time: local.toPlainTime().toString({ smallestUnit: 'minute' }),
    }
  } catch {
    return null
  }
}
