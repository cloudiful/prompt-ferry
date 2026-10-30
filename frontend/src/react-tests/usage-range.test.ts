import { describe, expect, it } from 'vitest'
import { USAGE_RANGE_PRESETS } from '@/components/usage/UsageRangePicker'
import {
  usageInstantToWallTime,
  usageWallTimeToInstant,
} from '@/lib/usage-range'

describe('usage range presets', () => {
  it('offers hour, day, month and custom ranges in order', () => {
    expect(USAGE_RANGE_PRESETS).toEqual([
      '1h',
      '5h',
      '24h',
      '7d',
      '30d',
      'month',
      'custom',
    ])
  })
})

describe('usage date-time bounds', () => {
  it('builds the half-open UTC window for a two-day selection', () => {
    const start = usageWallTimeToInstant('2026-10-01', '10:30', 'UTC')
    const end = usageWallTimeToInstant('2026-10-02', '15:45', 'UTC')
    expect({ start, end }).toEqual({
      start: '2026-10-01T10:30:00.000Z',
      end: '2026-10-02T15:45:00.000Z',
    })
    expect(Date.parse(end!) - Date.parse(start!)).toBe(29.25 * 60 * 60 * 1000)
  })

  it('converts precise same-day and cross-year wall times without rounding the exclusive end', () => {
    expect(usageWallTimeToInstant('2024-02-29', '12:15', 'UTC')).toBe(
      '2024-02-29T12:15:00.000Z',
    )
    expect(usageWallTimeToInstant('2025-01-01', '00:30', 'Asia/Tokyo')).toBe(
      '2024-12-31T15:30:00.000Z',
    )
    expect(
      usageInstantToWallTime('2024-12-31T15:30:00.000Z', 'Asia/Tokyo'),
    ).toEqual({ day: '2025-01-01', time: '00:30' })
  })

  it('rejects malformed or nonexistent dates and times', () => {
    expect(usageWallTimeToInstant('2026-02-29', '10:00', 'UTC')).toBeNull()
    expect(usageWallTimeToInstant('2024-02-29', '25:00', 'UTC')).toBeNull()
    expect(usageWallTimeToInstant('', '', 'UTC')).toBeNull()
  })
})
