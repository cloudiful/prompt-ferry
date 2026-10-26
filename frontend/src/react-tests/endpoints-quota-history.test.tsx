import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  CanonicalQuotaWindow,
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationHistoryResponse,
  QuotaKeyObservationTrendResponse,
  QuotaKeyObservationTrendSeries,
  QuotaSnapshotHistoryItem,
} from '@/generated/admin-api'

const sdkMock = vi.hoisted(() => ({
  quotaSnapshotHistory: vi.fn(),
  quotaKeyObservationHistory: vi.fn(),
  quotaKeyObservationTrend: vi.fn(),
}))
const echartsMock = vi.hoisted(() => ({
  dispose: vi.fn(),
  init: vi.fn(),
  options: [] as unknown[],
  resize: vi.fn(),
  use: vi.fn(),
}))

vi.mock('@/generated/admin-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/generated/admin-api')>()),
  quotaSnapshotHistory: sdkMock.quotaSnapshotHistory,
  quotaKeyObservationHistory: sdkMock.quotaKeyObservationHistory,
  quotaKeyObservationTrend: sdkMock.quotaKeyObservationTrend,
}))

vi.mock('echarts/core', () => ({
  init: echartsMock.init,
  use: echartsMock.use,
}))
vi.mock('echarts/charts', () => ({ BarChart: {}, LineChart: {} }))
vi.mock('echarts/components', () => ({
  GridComponent: {},
  LegendComponent: {},
  TooltipComponent: {},
}))
vi.mock('echarts/renderers', () => ({ CanvasRenderer: {} }))

import { LocaleProvider } from '@/lib/i18n'
import { ThemeProvider } from '@/lib/theme'
import { KeyQuotaObservationHistory } from '@/components/endpoints/KeyQuotaObservationHistory'
import { SubscriptionQuotaHistory } from '@/components/endpoints/SubscriptionQuotaHistory'

const NOW = Date.parse('2026-10-24T00:00:00.000Z')
const originalScrollIntoView = HTMLElement.prototype.scrollIntoView
const originalClientWidth = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  'clientWidth',
)
const originalClientHeight = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  'clientHeight',
)
let layoutWidth = 0
let layoutHeight = 0

class TestResizeObserver implements ResizeObserver {
  readonly callback: ResizeObserverCallback
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback
    resizeObservers.push(this)
  }

  trigger(): void {
    this.callback([], this)
  }
}

const resizeObservers: TestResizeObserver[] = []

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

function snapshot(
  snapshotId: string,
  overrides: Partial<QuotaSnapshotHistoryItem> = {},
): QuotaSnapshotHistoryItem {
  return {
    snapshot_id: snapshotId,
    observed_at: '2026-10-24T00:00:00.000Z',
    plan_type: null,
    source: 'manual',
    limit_reached: false,
    windows: [],
    ...overrides,
  }
}

function observation(
  observationId: string,
  overrides: Partial<QuotaKeyObservationHistoryItem> = {},
): QuotaKeyObservationHistoryItem {
  return {
    observation_id: observationId,
    key_id: 'key-a',
    key_label: 'History key',
    observed_at: '2026-10-24T00:00:00.000Z',
    provider: 'minimax',
    source: 'periodic',
    ok: true,
    status: '200',
    error_code: null,
    limit_reached: false,
    plan_type: null,
    windows: [],
    ...overrides,
  }
}

function trendSeries(
  overrides: Partial<QuotaKeyObservationTrendSeries> = {},
): QuotaKeyObservationTrendSeries {
  return {
    key_id: 'key-a',
    key_label: 'Trend key',
    source_id: 'minimax:general:interval',
    kind: 'five_hour',
    metric: 'remaining_percent',
    unit: 'percent',
    points: [{ at: '2026-10-24T00:00:00.000Z', observed_at: null, value: 50 }],
    ...overrides,
  }
}

function trend(
  overrides: Partial<QuotaKeyObservationTrendResponse> = {},
): QuotaKeyObservationTrendResponse {
  return {
    bucket_count: 1,
    bucket_seconds: 900,
    failed_observations: 0,
    from: '2026-10-24T00:00:00.000Z',
    keys_truncated: false,
    range: '24h',
    sampled_observations: 1,
    series: [],
    series_truncated: false,
    to: '2026-10-24T00:00:00.000Z',
    ...overrides,
  }
}

function renderWithProviders(children: React.ReactNode) {
  return render(
    <LocaleProvider>
      <ThemeProvider>{children}</ThemeProvider>
    </LocaleProvider>,
  )
}

function openSection(label: string): void {
  fireEvent.click(screen.getByText(label))
}

async function chooseSelect(name: string, option: string): Promise<void> {
  fireEvent.click(screen.getByRole('combobox', { name }))
  fireEvent.click(await screen.findByRole('option', { name: option }))
}

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
    configurable: true,
    value: vi.fn(),
  })
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => layoutWidth,
  })
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
    configurable: true,
    get: () => layoutHeight,
  })
  vi.stubGlobal('ResizeObserver', TestResizeObserver)
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  localStorage.setItem('prompt-ferry:theme-mode', 'dark')
  vi.setSystemTime(NOW)
  resizeObservers.length = 0
  echartsMock.options.length = 0
  echartsMock.init.mockImplementation(() => ({
    setOption: (option: unknown) => echartsMock.options.push(option),
    resize: echartsMock.resize,
    dispose: echartsMock.dispose,
  }))
  sdkMock.quotaSnapshotHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
  sdkMock.quotaKeyObservationHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
  sdkMock.quotaKeyObservationTrend.mockResolvedValue({ data: trend() })
})

afterEach(() => {
  localStorage.clear()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  if (originalClientWidth) {
    Object.defineProperty(
      HTMLElement.prototype,
      'clientWidth',
      originalClientWidth,
    )
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, 'clientWidth')
  }
  if (originalClientHeight) {
    Object.defineProperty(
      HTMLElement.prototype,
      'clientHeight',
      originalClientHeight,
    )
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight')
  }
  if (originalScrollIntoView) {
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: originalScrollIntoView,
    })
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, 'scrollIntoView')
  }
})

describe('subscription quota history', () => {
  it('loads lazily, preserves opaque cursors, deduplicates snapshots, and anchors resets to observation time', async () => {
    const cursor = 'opaque/cursor?<token>'
    sdkMock.quotaSnapshotHistory
      .mockResolvedValueOnce({
        data: {
          items: [
            snapshot('snapshot-one', {
              observed_at: '2026-10-24T00:00:00.000Z',
              plan_type: '<plan>',
              source: 'request',
              limit_reached: true,
              windows: [
                {
                  source_window: 'primary',
                  window_seconds: 18_000,
                  remaining_percent: 40,
                  used_percent: 60,
                  availability: 'known',
                  reset_at: null,
                  reset_after_seconds: 60,
                },
              ],
            }),
          ],
          next_cursor: cursor,
          retention_days: 9,
        },
      })
      .mockResolvedValueOnce({
        data: {
          items: [
            snapshot('snapshot-one'),
            snapshot('snapshot-two', { windows: [] }),
          ],
          next_cursor: null,
          retention_days: 9,
        },
      })

    renderWithProviders(
      <SubscriptionQuotaHistory
        endpointId="openai-endpoint"
        nowMs={Date.parse('2026-10-24T00:02:00.000Z')}
        refreshKey=""
      />,
    )
    expect(sdkMock.quotaSnapshotHistory).not.toHaveBeenCalled()

    openSection('Quota history')
    expect(await screen.findByText('<plan>')).toBeTruthy()
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledWith({
      path: { endpoint_id: 'openai-endpoint' },
      query: { limit: 50 },
      responseStyle: 'data',
    })
    expect(screen.getByText('Upstream limit reached')).toBeTruthy()
    expect(screen.getByText('Request trigger')).toBeTruthy()
    expect(screen.getByText('Expired')).toBeTruthy()
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe(
      '40',
    )
    expect(screen.queryByText(cursor)).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }))
    await screen.findByText('No window details')
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'openai-endpoint' },
      query: { limit: 50, before_id: cursor },
      responseStyle: 'data',
    })
    expect(
      document.querySelectorAll('[data-quota-snapshot-id="snapshot-one"]'),
    ).toHaveLength(1)
    expect(
      document.querySelectorAll('[data-quota-snapshot-id="snapshot-two"]'),
    ).toHaveLength(1)
  })

  it('retries a cold failure and reports the returned retention period when empty', async () => {
    sdkMock.quotaSnapshotHistory
      .mockRejectedValueOnce(new Error('history unavailable'))
      .mockResolvedValueOnce({
        data: { items: [], retention_days: 12, next_cursor: null },
      })
    renderWithProviders(
      <SubscriptionQuotaHistory
        endpointId="openai-retry"
        nowMs={NOW}
        refreshKey=""
      />,
    )
    openSection('Quota history')
    expect(
      await screen.findByText('Quota history could not be loaded.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(
      await screen.findByText(
        'No successful observations in the last 12 days.',
      ),
    ).toBeTruthy()
    expect(sdkMock.quotaSnapshotHistory).toHaveBeenCalledTimes(2)
    expect(sdkMock.quotaSnapshotHistory.mock.calls[1][0].query).toEqual({
      limit: 50,
    })
  })
})

describe('per-key quota history and trend', () => {
  it('defaults to the lazy 24-hour chart, renders native legends, and resets hidden series for a new range', async () => {
    const firstTrend = trend({
      failed_observations: 2,
      keys_truncated: true,
      series_truncated: true,
      series: [
        trendSeries({ key_id: 'key-a', key_label: 'Trend key' }),
        trendSeries({
          key_id: 'key-b',
          key_label: 'Other key',
          kind: 'weekly',
          source_id: 'minimax:general:weekly',
        }),
      ],
    })
    const nextTrend = trend({
      range: '7d',
      series: [
        trendSeries({
          key_id: 'key-c',
          key_label: 'New trend key',
          kind: 'rolling',
          source_id: 'opencodego:rolling',
        }),
      ],
    })
    sdkMock.quotaKeyObservationTrend
      .mockResolvedValueOnce({ data: firstTrend })
      .mockResolvedValueOnce({ data: nextTrend })
      .mockResolvedValueOnce({ data: nextTrend })

    const view = renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-a" />,
    )
    expect(sdkMock.quotaKeyObservationTrend).not.toHaveBeenCalled()
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    expect(echartsMock.use).not.toHaveBeenCalled()
    expect(echartsMock.init).not.toHaveBeenCalled()

    openSection('Per-key quota history')
    await waitFor(() =>
      expect(document.querySelector('[data-usage-chart]')).not.toBeNull(),
    )
    expect(echartsMock.use).toHaveBeenCalled()
    expect(echartsMock.init).not.toHaveBeenCalled()
    layoutWidth = 640
    layoutHeight = 320
    await act(async () => resizeObservers.at(-1)?.trigger())
    expect(echartsMock.init).toHaveBeenCalledTimes(1)
    expect(echartsMock.options.length).toBeGreaterThan(0)
    await act(async () => resizeObservers.at(-1)?.trigger())
    expect(echartsMock.resize).toHaveBeenCalled()
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-a' },
      query: { range: '24h' },
      responseStyle: 'data',
    })
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    expect(screen.getByText('2 quota collection failures')).toBeTruthy()
    expect(
      screen.getByText('Key cap reached; some keys are omitted'),
    ).toBeTruthy()
    expect(
      screen.getByText('Series cap reached; some series are omitted'),
    ).toBeTruthy()

    const firstLegend = screen.getByRole('button', {
      name: 'Trend key · 5-hour window',
    })
    expect(firstLegend.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(firstLegend)
    expect(firstLegend.getAttribute('aria-pressed')).toBe('false')
    expect(
      (echartsMock.options.at(-1) as { series: unknown[] }).series,
    ).toHaveLength(1)
    fireEvent.click(
      screen.getByRole('button', { name: 'Other key · Weekly window' }),
    )
    expect(
      (echartsMock.options.at(-1) as { series: unknown[] }).series,
    ).toHaveLength(0)

    await chooseSelect('Range', 'Last 7 days')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(2),
    )
    expect(
      sdkMock.quotaKeyObservationTrend.mock.calls.map(([input]) => input.query),
    ).toEqual([{ range: '24h' }, { range: '7d' }])
    const restoredLegend = await screen.findByRole('button', {
      name: 'Rolling window',
    })
    expect(restoredLegend.getAttribute('aria-pressed')).toBe('true')
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-a' },
      query: { range: '7d' },
      responseStyle: 'data',
    })
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    fireEvent.click(restoredLegend)
    expect(restoredLegend.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(1),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Chart' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(3),
    )
    const remountedLegend = await screen.findByRole('button', {
      name: 'Rolling window',
    })
    expect(remountedLegend.getAttribute('aria-pressed')).toBe('true')
    view.unmount()
    expect(resizeObservers.at(-1)?.disconnect).toHaveBeenCalled()
    expect(echartsMock.dispose).toHaveBeenCalled()
  })

  it('pages only in list mode, maps All to an omitted API key, retries load-more, and retains pages across mode switches', async () => {
    vi.setSystemTime(NOW + 60 * 60 * 1000)
    const cursor = 'cursor:{opaque}/next'
    sdkMock.quotaKeyObservationHistory
      .mockResolvedValueOnce({
        data: {
          items: [
            observation('obs-one', {
              key_id: 'history-key',
              key_label: 'List-only key',
              windows: [
                quotaWindow({
                  source_id: 'history-window',
                  kind: 'custom',
                  duration_seconds: 18_000,
                  remaining_percent: 55,
                  reset_at: new Date(NOW + 30 * 60 * 1000).toISOString(),
                }),
              ],
            }),
          ],
          next_cursor: cursor,
          retention_days: 21,
        },
      })
      .mockResolvedValueOnce({
        data: {
          items: [
            observation('obs-one', {
              key_id: 'history-key',
              key_label: 'List-only key',
            }),
          ],
          next_cursor: cursor,
          retention_days: 21,
        },
      })
      .mockRejectedValueOnce(new Error('page failed'))
      .mockResolvedValueOnce({
        data: {
          items: [
            observation('obs-one', {
              key_id: 'history-key',
              key_label: 'List-only key',
            }),
            observation('obs-two', {
              key_id: 'history-key',
              key_label: 'List-only key',
              source: 'manual',
              windows: [],
            }),
          ],
          next_cursor: null,
          retention_days: 21,
        },
      })

    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-list" />,
    )
    openSection('Per-key quota history')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(1),
    )
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    expect(await screen.findByText('List-only key')).toBeTruthy()
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-list' },
      query: { limit: 50 },
      responseStyle: 'data',
    })
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe(
      '55',
    )

    await chooseSelect('Key', 'List-only key')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(2),
    )
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-list' },
      query: { limit: 50, key_id: 'history-key' },
      responseStyle: 'data',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }))
    expect(
      await screen.findByText('Quota history could not be loaded.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await screen.findByText('No window details')
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-list' },
      query: { limit: 50, key_id: 'history-key', before_id: cursor },
      responseStyle: 'data',
    })
    expect(
      document.querySelectorAll('[data-quota-observation-id="obs-one"]'),
    ).toHaveLength(1)
    expect(
      document.querySelectorAll('[data-quota-observation-id="obs-two"]'),
    ).toHaveLength(1)
    expect(screen.queryByText(cursor)).toBeNull()

    const callsBeforeChart =
      sdkMock.quotaKeyObservationHistory.mock.calls.length
    fireEvent.click(screen.getByRole('button', { name: 'Chart' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(2),
    )
    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    expect(
      document.querySelectorAll('[data-quota-observation-id="obs-two"]'),
    ).toHaveLength(1)
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(
      callsBeforeChart,
    )

    await chooseSelect('Key', 'All keys')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(
        callsBeforeChart + 1,
      ),
    )
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-list' },
      query: { limit: 50 },
      responseStyle: 'data',
    })
  })

  it('retries a failed trend independently and renders empty data without a chart', async () => {
    sdkMock.quotaKeyObservationTrend
      .mockRejectedValueOnce(new Error('trend unavailable'))
      .mockResolvedValueOnce({ data: trend({ series: [] }) })

    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-trend-retry" />,
    )
    openSection('Per-key quota history')
    expect(
      await screen.findByText('Quota collection trend could not be loaded.'),
    ).toBeTruthy()
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(
      await screen.findByText('No plottable quota data in the selected range.'),
    ).toBeTruthy()
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(2)
    expect(sdkMock.quotaKeyObservationHistory).not.toHaveBeenCalled()
    expect(document.querySelector('[data-usage-chart]')).toBeNull()
    expect(echartsMock.init).not.toHaveBeenCalled()
  })

  it('ignores an old list response after the key filter changes', async () => {
    const oldAllKeysPage = deferred<{
      data: QuotaKeyObservationHistoryResponse
    }>()
    sdkMock.quotaKeyObservationTrend.mockResolvedValueOnce({
      data: trend({
        series: [
          trendSeries({ key_id: 'key-a', key_label: 'Key A' }),
          trendSeries({
            key_id: 'key-b',
            key_label: 'Key B',
            kind: 'weekly',
            source_id: 'minimax:general:weekly',
          }),
        ],
      }),
    })
    sdkMock.quotaKeyObservationHistory
      .mockReturnValueOnce(oldAllKeysPage.promise)
      .mockResolvedValueOnce({
        data: {
          items: [
            observation('fresh-key-page', {
              key_id: 'key-b',
              key_label: 'Key B history',
            }),
          ],
          next_cursor: null,
          retention_days: 30,
        },
      })

    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-key-race" />,
    )
    openSection('Per-key quota history')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(1),
    )
    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(1),
    )
    await chooseSelect('Key', 'Key B')
    expect(await screen.findByText('Key B history')).toBeTruthy()
    expect(
      sdkMock.quotaKeyObservationHistory.mock.calls.map(
        ([input]) => input.query,
      ),
    ).toEqual([{ limit: 50 }, { limit: 50, key_id: 'key-b' }])

    oldAllKeysPage.resolve({
      data: {
        items: [
          observation('stale-all-keys-page', {
            key_id: 'key-a',
            key_label: 'Stale key A response',
          }),
        ],
        next_cursor: null,
        retention_days: 30,
      },
    })
    await act(async () => flushMicrotasks())
    expect(screen.queryByText('Stale key A response')).toBeNull()
  })

  it('retries failed first pages, ignores stale range results, and clears both readers on collapse', async () => {
    const oldRange = deferred<{ data: QuotaKeyObservationTrendResponse }>()
    sdkMock.quotaKeyObservationTrend
      .mockReturnValueOnce(oldRange.promise)
      .mockResolvedValueOnce({ data: trend({ range: '7d' }) })
    sdkMock.quotaKeyObservationHistory
      .mockRejectedValueOnce(new Error('first page failed'))
      .mockResolvedValueOnce({ data: { items: [], retention_days: 6 } })

    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-race" />,
    )
    openSection('Per-key quota history')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(1),
    )
    await chooseSelect('Range', 'Last 7 days')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(2),
    )
    await chooseSelect('Range', 'Last 30 days')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(3),
    )
    oldRange.resolve({
      data: trend({ series: [trendSeries({ key_label: 'Stale result' })] }),
    })
    await act(async () => Promise.resolve())
    expect(screen.queryByText('Stale result · 5-hour window')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    expect(
      await screen.findByText('Quota history could not be loaded.'),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(
      await screen.findByText('No successful observations in the last 6 days.'),
    ).toBeTruthy()
    const listCalls = sdkMock.quotaKeyObservationHistory.mock.calls.length
    fireEvent.click(screen.getByRole('button', { name: 'Chart' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(4),
    )

    fireEvent.click(screen.getByText('Per-key quota history'))
    await waitFor(() =>
      expect(
        screen.queryByText('No successful observations in the last 6 days.'),
      ).toBeNull(),
    )
    openSection('Per-key quota history')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(5),
    )
    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(
        listCalls + 1,
      ),
    )
  })
})
