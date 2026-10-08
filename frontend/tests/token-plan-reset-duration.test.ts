import { expect, test } from 'bun:test'
import { formatResetDuration } from '../src/composables/useTokenPlanWindowEntries'
import { messages, type Locale, type MessageKey } from '../src/i18n'

// Reset-window duration formatting, exercised against the real locale copy so
// a message that loses its `{days}`/`{hours}`/`{minutes}` placeholders — or
// exists in only one locale — fails here instead of shipping.

function translator(locale: Locale): TranslateFn {
  return ((key: MessageKey, params?: unknown) => {
    const template = messages[locale][key] as string
    if (params === null || typeof params !== 'object') return template
    const values = params as Record<string, unknown>
    return template.replace(/\{(\w+)\}/g, (match, name: string) =>
      name in values ? String(values[name]) : match,
    )
  }) as unknown as TranslateFn
}

const zh = translator('zh-CN')
const en = translator('en-US')

const SECOND = 1000
const MINUTE = 60 * SECOND
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR

test('a window of exactly one day renders days, hours and minutes', () => {
  expect(formatResetDuration(DAY, zh)).toBe('还有 1 天 0 小时 0 分钟重置')
  expect(formatResetDuration(DAY, en)).toBe('Resets in 1d 0h 0m')
})

test('a multi-day window renders days with the remaining hours and minutes', () => {
  const sixDays = 6 * DAY + 7 * HOUR + 10 * MINUTE
  expect(formatResetDuration(sixDays, zh)).toBe('还有 6 天 7 小时 10 分钟重置')
  expect(formatResetDuration(sixDays, en)).toBe('Resets in 6d 7h 10m')
  // Just past the day boundary, and a long window well past a week.
  expect(formatResetDuration(DAY + 1, zh)).toBe('还有 1 天 0 小时 0 分钟重置')
  expect(formatResetDuration(30 * DAY, en)).toBe('Resets in 30d 0h 0m')
})

test('a window one millisecond short of a day keeps the hour format', () => {
  expect(formatResetDuration(DAY - 1, zh)).toBe('还有 23 小时 59 分钟重置')
  expect(formatResetDuration(DAY - 1, en)).toBe('Resets in 23h 59m')
})

test('sub-day windows keep the hour, minute and second formats', () => {
  expect(formatResetDuration(HOUR, zh)).toBe('还有 1 小时 0 分钟重置')
  expect(formatResetDuration(23 * HOUR + 59 * MINUTE, zh)).toBe(
    '还有 23 小时 59 分钟重置',
  )
  expect(formatResetDuration(59 * MINUTE + 59 * SECOND, en)).toBe(
    'Resets in 59m',
  )
  expect(formatResetDuration(MINUTE, zh)).toBe('还有 1 分钟重置')
  // Seconds are floored, and a sub-minute remainder never reads as minutes.
  expect(formatResetDuration(59 * SECOND + 999, en)).toBe('Resets in 59s')
  expect(formatResetDuration(1500, zh)).toBe('还有 1 秒重置')
})

test('a window in the past is expired and a missing window stays a dash', () => {
  expect(formatResetDuration(0, zh)).toBe('已到期')
  expect(formatResetDuration(-5 * SECOND, en)).toBe('Expired')
  expect(formatResetDuration(null, zh)).toBe('-')
  expect(formatResetDuration(null, en)).toBe('-')
})

test('the reset-duration messages exist in both locales', () => {
  const keys = [
    'tokenPlanResetExpiresDaysHoursMinutes',
    'tokenPlanResetExpiresHoursMinutes',
    'tokenPlanResetExpiresMinutes',
    'tokenPlanResetExpiresSeconds',
    'tokenPlanExpired',
    'tokenPlanBalanceUnknown',
  ] as const
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const localeMessages = messages[locale] as Record<string, string>
    for (const key of keys) {
      expect(localeMessages[key]?.length ?? 0).toBeGreaterThan(0)
    }
  }
  // The day message carries all three units; losing one would silently drop
  // the value it names.
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const template = (messages[locale] as Record<string, string>)[
      'tokenPlanResetExpiresDaysHoursMinutes'
    ]
    expect(template).toContain('{days}')
    expect(template).toContain('{hours}')
    expect(template).toContain('{minutes}')
  }
})
