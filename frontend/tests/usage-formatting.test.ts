import { expect, test } from 'bun:test'
import {
  formatTokenQuantity,
  formatTokensPerSecondValue,
} from '../src/composables/useUsageFormatting'

test('formatTokenQuantity renders exact localized integer below 1000', () => {
  expect(formatTokenQuantity(0)).toBe('0')
  expect(formatTokenQuantity(1)).toBe('1')
  expect(formatTokenQuantity(42)).toBe('42')
  expect(formatTokenQuantity(999)).toBe('999')
})

test('formatTokenQuantity uses K for 1_000 through 999_999 with at most one decimal', () => {
  expect(formatTokenQuantity(1_000)).toBe('1K')
  expect(formatTokenQuantity(1_234)).toBe('1.2K')
  expect(formatTokenQuantity(1_500)).toBe('1.5K')
  expect(formatTokenQuantity(12_345)).toBe('12.3K')
  expect(formatTokenQuantity(999_999)).toBe('1,000K')
})

test('formatTokenQuantity uses M for 1_000_000 through 999_999_999', () => {
  expect(formatTokenQuantity(1_000_000)).toBe('1M')
  expect(formatTokenQuantity(1_500_000)).toBe('1.5M')
  expect(formatTokenQuantity(12_345_678)).toBe('12.3M')
  expect(formatTokenQuantity(999_999_999)).toBe('1,000M')
})

test('formatTokenQuantity uses B for 1_000_000_000 through 999_999_999_999', () => {
  expect(formatTokenQuantity(1_000_000_000)).toBe('1B')
  expect(formatTokenQuantity(4_355_648_670)).toBe('4.4B')
  expect(formatTokenQuantity(4_700_000_000)).toBe('4.7B')
  expect(formatTokenQuantity(12_345_678_901)).toBe('12.3B')
  expect(formatTokenQuantity(999_999_999_999)).toBe('1,000B')
})

test('formatTokenQuantity uses T for 1_000_000_000_000 and above', () => {
  expect(formatTokenQuantity(1_000_000_000_000)).toBe('1T')
  expect(formatTokenQuantity(1_500_000_000_000)).toBe('1.5T')
  expect(formatTokenQuantity(123_456_789_012_345)).toBe('123.5T')
})

test('formatTokenQuantity trims trailing .0', () => {
  expect(formatTokenQuantity(2_000)).toBe('2K')
  expect(formatTokenQuantity(30_000)).toBe('30K')
  expect(formatTokenQuantity(4_000_000)).toBe('4M')
  expect(formatTokenQuantity(5_000_000_000)).toBe('5B')
})

test('formatTokenQuantity renders - for null, undefined, and non-finite values', () => {
  expect(formatTokenQuantity(null)).toBe('-')
  expect(formatTokenQuantity(undefined)).toBe('-')
  expect(formatTokenQuantity(Number.NaN)).toBe('-')
  expect(formatTokenQuantity(Number.POSITIVE_INFINITY)).toBe('-')
  expect(formatTokenQuantity(Number.NEGATIVE_INFINITY)).toBe('-')
})

test('formatTokenQuantity preserves negative sign for tokens', () => {
  expect(formatTokenQuantity(-42)).toBe('-42')
  expect(formatTokenQuantity(-1_500)).toBe('-1.5K')
})

test('formatTokensPerSecondValue labels small rates with one decimal', () => {
  expect(formatTokensPerSecondValue(0.1)).toBe('0.1 token/s')
  expect(formatTokensPerSecondValue(23.7)).toBe('23.7 token/s')
  expect(formatTokensPerSecondValue(42.5)).toBe('42.5 token/s')
})

test('formatTokensPerSecondValue rounds fast rates to integers', () => {
  expect(formatTokensPerSecondValue(100)).toBe('100 token/s')
  expect(formatTokensPerSecondValue(1_500)).toBe('1500 token/s')
})

test('formatTokensPerSecondValue renders - for null, invalid, or non-positive values', () => {
  expect(formatTokensPerSecondValue(null)).toBe('-')
  expect(formatTokensPerSecondValue(undefined)).toBe('-')
  expect(formatTokensPerSecondValue(Number.NaN)).toBe('-')
  expect(formatTokensPerSecondValue(0)).toBe('-')
  expect(formatTokensPerSecondValue(-1)).toBe('-')
})
