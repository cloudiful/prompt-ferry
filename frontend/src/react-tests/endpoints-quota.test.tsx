import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  CanonicalQuotaWindow,
  DeepSeekBalance,
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'

const sdkMock = vi.hoisted(() => ({
  tokenPlanUsage: vi.fn(),
  quotaKeyObservationHistory: vi.fn(),
  quotaKeyObservationTrend: vi.fn(),
}))

vi.mock('@/generated/admin-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/generated/admin-api')>()),
  tokenPlanUsage: sdkMock.tokenPlanUsage,
  quotaKeyObservationHistory: sdkMock.quotaKeyObservationHistory,
  quotaKeyObservationTrend: sdkMock.quotaKeyObservationTrend,
}))

import { tokenPlanUsage } from '@/generated/admin-api'
import { LocaleProvider } from '@/lib/i18n'
import {
  __resetTokenPlanCacheForTests,
  getTokenPlanUsageSnapshot,
  prefetchTokenPlanUsage,
  requestTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} from '@/lib/quota-cache'
import {
  getDisplayTimeZoneSnapshot,
  setDisplayTimeZone,
} from '@/lib/display-timezone'
import { TokenPlanUsageDialog } from '@/components/endpoints/TokenPlanUsageDialog'

const NOW = Date.parse('2026-10-24T00:00:00.000Z')

function emptyUsage(
  provider: TokenPlanUsageResponse['provider'] = 'minimax',
): TokenPlanUsageResponse {
  return { keys: [], provider, provider_region: null }
}

function quotaWindow(
  overrides: Partial<CanonicalQuotaWindow> & {
    kind: CanonicalQuotaWindow['kind']
    source_id: string
  },
): CanonicalQuotaWindow {
  return {
    availability: 'known',
    current: null,
    duration_seconds: null,
    is_default_scope: true,
    limit: null,
    remaining: null,
    remaining_percent: null,
    reset_at: null,
    source_model: null,
    unit: null,
    ...overrides,
  }
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

function keyWithQuota(): TokenPlanKeyUsage {
  const deepseekBalance: DeepSeekBalance = {
    is_available: true,
    balances: [
      {
        currency: 'USD',
        total_balance: 0,
        granted_balance: 0,
        topped_up_balance: 0,
      },
      {
        currency: 'CNY',
        total_balance: null,
        granted_balance: null,
        topped_up_balance: null,
      },
    ],
  }
  return {
    key_id: 'quota-key',
    key_label: 'Fixture quota key',
    ok: true,
    quota_status: {
      state: 'available',
      scope: 'text_inference',
      observed_at: '2026-10-24T00:00:00.000Z',
      cooldown_until: null,
      refresh_failed: false,
    },
    deepseek_balance: deepseekBalance,
    openrouter_balance: null,
    openrouter_spend: null,
    balances: null,
    five_hour: null,
    weekly: null,
    glm_five_hour: null,
    glm_weekly: null,
    opencodego_rolling: null,
    opencodego_weekly: null,
    opencodego_monthly: null,
    model_remains: [
      {
        model_name: 'Default model',
        interval: null,
        weekly: null,
        observation: {
          observed_at: '2026-10-24T00:00:00.000Z',
          source: 'request',
          refreshing: true,
          stale: true,
          last_error_code: 'temporary_failure',
          next_retry_at: '2026-10-24T01:00:00.000Z',
        },
      },
      {
        model_name: 'Detail model',
        interval: null,
        weekly: null,
        observation: null,
      },
    ],
    quota_windows: [
      quotaWindow({
        source_id: 'deepseek:USD',
        kind: 'balance',
        unit: 'USD',
        current: 0,
        remaining: 0,
      }),
      quotaWindow({
        source_id: 'deepseek:CNY',
        kind: 'balance',
        unit: 'CNY',
        availability: 'unknown',
        current: null,
        remaining: null,
      }),
      quotaWindow({
        source_id: 'model-a:primary',
        source_model: 'Default model',
        kind: 'custom',
        duration_seconds: 18_000,
        remaining_percent: 40,
        reset_at: '2026-10-24T01:00:00.000Z',
      }),
      quotaWindow({
        source_id: 'model-b:detail',
        source_model: 'Detail model',
        kind: 'custom',
        is_default_scope: false,
        availability: 'unknown',
        remaining_percent: null,
      }),
    ],
  }
}

function renderDialog(
  endpointId: string,
  onError = vi.fn(),
  open = true,
  provider: EndpointProvider = 'deepseek',
) {
  return render(
    <LocaleProvider>
      <TokenPlanUsageDialog
        endpointId={endpointId}
        endpointName="Fixture upstream"
        onError={onError}
        onOpenChange={() => undefined}
        open={open}
        provider={provider}
      />
    </LocaleProvider>,
  )
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  vi.setSystemTime(NOW)
  sdkMock.tokenPlanUsage.mockImplementation(async () => ({
    data: emptyUsage(),
  }))
  sdkMock.quotaKeyObservationHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
  sdkMock.quotaKeyObservationTrend.mockResolvedValue({
    data: {
      bucket_count: 0,
      bucket_seconds: 0,
      failed_observations: 0,
      from: '',
      keys_truncated: false,
      range: '24h',
      sampled_observations: 0,
      series: [],
      series_truncated: false,
      to: '',
    },
  })
  __resetTokenPlanCacheForTests()
  setDisplayTimeZone('auto')
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
  vi.useRealTimers()
  setDisplayTimeZone('auto')
  localStorage.clear()
})

describe('React quota usage dialog', () => {
  it('renders provider balances, known zero, unknown values, model groups and localized observation times', async () => {
    const previousZone = getDisplayTimeZoneSnapshot().selected
    setDisplayTimeZone('Asia/Shanghai')
    const payload = {
      keys: [keyWithQuota()],
      provider: 'deepseek' as const,
      provider_region: null,
    }
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({ data: payload })
    renderDialog('endpoint-detail')

    const key = await screen.findByText('Fixture quota key')
    const keyDisclosure = key.closest('[data-quota-key]') as HTMLElement
    expect(keyDisclosure.textContent).toContain('Total balance: $0.00')
    expect(keyDisclosure.textContent).toContain(
      'Total balance: Balance unknown',
    )
    expect(
      keyDisclosure.querySelector('[data-quota-row="deepseek:USD"]')
        ?.textContent,
    ).toContain('0.00')
    expect(
      keyDisclosure.querySelector('[data-quota-row="deepseek:CNY"]')
        ?.textContent,
    ).toContain('Usage unknown')
    expect(keyDisclosure.textContent).not.toContain('2 windows')
    expect(keyDisclosure.textContent).not.toContain(' | ')
    expect(within(keyDisclosure).getByText('Default model')).toBeTruthy()
    expect(
      within(keyDisclosure).getAllByText('Detail model').length,
    ).toBeGreaterThan(0)
    expect(within(keyDisclosure).getByText('Cached observation')).toBeTruthy()
    expect(within(keyDisclosure).queryByText('Refreshing')).toBeNull()
    expect(
      keyDisclosure
        .querySelector(
          '[data-quota-row="model-a:primary"] [role="progressbar"]',
        )
        ?.getAttribute('aria-valuenow'),
    ).toBe('40')
    expect(
      keyDisclosure.querySelector(
        '[data-quota-row="model-b:detail"] [role="progressbar"]',
      ),
    ).toBeNull()

    const status = keyDisclosure.querySelector(
      '[data-quota-key-status="available"]',
    )
    expect(status?.getAttribute('title')).not.toContain('Oct 24, 2026')
    expect(screen.getByText('Token plan usage / Fixture upstream')).toBeTruthy()
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledTimes(1)
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-detail' },
      query: { force: false },
      responseStyle: 'data',
    })

    act(() => setDisplayTimeZone(previousZone))
  })

  it('renders keys in configured order, hides minmax video, and omits observation source metadata', async () => {
    const firstKey = keyWithQuota()
    firstKey.model_remains.push({
      model_name: 'video',
      interval: { remaining_percent: 99 },
      weekly: { remaining_percent: 99 },
    })
    firstKey.quota_windows?.push(
      quotaWindow({
        source_id: 'minimax:video:interval',
        source_model: 'video',
        kind: 'five_hour',
        duration_seconds: 18_000,
        is_default_scope: false,
        remaining_percent: 99,
      }),
    )
    const secondKey: TokenPlanKeyUsage = {
      key_id: 'second-key',
      key_label: 'Second key',
      ok: true,
      model_remains: [],
      quota_windows: [
        quotaWindow({
          source_id: 'minimax:primary',
          source_model: 'general',
          kind: 'five_hour',
          duration_seconds: 18_000,
          remaining_percent: 55,
        }),
      ],
    }
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({
      data: {
        keys: [firstKey, secondKey],
        provider: 'openai',
        provider_region: null,
      },
    })
    renderDialog('endpoint-ordered-video', vi.fn(), true, 'openai')
    await screen.findByText('Fixture quota key')
    const scroller = document.querySelector(
      '[data-quota-dialog="endpoint-ordered-video"]',
    ) as HTMLElement
    const keys = [...scroller.querySelectorAll('[data-quota-key]')].map(
      (element) => element.getAttribute('data-quota-key'),
    )
    expect(keys).toEqual(['quota-key', 'second-key'])
    expect(
      scroller.querySelector('[data-quota-row="minimax:video:interval"]'),
    ).toBeNull()
    expect(within(scroller).queryByText('video')).toBeNull()
    expect(scroller.textContent).not.toContain('Request trigger')
    expect(scroller.textContent).not.toContain('Observed')
  })

  it('renders localized compact labels and key history terminology in Chinese', async () => {
    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({
      data: {
        keys: [keyWithQuota()],
        provider: 'deepseek',
        provider_region: null,
      },
    })
    renderDialog('endpoint-detail-zh')
    await screen.findByText('Fixture quota key')
    expect(screen.getAllByText('5小时').length).toBeGreaterThan(0)
    expect(screen.getByText('按 key 的额度历史')).toBeTruthy()
    expect(document.body.textContent).not.toContain('按键')
  })

  it('keeps stale snapshots and numeric pills while background revalidation fails quietly', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(NOW)
    const payload: TokenPlanUsageResponse = {
      keys: [
        {
          key_id: 'stale-key',
          key_label: 'Stale quota key',
          ok: true,
          model_remains: [
            {
              model_name: 'General',
              interval: { remaining_percent: 35 },
              weekly: { remaining_percent: 65 },
            },
          ],
        },
      ],
      provider: 'minimax',
      provider_region: null,
    }
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({ data: payload })
    await prefetchTokenPlanUsage('endpoint-stale-dialog')
    await vi.advanceTimersByTimeAsync(TOKEN_PLAN_CACHE_TTL_MS + 1)
    expect(getTokenPlanUsageSnapshot('endpoint-stale-dialog').state).toBe(
      'stale',
    )

    const pendingRefresh = deferred<{ data: TokenPlanUsageResponse }>()
    sdkMock.tokenPlanUsage.mockReturnValueOnce(pendingRefresh.promise)
    const onError = vi.fn()
    const view = renderDialog('endpoint-stale-dialog', onError)
    await act(async () => flushMicrotasks())

    expect(screen.getByText('Stale quota key')).toBeTruthy()
    expect(screen.getByText('Cached observation')).toBeTruthy()
    expect(screen.getByText('35.0%')).toBeTruthy()
    expect(screen.getByText('65.0%')).toBeTruthy()
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledTimes(2)

    pendingRefresh.reject(new Error('background refresh failed'))
    await act(async () => flushMicrotasks())
    expect(getTokenPlanUsageSnapshot('endpoint-stale-dialog')).toMatchObject({
      state: 'stale',
      usage: payload,
    })
    expect(onError).not.toHaveBeenCalled()
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledTimes(2)
    view.unmount()
  })

  it('does not repeat a cached cold failure notification and releases the visible ticker', async () => {
    const failure = new Error('cold quota failure')
    sdkMock.tokenPlanUsage.mockRejectedValueOnce(failure)
    await prefetchTokenPlanUsage('endpoint-negative-dialog')
    expect(getTokenPlanUsageSnapshot('endpoint-negative-dialog').state).toBe(
      'error',
    )
    const onError = vi.fn()
    const setInterval = vi.spyOn(window, 'setInterval')
    const clearInterval = vi.spyOn(window, 'clearInterval')
    const view = renderDialog('endpoint-negative-dialog', onError)

    expect(await screen.findByText('Usage query failed')).toBeTruthy()
    expect(onError).not.toHaveBeenCalled()
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledTimes(1)
    expect(setInterval).toHaveBeenCalled()

    const ticker = setInterval.mock.results.at(-1)?.value
    view.unmount()
    expect(clearInterval).toHaveBeenCalledWith(ticker)
    setInterval.mockRestore()
    clearInterval.mockRestore()
  })

  it('renders OpenRouter and CommandCode balance details without changing zero or unlimited formatting', async () => {
    const openRouterKey: TokenPlanKeyUsage = {
      key_id: 'openrouter-key',
      key_label: 'OpenRouter balance key',
      ok: true,
      model_remains: [],
      openrouter_balance: {
        is_free_tier: true,
        limit: null,
        limit_remaining: null,
        limit_reset: 'daily',
        total_credits: 0,
        total_usage: 0,
      },
      openrouter_spend: { daily: 0, weekly: 1, monthly: 2, usage: 3 },
    }
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({
      data: {
        keys: [openRouterKey],
        provider: 'openrouter',
        provider_region: null,
      },
    })
    const openRouterView = renderDialog('endpoint-openrouter-balances')
    const openRouterDisclosure = (
      await screen.findByText('OpenRouter balance key')
    ).closest('[data-quota-key]') as HTMLElement
    expect(openRouterDisclosure.textContent).toContain(
      'Credit limit: Unlimited',
    )
    expect(openRouterDisclosure.textContent).toContain('Total credits: 0.00')
    expect(openRouterDisclosure.textContent).toContain('Daily usage: 0.00')
    expect(within(openRouterDisclosure).getByText('Free tier')).toBeTruthy()
    openRouterView.unmount()
    __resetTokenPlanCacheForTests()

    const commandCodeKey: TokenPlanKeyUsage = {
      key_id: 'command-key',
      key_label: 'CommandCode balance key',
      ok: true,
      model_remains: [],
      balances: {
        monthly_credits: 10,
        purchased_credits: 2,
        free_credits: 1,
        remaining_credits: 0,
      },
    }
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({
      data: {
        keys: [commandCodeKey],
        provider: 'command_code',
        provider_region: null,
      },
    })
    const commandCodeView = renderDialog('endpoint-commandcode-balances')
    const commandCodeDisclosure = (
      await screen.findByText('CommandCode balance key')
    ).closest('[data-quota-key]') as HTMLElement
    expect(commandCodeDisclosure.textContent).toContain(
      'Remaining credits: 0.00',
    )
    expect(commandCodeDisclosure.textContent).toContain(
      'Monthly credits: 10.00',
    )
    commandCodeView.unmount()
  })

  it('mutes every window of a blocked key while available and unknown keys stay colored', async () => {
    const blockedStates = [
      'exhausted',
      'cooling',
      'rate_limited',
      'upstream_unavailable',
    ] as const
    const keys: TokenPlanKeyUsage[] = blockedStates.map((state) => {
      const key = keyWithQuota()
      return {
        ...key,
        key_id: `key-${state}`,
        key_label: `Key ${state}`,
        model_remains: [],
        deepseek_balance: null,
        quota_status: {
          state,
          scope: 'text_inference',
          observed_at: '2026-10-24T00:00:00.000Z',
          cooldown_until: null,
          refresh_failed: false,
        },
        quota_windows: [
          quotaWindow({
            source_id: `${state}:primary`,
            kind: 'five_hour',
            duration_seconds: 18_000,
            remaining_percent: 100,
            reset_at: '2026-10-24T01:00:00.000Z',
          }),
        ],
      }
    })
    const available = keyWithQuota()
    available.key_id = 'key-available'
    available.key_label = 'Key available'
    available.model_remains = []
    available.quota_windows = [
      quotaWindow({
        source_id: 'available:primary',
        kind: 'five_hour',
        duration_seconds: 18_000,
        remaining_percent: 100,
        reset_at: '2026-10-24T01:00:00.000Z',
      }),
    ]
    const unknown = keyWithQuota()
    unknown.key_id = 'key-unknown'
    unknown.key_label = 'Key unknown'
    unknown.model_remains = []
    unknown.quota_status = {
      state: 'unknown',
      scope: 'text_inference',
      observed_at: '2026-10-24T00:00:00.000Z',
      cooldown_until: null,
      refresh_failed: true,
    }
    unknown.quota_windows = [
      quotaWindow({
        source_id: 'unknown:primary',
        source_model: 'Default model',
        kind: 'five_hour',
        duration_seconds: 18_000,
        remaining_percent: 100,
        reset_at: '2026-10-24T01:00:00.000Z',
      }),
    ]
    unknown.model_remains = [
      {
        model_name: 'Default model',
        interval: null,
        weekly: null,
        observation: {
          observed_at: '2026-10-24T00:00:00.000Z',
          source: 'request',
          refreshing: false,
          stale: false,
          last_error_code: 'temporary_failure',
          next_retry_at: null,
        },
      },
    ]
    sdkMock.tokenPlanUsage.mockResolvedValueOnce({
      data: {
        keys: [...keys, available, unknown],
        provider: 'minimax',
        provider_region: null,
      },
    })
    renderDialog('endpoint-muted-status')

    for (const state of blockedStates) {
      const key = await screen.findByText(`Key ${state}`)
      const details = key.closest('[data-quota-key]') as HTMLElement
      expect(details.getAttribute('data-quota-muted')).toBe('true')
      expect(details.textContent).toContain('100.0%')
      const row = details.querySelector('[data-quota-row]') as HTMLElement
      expect(row.getAttribute('data-quota-muted')).toBe('true')
      expect(
        (row.querySelector('[data-quota-fill]') as HTMLElement).getAttribute(
          'data-quota-fill',
        ),
      ).toBe('muted')
    }

    const availableKey = screen
      .getByText('Key available')
      .closest('[data-quota-key]') as HTMLElement
    expect(availableKey.getAttribute('data-quota-muted')).toBe('false')
    const availableFill = availableKey.querySelector(
      '[data-quota-fill]',
    ) as HTMLElement
    expect(availableFill.getAttribute('data-quota-fill')).toBe('colored')
    expect(availableFill.getAttribute('style') ?? '').toContain(
      'background-color',
    )

    const unknownKey = screen
      .getByText('Key unknown')
      .closest('[data-quota-key]') as HTMLElement
    expect(unknownKey.getAttribute('data-quota-muted')).toBe('false')
    expect(unknownKey.textContent).toContain(
      'Refresh failed; showing the latest successful observation.',
    )
  })

  it('keeps the shared key-history slot visible during usage loading and error and gates providers', async () => {
    const pending = deferred<{ data: TokenPlanUsageResponse }>()
    sdkMock.tokenPlanUsage.mockReturnValueOnce(pending.promise)
    const loadingView = renderDialog(
      'openai-history-loading',
      vi.fn(),
      true,
      'openai',
    )
    expect(screen.getByText('Per-key quota history')).toBeTruthy()
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()

    pending.resolve({ data: emptyUsage('openai') })
    await act(async () => flushMicrotasks())
    loadingView.unmount()
    __resetTokenPlanCacheForTests()

    sdkMock.tokenPlanUsage.mockRejectedValueOnce(new Error('cold failure'))
    const errorView = renderDialog(
      'openai-history-error',
      vi.fn(),
      true,
      'openai',
    )
    expect(await screen.findByText('Usage query failed')).toBeTruthy()
    expect(screen.getByText('Per-key quota history')).toBeTruthy()
    errorView.unmount()
    __resetTokenPlanCacheForTests()

    const genericView = renderDialog(
      'generic-history-hidden',
      vi.fn(),
      true,
      'generic',
    )
    await screen.findByText('No usage data')
    expect(screen.queryByText('Per-key quota history')).toBeNull()
    genericView.unmount()
    __resetTokenPlanCacheForTests()

    const otherProviderView = renderDialog(
      'other-provider-history',
      vi.fn(),
      true,
      'deepseek',
    )
    await screen.findByText('No usage data')
    expect(screen.getByText('Per-key quota history')).toBeTruthy()
    otherProviderView.unmount()
  })
})
