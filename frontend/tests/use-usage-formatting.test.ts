import { expect, test } from 'bun:test'
import type { RequestRecordTiming } from '../src/models/request-record-formatting'
import {
  formatRequestRecordOutputTokensPerSecond,
  hasOutputRate,
} from '../src/composables/useUsageFormatting'

function record(
  overrides: Partial<RequestRecordTiming> = {},
): RequestRecordTiming {
  return {
    request_category: 'ai',
    request_state: 'completed',
    duration_ms: null,
    ttft_ms: null,
    input_tokens: null,
    output_tokens: null,
    ...overrides,
  }
}

test('output rate uses end-to-end duration, not duration minus TTFT', () => {
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ output_tokens: 250, duration_ms: 10539, ttft_ms: 10402 }),
    ),
  ).toBe('23.7')
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ output_tokens: 353, duration_ms: 10900, ttft_ms: 5821 }),
    ),
  ).toBe('32.4')
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ output_tokens: 128, duration_ms: 3366 }),
    ),
  ).toBe('38.0')
})

test('output rate is unchanged by the presence of TTFT', () => {
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ output_tokens: 250, duration_ms: 10539, ttft_ms: null }),
    ),
  ).toBe('23.7')
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ output_tokens: 250, duration_ms: 10539, ttft_ms: 10402 }),
    ),
  ).toBe('23.7')
})

test('hasOutputRate is true for completed AI records with tokens and duration', () => {
  expect(
    hasOutputRate(
      record({ output_tokens: 250, duration_ms: 10539, ttft_ms: 10402 }),
    ),
  ).toBe(true)
})

test('hasOutputRate is false without duration, tokens, or for non-AI records', () => {
  expect(hasOutputRate(record({ output_tokens: 250, duration_ms: null }))).toBe(
    false,
  )
  expect(hasOutputRate(record({ output_tokens: 0, duration_ms: 100 }))).toBe(
    false,
  )
  expect(
    hasOutputRate(
      record({ request_category: 'mcp', output_tokens: 250, duration_ms: 100 }),
    ),
  ).toBe(false)
  expect(
    hasOutputRate(
      record({ request_state: 'failed', output_tokens: 250, duration_ms: 100 }),
    ),
  ).toBe(false)
})

test('output rate shows dash for incomplete data', () => {
  expect(
    formatRequestRecordOutputTokensPerSecond(record({ output_tokens: 250 })),
  ).toBe('-')
  expect(
    formatRequestRecordOutputTokensPerSecond(record({ duration_ms: 100 })),
  ).toBe('-')
  expect(
    formatRequestRecordOutputTokensPerSecond(
      record({ request_category: 'mcp', output_tokens: 250, duration_ms: 100 }),
    ),
  ).toBe('-')
})
