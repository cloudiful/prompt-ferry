import { describe, expect, it } from 'vitest'
import { messages } from '@/i18n'
import {
  providerLabelKey,
  providerVisual,
} from '@/components/providers/provider-visuals'

describe('provider visuals', () => {
  it('resolves the brand providers to their vendored simple-icons mark', () => {
    expect(providerVisual('minimax')).toEqual({
      kind: 'brand',
      brand: 'minimax',
    })
    expect(providerVisual('deepseek')).toEqual({
      kind: 'brand',
      brand: 'deepseek',
    })
    expect(providerVisual('openai')).toEqual({ kind: 'brand', brand: 'openai' })
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

  it('falls back to a letter avatar for the mark-less providers', () => {
    expect(providerVisual('command_code')).toEqual({
      kind: 'letter',
      letter: 'CC',
    })
    expect(providerVisual('glm')).toEqual({ kind: 'letter', letter: 'GLM' })
    expect(providerVisual('generic')).toEqual({ kind: 'letter', letter: 'G' })
  })

  it('falls back to the generic glyph for an unknown provider', () => {
    expect(providerVisual('mystery')).toEqual({ kind: 'fallback' })
  })

  it('maps every endpoint provider to a localized label key', () => {
    for (const provider of [
      'generic',
      'minimax',
      'command_code',
      'opencode_go',
      'openrouter',
      'glm',
      'deepseek',
      'openai',
    ]) {
      const key = providerLabelKey(provider)
      expect(key, provider).not.toBeNull()
      expect(typeof messages['zh-CN'][key!]).toBe('string')
      expect(messages['zh-CN'][key!].length).toBeGreaterThan(0)
      expect(typeof messages['en-US'][key!]).toBe('string')
      expect(messages['en-US'][key!].length).toBeGreaterThan(0)
    }
    expect(providerLabelKey('mystery')).toBeNull()
  })
})
