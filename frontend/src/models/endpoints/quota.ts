import type { EndpointProvider } from '@/generated/admin-api'

// Issue #599 R2e.3: single quota-eligibility gate shared by the upstream
// list (desktop table + mobile card), the badge prefetch and the usage
// dialog. `ProviderEndpoint` (store rows) and `EndpointListItemView` (list
// rows) both satisfy this shape, so every caller decides from the same
// fields instead of keeping a private provider set.
export type QuotaEligibilitySource = {
  provider: EndpointProvider
  has_oauth_token?: boolean | null
}

// Providers whose token-plan quota exists without a login step; unchanged
// by the OpenAI subscription work.
const PLATFORM_QUOTA_PROVIDERS: ReadonlySet<EndpointProvider> = new Set([
  'minimax',
  'command_code',
  'opencode_go',
  'openrouter',
  'glm',
  'deepseek',
])

export function isQuotaEligible(source: QuotaEligibilitySource): boolean {
  // OpenAI only carries the ChatGPT subscription quota: a platform-API-key
  // endpoint has no token-plan windows, so the usage surface and its
  // `/usage` call stay off until a stored OAuth token exists.
  if (source.provider === 'openai') {
    return source.has_oauth_token === true
  }
  return PLATFORM_QUOTA_PROVIDERS.has(source.provider)
}
