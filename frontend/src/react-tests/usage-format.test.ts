import { describe, expect, it } from 'vitest'
import { formatDateOnly, formatNumber } from '@/format/datetime-core'
import {
  formatUsageCount,
  formatUsageInstant,
  formatUsageMs,
  formatUsagePercent,
  formatUsageTokenQuantity,
  formatUsageTokensPerSecond,
  formatUsageUtcDate,
} from '@/lib/usage-format'

describe('usage count formatting', () => {
  it('keeps an exact zero and dashes an unknown count', () => {
    expect(formatUsageCount(0, 'en-US')).toBe('0')
    expect(formatUsageCount(42, 'en-US')).toBe('42')
    expect(formatUsageCount(null, 'en-US')).toBe('-')
    expect(formatUsageCount(undefined, 'en-US')).toBe('-')
    expect(formatUsageCount(Number.NaN, 'en-US')).toBe('-')
  })

  it('uses the given locale', () => {
    expect(formatUsageCount(1234567, 'en-US')).toBe(
      formatNumber(1234567, 'en-US'),
    )
    expect(formatUsageCount(1234567, 'zh-CN')).toBe(
      formatNumber(1234567, 'zh-CN'),
    )
  })
})

describe('usage percent formatting', () => {
  it('renders one decimal and keeps a truthful zero', () => {
    expect(formatUsagePercent(0)).toBe('0%')
    expect(formatUsagePercent(1)).toBe('100%')
    expect(formatUsagePercent(0.1234)).toBe('12.3%')
    expect(formatUsagePercent(0.708)).toBe('70.8%')
  })

  it('dashes an unknown ratio', () => {
    expect(formatUsagePercent(null)).toBe('-')
    expect(formatUsagePercent(undefined)).toBe('-')
    expect(formatUsagePercent(Number.NaN)).toBe('-')
  })
})

describe('usage duration formatting', () => {
  it('compacts milliseconds, seconds and minutes', () => {
    expect(formatUsageMs(0)).toBe('0ms')
    expect(formatUsageMs(999)).toBe('999ms')
    expect(formatUsageMs(1500)).toBe('1.5s')
    expect(formatUsageMs(15000)).toBe('15s')
    expect(formatUsageMs(60000)).toBe('1m')
    expect(formatUsageMs(65000)).toBe('1m 5s')
  })

  it('clamps a negative value and dashes an unknown duration', () => {
    expect(formatUsageMs(-5)).toBe('0ms')
    expect(formatUsageMs(null)).toBe('-')
    expect(formatUsageMs(undefined)).toBe('-')
    expect(formatUsageMs(Number.NaN)).toBe('-')
  })
})

describe('usage output rate formatting', () => {
  it('uses one decimal below 100 and none above', () => {
    expect(formatUsageTokensPerSecond(42.56)).toBe('42.6 token/s')
    expect(formatUsageTokensPerSecond(123.4)).toBe('123 token/s')
  })

  it('dashes a missing, non-finite or non-positive rate', () => {
    expect(formatUsageTokensPerSecond(0)).toBe('-')
    expect(formatUsageTokensPerSecond(-1)).toBe('-')
    expect(formatUsageTokensPerSecond(null)).toBe('-')
    expect(formatUsageTokensPerSecond(Number.NaN)).toBe('-')
  })
})

describe('usage token formatting', () => {
  it('keeps an exact zero and dashes an unknown total', () => {
    expect(formatUsageTokenQuantity(0, 'en-US')).toBe('0')
    expect(formatUsageTokenQuantity(null, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(undefined, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(Number.NaN, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(1500, 'en-US')).toBe('1.5K')
  })
})

describe('usage date formatting', () => {
  it('renders an instant in the explicit display time zone', () => {
    expect(
      formatUsageInstant('2026-10-20T09:30:00.000Z', 'en-US', 'UTC', {
        day: '2-digit',
        hour: '2-digit',
        hour12: false,
        minute: '2-digit',
        month: '2-digit',
      }),
    ).toContain('09:30')
    expect(
      formatUsageInstant('2026-10-20T09:30:00.000Z', 'en-US', 'Asia/Shanghai', {
        hour: '2-digit',
        hour12: false,
        minute: '2-digit',
      }),
    ).toContain('17:30')
  })

  it('keeps a logical UTC day stable in a negative-offset zone', () => {
    // Midnight UTC would fall on the previous day in a negative-offset zone;
    // the day bucket must keep its logical UTC date.
    expect(formatUsageUtcDate('2026-10-20T00:00:00.000Z', 'en-US')).toBe(
      formatDateOnly('2026-10-20T00:00:00.000Z', 'en-US', {
        timeZone: 'UTC',
      }),
    )
    // The day label is the normalized logical date, so it cannot shift.
    expect(formatUsageUtcDate('2026-10-20T00:00:00.000Z', 'en-US')).toBe(
      '2026-10-20',
    )
  })
})
