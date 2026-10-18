import type { Locale } from '../i18n'
import { locale } from '../composables/useLocale'

export type DateTimeInput = string | number | Date | null | undefined

const dateTimeFormatters = new Map<string, Intl.DateTimeFormat>()
const numberFormatters = new Map<string, Intl.NumberFormat>()

// Match the date-and-time shape of Date#toLocaleString, which these displays
// replaced, instead of Intl's date-only default.
const DEFAULT_DATE_TIME_OPTIONS: Intl.DateTimeFormatOptions = {
  day: 'numeric',
  hour: 'numeric',
  minute: 'numeric',
  month: 'numeric',
  second: 'numeric',
  year: 'numeric',
}

const SHAPE_KEYS = [
  'dateStyle',
  'timeStyle',
  'year',
  'month',
  'day',
  'hour',
  'minute',
  'second',
  'weekday',
  'era',
  'fractionalSecondDigits',
  'timeZoneName',
] as const

function resolvedOptions(
  options?: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormatOptions {
  if (!options) return DEFAULT_DATE_TIME_OPTIONS
  // A caller may pass only modifiers such as `timeZone`; those keep the
  // date-and-time shape, while any field that selects the shape wins.
  const definesShape = SHAPE_KEYS.some((key) => key in options)
  return definesShape ? options : { ...DEFAULT_DATE_TIME_OPTIONS, ...options }
}

function dateTimeFormatter(
  localeValue: Locale,
  options?: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormat {
  const resolved = resolvedOptions(options)
  const key = `${localeValue}:${JSON.stringify(resolved)}`
  const cached = dateTimeFormatters.get(key)
  if (cached) return cached
  const formatter = new Intl.DateTimeFormat(localeValue, resolved)
  dateTimeFormatters.set(key, formatter)
  return formatter
}

function numberFormatter(
  localeValue: Locale,
  options?: Intl.NumberFormatOptions,
): Intl.NumberFormat {
  const key = `${localeValue}:${JSON.stringify(options ?? {})}`
  const cached = numberFormatters.get(key)
  if (cached) return cached
  const formatter = new Intl.NumberFormat(localeValue, options)
  numberFormatters.set(key, formatter)
  return formatter
}

function toDate(value: DateTimeInput): Date | null {
  if (value == null || value === '') return null
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? null : value
  }
  if (typeof value === 'number' && !Number.isFinite(value)) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date
}

export function formatDateTime(
  value: DateTimeInput,
  localeValue: Locale,
  options?: Intl.DateTimeFormatOptions,
): string {
  if (value == null || value === '') return '-'
  const date = toDate(value)
  if (!date) return typeof value === 'string' ? value : '-'
  return dateTimeFormatter(localeValue, options).format(date)
}

export function formatAppDateTime(
  value: DateTimeInput,
  options?: Intl.DateTimeFormatOptions,
): string {
  return formatDateTime(value, locale.value, options)
}

export function formatNumber(
  value: number,
  localeValue: Locale,
  options?: Intl.NumberFormatOptions,
): string {
  if (!Number.isFinite(value)) return '-'
  return numberFormatter(localeValue, options).format(value)
}

export function formatAppNumber(
  value: number,
  options?: Intl.NumberFormatOptions,
): string {
  return formatNumber(value, locale.value, options)
}
