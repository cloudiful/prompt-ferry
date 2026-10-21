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

// Cache-level state of one token-plan usage snapshot. `stale` preserves the
// last good payload without treating its quota as current; `error` is a pinned
// cold-fetch failure.
export type TokenPlanFetchState = 'loading' | 'ready' | 'stale' | 'error'

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
  'loading' | 'ready' | 'stale' | 'unavailable' | 'unknown' | 'error'

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
