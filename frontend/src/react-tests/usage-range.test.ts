import { describe, expect, it } from 'vitest'
import {
  USAGE_RANGE_PRESETS,
  isUsageDay,
  usageCustomRangeInput,
  usageRangeDay,
  usageRangeEquals,
  usageRangeLabel,
  usageRangeVisibleEnd,
} from '@/components/usage/UsageRangePicker'
import type { UsageRange } from '@/lib/usage-model'

const labels: Record<UsageRange, string> = {
  '24h': 'Last 24h',
  '30d': 'Last 30d',
  '7d': 'Last 7d',
  custom: 'Custom range',
  month: 'This month',
}

describe('usage range presets', () => {
  it('offers month, 24h, 7d, 30d and custom in order', () => {
    expect(USAGE_RANGE_PRESETS).toEqual(['month', '24h', '7d', '30d', 'custom'])
  })

  it('applies a preset without bounds', () => {
    // The store only receives the preset; no window is invented here.
    expect(usageRangeLabel('7d', '', '', labels)).toBe('Last 7d')
    expect(usageRangeLabel('month', '', '', labels)).toBe('This month')
  })
})

describe('usage custom range arithmetic', () => {
  it('accepts a real calendar day and rejects a normalized impossible one', () => {
    expect(isUsageDay('2026-10-20')).toBe(true)
    expect(isUsageDay('2024-02-29')).toBe(true)
    expect(isUsageDay('2026-02-30')).toBe(false)
    expect(isUsageDay('2026-13-01')).toBe(false)
    expect(isUsageDay('2026-1-1')).toBe(false)
    expect(isUsageDay('')).toBe(false)
  })

  it('builds the half-open UTC window for a two-day selection', () => {
    expect(usageCustomRangeInput('2026-10-01', '2026-10-20')).toEqual({
      end: '2026-10-21T00:00:00.000Z',
      range: 'custom',
      start: '2026-10-01T00:00:00.000Z',
    })
  })

  it('crosses a leap February without losing a day', () => {
    expect(usageCustomRangeInput('2024-02-29', '2024-02-29')).toEqual({
      end: '2024-03-01T00:00:00.000Z',
      range: 'custom',
      start: '2024-02-29T00:00:00.000Z',
    })
    expect(usageCustomRangeInput('2024-02-28', '2024-02-29')).toEqual({
      end: '2024-03-01T00:00:00.000Z',
      range: 'custom',
      start: '2024-02-28T00:00:00.000Z',
    })
  })

  it('keeps UTC midnights across DST transitions', () => {
    // 2026-03-08 is a US DST start and 2026-03-29 an EU DST start; a local-time
    // addition would drift by an hour and move the exclusive bound.
    expect(usageCustomRangeInput('2026-03-08', '2026-03-09')).toEqual({
      end: '2026-03-10T00:00:00.000Z',
      range: 'custom',
      start: '2026-03-08T00:00:00.000Z',
    })
    expect(usageCustomRangeInput('2026-10-25', '2026-10-25')).toEqual({
      end: '2026-10-26T00:00:00.000Z',
      range: 'custom',
      start: '2026-10-25T00:00:00.000Z',
    })
  })

  it('rejects an incomplete or reversed window', () => {
    expect(usageCustomRangeInput('', '2026-10-20')).toBeNull()
    expect(usageCustomRangeInput('2026-10-20', '')).toBeNull()
    expect(usageCustomRangeInput('2026-10-20', '2026-10-01')).toBeNull()
    expect(usageCustomRangeInput('2026-02-30', '2026-03-01')).toBeNull()
  })

  it('compares windows by their stored bounds', () => {
    const one = usageCustomRangeInput('2026-10-01', '2026-10-20')
    const two = usageCustomRangeInput('2026-10-01', '2026-10-20')
    expect(one && two && usageRangeEquals(one, two)).toBe(true)
    expect(
      usageRangeEquals(
        { end: '2026-10-21T00:00:00.000Z', start: '2026-10-01T00:00:00.000Z' },
        { end: '2026-10-21T00:00:00.000Z', start: '2026-10-02T00:00:00.000Z' },
      ),
    ).toBe(false)
  })
})

describe('usage range stored days', () => {
  it('reads the UTC day of a stored bound without local-offset drift', () => {
    expect(usageRangeDay('2026-10-20T00:00:00.000Z')).toBe('2026-10-20')
    expect(usageRangeDay('2026-10-20T00:00:00+08:00')).toBe('2026-10-20')
    expect(usageRangeDay('not-a-day')).toBeNull()
    expect(usageRangeDay(null)).toBeNull()
  })

  it('shows the exclusive end as its last selected day', () => {
    expect(usageRangeVisibleEnd('2026-10-21T00:00:00.000Z')).toBe('2026-10-20')
    expect(usageRangeVisibleEnd('2024-03-01T00:00:00.000Z')).toBe('2024-02-29')
  })

  it('labels a stored custom window with both selected days', () => {
    expect(
      usageRangeLabel(
        'custom',
        '2026-10-01T00:00:00.000Z',
        '2026-10-21T00:00:00.000Z',
        labels,
      ),
    ).toBe('2026-10-01 – 2026-10-20')
    expect(usageRangeLabel('custom', '', '', labels)).toBe('Custom range')
  })
})
