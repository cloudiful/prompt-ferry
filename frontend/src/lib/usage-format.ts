import {
  formatDateOnly,
  formatDateTime,
  formatNumber,
  type DateTimeInput,
} from '@/format/datetime-core'
import type { Locale } from '@/i18n'
import { formatUsageTokenQuantity } from './usage-model'

/**
 * A localized count, or `-` when the value is unknown.
 *
 * An exact `0` stays `0`; only a missing measurement becomes a dash.
 */
export function formatUsageCount(
  value: number | null | undefined,
  locale: Locale,
): string {
  if (value == null || !Number.isFinite(value)) return '-'
  return formatNumber(value, locale)
}

/** A ratio rendered as a percentage with one decimal, or `-`. */
export function formatUsagePercent(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '-'
  return `${Math.round(value * 1000) / 10}%`
}

/** A duration in milliseconds, compacted to `ms`, `s` or `m`, or `-`. */
export function formatUsageMs(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '-'
  const ms = Math.max(0, value)
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) {
    const seconds = ms / 1000
    return `${seconds >= 10 ? seconds.toFixed(0) : seconds.toFixed(1)}s`
  }
  const totalSeconds = Math.round(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return seconds === 0 ? `${minutes}m` : `${minutes}m ${seconds}s`
}

/**
 * An output rate in tokens per second.
 *
 * A missing, non-finite or non-positive rate is `-`: a completed request that
 * produced no output has no meaningful speed.
 */
export function formatUsageTokensPerSecond(
  value: number | null | undefined,
): string {
  if (value == null || !Number.isFinite(value) || value <= 0) return '-'
  const rounded = value >= 100 ? value.toFixed(0) : value.toFixed(1)
  return `${rounded} token/s`
}

export { formatUsageTokenQuantity }

/** An instant in the explicit display time zone. */
export function formatUsageInstant(
  value: DateTimeInput,
  locale: Locale,
  timeZone: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  return formatDateTime(value, locale, options, timeZone)
}

/** A calendar date in the explicit display time zone. */
export function formatUsageDate(
  value: DateTimeInput,
  locale: Locale,
  timeZone: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  return formatDateOnly(value, locale, options, timeZone)
}

/** A logical calendar date that must not shift with the display time zone. */
export function formatUsageUtcDate(
  value: DateTimeInput,
  locale: Locale,
  options?: Intl.DateTimeFormatOptions,
): string {
  return formatDateOnly(value, locale, { timeZone: 'UTC', ...options })
}
