import { expect, test } from 'bun:test'
import {
  providerLabelKey,
  providerVisual,
} from '../src/components/providers/provider-visuals'

test('brand providers resolve to their vendored simple-icons mark', () => {
  expect(providerVisual('minimax')).toEqual({ kind: 'brand', brand: 'minimax' })
  expect(providerVisual('deepseek')).toEqual({
    kind: 'brand',
    brand: 'deepseek',
  })
  expect(providerVisual('openrouter')).toEqual({
    kind: 'brand',
    brand: 'openrouter',
  })
  // The endpoint provider token is `opencode_go`; the vendored mark is the
  // simple-icons `opencode` glyph.
  expect(providerVisual('opencode_go')).toEqual({
    kind: 'brand',
    brand: 'opencode',
  })
})

test('mark-less providers fall back to a letter avatar', () => {
  expect(providerVisual('command_code')).toEqual({
    kind: 'letter',
    letter: 'CC',
  })
  expect(providerVisual('glm')).toEqual({ kind: 'letter', letter: 'GLM' })
  expect(providerVisual('generic')).toEqual({ kind: 'letter', letter: 'G' })
})

test('unknown providers fall back to the generic lucide glyph', () => {
  expect(providerVisual('mystery')).toEqual({ kind: 'fallback' })
})

test('every endpoint provider maps to a localized label key', () => {
  for (const provider of [
    'generic',
    'minimax',
    'command_code',
    'opencode_go',
    'openrouter',
    'glm',
    'deepseek',
  ]) {
    expect(providerLabelKey(provider)).not.toBeNull()
  }
  expect(providerLabelKey('mystery')).toBeNull()
})
