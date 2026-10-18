import { expect, test } from 'bun:test'

import {
  formatAppDateTime,
  formatAppNumber,
  formatDateTime,
  formatNumber,
} from '../src/format/datetime'
import { locale } from '../src/composables/useLocale'

const SAMPLE = '2026-03-05T12:30:00Z'

test('formatDateTime renders the requested app locale, not the host locale', () => {
  const zh = formatDateTime(SAMPLE, 'zh-CN', {
    day: '2-digit',
    month: '2-digit',
    timeZone: 'UTC',
    year: 'numeric',
  })
  const en = formatDateTime(SAMPLE, 'en-US', {
    day: '2-digit',
    month: '2-digit',
    timeZone: 'UTC',
    year: 'numeric',
  })

  expect(zh).toBe('2026/03/05')
  expect(en).toBe('03/05/2026')
})

test('formatDateTime keeps an explicit UTC timezone', () => {
  expect(
    formatDateTime(SAMPLE, 'en-US', {
      hour: '2-digit',
      hour12: false,
      minute: '2-digit',
      timeZone: 'UTC',
    }),
  ).toBe('12:30')
})

test('a timezone-only option keeps the date-and-time shape', () => {
  const rendered = formatDateTime(SAMPLE, 'en-US', { timeZone: 'UTC' })
  expect(rendered).toContain('2026')
  expect(rendered).toContain('12:30')
  expect(rendered).not.toBe('03/05/2026')
})

test('formatDateTime returns an honest fallback for empty or invalid values', () => {
  expect(formatDateTime(null, 'zh-CN')).toBe('-')
  expect(formatDateTime(undefined, 'en-US')).toBe('-')
  expect(formatDateTime('', 'zh-CN')).toBe('-')
  expect(formatDateTime('not-a-date', 'zh-CN')).toBe('not-a-date')
})

test('formatAppDateTime follows live locale changes', () => {
  const previous = locale.value
  try {
    locale.value = 'zh-CN'
    expect(
      formatAppDateTime(SAMPLE, {
        day: '2-digit',
        month: '2-digit',
        timeZone: 'UTC',
        year: 'numeric',
      }),
    ).toBe('2026/03/05')

    locale.value = 'en-US'
    expect(
      formatAppDateTime(SAMPLE, {
        day: '2-digit',
        month: '2-digit',
        timeZone: 'UTC',
        year: 'numeric',
      }),
    ).toBe('03/05/2026')
  } finally {
    locale.value = previous
  }
})

test('number formatting follows the app locale', () => {
  expect(formatNumber(1234567.5, 'zh-CN')).toBe(
    formatNumber(1234567.5, 'en-US'),
  )
  expect(formatAppNumber(1234567.5)).toBe('1,234,567.5')
  expect(formatNumber(Number.NaN, 'zh-CN')).toBe('-')
})
