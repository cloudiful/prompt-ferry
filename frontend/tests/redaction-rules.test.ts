import { expect, test } from 'bun:test'
import {
  findRuleWarnings,
  validateCustomStringRule,
} from '../src/redaction-rules'
import type { CustomStringRuleSchema } from '../src/generated/admin-api'

function rule(
  pattern: string,
  match_type: 'exact' | 'contains' | 'regex' = 'contains',
  scope: 'text' | 'line' = 'text',
): CustomStringRuleSchema {
  return { pattern, match_type, scope }
}

test('validateCustomStringRule flags invalid regex and accepts compileable ones', () => {
  expect(validateCustomStringRule('hello', 'regex')).toEqual({
    invalidRegex: false,
  })
  expect(validateCustomStringRule('\\b(foo', 'regex')).toEqual({
    invalidRegex: true,
  })
  expect(validateCustomStringRule('plain', 'contains')).toEqual({
    invalidRegex: false,
  })
  expect(validateCustomStringRule('', 'regex')).toEqual({ invalidRegex: false })
  expect(validateCustomStringRule('   ', 'regex')).toEqual({
    invalidRegex: false,
  })
})

test('findRuleWarnings flags invalid regex even when alone', () => {
  const warnings = findRuleWarnings([rule('oops(', 'regex')])
  expect(warnings[0]?.invalidRegex).toBe(true)
  expect(warnings[0]?.duplicate).toBe(false)
  expect(warnings[0]?.conflict).toBe(false)
})

test('findRuleWarnings leaves empty-pattern rules clean', () => {
  const warnings = findRuleWarnings([rule('', 'regex')])
  expect(warnings[0]).toEqual({
    duplicate: false,
    conflict: false,
    invalidRegex: false,
  })
})

test('findRuleWarnings marks identical rules as duplicates', () => {
  const warnings = findRuleWarnings([
    rule('acme', 'exact'),
    rule('acme', 'exact'),
    rule('plain'),
  ])
  expect(warnings[0]?.duplicate).toBe(true)
  expect(warnings[1]?.duplicate).toBe(true)
  expect(warnings[2]?.duplicate).toBe(false)
})

test('findRuleWarnings marks same pattern with different match_type as conflicts', () => {
  const warnings = findRuleWarnings([
    rule('beta', 'contains'),
    rule('beta', 'regex'),
  ])
  expect(warnings[0]?.conflict).toBe(true)
  expect(warnings[1]?.conflict).toBe(true)
  expect(warnings[0]?.duplicate).toBe(false)
})

test('findRuleWarnings marks same pattern with different scope as conflicts', () => {
  const warnings = findRuleWarnings([
    rule('gamma', 'contains', 'text'),
    rule('gamma', 'contains', 'line'),
  ])
  expect(warnings[0]?.conflict).toBe(true)
  expect(warnings[1]?.conflict).toBe(true)
})

test('findRuleWarnings prioritizes duplicate flag over conflict when rule is exact dup', () => {
  const warnings = findRuleWarnings([
    rule('delta', 'regex'),
    rule('delta', 'regex'),
    rule('delta', 'contains'),
  ])
  expect(warnings[0]?.duplicate).toBe(true)
  expect(warnings[1]?.duplicate).toBe(true)
  expect(warnings[0]?.conflict).toBe(false)
  expect(warnings[2]?.conflict).toBe(true)
})
