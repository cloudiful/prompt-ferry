import type {
  CachedQuotaResponse,
  CanonicalQuotaWindow,
  DeepSeekBalance,
  EndpointProvider,
} from '@/generated/admin-api'
import {
  canonicalRowsFromWindows,
  type CanonicalQuotaRow,
} from '@/composables/useCanonicalQuotaWindows'

// Issue #599 R2e.3: single quota-eligibility gate shared by the upstream
// list (desktop table + mobile card), the badge prefetch and the usage
// dialog. `ProviderEndpoint` (store rows) and `EndpointListItemView` (list
// rows) both satisfy this shape, so every caller decides from the same
// fields instead of keeping a private provider set.
export type QuotaEligibilitySource = {
  provider: EndpointProvider
  has_oauth_token?: boolean | null
  // The safe catalog answers the eligibility question on the server, so the
  // read-only rows carry the answer instead of a credential the viewer never
  // sees. An absent answer keeps the provider rule.
  quota_eligible?: boolean
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
  // The safe catalog carries the server-side answer, so the read-only view
  // never re-derives it from a credential the viewer cannot see.
  if (typeof source.quota_eligible === 'boolean') {
    return source.quota_eligible
  }
  // OpenAI only carries the ChatGPT subscription quota: a platform-API-key
  // endpoint has no token-plan windows, so the usage surface and its
  // `/usage` call stay off until a stored OAuth token exists.
  if (source.provider === 'openai') {
    return source.has_oauth_token === true
  }
  return PLATFORM_QUOTA_PROVIDERS.has(source.provider)
}

// One cached observation of the safe catalog, reduced to the canonical rows
// the shared quota helpers understand.
//
// The safe answer never names a key, so one observation is one cached
// snapshot, not one key. The observations are never averaged or summed: each
// snapshot keeps its own rows and its own freshness.
export type CatalogQuotaGroup = {
  observedAt: string | null
  rows: CanonicalQuotaRow[]
  stale: boolean
}

// The canonical rows of one cached observation.
//
// The safe windows are already canonical, so they are read through the
// unfiltered shared adapter instead of a second parsing path. The safe answer
// owns its rows: every window, unit, model, duration and reset stays in
// payload order, with no video filter and no collapse.
function catalogQuotaRows(
  windows: readonly CanonicalQuotaWindow[],
  nowMs: number,
): CanonicalQuotaRow[] {
  if (windows.length === 0) return []
  return canonicalRowsFromWindows(windows, nowMs)
}

// The cached observations of the safe catalog, as canonical row groups.
//
// An observation without a usable window carries no row: an unknown balance
// is never reported as a zero. Every row of a usable observation is kept.
export function catalogQuotaGroups(
  response: CachedQuotaResponse | null | undefined,
  nowMs: number,
): CatalogQuotaGroup[] {
  if (!response) return []
  const groups: CatalogQuotaGroup[] = []
  for (const observation of response.observations ?? []) {
    const rows = catalogQuotaRows(observation.windows ?? [], nowMs)
    if (rows.length === 0) continue
    groups.push({
      observedAt: observation.observed_at ?? null,
      rows,
      stale: observation.stale === true,
    })
  }
  return groups
}

// The freshness of the safe catalog answer.
//
// `unavailable` means the cache holds no usable observation, `stale` means
// the cached observation is past its freshness window. An unknown balance is
// never reported as a zero.
export type CatalogQuotaState = 'unavailable' | 'ready' | 'stale'

export function catalogQuotaState(
  response: CachedQuotaResponse | null | undefined,
  groups: CatalogQuotaGroup[],
): CatalogQuotaState {
  if (!response || response.available !== true || groups.length === 0) {
    return 'unavailable'
  }
  return groups.some((group) => group.stale) ? 'stale' : 'ready'
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
