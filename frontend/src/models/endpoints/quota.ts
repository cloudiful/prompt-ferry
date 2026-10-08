import type { DeepSeekBalance, EndpointProvider } from '@/generated/admin-api'

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

// Issue #589 P2c: the OpenAI Platform organization usage surface is a
// different axis from the ChatGPT subscription quota above. It reads the
// endpoint's separate Admin API Key, is organization-level (UTC month to
// date), and stays off unless an OpenAI endpoint has one stored. It never
// feeds routing weights or remaining-credit math.
export type OrganizationUsageEligibilitySource = {
  provider: EndpointProvider
  has_admin_api_key?: boolean | null
}

export function isOrganizationUsageEligible(
  source: OrganizationUsageEligibilitySource,
): boolean {
  return source.provider === 'openai' && source.has_admin_api_key === true
}

// Cache-level state of one token-plan usage snapshot. `loading` is a cold
// entry (nothing cached yet), `ready` carries a payload, and `error` is the
// negative-cache entry a failed cold fetch pins for one TTL window. A
// dedicated value is what keeps a failed request from rendering like a
// provider that simply reports nothing.
export type TokenPlanFetchState = 'loading' | 'ready' | 'error'

// One DeepSeek currency entry as the display surface consumes it. `total` is
// `null` when the amount is unknown — the provider omitted it or the parser
// could not coerce it — which is never the same as a reported `0`.
export type DeepSeekBalanceAmount = {
  currency: string
  total: number | null
}

// Badge state of the DeepSeek balance surface: the fetch state first, then the
// provider's own availability, which stays independent of the amounts.
export type DeepSeekBalanceState =
  'loading' | 'ready' | 'unavailable' | 'unknown' | 'error'

function reportedAmount(value: number | null | undefined): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

// The contract reports one entry per currency in a deterministic
// (currency, then amount) order, and a repeated currency stays complete. The
// entries are therefore carried through in payload order: no FX conversion,
// no cross-currency sum, and no merge of two entries into one figure.
export function deepseekBalanceAmounts(
  balance: DeepSeekBalance | null | undefined,
): DeepSeekBalanceAmount[] {
  if (!balance) return []
  return balance.balances.map((entry) => ({
    currency: entry.currency,
    total: reportedAmount(entry.total_balance),
  }))
}

export function deepseekBalanceState(
  fetchState: TokenPlanFetchState,
  amounts: readonly DeepSeekBalanceAmount[],
  available: boolean | null,
): DeepSeekBalanceState {
  // A cold cache or a failed fetch describes the request, not the balance, so
  // it outranks whatever the payload says.
  if (fetchState !== 'ready') return fetchState
  // `is_available` is the routing input and stays independent of the amounts:
  // a reported zero under an unavailable account is still a real zero.
  if (available === false) return 'unavailable'
  // Nothing known anywhere: unknown, never a rendered zero.
  return amounts.some((amount) => amount.total !== null) ? 'ready' : 'unknown'
}
