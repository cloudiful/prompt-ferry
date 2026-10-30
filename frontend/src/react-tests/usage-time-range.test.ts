import { describe, expect, it } from 'vitest'
import {
  slidingUsageWindow,
  usageInstantToWallTime,
  usageWallTimeToInstant,
} from '@/lib/usage-range'

describe('usage time ranges', () => {
  it('resolves one and five hours from the same clock instant', () => {
    const now = Date.parse('2026-01-01T00:30:00Z')
    expect(slidingUsageWindow('1h', now)).toEqual({
      start: '2025-12-31T23:30:00.000Z',
      end: '2026-01-01T00:30:00.000Z',
    })
    expect(slidingUsageWindow('5h', now)).toEqual({
      start: '2025-12-31T19:30:00.000Z',
      end: '2026-01-01T00:30:00.000Z',
    })
  })

  it('converts the chosen display zone and rejects DST gaps and folds', () => {
    expect(usageWallTimeToInstant('2026-01-01', '09:00', 'Asia/Tokyo')).toBe(
      '2026-01-01T00:00:00.000Z',
    )
    expect(
      usageInstantToWallTime('2026-01-01T00:00:00Z', 'Asia/Tokyo'),
    ).toEqual({ day: '2026-01-01', time: '09:00' })
    expect(
      usageWallTimeToInstant('2026-03-08', '02:30', 'America/New_York'),
    ).toBeNull()
    expect(
      usageWallTimeToInstant('2026-11-01', '01:30', 'America/New_York'),
    ).toBeNull()
    expect(usageWallTimeToInstant('2026-02-30', '10:00', 'UTC')).toBeNull()
  })
})
