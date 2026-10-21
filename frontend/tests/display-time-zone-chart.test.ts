import { expect, test } from 'bun:test'

import { displayTimeZone, setDisplayTimeZone } from '../src/format/timezone'
import { formatAppDateOnly, formatAppDateTime } from '../src/format/datetime'
import { formatTrendBucketLabel } from '../src/request-overview'
import {
  createQuotaTrendPanelOption,
  groupQuotaTrendSeries,
} from '../src/quota-observation-charts'

const KINDS = {
  five_hour: '5-hour window',
  rolling: 'Rolling window',
  weekly: 'Weekly window',
  monthly: 'Monthly window',
  balance: 'Balance',
  unlimited: 'Unlimited',
  custom: 'Custom window',
} as const

function optionInput() {
  return {
    range: '24h' as const,
    formatAxisDate: (value: string) => value,
    formatAxisTime: (value: string) => value,
    formatTooltipTime: (value: string) => value,
    formatAmount: (value: number, unit: string) => `${value} ${unit}`,
    labels: { percentSuffix: '%', unavailable: 'n/a', kinds: { ...KINDS } },
  }
}

function trendSeries(points: Array<[string, number]>) {
  return {
    key_id: 'key-a',
    key_label: 'Key A',
    source_id: 'glm:five_hour',
    kind: 'five_hour' as const,
    unit: 'percent',
    metric: 'remaining_percent' as const,
    points: points.map(([at, value]) => ({
      at,
      observed_at: at,
      value,
    })),
  }
}

function axisLabels(points: Array<[string, number]>): string[] {
  const groups = groupQuotaTrendSeries([trendSeries(points)] as never)
  const group = groups[0]
  const option = createQuotaTrendPanelOption(group, {
    ...optionInput(),
  }) as unknown as {
    xAxis: {
      data: string[]
      axisLabel: { formatter: (value: string) => string }
    }
  }
  return option.xAxis.data.map((value) =>
    option.xAxis.axisLabel.formatter(value),
  )
}

test('the quota chart compares days in the selected zone, not the browser day', () => {
  const previous = displayTimeZone.value
  try {
    // 23:30Z and 00:30Z are two different UTC days, and also two different
    // days in Asia/Shanghai.
    setDisplayTimeZone('UTC')
    const utc = axisLabels([
      ['2026-03-05T23:30:00Z', 80],
      ['2026-03-06T00:30:00Z', 70],
    ])
    expect(utc[0]).toBe('2026-03-05T23:30:00Z')
    expect(utc[1]).toBe('2026-03-06T00:30:00Z 2026-03-06T00:30:00Z')

    // In Asia/Shanghai both instants fall on the same day, so the second
    // label carries no date.
    setDisplayTimeZone('Asia/Shanghai')
    const shanghai = axisLabels([
      ['2026-03-05T23:30:00Z', 80],
      ['2026-03-06T00:30:00Z', 70],
    ])
    expect(shanghai[1]).not.toContain('2026-03-05T23:30:00Z')
    expect(shanghai[1]).toBe('2026-03-06T00:30:00Z')
  } finally {
    setDisplayTimeZone(previous)
  }
})

test('a day trend bucket keeps its logical UTC date in a negative-offset zone', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('America/New_York')
    const label = formatTrendBucketLabel(
      'day',
      '2026-03-05T00:00:00Z',
      (instant) => formatAppDateTime(instant),
      (utcDate) => formatAppDateOnly(utcDate, { timeZone: 'UTC' }),
    )

    expect(label).toBe('2026-03-05')
  } finally {
    setDisplayTimeZone(previous)
  }
})

test('an hour trend bucket is an instant and follows the display zone', () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('Asia/Shanghai')
    const label = formatTrendBucketLabel(
      'hour',
      '2026-03-05T16:30:00Z',
      (instant) =>
        formatAppDateTime(instant, {
          day: '2-digit',
          hour: '2-digit',
          hour12: false,
          minute: '2-digit',
        }),
      (utcDate) => formatAppDateOnly(utcDate, { timeZone: 'UTC' }),
    )

    expect(label).toContain('00:30')
    expect(label).not.toContain('16:30')
  } finally {
    setDisplayTimeZone(previous)
  }
})
