import { expect, test } from 'bun:test'
import { nextTick } from 'vue'

import {
  AUTO_DISPLAY_TIME_ZONE,
  browserTimeZone,
  displayTimeZone,
  displayTimeZoneOptions,
  effectiveTimeZone,
  isValidTimeZone,
  resolveDisplayTimeZone,
  setDisplayTimeZone,
  supportedTimeZones,
} from '../src/format/timezone'
import {
  currentDisplayTimeZone,
  formatAppDateOnly,
  formatAppDateTime,
} from '../src/format/datetime'
import { DISPLAY_TIME_ZONE_STORAGE_KEY } from '../src/storage'

const SAMPLE = '2026-03-05T12:30:00Z'

function restore(previous: string): void {
  displayTimeZone.value = previous
}

test('the automatic choice follows the browser zone and a manual zone overrides it', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone(AUTO_DISPLAY_TIME_ZONE)
    expect(effectiveTimeZone()).toBe(browserTimeZone())

    setDisplayTimeZone('Asia/Shanghai')
    expect(effectiveTimeZone()).toBe('Asia/Shanghai')
    expect(currentDisplayTimeZone()).toBe('Asia/Shanghai')
  } finally {
    restore(previous)
  }
})

test('a malformed or unknown stored zone falls back to the automatic choice', () => {
  expect(resolveDisplayTimeZone(null)).toBe(AUTO_DISPLAY_TIME_ZONE)
  expect(resolveDisplayTimeZone('')).toBe(AUTO_DISPLAY_TIME_ZONE)
  expect(resolveDisplayTimeZone('   ')).toBe(AUTO_DISPLAY_TIME_ZONE)
  expect(resolveDisplayTimeZone('Not/AZone')).toBe(AUTO_DISPLAY_TIME_ZONE)
  expect(resolveDisplayTimeZone('UTC')).toBe('UTC')
  expect(resolveDisplayTimeZone('Asia/Shanghai')).toBe('Asia/Shanghai')
  expect(isValidTimeZone('Not/AZone')).toBe(false)
  expect(isValidTimeZone('UTC')).toBe(true)
})

test('the selectable zones contain the automatic choice, the browser zone, UTC and Asia/Shanghai', () => {
  const options = displayTimeZoneOptions('Automatic')
  const values = options.map((option) => option.value)

  expect(values).toContain(AUTO_DISPLAY_TIME_ZONE)
  expect(values).toContain(browserTimeZone())
  expect(values).toContain('UTC')
  expect(values).toContain('Asia/Shanghai')
  expect(options[0]).toEqual({
    label: 'Automatic',
    value: AUTO_DISPLAY_TIME_ZONE,
  })
  expect(supportedTimeZones().length).toBeGreaterThan(0)
})

test('blocked storage never breaks the in-memory selection', async () => {
  const previous = displayTimeZone.value
  const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: () => {
        throw new Error('storage is blocked')
      },
      setItem: () => {
        throw new Error('storage is blocked')
      },
    },
  })
  try {
    setDisplayTimeZone('Asia/Shanghai')
    await nextTick()
    expect(displayTimeZone.value).toBe('Asia/Shanghai')
    expect(effectiveTimeZone()).toBe('Asia/Shanghai')
  } finally {
    if (original) Object.defineProperty(globalThis, 'localStorage', original)
    restore(previous)
  }
})

test('the shared formatter follows the selected zone', () => {
  const previous = displayTimeZone.value
  try {
    const options: Intl.DateTimeFormatOptions = {
      day: '2-digit',
      hour: '2-digit',
      hour12: false,
      minute: '2-digit',
    }

    setDisplayTimeZone('Asia/Shanghai')
    const shanghai = formatAppDateTime(SAMPLE, options)
    setDisplayTimeZone('UTC')
    const utc = formatAppDateTime(SAMPLE, options)

    expect(shanghai).toBe(
      new Intl.DateTimeFormat('zh-CN', {
        ...options,
        timeZone: 'Asia/Shanghai',
      }).format(new Date(SAMPLE)),
    )
    expect(utc).toBe(
      new Intl.DateTimeFormat('zh-CN', { ...options, timeZone: 'UTC' }).format(
        new Date(SAMPLE),
      ),
    )
    expect(shanghai).not.toBe(utc)
  } finally {
    restore(previous)
  }
})

test('an explicit timeZone wins over the display preference', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('Asia/Shanghai')
    expect(
      formatAppDateTime(SAMPLE, {
        day: '2-digit',
        hour: '2-digit',
        hour12: false,
        minute: '2-digit',
        timeZone: 'UTC',
      }),
    ).toBe(
      new Intl.DateTimeFormat('zh-CN', {
        day: '2-digit',
        hour: '2-digit',
        hour12: false,
        minute: '2-digit',
        timeZone: 'UTC',
      }).format(new Date(SAMPLE)),
    )
  } finally {
    restore(previous)
  }
})

test('the date-only label is stable and follows the display zone', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('Asia/Shanghai')
    expect(formatAppDateOnly(SAMPLE)).toBe('2026-03-05')
    setDisplayTimeZone('UTC')
    expect(formatAppDateOnly(SAMPLE)).toBe('2026-03-05')
    // An instant just before the UTC midnight is the previous day in a
    // positive-offset zone.
    setDisplayTimeZone('Asia/Shanghai')
    expect(formatAppDateOnly('2026-03-04T23:30:00Z')).toBe('2026-03-05')
    setDisplayTimeZone('UTC')
    expect(formatAppDateOnly('2026-03-04T23:30:00Z')).toBe('2026-03-04')
  } finally {
    restore(previous)
  }
})

test('a negative-offset zone keeps the UTC calendar bucket on its own date', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('America/New_York')
    expect(formatAppDateOnly('2026-03-05T02:00:00Z', { timeZone: 'UTC' })).toBe(
      '2026-03-05',
    )
    expect(formatAppDateOnly('2026-03-05T02:00:00Z')).toBe('2026-03-04')
  } finally {
    restore(previous)
  }
})

test('a daylight saving change keeps the same instant', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('America/New_York')
    const before = formatAppDateTime('2026-03-07T12:00:00Z', {
      hour: '2-digit',
      hour12: false,
      minute: '2-digit',
      timeZone: 'America/New_York',
    })
    const after = formatAppDateTime('2026-11-07T12:00:00Z', {
      hour: '2-digit',
      hour12: false,
      minute: '2-digit',
      timeZone: 'America/New_York',
    })
    expect(before).toBe('07:00')
    expect(after).toBe('07:00')
  } finally {
    restore(previous)
  }
})

test('the preference key is the one the settings page persists', () => {
  expect(DISPLAY_TIME_ZONE_STORAGE_KEY).toBe('prompt-ferry:display-time-zone')
})

test('both record tables render the date from the instant, not from a baked value', async () => {
  const source = (relative: string) =>
    Bun.file(new URL(relative, import.meta.url)).text()
  const [ai, mcp] = await Promise.all([
    source('../src/components/usage/UsagePanel.vue'),
    source('../src/components/mcp/McpUsagePanel.vue'),
  ])

  for (const panel of [ai, mcp]) {
    expect(panel).toContain('formatAppDateOnly(row.original.created_at)')
    expect(panel).not.toContain('row.original.request_date')
  }
})
