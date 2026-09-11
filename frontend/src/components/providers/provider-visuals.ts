import type { MessageKey } from '@/i18n'

export type ProviderBrand = 'deepseek' | 'minimax' | 'opencode' | 'openrouter'

export type ProviderVisual =
  | { kind: 'brand'; brand: ProviderBrand }
  | { kind: 'letter'; letter: string }
  | { kind: 'fallback' }

// Brand marks for minimax/deepseek/openrouter/opencode are vendored from
// simple-icons (CC0). Providers without a vendored mark get a letter avatar;
// anything unknown falls back to a generic lucide glyph.
const BRAND_BY_PROVIDER: Record<string, ProviderBrand> = {
  deepseek: 'deepseek',
  minimax: 'minimax',
  opencode_go: 'opencode',
  openrouter: 'openrouter',
}

const LETTER_BY_PROVIDER: Record<string, string> = {
  command_code: 'CC',
  generic: 'G',
  glm: 'GLM',
}

const LABEL_KEY_BY_PROVIDER: Record<string, MessageKey> = {
  command_code: 'providerCommandCode',
  deepseek: 'providerDeepSeek',
  generic: 'providerGeneric',
  glm: 'providerGlm',
  minimax: 'providerMinimax',
  opencode_go: 'providerOpencodeGo',
  openrouter: 'providerOpenRouter',
}

export function providerVisual(provider: string): ProviderVisual {
  const brand = BRAND_BY_PROVIDER[provider]
  if (brand) return { kind: 'brand', brand }
  const letter = LETTER_BY_PROVIDER[provider]
  if (letter) return { kind: 'letter', letter }
  return { kind: 'fallback' }
}

export function providerLabelKey(provider: string): MessageKey | null {
  return LABEL_KEY_BY_PROVIDER[provider] ?? null
}
