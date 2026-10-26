import { act, fireEvent, render, screen } from '@testing-library/react'
import { createElement } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const endpointApiMock = vi.hoisted(() => ({
  fetchTokenPlanUsage: vi.fn(),
}))

vi.mock('@/stores/endpoints-api', () => endpointApiMock)

import type {
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
  SubscriptionWindowUsage,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { fetchTokenPlanUsage } from '@/stores/endpoints-api'
import {
  __resetTokenPlanCacheForTests,
  getCachedTokenPlanUsage,
  getTokenPlanUsageRevision,
  getTokenPlanUsageSnapshot,
  isTokenPlanUsageFresh,
  prefetchTokenPlanBatch,
  prefetchTokenPlanUsage,
  requestTokenPlanUsage,
  subscribeTokenPlanUsage,
  subscribeTokenPlanUsageExpiry,
  subscribeTokenPlanUsageRevision,
  TOKEN_PLAN_CACHE_TTL_MS,
} from '@/lib/quota-cache'
import {
  __resetTokenPlanForegroundForTests,
  registerTokenPlanForegroundIds,
} from '@/lib/quota-foreground'
import {
  getDisplayTimeZoneSnapshot,
  setDisplayTimeZone as setReactDisplayTimeZone,
  useDisplayTimeZone,
} from '@/lib/display-timezone'
import { DISPLAY_TIME_ZONE_STORAGE_KEY } from '@/storage'
import {
  chartThemeFor,
  DARK_CHART_THEME,
  LIGHT_CHART_THEME,
} from '@/theme/chart-tokens'
import {
  formatDateOnly as formatDateOnlyCore,
  formatDateTime as formatDateTimeCore,
  formatNumber,
} from '@/format/datetime-core'
import {
  effectiveTimeZoneFor,
  resolveDisplayTimeZone,
} from '@/format/timezone-core'
import { formatTokenQuantity } from '@/format/quantity'
import {
  createQuotaTrendPanelOption as createCoreTrendOption,
  duplicateKeyLabels as findDuplicateTrendLabels,
  escapeHtml as escapeTrendHtml,
  groupQuotaTrendSeries as groupTrendSeries,
  quotaTrendLegendEntries as createTrendLegendEntries,
  quotaTrendSeriesName as trendSeriesName,
  shortKeyId as shortTrendKeyId,
} from '@/quota-observation-charts-core'
import {
  subscriptionQuotaMinimumForBand,
  subscriptionQuotaMinimumRemainingPercent,
  subscriptionQuotaWindowCount,
  subscriptionQuotaWindows,
  subscriptionWindowBand,
  subscriptionWindowLabel,
  subscriptionWindowProgressColor,
  subscriptionWindowRemainingPercent,
  subscriptionWindowResetMs,
} from '@/models/endpoints/subscription-window'
import {
  formatMoney,
  formatOpenRouterCredits,
  formatResetDuration,
  knownRemainingPercent,
  progressColor,
} from '@/models/endpoints/token-plan-window'
import {
  badgeColorForPercent,
  computeTokenPlanBadges,
  tokenPlanBadgePills,
} from '@/models/endpoints/quota-badges'
import {
  canonicalBandPercent,
  toCanonicalQuotaRows,
} from '@/composables/useCanonicalQuotaWindows'
import { summarizeTokenPlanQuota } from '@/composables/tokenPlanQuotaSummary'
import { minimaxQuotaKey, usage } from './helpers/token-plan-quota-fixtures'
import { messages, type Locale, type MessageKey } from '@/i18n'

const originalVisibilityDescriptor = Object.getOwnPropertyDescriptor(
  document,
  'visibilityState',
)

function setVisibility(value: 'visible' | 'hidden'): void {
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    value,
  })
}

function fireResume(): void {
  document.dispatchEvent(new Event('visibilitychange'))
  window.dispatchEvent(new Event('focus'))
  window.dispatchEvent(new Event('pageshow'))
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

async function flushMicrotasks(count = 24): Promise<void> {
  for (let index = 0; index < count; index += 1) await Promise.resolve()
}

function translate(localeValue: Locale): TranslateFn {
  return ((key: MessageKey, values?: unknown) => {
    const template = messages[localeValue][key] as string
    if (values === null || typeof values !== 'object') return template
    return template.replace(/\{(\w+)\}/g, (match, name: string) =>
      name in (values as Record<string, unknown>)
        ? String((values as Record<string, unknown>)[name])
        : match,
    )
  }) as unknown as TranslateFn
}

const t = translate('en-US')

function emptyUsage(
  provider: TokenPlanUsageResponse['provider'] = 'minimax',
): TokenPlanUsageResponse {
  return { keys: [], provider, provider_region: null }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date', 'setTimeout', 'clearTimeout'] })
  vi.setSystemTime(new Date('2026-10-24T00:00:00.000Z'))
  setVisibility('visible')
  vi.mocked(fetchTokenPlanUsage).mockReset()
  vi.mocked(fetchTokenPlanUsage).mockImplementation(async () => emptyUsage())
  __resetTokenPlanForegroundForTests()
  __resetTokenPlanCacheForTests()
})

afterEach(() => {
  __resetTokenPlanForegroundForTests()
  __resetTokenPlanCacheForTests()
  vi.useRealTimers()
  if (originalVisibilityDescriptor) {
    Object.defineProperty(
      document,
      'visibilityState',
      originalVisibilityDescriptor,
    )
  } else {
    Reflect.deleteProperty(document, 'visibilityState')
  }
})

describe('React quota singleton', () => {
  it('coalesces cold reads and serves a fresh snapshot without another request', async () => {
    const firstPayload = emptyUsage('deepseek')
    const pending = deferred<TokenPlanUsageResponse>()
    vi.mocked(fetchTokenPlanUsage).mockReturnValueOnce(pending.promise)

    const first = requestTokenPlanUsage('endpoint-cold')
    const second = requestTokenPlanUsage('endpoint-cold')
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)
    expect(getTokenPlanUsageSnapshot('endpoint-cold').state).toBe('loading')
    pending.resolve(firstPayload)
    const [firstResult, secondResult] = await Promise.all([first, second])

    expect(firstResult).toMatchObject({
      failed: false,
      requested: true,
      state: 'ready',
      usage: firstPayload,
    })
    expect(secondResult.usage).toBe(firstPayload)
    const readySnapshot = getTokenPlanUsageSnapshot('endpoint-cold')
    expect(Object.isFrozen(readySnapshot)).toBe(true)
    expect(getTokenPlanUsageSnapshot('endpoint-cold')).toBe(readySnapshot)
    expect((await requestTokenPlanUsage('endpoint-cold')).requested).toBe(false)
    expect(getCachedTokenPlanUsage('endpoint-cold')).toBe(firstPayload)
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)
  })

  it('keeps empty identifiers and cold failures network-free after one negative entry', async () => {
    expect(await requestTokenPlanUsage('')).toMatchObject({
      cause: null,
      failed: false,
      requested: false,
      state: 'loading',
      usage: null,
    })
    expect(vi.mocked(fetchTokenPlanUsage)).not.toHaveBeenCalled()

    const cause = new Error('synthetic quota failure')
    vi.mocked(fetchTokenPlanUsage).mockRejectedValueOnce(cause)
    const failed = await requestTokenPlanUsage('endpoint-negative')
    expect(failed).toMatchObject({
      cause,
      failed: true,
      requested: true,
      state: 'error',
      usage: null,
    })
    expect(getTokenPlanUsageSnapshot('endpoint-negative').state).toBe('error')

    const replayed = await requestTokenPlanUsage('endpoint-negative')
    expect(replayed).toMatchObject({
      cause: null,
      failed: false,
      requested: false,
      state: 'error',
    })
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)

    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage())
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS)
    expect((await requestTokenPlanUsage('endpoint-negative')).requested).toBe(
      true,
    )
    await flushMicrotasks()
    expect(getTokenPlanUsageSnapshot('endpoint-negative').state).toBe('ready')
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(2)
  })

  it('serves stale data immediately, coalesces automatic revalidation, and backs off after warm failure', async () => {
    const original = emptyUsage('deepseek')
    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(original)
    await prefetchTokenPlanUsage('endpoint-swr')
    expect(isTokenPlanUsageFresh('endpoint-swr')).toBe(true)

    const refreshed = emptyUsage('command_code')
    const pending = deferred<TokenPlanUsageResponse>()
    vi.mocked(fetchTokenPlanUsage).mockReturnValueOnce(pending.promise)
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    expect(isTokenPlanUsageFresh('endpoint-swr')).toBe(false)

    const stale = await requestTokenPlanUsage('endpoint-swr')
    const coalesced = await requestTokenPlanUsage('endpoint-swr')
    expect(stale).toMatchObject({
      requested: true,
      state: 'stale',
      usage: original,
    })
    expect(coalesced).toMatchObject({
      requested: true,
      state: 'stale',
      usage: original,
    })
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(2)
    pending.resolve(refreshed)
    await flushMicrotasks()
    expect(getCachedTokenPlanUsage('endpoint-swr')).toBe(refreshed)
    expect(getTokenPlanUsageSnapshot('endpoint-swr').state).toBe('ready')

    const failure = new Error('synthetic warm failure')
    vi.mocked(fetchTokenPlanUsage).mockRejectedValueOnce(failure)
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    const staleAfterTtl = await requestTokenPlanUsage('endpoint-swr')
    expect(staleAfterTtl).toMatchObject({
      failed: false,
      requested: true,
      state: 'stale',
      usage: refreshed,
    })
    await flushMicrotasks()
    expect(getCachedTokenPlanUsage('endpoint-swr')).toBe(refreshed)
    expect(getTokenPlanUsageSnapshot('endpoint-swr').state).toBe('stale')

    const cachedFailure = await requestTokenPlanUsage('endpoint-swr')
    expect(cachedFailure).toMatchObject({
      cause: null,
      failed: false,
      requested: false,
      state: 'stale',
    })
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(3)
    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage('openai'))
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS)
    expect((await requestTokenPlanUsage('endpoint-swr')).requested).toBe(true)
    await flushMicrotasks()
    expect(getTokenPlanUsageSnapshot('endpoint-swr').state).toBe('ready')
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(4)
  })

  it('isolates forced and automatic flights and keeps the last successful response', async () => {
    const original = emptyUsage('minimax')
    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(original)
    await prefetchTokenPlanUsage('endpoint-force-isolation')
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)

    const automaticPayload = emptyUsage('openai')
    const forcedPayload = emptyUsage('deepseek')
    const automatic = deferred<TokenPlanUsageResponse>()
    const forced = deferred<TokenPlanUsageResponse>()
    vi.mocked(fetchTokenPlanUsage).mockImplementation((endpointId, force) =>
      force ? forced.promise : automatic.promise,
    )

    expect(
      (await requestTokenPlanUsage('endpoint-force-isolation')).usage,
    ).toBe(original)
    const forcedRead = requestTokenPlanUsage('endpoint-force-isolation', {
      refresh: true,
    })
    const duplicateForceRead = requestTokenPlanUsage(
      'endpoint-force-isolation',
      {
        refresh: true,
      },
    )
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(3)
    expect(vi.mocked(fetchTokenPlanUsage).mock.calls.slice(1)).toEqual([
      ['endpoint-force-isolation', false],
      ['endpoint-force-isolation', true],
    ])

    forced.resolve(forcedPayload)
    const forceResult = await forcedRead
    const duplicateResult = await duplicateForceRead
    expect(forceResult.usage).toBe(forcedPayload)
    expect(duplicateResult.usage).toBe(forcedPayload)
    expect(getCachedTokenPlanUsage('endpoint-force-isolation')).toBe(
      forcedPayload,
    )

    automatic.resolve(automaticPayload)
    await flushMicrotasks()
    expect(getCachedTokenPlanUsage('endpoint-force-isolation')).toBe(
      automaticPayload,
    )
  })

  it('deduplicates batch ids and bounds real requests to four workers', async () => {
    let active = 0
    let maximumActive = 0
    const gates: Array<() => void> = []
    vi.mocked(fetchTokenPlanUsage).mockImplementation(async () => {
      active += 1
      maximumActive = Math.max(maximumActive, active)
      await new Promise<void>((resolve) => gates.push(resolve))
      active -= 1
      return emptyUsage()
    })

    const batch = prefetchTokenPlanBatch(
      [
        'batch-a',
        'batch-a',
        'batch-b',
        'batch-c',
        'batch-d',
        'batch-e',
        'batch-f',
        'batch-g',
        'batch-h',
      ],
      4,
    )
    await flushMicrotasks()
    expect(maximumActive).toBe(4)
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(4)

    while (gates.length > 0) {
      gates.splice(0, 4).forEach((release) => release())
      await flushMicrotasks()
    }
    await batch
    expect(maximumActive).toBeLessThanOrEqual(4)
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(8)
    expect(vi.mocked(fetchTokenPlanUsage).mock.calls.map(([id]) => id)).toEqual(
      [
        'batch-a',
        'batch-b',
        'batch-c',
        'batch-d',
        'batch-e',
        'batch-f',
        'batch-g',
        'batch-h',
      ],
    )
  })

  it('checks batch eligibility immediately before starting each queued id', async () => {
    const first = deferred<TokenPlanUsageResponse>()
    vi.mocked(fetchTokenPlanUsage).mockImplementation((endpointId) =>
      endpointId === 'queue-first'
        ? first.promise
        : Promise.resolve(emptyUsage()),
    )
    let queuedEligible = true
    const batch = prefetchTokenPlanBatch(
      ['queue-first', 'queue-skip', 'queue-last'],
      1,
      (endpointId) => endpointId !== 'queue-skip' || queuedEligible,
    )
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage).mock.calls.map(([id]) => id)).toEqual(
      ['queue-first'],
    )

    queuedEligible = false
    first.resolve(emptyUsage())
    await batch
    expect(vi.mocked(fetchTokenPlanUsage).mock.calls.map(([id]) => id)).toEqual(
      ['queue-first', 'queue-last'],
    )
  })

  it('does not let a batch wait on or join a forced cold flight', async () => {
    const pendingForce = deferred<TokenPlanUsageResponse>()
    vi.mocked(fetchTokenPlanUsage).mockReturnValueOnce(pendingForce.promise)
    const forcedRead = requestTokenPlanUsage('endpoint-batch-force', {
      refresh: true,
    })
    const batch = prefetchTokenPlanBatch(['endpoint-batch-force'])
    await batch
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)
    expect(getTokenPlanUsageSnapshot('endpoint-batch-force').state).toBe(
      'loading',
    )
    pendingForce.resolve(emptyUsage())
    await forcedRead
  })

  it('uses one nearest expiry timer and publishes each due id set once', async () => {
    const expired: string[][] = []
    const stopExpiry = subscribeTokenPlanUsageExpiry((ids) => {
      expired.push([...ids])
    })
    const a = emptyUsage('minimax')
    const b = emptyUsage('deepseek')
    vi.mocked(fetchTokenPlanUsage)
      .mockResolvedValueOnce(a)
      .mockResolvedValueOnce(b)

    await prefetchTokenPlanUsage('endpoint-expiry-a')
    expect(vi.getTimerCount()).toBe(1)
    await vi.advanceTimersByTimeAsync(10_000)
    await prefetchTokenPlanUsage('endpoint-expiry-b')
    expect(vi.getTimerCount()).toBe(1)

    const revisionBeforeExpiry = getTokenPlanUsageRevision()
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS - 10_000)
    expect(expired).toEqual([['endpoint-expiry-a']])
    expect(getTokenPlanUsageSnapshot('endpoint-expiry-a').state).toBe('stale')
    expect(getTokenPlanUsageSnapshot('endpoint-expiry-b').state).toBe('ready')
    expect(getTokenPlanUsageRevision()).toBe(revisionBeforeExpiry + 1)
    expect(vi.getTimerCount()).toBe(1)

    await vi.advanceTimersByTimeAsync(10_000)
    expect(expired).toEqual([['endpoint-expiry-a'], ['endpoint-expiry-b']])
    expect(vi.getTimerCount()).toBe(0)
    stopExpiry()
  })

  it('notifies per-endpoint and revision subscribers and releases them cleanly', async () => {
    const endpointListener = vi.fn()
    const revisionListener = vi.fn()
    const stopEndpoint = subscribeTokenPlanUsage(
      'endpoint-subscribed',
      endpointListener,
    )
    const stopRevision = subscribeTokenPlanUsageRevision(revisionListener)
    const before = getTokenPlanUsageRevision()
    await prefetchTokenPlanUsage('endpoint-subscribed')
    expect(endpointListener).toHaveBeenCalled()
    expect(revisionListener).toHaveBeenCalled()
    expect(getTokenPlanUsageRevision()).toBeGreaterThan(before)

    const endpointCalls = endpointListener.mock.calls.length
    const revisionCalls = revisionListener.mock.calls.length
    stopEndpoint()
    stopRevision()
    await requestTokenPlanUsage('endpoint-subscribed', { refresh: true })
    expect(endpointListener).toHaveBeenCalledTimes(endpointCalls)
    expect(revisionListener).toHaveBeenCalledTimes(revisionCalls)
  })

  it('does not create autonomous expiry timers without a browser window', async () => {
    const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: undefined,
    })
    try {
      await prefetchTokenPlanUsage('server-cache')
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      if (previousWindow) {
        Object.defineProperty(globalThis, 'window', previousWindow)
      } else {
        Reflect.deleteProperty(globalThis, 'window')
      }
    }
  })
})

describe('foreground refresh engine', () => {
  it('does not fetch on registration and defers stale refresh while hidden', async () => {
    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage())
    await prefetchTokenPlanUsage('endpoint-hidden')
    const release = registerTokenPlanForegroundIds(['endpoint-hidden'])
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)

    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage('deepseek'))
    setVisibility('hidden')
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)
    expect(getTokenPlanUsageSnapshot('endpoint-hidden').state).toBe('stale')

    setVisibility('visible')
    fireResume()
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(2)
    expect(getTokenPlanUsageSnapshot('endpoint-hidden').state).toBe('ready')
    release()
  })

  it('coalesces visible resume events and shares refcounted endpoint ownership', async () => {
    vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage())
    await prefetchTokenPlanUsage('endpoint-coalesced')
    const releaseTable = registerTokenPlanForegroundIds(['endpoint-coalesced'])
    const releaseCard = registerTokenPlanForegroundIds(['endpoint-coalesced'])
    vi.mocked(fetchTokenPlanUsage).mockClear()
    vi.mocked(fetchTokenPlanUsage).mockResolvedValue(emptyUsage('openai'))
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)

    fireResume()
    document.dispatchEvent(new Event('visibilitychange'))
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(1)
    releaseTable()

    vi.mocked(fetchTokenPlanUsage).mockResolvedValue(emptyUsage('deepseek'))
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(2)
    releaseCard()

    vi.mocked(fetchTokenPlanUsage).mockClear()
    fireResume()
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).not.toHaveBeenCalled()
  })

  it('invalidates queued old-epoch work after release and immediate re-registration', async () => {
    const ids = ['epoch-a', 'epoch-b', 'epoch-c', 'epoch-d', 'epoch-e']
    for (const endpointId of ids) {
      vi.mocked(fetchTokenPlanUsage).mockResolvedValueOnce(emptyUsage())
      await prefetchTokenPlanUsage(endpointId)
    }
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    vi.mocked(fetchTokenPlanUsage).mockClear()

    const gates: Array<() => void> = []
    vi.mocked(fetchTokenPlanUsage).mockImplementation(async () => {
      await new Promise<void>((resolve) => gates.push(resolve))
      return emptyUsage('deepseek')
    })
    const releaseOld = registerTokenPlanForegroundIds(ids)
    fireResume()
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(4)

    releaseOld()
    const releaseNew = registerTokenPlanForegroundIds(ids)
    gates.splice(0).forEach((release) => release())
    await flushMicrotasks()
    expect(vi.mocked(fetchTokenPlanUsage)).toHaveBeenCalledTimes(4)
    releaseNew()
  })

  it('removes listeners when the final foreground consumer releases', async () => {
    const documentAdd = vi.spyOn(document, 'addEventListener')
    const documentRemove = vi.spyOn(document, 'removeEventListener')
    const windowAdd = vi.spyOn(window, 'addEventListener')
    const windowRemove = vi.spyOn(window, 'removeEventListener')

    const release = registerTokenPlanForegroundIds(['endpoint-listeners'])
    expect(documentAdd).toHaveBeenCalledWith(
      'visibilitychange',
      expect.any(Function),
    )
    expect(windowAdd).toHaveBeenCalledWith('focus', expect.any(Function))
    expect(windowAdd).toHaveBeenCalledWith('pageshow', expect.any(Function))
    release()
    expect(documentRemove).toHaveBeenCalledWith(
      'visibilitychange',
      expect.any(Function),
    )
    expect(windowRemove).toHaveBeenCalledWith('focus', expect.any(Function))
    expect(windowRemove).toHaveBeenCalledWith('pageshow', expect.any(Function))
    documentAdd.mockRestore()
    documentRemove.mockRestore()
    windowAdd.mockRestore()
    windowRemove.mockRestore()
  })
})

describe('neutral quota calculations and formatters', () => {
  it('keeps canonical empty, unknown, and real zero observations distinct', () => {
    const unknown = minimaxQuotaKey('unknown', 'available', null, null)
    const zero = minimaxQuotaKey('zero', 'available', 0, null)
    expect(
      canonicalBandPercent(toCanonicalQuotaRows(unknown, Date.now()), 'short'),
    ).toBeNull()
    expect(
      canonicalBandPercent(toCanonicalQuotaRows(zero, Date.now()), 'short'),
    ).toBe(0)

    const authoritativeEmpty = minimaxQuotaKey('empty', 'available', 25, 50)
    authoritativeEmpty.quota_windows = []
    expect(toCanonicalQuotaRows(authoritativeEmpty, Date.now())).toEqual([])
  })

  it('keeps stale exhausted observations historical and only marks ready pools exhausted', () => {
    const exhausted = minimaxQuotaKey('stale-exhausted', 'exhausted', 0, 0)
    const stale = summarizeTokenPlanQuota('minimax', [exhausted], 'stale')
    expect(stale).toMatchObject({
      allExhausted: false,
      short: 0,
      long: 0,
      state: 'unknown',
      usableKeys: [],
    })
    const ready = summarizeTokenPlanQuota('minimax', [exhausted], 'ready')
    expect(ready).toMatchObject({
      allExhausted: true,
      short: 0,
      long: 0,
      state: 'exhausted',
    })
  })

  it('preserves provider-specific badge ordering and unknown-versus-zero balances', () => {
    const minimax = usage('minimax', [
      minimaxQuotaKey('window-key', 'available', 40, 80),
    ])
    const windowSource = computeTokenPlanBadges({
      state: 'ready',
      usage: minimax,
    })
    expect(
      tokenPlanBadgePills(windowSource, t, 'en-US', 'UTC').map(
        (pill) => pill.label,
      ),
    ).toEqual(['Short 40%', 'Long 80%'])

    const openRouterKey = minimaxQuotaKey('openrouter-key', null, null, null)
    openRouterKey.openrouter_balance = {
      is_free_tier: false,
      limit: 20,
      limit_remaining: 10,
    }
    openRouterKey.openrouter_spend = {
      daily: 2,
      monthly: 9,
      usage: 4,
      weekly: 6,
    }
    const openRouter = computeTokenPlanBadges({
      state: 'ready',
      usage: usage('openrouter', [openRouterKey]),
    })
    expect(
      tokenPlanBadgePills(openRouter, t, 'en-US', 'UTC').map(
        (pill) => pill.label,
      ),
    ).toEqual(['Remaining $10.00', 'Daily usage $2.00'])

    const deepSeekKey = minimaxQuotaKey('deepseek-key', null, null, null)
    deepSeekKey.deepseek_balance = {
      balances: [
        { currency: 'USD', total_balance: 0 },
        { currency: 'CNY', total_balance: null },
      ],
      is_available: true,
    }
    const deepSeek = computeTokenPlanBadges({
      state: 'ready',
      usage: { ...usage('deepseek', [deepSeekKey]), local_today_tokens: 1200 },
    })
    expect(
      tokenPlanBadgePills(deepSeek, t, 'en-US', 'UTC').map(
        (pill) => pill.label,
      ),
    ).toEqual(['$0.00', 'Balance unknown', 'Daily tokens 1.2K'])
    expect(knownRemainingPercent({ end_at: null, remaining_percent: 0 })).toBe(
      0,
    )
    expect(
      knownRemainingPercent({ end_at: null, remaining_percent: null }),
    ).toBeNull()
  })

  it('keeps the badge layer on the shared pure calculation layer', () => {
    const payload = usage('minimax', [
      minimaxQuotaKey('badge-wrapper', 'available', 35, 70),
    ])
    const badges = computeTokenPlanBadges({
      state: 'ready',
      usage: payload,
    })

    expect(badges.short).toBe(35)
    expect(badges.long).toBe(70)
    expect(
      tokenPlanBadgePills(badges, t, 'en-US', 'UTC').map((pill) => pill.label),
    ).toEqual(['Short 35%', 'Long 70%'])
  })

  it('keeps pure quantity, money, reset, and progress formatting explicit', () => {
    expect(formatTokenQuantity(12_345, 'en-US')).toBe('12.3K')
    expect(formatTokenQuantity(-1_500, 'en-US')).toBe('-1.5K')
    expect(formatTokenQuantity(null, 'zh-CN')).toBe('-')
    expect(formatMoney('CNY', 2)).toBe('¥2.00')
    expect(formatOpenRouterCredits(null, t)).toBe('Unlimited')
    expect(progressColor({ end_at: null, remaining_percent: 100 })).toBe(
      'hsl(120 80% 45%)',
    )
    expect(formatResetDuration(86_400_000, translate('en-US'))).toBe(
      'Resets in 1d 0h 0m',
    )
  })

  it('delegates date, timezone, and chart theme through explicit neutral cores', () => {
    const instant = '2026-03-04T23:30:00Z'
    expect(resolveDisplayTimeZone('not/a-zone')).toBe('auto')
    expect(effectiveTimeZoneFor('Asia/Shanghai')).toBe('Asia/Shanghai')
    expect(
      formatDateTimeCore(instant, 'en-US', {
        hour: '2-digit',
        hour12: false,
        timeZone: 'UTC',
      }),
    ).toContain('23')
    expect(
      formatDateOnlyCore(instant, 'en-US', undefined, 'Asia/Shanghai'),
    ).toBe('2026-03-05')
    expect(formatNumber(1234.5, 'en-US')).toBe('1,234.5')
    expect(chartThemeFor('dark')).toBe(DARK_CHART_THEME)
    expect(chartThemeFor('light')).toBe(LIGHT_CHART_THEME)
  })

  it('reads and writes the existing React display-zone preference', () => {
    const previous = getDisplayTimeZoneSnapshot()
    const view = render(createElement(TimeZoneProbe))
    try {
      act(() => setReactDisplayTimeZone('Asia/Shanghai'))
      expect(screen.getByText('Asia/Shanghai')).toBeTruthy()
      expect(localStorage.getItem(DISPLAY_TIME_ZONE_STORAGE_KEY)).toBe(
        'Asia/Shanghai',
      )
      expect(getDisplayTimeZoneSnapshot().effective).toBe('Asia/Shanghai')
    } finally {
      view.unmount()
      setReactDisplayTimeZone(previous.selected)
    }
  })
})

describe('quota history pure cores', () => {
  const kindLabels = {
    five_hour: 'Five hour',
    rolling: 'Rolling',
    weekly: 'Weekly',
    monthly: 'Monthly',
    balance: 'Balance',
    unlimited: 'Unlimited',
    custom: 'Custom',
  } as const

  function trendPoint(
    at: string,
    value: number | null,
  ): QuotaKeyObservationTrendPoint {
    return { at, observed_at: value === null ? null : at, value }
  }

  function trendSeries(
    overrides: Partial<QuotaKeyObservationTrendSeries> = {},
  ): QuotaKeyObservationTrendSeries {
    return {
      key_id: 'key-a',
      key_label: 'Key A',
      source_id: 'minimax:general:interval',
      kind: 'five_hour',
      unit: 'percent',
      metric: 'remaining_percent',
      points: [trendPoint('2026-10-16T00:00:00Z', 40)],
      ...overrides,
    }
  }

  function coreOptionInput(range: '24h' | '7d' | '30d' = '24h') {
    return {
      range,
      formatAxisDate: (value: string) => `date:${value}`,
      formatAxisTime: (value: string) => `time:${value}`,
      formatTooltipTime: (value: string) => `full:${value}`,
      formatAmount: (value: number, unit: string) => `${value} ${unit}`,
      formatDayKey: (value: string | number | Date) =>
        new Date(value).toISOString().slice(0, 10),
      theme: LIGHT_CHART_THEME,
      labels: {
        percentSuffix: '%',
        unavailable: 'unavailable',
        kinds: { ...kindLabels },
      },
    }
  }

  it('groups by metric and unit with deterministic amount and percent order', () => {
    const groups = groupTrendSeries([
      trendSeries({ metric: 'remaining_percent', unit: 'percent' }),
      trendSeries({ metric: 'amount', unit: 'USD', kind: 'balance' }),
      trendSeries({ metric: 'amount', unit: 'CNY', kind: 'balance' }),
      trendSeries({ metric: 'amount', unit: 'credits', kind: 'balance' }),
    ])
    expect(groups.map(({ metric, unit }) => `${metric}:${unit}`)).toEqual([
      'amount:CNY',
      'amount:USD',
      'amount:credits',
      'remaining_percent:percent',
    ])
  })

  it('keeps duplicate key identities, legend names, theme colors, and hidden series stable', () => {
    const entries = [
      trendSeries({ key_id: 'key-a-1234', key_label: 'Shared' }),
      trendSeries({
        key_id: 'key-b-5678',
        key_label: 'Shared',
        kind: 'weekly',
      }),
    ]
    expect(findDuplicateTrendLabels(entries)).toEqual(new Set(['Shared']))
    expect(shortTrendKeyId('key-a-1234')).toBe('keya1234')
    const group = groupTrendSeries(entries)[0]
    const input = coreOptionInput()
    const legend = createTrendLegendEntries(group, input)
    expect(legend.map((entry) => entry.name)).toEqual([
      'Shared #keya1234 · Five hour',
      'Shared #keyb5678 · Weekly',
    ])
    expect(legend.map((entry) => entry.color)).toEqual(
      LIGHT_CHART_THEME.categorical.slice(0, 2),
    )
    expect(
      trendSeriesName(entries[0], {
        duplicateLabels: new Set(),
        singleKey: true,
        kinds: kindLabels,
      }),
    ).toBe('Five hour')

    const option = createCoreTrendOption(group, {
      ...input,
      hiddenSeries: new Set([legend[0].name]),
    }) as unknown as {
      series: Array<{ name: string; itemStyle: { color: string } }>
    }
    expect(option.series.map((series) => series.name)).toEqual([legend[1].name])
    expect(option.series[0].itemStyle.color).toBe(legend[1].color)
  })

  it('bounds percent axes and retains zero, null gaps, and the injected day boundary', () => {
    const series = trendSeries({
      points: [
        trendPoint('2026-10-15T23:30:00Z', 0),
        trendPoint('2026-10-16T01:30:00Z', null),
        trendPoint('2026-10-16T02:30:00Z', 100),
      ],
    })
    const group = groupTrendSeries([series])[0]
    const input = {
      ...coreOptionInput(),
      formatDayKey: (value: string | number | Date) => {
        const day = new Date(value)
        day.setUTCHours(day.getUTCHours() + 2)
        return day.toISOString().slice(0, 10)
      },
    }
    const option = createCoreTrendOption(group, input) as unknown as {
      series: Array<{ data: Array<number | null> }>
      xAxis: { axisLabel: { formatter: (value: string) => string } }
      yAxis: { min: number; max: number; axisLabel: { formatter: string } }
    }
    expect(option.yAxis).toMatchObject({ min: 0, max: 100 })
    expect(option.yAxis.axisLabel.formatter).toContain('%')
    expect(option.series[0].data).toEqual([0, null, 100])
    const labels = option.xAxis.axisLabel.formatter
    expect(labels('2026-10-15T23:30:00Z')).toBe('time:2026-10-15T23:30:00Z')
    expect(labels('2026-10-16T01:30:00Z')).toBe('time:2026-10-16T01:30:00Z')
  })

  it('escapes every untrusted tooltip string and leaves only ECharts markers and separators literal', () => {
    expect(escapeTrendHtml(`<tag attr="x">'&`)).toBe(
      '&lt;tag attr=&quot;x&quot;&gt;&#39;&amp;',
    )
    const optionInput = {
      ...coreOptionInput(),
      formatTooltipTime: () => '<b>timestamp</b>',
      formatAmount: (value: number, unit: string) =>
        `${value} ${unit} <img src=x>`,
    }
    const group = groupTrendSeries([
      trendSeries({
        key_id: 'one',
        key_label: '<svg onload=1>',
        source_id: 'balance:<script>',
        kind: 'custom',
        metric: 'amount',
        unit: '<img>',
      }),
      trendSeries({
        key_id: 'two',
        key_label: 'Other',
        source_id: 'balance:<script>',
        kind: 'custom',
        metric: 'amount',
        unit: '<img>',
      }),
    ])[0]
    const option = createCoreTrendOption(group, optionInput) as unknown as {
      series: Array<{ name: string }>
      tooltip: { formatter: (params: unknown[]) => string }
    }
    const html = option.tooltip.formatter([
      {
        axisValue: '<b>timestamp</b>',
        marker: '<span class="echarts-marker">●</span>',
        seriesName: option.series[0].name,
        value: 9,
      },
    ])
    expect(html).toContain('&lt;b&gt;timestamp&lt;/b&gt;')
    expect(html).toContain('&lt;svg onload=1&gt;')
    expect(html).toContain('&lt;script&gt;')
    expect(html).toContain('&lt;img&gt;')
    expect(html).toContain('&lt;img src=x&gt;')
    expect(html).toContain('<span class="echarts-marker">●</span>')
    expect(html.match(/<[^>]+>/g)).toEqual([
      '<br/>',
      '<span class="echarts-marker">',
      '</span>',
    ])
  })

  it('keeps subscription window unknown and zero values distinct and anchors relative resets', () => {
    const windowUsage = (
      overrides: Partial<SubscriptionWindowUsage> = {},
    ): SubscriptionWindowUsage => ({
      availability: 'known',
      source_window: 'primary',
      window_seconds: 18_000,
      remaining_percent: 50,
      reset_at: null,
      reset_after_seconds: null,
      used_percent: 50,
      ...overrides,
    })
    const windows = [
      windowUsage({ remaining_percent: 0 }),
      windowUsage({
        availability: 'unknown',
        source_window: 'secondary',
        window_seconds: 604_800,
        remaining_percent: 0,
      }),
      windowUsage({
        source_window: 'detail',
        window_seconds: 43_200,
        remaining_percent: 70,
      }),
    ]
    const key = {
      key_id: 'subscription-key',
      key_label: 'Subscription',
      ok: true,
      model_remains: [{ model_name: 'subscription', windows }],
    }
    expect(subscriptionQuotaWindows(key)).toEqual(windows)
    expect(subscriptionQuotaWindowCount(key, 2)).toBe(5)
    expect(subscriptionWindowRemainingPercent(windows[0])).toBe(0)
    expect(subscriptionWindowRemainingPercent(windows[1])).toBeNull()
    expect(subscriptionWindowBand(windows[0])).toBe('short')
    expect(subscriptionWindowBand(windows[1])).toBe('long')
    expect(subscriptionQuotaMinimumRemainingPercent(key, [20])).toBe(0)
    expect(subscriptionQuotaMinimumForBand(key, 'short')).toBe(0)
    expect(subscriptionQuotaMinimumForBand(key, 'long')).toBeNull()
    expect(subscriptionWindowLabel(windows[0], t)).toBe('5-hour window')
    expect(
      subscriptionWindowResetMs(
        windowUsage({ reset_after_seconds: 3600 }),
        1_000_000,
        100_000,
      ),
    ).toBe(2_700_000)
    expect(subscriptionWindowProgressColor(0)).toBe('hsl(0 80% 45%)')
  })
})

function TimeZoneProbe() {
  const zone = useDisplayTimeZone()
  return createElement('output', null, zone.effective)
}
