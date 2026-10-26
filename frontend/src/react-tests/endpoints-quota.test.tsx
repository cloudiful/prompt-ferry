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
  quotaSnapshotHistory: vi.fn(),
  quotaKeyObservationHistory: vi.fn(),
  quotaKeyObservationTrend: vi.fn(),
}))

vi.mock('@/generated/admin-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/generated/admin-api')>()),
  tokenPlanUsage: sdkMock.tokenPlanUsage,
  quotaSnapshotHistory: sdkMock.quotaSnapshotHistory,
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

function openAiUsageWithObservation(
  observedAt: string,
  source: string,
  errorCode: string | null,
): TokenPlanUsageResponse {
  const key = keyWithQuota()
  const [first, ...rest] = key.model_remains
  key.model_remains = [
    {
      ...first,
      observation: {
        observed_at: observedAt,
        source,
        refreshing: false,
        stale: false,
        last_error_code: errorCode,
        next_retry_at: null,
      },
    },
    ...rest,
  ]
  return { keys: [key], provider: 'openai', provider_region: null }
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
  sdkMock.quotaSnapshotHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
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
    expect(keyDisclosure.textContent).toContain('2 windows')
    expect(keyDisclosure.textContent).not.toContain(' | ')
    expect(within(keyDisclosure).getByText('Default model')).toBeTruthy()
    expect(within(keyDisclosure).getByText('Detail model')).toBeTruthy()
    expect(within(keyDisclosure).getByText('Cached observation')).toBeTruthy()
    expect(within(keyDisclosure).getByText('Refreshing')).toBeTruthy()
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
    expect(status?.getAttribute('title')).toContain('Oct 24, 2026, 8:00 AM')
    expect(screen.getByText('Token plan usage / Fixture upstream')).toBeTruthy()
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledTimes(1)
    expect(sdkMock.tokenPlanUsage).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-detail' },
      query: { force: false },
      responseStyle: 'data',
    })

    act(() => setDisplayTimeZone(previousZone))
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
    expect(screen.getByText('Remaining 35.0%')).toBeTruthy()
    expect(screen.getByText('Remaining 65.0%')).toBeTruthy()
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

  it('keeps history slots visible during usage loading and error and gates providers', async () => {
    const pending = deferred<{ data: TokenPlanUsageResponse }>()
    sdkMock.tokenPlanUsage.mockReturnValueOnce(pending.promise)
    const loadingView = renderDialog(
      'openai-history-loading',
      vi.fn(),
      true,
      'openai',
    )
    expect(screen.getByText('Quota history')).toBeTruthy()
    expect(screen.getByText('Per-key quota history')).toBeTruthy()
    expect(sdkMock.quotaSnapshotHistory).not.toHaveBeenCalled()
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
    expect(screen.getByText('Quota history')).toBeTruthy()
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
    expect(screen.queryByText('Quota history')).toBeNull()
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
    expect(screen.queryByText('Quota history')).toBeNull()
    expect(screen.getByText('Per-key quota history')).toBeTruthy()
    otherProviderView.unmount()
  })

  it('refreshes the open subscription history only when the first model observation changes', async () => {
    sdkMock.tokenPlanUsage
      .mockResolvedValueOnce({
        data: openAiUsageWithObservation(
          '2026-10-24T00:00:00.000Z',
          'request',
          null,
        ),
      })
      .mockResolvedValueOnce({
        data: openAiUsageWithObservation(
          '2026-10-24T00:01:00.000Z',
          'periodic',
          'temporary_failure',
        ),
      })
      .mockResolvedValueOnce({
        data: openAiUsageWithObservation(
          '2026-10-24T00:02:00.000Z',
          'manual',
          null,
        ),
      })

    const setInterval = vi.spyOn(window, 'setInterval')
    const view = renderDialog('openai-history-refresh', vi.fn(), true, 'openai')
    await act(async () => flushMicrotasks())
    const historyDetails = screen
      .getByText('Quota history')
      .closest('details') as HTMLDetailsElement
    historyDetails.open = true
    fireEvent(historyDetails, new Event('toggle'))
    await waitFor(() =>
      expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledTimes(1),
    )

    await act(async () => {
      await requestTokenPlanUsage('openai-history-refresh', { refresh: true })
    })
    await act(async () => flushMicrotasks())
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledTimes(2)
    expect(sdkMock.quotaSnapshotHistory.mock.calls[1][0].query).toEqual({
      limit: 50,
    })

    const ticker = setInterval.mock.calls.at(-1)?.[0]
    expect(typeof ticker).toBe('function')
    await act(async () => {
      vi.setSystemTime(NOW + 3000)
      ;(ticker as () => void)()
    })
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledTimes(2)

    historyDetails.open = false
    fireEvent(historyDetails, new Event('toggle'))
    await waitFor(() => expect(historyDetails.open).toBe(false))
    await act(async () => {
      await requestTokenPlanUsage('openai-history-refresh', { refresh: true })
    })
    await act(async () => flushMicrotasks())
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledTimes(2)
    view.unmount()
    setInterval.mockRestore()
  })
})
