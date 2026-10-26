import type { Locale } from '@/i18n'

export type DateTimeInput = string | number | Date | null | undefined

const dateTimeFormatters = new Map<string, Intl.DateTimeFormat>()
const dateOnlyFormatters = new Map<string, Intl.DateTimeFormat>()
const numberFormatters = new Map<string, Intl.NumberFormat>()

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

function withTimeZone(
  options: Intl.DateTimeFormatOptions,
  timeZone?: string,
): Intl.DateTimeFormatOptions {
  if ('timeZone' in options || timeZone === undefined) return options
  return { ...options, timeZone }
}

function resolvedOptions(
  options?: Intl.DateTimeFormatOptions,
  timeZone?: string,
): Intl.DateTimeFormatOptions {
  const base = options ?? DEFAULT_DATE_TIME_OPTIONS
  const definesShape = SHAPE_KEYS.some((key) => key in base)
  const resolved = definesShape
    ? base
    : { ...DEFAULT_DATE_TIME_OPTIONS, ...base }
  return withTimeZone(resolved, timeZone)
}

function dateTimeFormatter(
  locale: Locale,
  options?: Intl.DateTimeFormatOptions,
  timeZone?: string,
): Intl.DateTimeFormat {
  const resolved = resolvedOptions(options, timeZone)
  const key = `${locale}:${JSON.stringify(resolved)}`
  const cached = dateTimeFormatters.get(key)
  if (cached) return cached
  const formatter = new Intl.DateTimeFormat(locale, resolved)
  dateTimeFormatters.set(key, formatter)
  return formatter
}

function numberFormatter(
  locale: Locale,
  options?: Intl.NumberFormatOptions,
): Intl.NumberFormat {
  const key = `${locale}:${JSON.stringify(options ?? {})}`
  const cached = numberFormatters.get(key)
  if (cached) return cached
  const formatter = new Intl.NumberFormat(locale, options)
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
  locale: Locale,
  options?: Intl.DateTimeFormatOptions,
  timeZone?: string,
): string {
  if (value == null || value === '') return '-'
  const date = toDate(value)
  if (!date) return typeof value === 'string' ? value : '-'
  return dateTimeFormatter(locale, options, timeZone).format(date)
}

export function formatDateOnly(
  value: DateTimeInput,
  locale: Locale,
  options?: Intl.DateTimeFormatOptions,
  timeZone?: string,
): string {
  if (value == null || value === '') return '-'
  const date = toDate(value)
  if (!date) return typeof value === 'string' ? value : '-'
  const resolved = withTimeZone(
    {
      ...(options ?? {}),
      day: '2-digit',
      month: '2-digit',
      year: 'numeric',
    },
    timeZone,
  )
  const key = `${locale}:${JSON.stringify(resolved)}`
  let formatter = dateOnlyFormatters.get(key)
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, resolved)
    dateOnlyFormatters.set(key, formatter)
  }
  const parts = formatter.formatToParts(date)
  const part = (type: Intl.DateTimeFormatPartTypes): string =>
    parts.find((entry) => entry.type === type)?.value ?? ''
  return `${part('year')}-${part('month')}-${part('day')}`
}

export function formatNumber(
  value: number,
  locale: Locale,
  options?: Intl.NumberFormatOptions,
): string {
  if (!Number.isFinite(value)) return '-'
  return numberFormatter(locale, options).format(value)
}
