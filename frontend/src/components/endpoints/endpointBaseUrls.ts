// Endpoint provider base URL presets (issue #241).
//
// The frontend form ships a per-provider "set default base URL" helper
// that fires on provider switch. The defaults live here so the
// EndpointProviderFields.vue component stays focused on the form
// layout and the GLM preset selector child can reuse the constants
// without pulling in the parent.

export const MINIMAX_BASE_URLS = {
  cn: {
    openai: 'https://api.minimaxi.com',
    anthropic: 'https://api.minimaxi.com/anthropic',
  },
  global: {
    openai: 'https://api.minimax.io',
    anthropic: 'https://api.minimax.io/anthropic',
  },
} as const
export type MinimaxProtocol = 'openai' | 'anthropic'
export type MinimaxRegion = keyof typeof MINIMAX_BASE_URLS

export const COMMAND_CODE_BASE_URL =
  'https://api.commandcode.ai/provider' as const
export const OPENCODE_GO_BASE_URL = 'https://opencode.ai/zen/go' as const
export const OPENROUTER_BASE_URL = 'https://openrouter.ai/api' as const
export const GLM_DEFAULT_BASE_URL =
  'https://open.bigmodel.cn/api/coding/paas/v4' as const

/// Strip the trailing `/v1` (and a chained `…/v1/v1`) so the saved
/// value matches the canonical API root the runtime URL composer
/// expects. Mirrors the backend `normalize_endpoint_base_url`
/// behavior for non-GLM providers.
export function stripVersionSuffix(value: string): string {
  let normalized = value.trim()
  for (;;) {
    const withoutSlash = normalized.replace(/\/+$/, '')
    if (withoutSlash.endsWith('/v1')) {
      normalized = withoutSlash.slice(0, -3)
      continue
    }
    normalized = withoutSlash
    break
  }
  return normalized
}
