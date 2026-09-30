import { expect, test } from 'bun:test'
import type { RequestRecordTiming } from '../src/models/request-record-formatting'
import {
  formatRequestStateBadges,
  type RequestRecordStateChip,
} from '../src/composables/useUsageFormatting'
import { messages, type TranslateFn } from '../src/i18n'

function record(
  overrides: Partial<RequestRecordTiming> = {},
): RequestRecordTiming {
  return {
    request_category: 'ai',
    request_state: 'upstream_processing',
    duration_ms: null,
    ttft_ms: null,
    input_tokens: null,
    output_tokens: null,
    ...overrides,
  }
}

const zh: TranslateFn = (key) => messages['zh-CN'][key]
const en: TranslateFn = (key) => messages['en-US'][key]

function labels(chips: RequestRecordStateChip[]): string[] {
  return chips.map((chip) => chip.label)
}

test('a running request without output reads as waiting for the response', () => {
  const chips = formatRequestStateBadges(zh, record())

  expect(labels(chips)).toEqual(['等待响应'])
  expect(chips.map((chip) => chip.id)).toEqual(['state'])
  expect(chips.map((chip) => chip.color)).toEqual(['secondary'])
})

test('a running request that already produced output reads as streaming output', () => {
  const chips = formatRequestStateBadges(zh, record({ ttft_ms: 120 }))

  expect(labels(chips)).toEqual(['流式输出中'])
  expect(chips.map((chip) => chip.id)).toEqual(['state'])
  expect(chips.map((chip) => chip.color)).toEqual(['secondary'])
})

test('terminal states keep their own label and append the output capsule', () => {
  const terminal = [
    ['completed', '成功', 'success'],
    ['failed', '失败', 'warn'],
    ['aborted', '已中止', 'warn'],
  ] as const

  for (const [request_state, stateLabel, stateColor] of terminal) {
    const chips = formatRequestStateBadges(
      zh,
      record({ request_state, ttft_ms: 120 }),
    )

    expect(labels(chips)).toEqual([stateLabel, '已开始输出'])
    expect(chips.map((chip) => chip.id)).toEqual(['state', 'output'])
    expect(chips.map((chip) => chip.color)).toEqual([stateColor, 'neutral'])
    expect(chips.map((chip) => chip.variant)).toEqual([undefined, 'subtle'])
  }
})

test('terminal history without a recorded output keeps a single state capsule', () => {
  for (const request_state of ['completed', 'failed', 'aborted'] as const) {
    const chips = formatRequestStateBadges(zh, record({ request_state }))

    expect(chips.map((chip) => chip.id)).toEqual(['state'])
    expect(labels(chips)).not.toContain('已开始输出')
  }
})

test('a terminal row is never labelled as streaming output', () => {
  for (const request_state of ['completed', 'failed', 'aborted'] as const) {
    for (const ttft_ms of [null, 120]) {
      const chips = formatRequestStateBadges(
        zh,
        record({ request_state, ttft_ms }),
      )

      expect(labels(chips)).not.toContain('流式输出中')
    }
  }
})

test('MCP history reuses the same output capsule and never reads as streaming', () => {
  const chips = formatRequestStateBadges(
    zh,
    record({
      request_category: 'mcp',
      request_state: 'completed',
      ttft_ms: 90,
    }),
  )

  expect(labels(chips)).toEqual(['成功', '已开始输出'])
  expect(chips.map((chip) => chip.id)).toEqual(['state', 'output'])
  expect(labels(chips)).not.toContain('流式输出中')
})

test('admission states keep their own labels', () => {
  expect(
    labels(formatRequestStateBadges(zh, record({ request_state: 'received' }))),
  ).toEqual(['已接收'])
  expect(
    labels(
      formatRequestStateBadges(
        zh,
        record({ request_state: 'awaiting_approval' }),
      ),
    ),
  ).toEqual(['待审批'])
})

test('the waiting, streaming, and output capsules are translated in both locales', () => {
  expect(labels(formatRequestStateBadges(en, record()))).toEqual([
    'Waiting for response',
  ])
  expect(labels(formatRequestStateBadges(en, record({ ttft_ms: 5 })))).toEqual([
    'Streaming output',
  ])
  expect(
    labels(
      formatRequestStateBadges(
        en,
        record({ request_state: 'failed', ttft_ms: 5 }),
      ),
    ),
  ).toEqual(['Failed', 'Output started'])

  expect(messages['en-US'].requestStateWaitingForResponse).toBe(
    'Waiting for response',
  )
  expect(messages['en-US'].requestStateStreamingOutput).toBe('Streaming output')
  expect(messages['en-US'].requestOutputStarted).toBe('Output started')
  expect(messages['zh-CN'].requestStateWaitingForResponse).toBe('等待响应')
  expect(messages['zh-CN'].requestStateStreamingOutput).toBe('流式输出中')
  expect(messages['zh-CN'].requestOutputStarted).toBe('已开始输出')
})
