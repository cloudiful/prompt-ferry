import { useState } from 'react'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  CanonicalQuotaWindow,
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationHistoryResponse,
  QuotaKeyObservationTrendResponse,
  QuotaKeyObservationTrendSeries,
} from '@/generated/admin-api'

const sdkMock = vi.hoisted(() => ({
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
import { MultiSelect } from '@/components/ui/multi-select'

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
    points: [
      {
        at: '2026-10-24T00:00:00.000Z',
        observed_at: '2026-10-24T00:00:00.000Z',
        value: 50,
      },
    ],
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

async function chooseMulti(name: string, option: string): Promise<void> {
  fireEvent.click(screen.getByRole('button', { name }))
  fireEvent.click(await screen.findByRole('checkbox', { name: option }))
  fireEvent.keyDown(document, { key: 'Escape' })
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

  it('visibly identifies distinct models beside window labels in history rows', async () => {
    sdkMock.quotaKeyObservationHistory.mockResolvedValueOnce({
      data: {
        items: [
          observation('observation-models', {
            windows: [
              quotaWindow({
                source_id: 'minimax:model-a:interval',
                source_model: 'model-a',
                kind: 'five_hour',
                duration_seconds: 18_000,
                remaining_percent: 50,
              }),
              quotaWindow({
                source_id: 'minimax:model-b:interval',
                source_model: 'model-b',
                kind: 'five_hour',
                duration_seconds: 18_000,
                remaining_percent: 70,
              }),
            ],
          }),
        ],
        retention_days: 30,
      },
    })
    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-history-models" />,
    )
    openSection('Per-key quota history')
    fireEvent.click(await screen.findByRole('button', { name: 'List' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(1),
    )
    const first = await screen.findByText('model-a · 5h')
    const second = screen.getByText('model-b · 5h')
    expect(first).toBeTruthy()
    expect(second).toBeTruthy()
    const labels = [
      ...document.querySelectorAll('[data-quota-row] [role="progressbar"]'),
    ].map((bar) => bar.getAttribute('aria-label'))
    expect(labels).toEqual([
      'model-a · 5-hour window',
      'model-b · 5-hour window',
    ])
  })

  it('sends multi-key and kind selections in one request and filters list rows by kind', async () => {
    sdkMock.quotaKeyObservationTrend.mockResolvedValue({
      data: trend({
        series: [
          trendSeries({ key_id: 'key-a', key_label: 'Key A' }),
          trendSeries({
            key_id: 'key-b',
            key_label: 'Key B',
            kind: 'weekly',
            source_id: 'minimax:general:weekly',
          }),
          trendSeries({
            key_id: 'key-c',
            key_label: 'Key C',
            kind: 'rolling',
            source_id: 'minimax:general:rolling',
          }),
        ],
      }),
    })
    sdkMock.quotaKeyObservationHistory.mockResolvedValue({
      data: {
        items: [
          observation('obs-kinds', {
            windows: [
              quotaWindow({
                source_id: 'fixture:rolling',
                kind: 'rolling',
                remaining_percent: 40,
              }),
              quotaWindow({
                source_id: 'fixture:weekly',
                kind: 'weekly',
                remaining_percent: 60,
              }),
            ],
          }),
          observation('obs-error', {
            ok: false,
            error_code: 'auth',
            windows: [],
          }),
        ],
        next_cursor: null,
        retention_days: 30,
      },
    })

    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-filters" />,
    )
    openSection('Per-key quota history')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(1),
    )
    await chooseMulti('Keys', 'Key A')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(2),
    )
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-filters' },
      query: { range: '24h', key_id: 'key-a' },
      responseStyle: 'data',
    })
    await chooseMulti('Keys', 'Key B')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(3),
    )
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-filters' },
      query: { range: '24h', key_ids: 'key-a,key-b' },
      responseStyle: 'data',
    })

    await chooseMulti('Window kinds', 'Weekly window')
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationTrend).toHaveBeenCalledTimes(4),
    )
    expect(sdkMock.quotaKeyObservationTrend).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-filters' },
      query: { range: '24h', key_ids: 'key-a,key-b', kinds: 'weekly' },
      responseStyle: 'data',
    })

    fireEvent.click(screen.getByRole('button', { name: 'List' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(1),
    )
    expect(sdkMock.quotaKeyObservationHistory).toHaveBeenLastCalledWith({
      path: { endpoint_id: 'endpoint-filters' },
      query: { limit: 50, key_ids: 'key-a,key-b' },
      responseStyle: 'data',
    })
    await screen.findByText('auth')
    expect(
      document.querySelectorAll('[data-quota-row] [role="progressbar"]'),
    ).toHaveLength(1)
  })

  it('localizes quota history key terminology in Chinese', async () => {
    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    renderWithProviders(
      <KeyQuotaObservationHistory endpointId="endpoint-history-zh" />,
    )
    expect(screen.getByText('按 key 的额度历史')).toBeTruthy()
    openSection('按 key 的额度历史')
    fireEvent.click(await screen.findByRole('button', { name: '列表' }))
    await waitFor(() =>
      expect(sdkMock.quotaKeyObservationHistory).toHaveBeenCalledTimes(1),
    )
    fireEvent.click(screen.getByRole('button', { name: 'key' }))
    expect(
      await screen.findByRole('checkbox', { name: '全部 key' }),
    ).toBeTruthy()
    expect(screen.queryByText('全部按键')).toBeNull()
    const quotaSurface = document.querySelector('details') as HTMLElement
    expect(quotaSurface.textContent).not.toContain('按键')
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

    await chooseMulti('Keys', 'List-only key')
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

    await chooseMulti('Keys', 'All keys')
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
    await chooseMulti('Keys', 'Key B')
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

function MultiSelectHarness({
  maxSelected,
  optionCount = 3,
}: {
  maxSelected?: number
  optionCount?: number
}) {
  const options = Array.from({ length: optionCount }, (_, index) => ({
    value: `k${index}`,
    label: `k${index}`,
  }))
  const [selected, setSelected] = useState<string[]>([])
  return (
    <div>
      <MultiSelect
        allLabel="All options"
        label="Test filter"
        limitLabel={maxSelected ? `Up to ${maxSelected}` : undefined}
        maxSelected={maxSelected}
        onChange={setSelected}
        options={options}
        selected={selected}
        summary={selected.length > 0 ? selected.join(',') : 'All options'}
      />
      <output data-testid="selection">{selected.join(',')}</output>
    </div>
  )
}

function checkboxState(name: string): string | null {
  return screen.getByRole('checkbox', { name }).getAttribute('data-state')
}

describe('multi-select implicit-all semantics', () => {
  it('starts at All, switches to explicit choices, and unchecks inside an explicit set', () => {
    render(<MultiSelectHarness />)
    fireEvent.click(screen.getByRole('button', { name: 'Test filter' }))
    expect(checkboxState('All options')).toBe('checked')
    for (const name of ['k0', 'k1', 'k2']) {
      expect(checkboxState(name)).toBe('unchecked')
    }

    fireEvent.click(screen.getByRole('checkbox', { name: 'k0' }))
    expect(screen.getByTestId('selection').textContent).toBe('k0')
    expect(checkboxState('All options')).toBe('unchecked')
    expect(checkboxState('k0')).toBe('checked')

    fireEvent.click(screen.getByRole('checkbox', { name: 'k1' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'k2' }))
    expect(screen.getByTestId('selection').textContent).toBe('k0,k1,k2')
    expect(checkboxState('All options')).toBe('unchecked')

    fireEvent.click(screen.getByRole('checkbox', { name: 'k0' }))
    expect(screen.getByTestId('selection').textContent).toBe('k1,k2')
  })

  it('returns to All after removing the final explicit choice and clears with All', () => {
    render(<MultiSelectHarness />)
    fireEvent.click(screen.getByRole('button', { name: 'Test filter' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'k0' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'k0' }))
    expect(screen.getByTestId('selection').textContent).toBe('')
    expect(checkboxState('All options')).toBe('checked')

    fireEvent.click(screen.getByRole('checkbox', { name: 'k1' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'k2' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'All options' }))
    expect(screen.getByTestId('selection').textContent).toBe('')
    expect(checkboxState('All options')).toBe('checked')
  })

  it('caps explicit key choices at 24 while leaving All available', () => {
    render(<MultiSelectHarness maxSelected={24} optionCount={25} />)
    fireEvent.click(screen.getByRole('button', { name: 'Test filter' }))
    for (let index = 0; index < 24; index += 1) {
      fireEvent.click(screen.getByRole('checkbox', { name: `k${index}` }))
    }
    expect(screen.getByTestId('selection').textContent?.split(',').length).toBe(
      24,
    )
    expect(checkboxState('All options')).not.toBe('checked')
    expect(
      screen
        .getByRole('checkbox', { name: 'All options' })
        .hasAttribute('disabled'),
    ).toBe(false)
    expect(
      screen.getByRole('checkbox', { name: 'k24' }).hasAttribute('disabled'),
    ).toBe(true)
    expect(screen.getByText('Up to 24')).toBeTruthy()
  })

  it('does not impose the key cap on the kinds-style selector', () => {
    render(<MultiSelectHarness optionCount={25} />)
    fireEvent.click(screen.getByRole('button', { name: 'Test filter' }))
    for (let index = 0; index < 24; index += 1) {
      fireEvent.click(screen.getByRole('checkbox', { name: `k${index}` }))
    }
    expect(
      screen.getByRole('checkbox', { name: 'k24' }).hasAttribute('disabled'),
    ).toBe(false)
    expect(screen.queryByText('Up to 24')).toBeNull()
  })
})
