import type {
  CanonicalQuotaWindow,
  CanonicalQuotaWindowKind,
  TokenPlanKeyUsage,
} from '@/generated/admin-api'

// Single canonical quota adapter (issue #844 P2). Every display helper —
// dialog progress entries, quota summary bands, badges — reads quota state
// through these rows instead of re-parsing provider fields per component.
// A key with `quota_windows` present uses it (an explicit `Some([])` is an
// authoritative empty and never falls back); otherwise a conservative legacy
// fallback mirrors the backend projection. Presence is truthful throughout:
// `Some(0)` is a real zero, `null` is absent or non-finite, and
// `balance`/`unlimited` rows never carry a synthetic percentage.
export type CanonicalQuotaRow = {
  sourceId: string
  kind: CanonicalQuotaWindowKind
  sourceModel: string | null
  unit: string | null
  durationSeconds: number | null
  isDefaultScope: boolean
  availability: 'known' | 'unknown'
  // Observed share `0..=100` before freshness is applied. `null` when the
  // provider reported nothing usable.
  observedRemainingPercent: number | null
  // Share to display: `null` when unknown or when `resetAt` has passed
  // (`expired` stays `true` so callers can tell the two apart without
  // mutating the observed amounts).
  remainingPercent: number | null
  current: number | null
  limit: number | null
  remaining: number | null
  resetAt: string | null
  expired: boolean
}

export type CanonicalQuotaBand = 'short' | 'long'

function finiteNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function finitePercent(value: unknown): number | null {
  const parsed = finiteNumber(value)
  return parsed === null || parsed < 0 || parsed > 100 ? null : parsed
}

function isExpired(resetAt: string | null, nowMs: number): boolean {
  if (typeof resetAt !== 'string' || resetAt.length === 0) return false
  const ts = Date.parse(resetAt)
  if (Number.isNaN(ts)) return false
  return ts <= nowMs
}

function validateRow(
  window: CanonicalQuotaWindow,
  nowMs: number,
): CanonicalQuotaRow {
  const observedRemainingPercent = finitePercent(window.remaining_percent)
  const expired = isExpired(window.reset_at ?? null, nowMs)
  const availability =
    window.availability === 'known' ? 'known' : ('unknown' as const)
  return {
    sourceId: window.source_id,
    kind: window.kind,
    sourceModel: window.source_model ?? null,
    unit: typeof window.unit === 'string' ? window.unit : null,
    durationSeconds:
      typeof window.duration_seconds === 'number' &&
      Number.isFinite(window.duration_seconds)
        ? window.duration_seconds
        : null,
    isDefaultScope: window.is_default_scope === true,
    availability,
    observedRemainingPercent,
    remainingPercent:
      availability === 'known' && !expired ? observedRemainingPercent : null,
    current: finiteNumber(window.current),
    limit: finiteNumber(window.limit),
    remaining: finiteNumber(window.remaining),
    resetAt: typeof window.reset_at === 'string' ? window.reset_at : null,
    expired,
  }
}

// Quantity caption for one row. An actual amount unit with a finite current
// and a positive finite cap renders `used / total`; a `balance` observed
// without a positive cap is an amount in its own right and renders its total
// alone, including a real zero or a negative value. Nothing else fakes a cap,
// a capless window keeps its amount hidden, and unknown values are never
// formatted with `toFixed`/`Infinity`.
export function canonicalQuantityCaption(
  row: CanonicalQuotaRow,
): string | null {
  if (row.unit == null) return null
  if (row.current == null) return null
  if (row.limit != null && row.limit > 0) {
    return `${row.current.toFixed(2)} / ${row.limit.toFixed(2)}`
  }
  return row.kind === 'balance' ? row.current.toFixed(2) : null
}

function isDefaultMinimaxModel(modelName: string): boolean {
  return modelName.trim().toLowerCase() === 'general'
}

function minimaxFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const rows: CanonicalQuotaRow[] = []
  for (const model of key.model_remains) {
    // Subscription-shaped models carry real durations and are projected
    // as custom rows, not minimax count slots.
    if (model.windows != null) continue
    const sourceModel = model.model_name
    const isDefaultScope = isDefaultMinimaxModel(sourceModel)
    for (const [slot, kind, window] of [
      ['interval', 'five_hour', model.interval],
      ['weekly', 'weekly', model.weekly],
    ] as const) {
      if (window == null) continue
      const limit =
        typeof window.total_count === 'number' &&
        Number.isFinite(window.total_count)
          ? window.total_count
          : null
      const current =
        typeof window.usage_count === 'number' &&
        Number.isFinite(window.usage_count)
          ? window.usage_count
          : null
      rows.push(
        validateRow(
          {
            source_id: `minimax:${sourceModel}:${slot}`,
            kind: kind as CanonicalQuotaWindowKind,
            source_model: sourceModel,
            unit: 'tokens',
            duration_seconds: null,
            is_default_scope: isDefaultScope,
            availability:
              finitePercent(window.remaining_percent) === null
                ? 'unknown'
                : 'known',
            remaining_percent: window.remaining_percent,
            current,
            limit,
            remaining:
              limit !== null && current !== null ? limit - current : null,
            reset_at: window.end_at,
          } as CanonicalQuotaWindow,
          nowMs,
        ),
      )
    }
  }
  return rows
}

function subscriptionFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const rows: CanonicalQuotaRow[] = []
  for (const model of key.model_remains) {
    const windows = model.windows
    if (windows == null) continue
    for (const window of windows) {
      rows.push(
        validateRow(
          {
            source_id: `subscription:${window.source_window}`,
            kind: 'custom',
            source_model: model.model_name,
            unit: null,
            duration_seconds: window.window_seconds,
            is_default_scope: true,
            availability: window.availability,
            remaining_percent: window.remaining_percent,
            current: null,
            limit: null,
            remaining: null,
            reset_at: window.reset_at,
          } as CanonicalQuotaWindow,
          nowMs,
        ),
      )
    }
  }
  return rows
}

function commandCodeFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const rows: CanonicalQuotaRow[] = []
  for (const [slot, kind, window] of [
    ['command_code:five_hour', 'five_hour', key.five_hour],
    ['command_code:weekly', 'weekly', key.weekly],
  ] as const) {
    if (window == null) continue
    const current = finiteNumber(window.used)
    const cap = finiteNumber(window.cap)
    rows.push(
      validateRow(
        {
          source_id: slot,
          kind: kind as CanonicalQuotaWindowKind,
          source_model: null,
          unit: 'currency',
          duration_seconds: null,
          is_default_scope: true,
          availability:
            finitePercent(window.remaining_percent) === null &&
            current === null &&
            cap === null
              ? 'unknown'
              : 'known',
          remaining_percent: window.remaining_percent,
          current,
          limit: cap,
          remaining: current !== null && cap !== null ? cap - current : null,
          reset_at: window.reset_at,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    )
  }
  if (key.balances != null) {
    const remaining = finiteNumber(key.balances.remaining_credits)
    const limit = finiteNumber(
      key.balances.monthly_credits +
        key.balances.purchased_credits +
        key.balances.free_credits,
    )
    rows.push(
      validateRow(
        {
          source_id: 'command_code:balances',
          kind: 'balance',
          source_model: null,
          unit: 'credits',
          duration_seconds: null,
          is_default_scope: true,
          availability:
            remaining === null && limit === null ? 'unknown' : 'known',
          remaining_percent: null,
          current: remaining,
          limit,
          remaining,
          reset_at: null,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    )
  }
  return rows
}

function opencodeGoFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const rows: CanonicalQuotaRow[] = []
  for (const [slot, kind, window] of [
    ['opencode_go:rolling', 'rolling', key.opencodego_rolling],
    ['opencode_go:weekly', 'weekly', key.opencodego_weekly],
    ['opencode_go:monthly', 'monthly', key.opencodego_monthly],
  ] as const) {
    if (window == null) continue
    const used = finiteNumber(window.percent)
    rows.push(
      validateRow(
        {
          source_id: slot,
          kind: kind as CanonicalQuotaWindowKind,
          source_model: null,
          unit: null,
          duration_seconds: null,
          is_default_scope: true,
          availability: used === null ? 'unknown' : 'known',
          remaining_percent: used === null ? null : 100 - used,
          current: null,
          limit: null,
          remaining: null,
          reset_at: window.resets_at,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    )
  }
  return rows
}

function glmFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  // Legacy GLM amount presence is ambiguous (a missing amount defaults to
  // `0`), so the fallback keeps an honestly reported percent while leaving
  // quantities unknown instead of pretending a defaulted `0` was observed.
  const rows: CanonicalQuotaRow[] = []
  for (const [slot, kind, window] of [
    ['glm:five_hour', 'five_hour', key.glm_five_hour],
    ['glm:weekly', 'weekly', key.glm_weekly],
  ] as const) {
    if (window == null) continue
    const used = finiteNumber(window.percentage)
    rows.push(
      validateRow(
        {
          source_id: slot,
          kind: kind as CanonicalQuotaWindowKind,
          source_model: null,
          unit: null,
          duration_seconds: null,
          is_default_scope: true,
          availability: used === null ? 'unknown' : 'known',
          remaining_percent: used === null ? null : 100 - used,
          current: null,
          limit: null,
          remaining: null,
          reset_at: window.next_reset_at,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    )
  }
  return rows
}

function openrouterFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const balance = key.openrouter_balance
  if (balance == null) return []
  const limit = finiteNumber(balance.limit)
  const remaining = finiteNumber(balance.limit_remaining)
  if (limit !== null && limit > 0) {
    return [
      validateRow(
        {
          source_id: 'openrouter:balance',
          kind: 'balance',
          source_model: null,
          unit: 'credits',
          duration_seconds: null,
          is_default_scope: true,
          availability:
            remaining === null && limit === null ? 'unknown' : 'known',
          remaining_percent: null,
          current: remaining,
          limit,
          remaining,
          reset_at: null,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    ]
  }
  if (limit !== null) {
    // Zero/negative cap cannot spend: known exhausted balance, never a
    // synthetic percentage.
    return [
      validateRow(
        {
          source_id: 'openrouter:balance',
          kind: 'balance',
          source_model: null,
          unit: 'credits',
          duration_seconds: null,
          is_default_scope: true,
          availability: 'known',
          remaining_percent: null,
          current: 0,
          limit,
          remaining: 0,
          reset_at: null,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    ]
  }
  if (key.openrouter_spend != null) {
    return [
      validateRow(
        {
          source_id: 'openrouter:unlimited',
          kind: 'unlimited',
          source_model: null,
          unit: 'credits',
          duration_seconds: null,
          is_default_scope: true,
          availability: 'known',
          remaining_percent: null,
          current: null,
          limit: null,
          remaining: null,
          reset_at: null,
        } as CanonicalQuotaWindow,
        nowMs,
      ),
    ]
  }
  return []
}

function deepseekFallbackRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  const balance = key.deepseek_balance
  if (balance == null) return []
  return balance.balances.map((entry) =>
    validateRow(
      {
        source_id: `deepseek:${entry.currency.toUpperCase()}`,
        kind: 'balance',
        source_model: null,
        unit: entry.currency.toUpperCase(),
        duration_seconds: null,
        is_default_scope: true,
        availability:
          finiteNumber(entry.total_balance) === null ? 'unknown' : 'known',
        remaining_percent: null,
        current: entry.total_balance,
        limit: null,
        remaining: entry.total_balance,
        reset_at: null,
      } as CanonicalQuotaWindow,
      nowMs,
    ),
  )
}

// Validated rows for one key: canonical observations when present (an
// explicit empty stays empty and never falls back), otherwise the
// conservative legacy projection. Failed keys carry no rows, so a failure
// is unknown rather than available.
export function toCanonicalQuotaRows(
  key: TokenPlanKeyUsage,
  nowMs: number,
): CanonicalQuotaRow[] {
  if (!key.ok) return []
  if (key.quota_windows != null) {
    return key.quota_windows.map((window) => validateRow(window, nowMs))
  }
  return [
    ...minimaxFallbackRows(key, nowMs),
    ...subscriptionFallbackRows(key, nowMs),
    ...commandCodeFallbackRows(key, nowMs),
    ...opencodeGoFallbackRows(key, nowMs),
    ...glmFallbackRows(key, nowMs),
    ...openrouterFallbackRows(key, nowMs),
    ...deepseekFallbackRows(key, nowMs),
  ]
}

function customBand(durationSeconds: number | null): CanonicalQuotaBand | null {
  if (
    durationSeconds === null ||
    !Number.isFinite(durationSeconds) ||
    durationSeconds <= 0
  ) {
    return null
  }
  return durationSeconds < 86_400 ? 'short' : 'long'
}

// Shared default-row selector for summary bands and badges: short covers
// `five_hour`/`rolling`, long covers `weekly`/`monthly` (model-aware —
// detail models never bind the pool), and `custom` rows band by their real
// duration with the same default-scope guard. `balance`/`unlimited` rows
// never report a percentage.
export function canonicalBandPercent(
  rows: readonly CanonicalQuotaRow[],
  band: CanonicalQuotaBand,
): number | null {
  const values: number[] = []
  for (const row of rows) {
    if (row.remainingPercent == null) continue
    switch (row.kind) {
      case 'five_hour':
      case 'rolling':
        if (row.sourceModel != null && !row.isDefaultScope) continue
        if (band === 'short') values.push(row.remainingPercent)
        break
      case 'weekly':
      case 'monthly':
        if (row.sourceModel != null && !row.isDefaultScope) continue
        if (band === 'long') values.push(row.remainingPercent)
        break
      case 'custom':
        if (row.sourceModel != null && !row.isDefaultScope) continue
        if (customBand(row.durationSeconds) === band) {
          values.push(row.remainingPercent)
        }
        break
      default:
        break
    }
  }
  return values.length === 0 ? null : Math.min(...values)
}

// Convenience for callers that hold a key instead of rows.
export function keyBandPercent(
  key: TokenPlanKeyUsage,
  band: CanonicalQuotaBand,
  nowMs: number = Date.now(),
): number | null {
  return canonicalBandPercent(toCanonicalQuotaRows(key, nowMs), band)
}
