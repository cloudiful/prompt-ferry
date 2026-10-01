import { expect, test } from 'bun:test'
import { parseDate } from '@internationalized/date'
import type { DateRange } from 'reka-ui'
import {
  exclusiveEndIso,
  formatRangeLabel,
  isCompleteRange,
  parseCalendarDate,
  parseRange,
  rangesEqual,
  startOfDateIso,
  toCustomRangeInput,
} from '../src/components/usage/usage-range-picker'

function range(start: string, end: string): DateRange {
  return { start: parseDate(start), end: parseDate(end) }
}

test('parseCalendarDate keeps only a valid leading ISO date', () => {
  expect(parseCalendarDate('2026-09-01')?.toString()).toBe('2026-09-01')
  expect(parseCalendarDate('2026-09-01T12:34:56.000Z')?.toString()).toBe(
    '2026-09-01',
  )
  expect(parseCalendarDate('')).toBeNull()
  expect(parseCalendarDate(undefined)).toBeNull()
  expect(parseCalendarDate('2026/09/01')).toBeNull()
  expect(parseCalendarDate('not-a-date')).toBeNull()
})

test('parseRange needs both stored ends', () => {
  const parsed = parseRange(
    '2026-09-01T00:00:00.000Z',
    '2026-10-01T00:00:00.000Z',
  )
  expect(parsed?.start?.toString()).toBe('2026-09-01')
  expect(parsed?.end?.toString()).toBe('2026-09-30')
  expect(parseRange('2026-09-01', '')).toBeNull()
  expect(parseRange('', '2026-10-01')).toBeNull()
})

test('parseRange turns the stored exclusive end into the visible last day', () => {
  const parsed = parseRange(
    '2026-01-01T00:00:00.000Z',
    '2026-02-01T00:00:00.000Z',
  )
  expect(parsed?.end?.toString()).toBe('2026-01-31')

  const singleDay = parseRange(
    '2026-03-31T00:00:00.000Z',
    '2026-04-01T00:00:00.000Z',
  )
  expect(singleDay?.start?.toString()).toBe('2026-03-31')
  expect(singleDay?.end?.toString()).toBe('2026-03-31')
})

test('the stored range round-trips through the visible range', () => {
  const stored = {
    range: 'custom' as const,
    start: '2026-01-01T00:00:00.000Z',
    end: '2026-02-01T00:00:00.000Z',
  }
  const visible = parseRange(stored.start, stored.end)

  expect(visible?.start?.toString()).toBe('2026-01-01')
  expect(visible?.end?.toString()).toBe('2026-01-31')
  expect(toCustomRangeInput(visible!)).toEqual(stored)
})

test('startOfDateIso anchors the range start at midnight UTC', () => {
  expect(startOfDateIso(parseDate('2026-09-01'))).toBe(
    '2026-09-01T00:00:00.000Z',
  )
})

test('exclusiveEndIso advances the last day across month and year boundaries', () => {
  expect(exclusiveEndIso(parseDate('2026-09-30'))).toBe(
    '2026-10-01T00:00:00.000Z',
  )
  expect(exclusiveEndIso(parseDate('2026-01-31'))).toBe(
    '2026-02-01T00:00:00.000Z',
  )
  expect(exclusiveEndIso(parseDate('2026-12-31'))).toBe(
    '2027-01-01T00:00:00.000Z',
  )
})

test('exclusiveEndIso follows leap-year February', () => {
  expect(exclusiveEndIso(parseDate('2028-02-28'))).toBe(
    '2028-02-29T00:00:00.000Z',
  )
  expect(exclusiveEndIso(parseDate('2027-02-28'))).toBe(
    '2027-03-01T00:00:00.000Z',
  )
})

test('toCustomRangeInput emits the UTC half-open custom payload', () => {
  expect(toCustomRangeInput(range('2026-09-01', '2026-09-30'))).toEqual({
    range: 'custom',
    start: '2026-09-01T00:00:00.000Z',
    end: '2026-10-01T00:00:00.000Z',
  })
})

test('toCustomRangeInput ignores an incomplete range', () => {
  expect(
    toCustomRangeInput({ start: parseDate('2026-09-01'), end: undefined }),
  ).toBeNull()
  expect(toCustomRangeInput({ start: undefined, end: undefined })).toBeNull()
})

test('isCompleteRange only accepts both ends', () => {
  expect(isCompleteRange(range('2026-09-01', '2026-09-30'))).toBe(true)
  expect(
    isCompleteRange({ start: parseDate('2026-09-01'), end: undefined }),
  ).toBe(false)
  expect(isCompleteRange(null)).toBe(false)
})

test('rangesEqual compares the two dates regardless of stored precision', () => {
  expect(
    rangesEqual(
      range('2026-09-01', '2026-09-30'),
      parseRange('2026-09-01T08:00:00.000Z', '2026-10-01T20:00:00.000Z'),
    ),
  ).toBe(true)
  expect(
    rangesEqual(
      range('2026-09-01', '2026-09-30'),
      range('2026-09-01', '2026-09-29'),
    ),
  ).toBe(false)
  expect(rangesEqual(null, range('2026-09-01', '2026-09-30'))).toBe(false)
})

test('formatRangeLabel renders both selected dates', () => {
  expect(formatRangeLabel(range('2026-09-01', '2026-09-30'))).toBe(
    '2026-09-01 – 2026-09-30',
  )
  expect(formatRangeLabel(null)).toBe('')
})
