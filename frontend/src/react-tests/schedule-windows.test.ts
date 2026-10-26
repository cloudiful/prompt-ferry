import { describe, expect, it } from 'vitest'
import type { TranslateFn } from '@/i18n'
import {
  SCHEDULE_ALL_DAY_END,
  SCHEDULE_ALL_DAY_START,
  isBlankScheduleRow,
  isScheduleRowValid,
  normalizeScheduleWindowTimes,
  scheduleRowError,
} from '@/models/endpoints/schedule-windows'

// The identity stub keeps the expected strings as the message keys, so the
// error contract stays exact instead of turning into localized text.
const t = ((key: string) => key) as unknown as TranslateFn

describe('schedule window rows', () => {
  it('keeps a row with both times empty valid and reports no error', () => {
    expect(isBlankScheduleRow({ start: '', end: '' })).toBe(true)
    expect(scheduleRowError({ start: '', end: '' }, t)).toBe('')
    expect(isScheduleRowValid({ start: '', end: '' })).toBe(true)
  })

  it('keeps a partially filled row invalid', () => {
    expect(scheduleRowError({ start: '09:00', end: '' }, t)).toBe(
      'scheduleRequired',
    )
    expect(scheduleRowError({ start: '', end: '17:00' }, t)).toBe(
      'scheduleRequired',
    )
    expect(isScheduleRowValid({ start: '09:00', end: '' })).toBe(false)
    expect(isScheduleRowValid({ start: '', end: '17:00' })).toBe(false)
  })

  it('keeps a malformed or equal time pair invalid', () => {
    expect(scheduleRowError({ start: '9:00', end: '17:00' }, t)).toBe(
      'scheduleInvalid',
    )
    expect(scheduleRowError({ start: '09:00', end: '24:30' }, t)).toBe(
      'scheduleInvalid',
    )
    expect(scheduleRowError({ start: '09:00', end: '09:00' }, t)).toBe(
      'scheduleEqual',
    )
    expect(isScheduleRowValid({ start: '09:00', end: '09:00' })).toBe(false)
    expect(isScheduleRowValid({ start: '9:00', end: '17:00' })).toBe(false)
    expect(isScheduleRowValid({ start: '09:00', end: '24:30' })).toBe(false)
    expect(isScheduleRowValid({ start: '09:00', end: '17:00' })).toBe(true)
  })

  it('normalizes a blank row to the canonical all-day window', () => {
    expect(normalizeScheduleWindowTimes({ start: '', end: '' })).toEqual({
      start: SCHEDULE_ALL_DAY_START,
      end: SCHEDULE_ALL_DAY_END,
    })
    expect(normalizeScheduleWindowTimes({ start: ' 09:00 ', end: '' })).toEqual(
      {
        start: '09:00',
        end: '',
      },
    )
    expect(
      normalizeScheduleWindowTimes({ start: '22:00', end: '06:00' }),
    ).toEqual({ start: '22:00', end: '06:00' })
  })
})
