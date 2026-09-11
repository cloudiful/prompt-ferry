import { computed, type ComputedRef, type Ref } from 'vue'
import type {
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
  TokenPlanWindowUsage,
} from '@/generated/admin-api'
import {
  ccAsWindow,
  glmAsWindow,
  opencodeGoAsWindow,
  progressColor,
  remainingPercent,
} from './useTokenPlanWindowEntries'
import {
  getCachedTokenPlanUsage,
  prefetchTokenPlanUsage,
} from './useTokenPlanUsageCache'

export type TokenPlanBadges = {
  // Short-window remaining percent (0..100). Aggregated across all keys
  // for the endpoint, taking the worst (minimum). `null` means no short
  // window is reported (e.g. legacy / non-quota endpoint).
  short: number | null
  // Long-window remaining percent (0..100). Same aggregation rule.
  long: number | null
  // OpenRouter remaining-balance ratio (0..100). `null` when the key
  // has no finite cap or the endpoint isn't OpenRouter.
  openrouterRemaining: number | null
  // Raw OpenRouter limit/remaining pair, surfaced for the badge label
  // (e.g. "$4.21 / $10.00"). `null`/`null` when not applicable.
  openrouterLimit: number | null
  openrouterLimitRemaining: number | null
  // Raw cached payload — exposed so the popover can re-render the
  // breakdown without re-fetching.
  usage: TokenPlanUsageResponse | null
  // Per-key breakdown used by the endpoint list's crossfade rotation.
  // Empty when no snapshot is cached.
  keys: TokenPlanKeyBadges[]
}

// One key's derived badge values. `ok=false` keys carry no numbers so the
// rotation can skip them without re-reading the raw payload.
export type TokenPlanKeyBadges = {
  keyId: string
  keyLabel: string
  ok: boolean
  short: number | null
  long: number | null
  openrouterRemaining: number | null
  openrouterLimit: number | null
  openrouterLimitRemaining: number | null
}

const EMPTY_BADGES: TokenPlanBadges = {
  short: null,
  long: null,
  openrouterRemaining: null,
  openrouterLimit: null,
  openrouterLimitRemaining: null,
  usage: null,
  keys: [],
}

// Short window: min(5h / rolling / interval) across all keys. MiniMax
// surfaces each model_remains entry separately so we iterate them;
// CommandCode / OpencodeGo / GLM each carry a single short-window slot.
// We deliberately exclude OpenRouter (credit-balance-only, handled
// separately) and any empty `remaining_percent`.
function keyShortPercent(key: TokenPlanKeyUsage): number | null {
  const candidates: number[] = []
  for (const model of key.model_remains) {
    const window = model.interval as TokenPlanWindowUsage | null | undefined
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  const five = ccAsWindow(key.five_hour)
  if (five && five.remaining_percent != null) {
    candidates.push(remainingPercent(five))
  }
  const rolling = opencodeGoAsWindow(key.opencodego_rolling)
  if (rolling && rolling.remaining_percent != null) {
    candidates.push(remainingPercent(rolling))
  }
  const glmFive = glmAsWindow(key.glm_five_hour)
  if (glmFive && glmFive.remaining_percent != null) {
    candidates.push(remainingPercent(glmFive))
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

// Long window: min(weekly, monthly) across all keys. MiniMax weekly,
// CommandCode weekly (USD), OpencodeGo weekly+monthly, GLM weekly.
// Monthly-only shapes fall back to a single-entry min so the badge
// always renders something when the provider reports *any* long window.
function keyLongPercent(key: TokenPlanKeyUsage): number | null {
  const candidates: number[] = []
  for (const model of key.model_remains) {
    const window = model.weekly as TokenPlanWindowUsage | null | undefined
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  const weekly = ccAsWindow(key.weekly)
  if (weekly && weekly.remaining_percent != null) {
    candidates.push(remainingPercent(weekly))
  }
  for (const window of [
    opencodeGoAsWindow(key.opencodego_weekly),
    opencodeGoAsWindow(key.opencodego_monthly),
    glmAsWindow(key.glm_weekly),
  ]) {
    if (window && window.remaining_percent != null) {
      candidates.push(remainingPercent(window))
    }
  }
  return candidates.length === 0 ? null : Math.min(...candidates)
}

function openrouterPercent(key: TokenPlanKeyUsage): {
  remaining: number | null
  limit: number | null
  limitRemaining: number | null
} {
  const bal = key.openrouter_balance
  if (!bal) return { remaining: null, limit: null, limitRemaining: null }
  const limit =
    typeof bal.limit === 'number' && Number.isFinite(bal.limit)
      ? bal.limit
      : null
  const remaining =
    typeof bal.limit_remaining === 'number' &&
    Number.isFinite(bal.limit_remaining)
      ? bal.limit_remaining
      : null
  if (limit === null || remaining === null || limit <= 0) {
    return { remaining: null, limit, limitRemaining: remaining }
  }
  return {
    remaining: Math.max(0, Math.min(100, (remaining / limit) * 100)),
    limit,
    limitRemaining: remaining,
  }
}

function computeKeyBadges(key: TokenPlanKeyUsage): TokenPlanKeyBadges {
  if (!key.ok) {
    return {
      keyId: key.key_id,
      keyLabel: key.key_label,
      ok: false,
      short: null,
      long: null,
      openrouterRemaining: null,
      openrouterLimit: null,
      openrouterLimitRemaining: null,
    }
  }
  const or = openrouterPercent(key)
  return {
    keyId: key.key_id,
    keyLabel: key.key_label,
    ok: true,
    short: keyShortPercent(key),
    long: keyLongPercent(key),
    openrouterRemaining: or.remaining,
    openrouterLimit: or.limit,
    openrouterLimitRemaining: or.limitRemaining,
  }
}

function computeBadges(usage: TokenPlanUsageResponse | null): TokenPlanBadges {
  if (!usage) return EMPTY_BADGES
  const keys = usage.keys.map(computeKeyBadges)
  let short: number | null = null
  let long: number | null = null
  let orLimit: number | null = null
  let orRemaining: number | null = null
  let orRemainingPercent: number | null = null
  for (const key of keys) {
    if (!key.ok) continue
    if (key.short !== null)
      short = short === null ? key.short : Math.min(short, key.short)
    if (key.long !== null)
      long = long === null ? key.long : Math.min(long, key.long)
    if (key.openrouterRemaining !== null) {
      orRemainingPercent =
        orRemainingPercent === null
          ? key.openrouterRemaining
          : Math.min(orRemainingPercent, key.openrouterRemaining)
      if (key.openrouterLimit !== null) orLimit = key.openrouterLimit
      if (key.openrouterLimitRemaining !== null)
        orRemaining = key.openrouterLimitRemaining
    }
  }
  return {
    short,
    long,
    openrouterRemaining: orRemainingPercent,
    openrouterLimit: orLimit,
    openrouterLimitRemaining: orRemaining,
    usage,
    keys,
  }
}

// Reactive accessor for a single endpoint's badges. Reads the module
// cache synchronously and triggers a prefetch on the first observation
// so the badges become non-empty within one network round-trip after
// the table mounts. Accepts either a static string or a ref so the
// caller can pick whichever shape is convenient.
export function useTokenPlanBadges(
  endpointId: string | Ref<string>,
): ComputedRef<TokenPlanBadges> {
  const idRef = computed(() =>
    typeof endpointId === 'string' ? endpointId : endpointId.value,
  )
  // Kick off the prefetch eagerly so consumers that only read the ref
  // (no `watch`) still get fresh data on the next tick. The cache is
  // idempotent — re-mounting is a no-op for fresh entries.
  void prefetchTokenPlanUsage(idRef.value)
  return computed(() => computeBadges(getCachedTokenPlanUsage(idRef.value)))
}

// Re-export the adapt helpers so the badge template can color its pills
// the same way the dialog colors its progress bars.
export { progressColor, remainingPercent }

// `progressColor` expects a TokenPlanWindowUsage; the badge already holds
// the remaining percent, so we synthesize one with
// `remaining_percent = percent` and let progressColor fold used =
// 100 - remaining internally to drive the hue ramp. Shared by the desktop
// table and the mobile card.
export function badgeColorForPercent(percent: number): string {
  return progressColor({ end_at: null, remaining_percent: percent })
}

// Display descriptor for one badge pill. Keeping the label/color/title
// construction here means the desktop table and the mobile card only own
// their markup, not the provider-specific folding.
export type TokenPlanBadgePill = {
  label: string
  color: string
  title: string
}

// Build the pill descriptors for one key (or the aggregate). An
// OpenRouter balance collapses to a single pill; quota-bearing providers
// emit a short/long pair, each slot omitted when the provider reports no
// such window.
export function tokenPlanBadgePills(
  source: Pick<
    TokenPlanKeyBadges,
    | 'short'
    | 'long'
    | 'openrouterRemaining'
    | 'openrouterLimit'
    | 'openrouterLimitRemaining'
  >,
  t: TranslateFn,
): TokenPlanBadgePill[] {
  if (
    source.openrouterLimit !== null ||
    source.openrouterLimitRemaining !== null
  ) {
    const label =
      source.openrouterRemaining !== null
        ? `${t('tokenPlanOpenRouterRemaining')} ${source.openrouterRemaining.toFixed(0)}%`
        : t('tokenPlanNoQuota')
    const title =
      source.openrouterLimit !== null &&
      source.openrouterLimitRemaining !== null
        ? `${source.openrouterLimitRemaining.toFixed(2)} / ${source.openrouterLimit.toFixed(2)}`
        : ''
    return [
      {
        label,
        color:
          source.openrouterRemaining !== null
            ? badgeColorForPercent(source.openrouterRemaining)
            : '',
        title,
      },
    ]
  }
  const pills: TokenPlanBadgePill[] = []
  if (source.short !== null) {
    pills.push({
      label: `${t('tokenPlanShortBadge')} ${source.short.toFixed(0)}%`,
      color: badgeColorForPercent(source.short),
      title: t('tokenPlanShortBadgeHint'),
    })
  }
  if (source.long !== null) {
    pills.push({
      label: `${t('tokenPlanLongBadge')} ${source.long.toFixed(0)}%`,
      color: badgeColorForPercent(source.long),
      title: t('tokenPlanLongBadgeHint'),
    })
  }
  return pills
}
