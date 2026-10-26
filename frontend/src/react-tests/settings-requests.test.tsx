import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ModelRouteWhitelistResponse,
  RequestContentLoggingResponse,
  StreamDeltaBatchingSettings,
  UsageRetentionSettings,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  getModelRouteWhitelist: vi.fn(),
  getRequestContentLogging: vi.fn(),
  getStreamDeltaBatching: vi.fn(),
  getUsageRetention: vi.fn(),
  setModelRouteWhitelist: vi.fn(),
  setRequestContentLogging: vi.fn(),
  setStreamDeltaBatching: vi.fn(),
  setUsageRetention: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  disposeSettingsRequestsAutosave,
  setSettingsRequestsNotifier,
  useSettingsRequestsStore,
} from '@/lib/settings-requests'
import {
  SETTING_FORM_RANGES,
  clampSettingInteger,
  mergeRequestContentLoggingResponse,
  mergeStreamDeltaBatchingResponse,
  mergeUsageRetentionResponse,
  mirrorSharedRawRetention,
  parseSettingInteger,
  settingsRefreshBlocked,
  settingsSnapshot,
} from '@/lib/settings-requests-model'
import { settingFormKey } from '@/lib/settings-keys'
import { getPendingActions } from '@/lib/async-action'
import { SettingsPage } from '@/pages/SettingsPage'

const copy = messages['en-US']

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}

const loggingFixture: RequestContentLoggingResponse = {
  mode: 'normalized_and_raw',
  raw_retention_days: 7,
}

const retentionFixture: UsageRetentionSettings = {
  approval_retention_days: 30,
  content_retention_days: 30,
  metadata_retention_days: 90,
  raw_backend: 'local',
  raw_retention_days: 7,
  replay_enabled: true,
}

const batchingFixture: StreamDeltaBatchingSettings = {
  enabled: true,
  flush_on_line_break: true,
  flush_on_sentence_end: true,
  flush_window_ms: 40,
  max_buffer_bytes: 65536,
  max_buffer_chars: 32768,
}

const whitelistFixture: ModelRouteWhitelistResponse = { enabled: false }

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

function queryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function store() {
  return useSettingsRequestsStore.getState()
}

async function flushMicrotasks(times = 4) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

async function advance(milliseconds: number) {
  await act(async () => {
    vi.advanceTimersByTime(milliseconds)
  })
  await flushMicrotasks()
}

async function loadRequests(client = queryClient()) {
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(client)
  })
  return client
}

function renderPage(path = '/settings/requests') {
  return render(
    <QueryClientProvider client={queryClient()}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={[path]}>
            <SettingsPage />
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.useFakeTimers()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getRequestContentLogging.mockReset().mockResolvedValue(loggingFixture)
  api.getUsageRetention.mockReset().mockResolvedValue(retentionFixture)
  api.getStreamDeltaBatching.mockReset().mockResolvedValue(batchingFixture)
  api.getModelRouteWhitelist.mockReset().mockResolvedValue(whitelistFixture)
  api.setRequestContentLogging.mockReset()
  api.setUsageRetention.mockReset()
  api.setStreamDeltaBatching.mockReset()
  api.setModelRouteWhitelist.mockReset()
  useSettingsRequestsStore.getState().resetIdentity(null)
  setSettingsRequestsNotifier(null)
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: admin,
  })
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        readonly disconnect = vi.fn()
        readonly observe = vi.fn()
        readonly unobserve = vi.fn()
        constructor(_callback: ResizeObserverCallback) {}
      },
    })
  }
})

afterEach(() => {
  cleanup()
  disposeSettingsRequestsAutosave()
  useSettingsRequestsStore.getState().resetIdentity(null)
  vi.useRealTimers()
})

describe('requests settings pure model', () => {
  it('serializes a snapshot with sorted keys', () => {
    expect(settingsSnapshot({ b: 1, a: 2 })).toBe('{"a":2,"b":1}')
    expect(settingsSnapshot({ a: 2, b: 1 })).toBe('{"a":2,"b":1}')
  })

  it('clamps an edited integer and ignores an unparseable entry', () => {
    expect(clampSettingInteger(0, SETTING_FORM_RANGES.rawRetentionDays)).toBe(1)
    expect(clampSettingInteger(31, SETTING_FORM_RANGES.rawRetentionDays)).toBe(
      30,
    )
    expect(clampSettingInteger(12, SETTING_FORM_RANGES.rawRetentionDays)).toBe(
      12,
    )
    expect(
      clampSettingInteger(9999, SETTING_FORM_RANGES.approvalRetentionDays),
    ).toBe(9999)
    expect(
      parseSettingInteger('', SETTING_FORM_RANGES.rawRetentionDays),
    ).toBeNull()
    expect(
      parseSettingInteger('abc', SETTING_FORM_RANGES.rawRetentionDays),
    ).toBeNull()
    expect(
      parseSettingInteger('77', SETTING_FORM_RANGES.rawRetentionDays),
    ).toBe(30)
  })

  it('merges only the fields the operator did not change', () => {
    expect(
      mergeRequestContentLoggingResponse(
        { mode: 'off', raw_retention_days: 7 },
        { mode: 'off', raw_retention_days: 7 },
        { mode: 'off', raw_retention_days: 9 },
      ),
    ).toEqual({ mode: 'off', raw_retention_days: 9 })
    expect(
      mergeRequestContentLoggingResponse(
        { mode: 'normalized_only', raw_retention_days: 7 },
        { mode: 'off', raw_retention_days: 7 },
        { mode: 'off', raw_retention_days: 9 },
      ),
    ).toEqual({ mode: 'normalized_only', raw_retention_days: 9 })

    expect(
      mergeUsageRetentionResponse(
        { ...retentionFixture, approval_retention_days: 45 },
        retentionFixture,
        { ...retentionFixture, approval_retention_days: 30 },
      ).approval_retention_days,
    ).toBe(45)
    expect(
      mergeUsageRetentionResponse(retentionFixture, retentionFixture, {
        ...retentionFixture,
        approval_retention_days: 31,
      }).approval_retention_days,
    ).toBe(31)

    expect(mirrorSharedRawRetention({ raw_retention_days: 7 }, 7, 12)).toEqual({
      raw_retention_days: 12,
    })
    expect(mirrorSharedRawRetention({ raw_retention_days: 5 }, 7, 12)).toEqual({
      raw_retention_days: 5,
    })
    expect(
      mirrorSharedRawRetention({ raw_retention_days: 7 }, 7, undefined),
    ).toEqual({ raw_retention_days: 7 })

    expect(
      mergeStreamDeltaBatchingResponse(
        { ...batchingFixture, flush_window_ms: 200 },
        { ...batchingFixture, flush_window_ms: 100 },
        { ...batchingFixture, flush_window_ms: 100 },
      ).flush_window_ms,
    ).toBe(200)
  })

  it('blocks a scoped refresh while any requests draft or write is live', () => {
    const clean = {
      requestContentLogging: false,
      usageRetention: false,
      streamDeltaBatching: false,
      modelRouteWhitelist: false,
    }
    expect(
      settingsRefreshBlocked({
        pendingActions: {},
        dirty: clean,
        failed: clean,
      }),
    ).toBe(false)
    expect(
      settingsRefreshBlocked({
        pendingActions: {},
        dirty: { ...clean, streamDeltaBatching: true },
        failed: clean,
      }),
    ).toBe(true)
    expect(
      settingsRefreshBlocked({
        pendingActions: {
          [settingFormKey('requestContentLogging')]: 'settings',
        },
        dirty: clean,
        failed: clean,
      }),
    ).toBe(true)
  })
})

describe('requests settings store', () => {
  it('reads exactly the four requests-scoped settings and writes nothing on load', async () => {
    await loadRequests()

    expect(api.getRequestContentLogging).toHaveBeenCalledTimes(1)
    expect(api.getUsageRetention).toHaveBeenCalledTimes(1)
    expect(api.getStreamDeltaBatching).toHaveBeenCalledTimes(1)
    expect(api.getModelRouteWhitelist).toHaveBeenCalledTimes(1)
    expect(api.setRequestContentLogging).not.toHaveBeenCalled()
    expect(api.setUsageRetention).not.toHaveBeenCalled()
    expect(api.setStreamDeltaBatching).not.toHaveBeenCalled()
    expect(api.setModelRouteWhitelist).not.toHaveBeenCalled()

    expect(store().loaded).toBe(true)
    expect(store().requestContentLogging).toEqual(loggingFixture)
    expect(store().usageRetention).toEqual(retentionFixture)
    expect(store().streamDeltaBatching.max_buffer_chars).toBe(32768)
    expect(store().modelRouteWhitelist).toEqual({ enabled: false })
    expect(store().loadError).toBe(false)
  })

  it('coalesces the whitelist autosave and keeps the confirmed value', async () => {
    await loadRequests()
    api.setModelRouteWhitelist.mockResolvedValueOnce({ enabled: true })

    store().setModelRouteWhitelistEnabled(true)
    store().setModelRouteWhitelistEnabled(true)
    expect(api.setModelRouteWhitelist).not.toHaveBeenCalled()

    await advance(149)
    expect(api.setModelRouteWhitelist).not.toHaveBeenCalled()
    await advance(1)

    expect(api.setModelRouteWhitelist).toHaveBeenCalledTimes(1)
    expect(api.setModelRouteWhitelist.mock.calls[0]?.[0]?.body).toEqual({
      enabled: true,
    })
    expect(store().modelRouteWhitelist).toEqual({ enabled: true })
    expect(store().dirty.modelRouteWhitelist).toBe(false)
  })

  it('keeps the two retention forms on one key and retries the held one on release', async () => {
    await loadRequests()
    const loggingWrite = deferred<RequestContentLoggingResponse>()
    api.setRequestContentLogging.mockReturnValueOnce(loggingWrite.promise)
    api.setUsageRetention.mockResolvedValueOnce({
      ...retentionFixture,
      approval_retention_days: 45,
    })

    store().setRequestContentLoggingMode('normalized_only')
    await advance(250)
    expect(api.setRequestContentLogging).toHaveBeenCalledTimes(1)
    expect(Object.hasOwn(getPendingActions(), 'setting:rawRetention')).toBe(
      true,
    )

    store().setApprovalRetentionDays(45)
    await advance(250)
    expect(api.setUsageRetention).not.toHaveBeenCalled()
    expect(store().failed.usageRetention).toBe(false)
    expect(store().dirty.usageRetention).toBe(true)

    loggingWrite.resolve({ mode: 'normalized_only', raw_retention_days: 7 })
    await flushMicrotasks(8)

    expect(api.setUsageRetention).toHaveBeenCalledTimes(1)
    const body = api.setUsageRetention.mock.calls[0]?.[0]?.body
    expect(body.approval_retention_days).toBe(45)
    expect(body.raw_retention_days).toBe(7)
    expect(body.content_retention_days).toBe(30)
    expect(body.metadata_retention_days).toBe(90)
    expect(body.raw_backend).toBe('local')
    expect(body.replay_enabled).toBe(true)
    expect(store().dirty.usageRetention).toBe(false)
  })

  it('writes the logging read-modify-write and mirrors the shared raw days', async () => {
    await loadRequests()
    api.setRequestContentLogging.mockResolvedValueOnce({
      mode: 'off',
      raw_retention_days: 12,
    })

    store().setRequestContentLoggingMode('off')
    store().setRawRetentionDays(12)
    await advance(250)

    expect(api.setRequestContentLogging).toHaveBeenCalledTimes(1)
    expect(api.setRequestContentLogging.mock.calls[0]?.[0]?.body).toEqual({
      mode: 'off',
      raw_retention_days: 12,
    })
    expect(store().requestContentLogging).toEqual({
      mode: 'off',
      raw_retention_days: 12,
    })
    expect(store().usageRetention.raw_retention_days).toBe(12)
  })

  it('never overwrites an edit made while the same section is in flight', async () => {
    await loadRequests()
    const write = deferred<StreamDeltaBatchingSettings>()
    api.setStreamDeltaBatching
      .mockReturnValueOnce(write.promise)
      .mockResolvedValueOnce({ ...batchingFixture, flush_window_ms: 200 })

    store().setStreamDeltaBatching({ flush_window_ms: 100 })
    await advance(300)
    expect(api.setStreamDeltaBatching).toHaveBeenCalledTimes(1)

    store().setStreamDeltaBatching({ flush_window_ms: 200 })
    await advance(300)
    write.resolve({ ...batchingFixture, flush_window_ms: 100 })
    await flushMicrotasks(8)

    expect(store().streamDeltaBatching.flush_window_ms).toBe(200)
    expect(api.setStreamDeltaBatching).toHaveBeenCalledTimes(2)
    expect(
      api.setStreamDeltaBatching.mock.calls[1]?.[0]?.body.flush_window_ms,
    ).toBe(200)
  })

  it('keeps a failed draft, releases the key, notifies once and retries on request', async () => {
    await loadRequests()
    const notify = vi.fn()
    setSettingsRequestsNotifier(notify)
    api.setStreamDeltaBatching
      .mockRejectedValueOnce(new Error('503 service unavailable'))
      .mockResolvedValueOnce({ ...batchingFixture, flush_window_ms: 500 })

    store().setStreamDeltaBatching({ flush_window_ms: 500 })
    await advance(300)

    expect(notify).toHaveBeenCalledTimes(1)
    expect(store().failed.streamDeltaBatching).toBe(true)
    expect(store().dirty.streamDeltaBatching).toBe(true)
    expect(store().streamDeltaBatching.flush_window_ms).toBe(500)
    expect(
      Object.hasOwn(getPendingActions(), 'setting:streamDeltaBatching'),
    ).toBe(false)

    await advance(5000)
    expect(api.setStreamDeltaBatching).toHaveBeenCalledTimes(1)

    await act(async () => {
      store().retryFailedSection('streamDeltaBatching')
    })
    await flushMicrotasks(8)

    expect(api.setStreamDeltaBatching).toHaveBeenCalledTimes(2)
    expect(store().failed.streamDeltaBatching).toBe(false)
    expect(store().streamDeltaBatching.flush_window_ms).toBe(500)
  })

  it('commits only the latest read generation', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const resolvers: Array<(value: UsageRetentionSettings) => void> = []
    api.getUsageRetention.mockImplementation(
      () =>
        new Promise<UsageRetentionSettings>((resolve) => {
          resolvers.push(resolve)
        }),
    )

    void store().refresh(client)
    void store().refresh(client)
    await flushMicrotasks(4)
    expect(resolvers.length).toBeGreaterThan(0)

    const current = resolvers.at(-1) as (value: UsageRetentionSettings) => void
    current({ ...retentionFixture, approval_retention_days: 60 })
    await flushMicrotasks(8)
    expect(store().usageRetention.approval_retention_days).toBe(60)

    for (const stale of resolvers.slice(0, -1)) {
      stale({ ...retentionFixture, approval_retention_days: 99 })
    }
    await flushMicrotasks(8)
    expect(store().usageRetention.approval_retention_days).toBe(60)
  })

  it('drops an in-flight read once the operator edits', async () => {
    await loadRequests()
    const client = queryClient()
    const slowRead = deferred<UsageRetentionSettings>()
    api.getUsageRetention.mockReturnValueOnce(slowRead.promise)
    api.setUsageRetention.mockResolvedValueOnce({
      ...retentionFixture,
      approval_retention_days: 45,
    })

    void store().refresh(client)
    await flushMicrotasks(2)
    expect(store().loading).toBe(true)

    store().setApprovalRetentionDays(45)
    expect(store().loading).toBe(false)
    await advance(250)
    expect(api.setUsageRetention).toHaveBeenCalledTimes(1)

    slowRead.resolve({ ...retentionFixture, approval_retention_days: 30 })
    await flushMicrotasks(8)

    expect(store().usageRetention.approval_retention_days).toBe(45)
    expect(store().failed.usageRetention).toBe(false)
  })

  it('drops a completion that belongs to a previous identity', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const pending = deferred<RequestContentLoggingResponse>()
    api.getRequestContentLogging.mockReturnValueOnce(pending.promise)

    const read = store().refresh(client)
    store().resetIdentity(operator)
    pending.resolve({ mode: 'normalized_and_raw', raw_retention_days: 12 })
    await act(async () => {
      await read
    })

    expect(store().identityKey).toBe('2:fixture-operator:false')
    expect(store().loaded).toBe(false)
    expect(store().requestContentLogging).toEqual({
      mode: 'off',
      raw_retention_days: 1,
    })
  })

  it('does not refetch when the page rerenders for an unrelated reason', async () => {
    const client = await loadRequests()
    expect(api.getRequestContentLogging).toHaveBeenCalledTimes(1)

    await act(async () => {
      await store().ensureLoaded(client)
    })

    expect(api.getRequestContentLogging).toHaveBeenCalledTimes(1)
  })
})

describe('requests settings tab', () => {
  it('renders the four stored settings with their controls and no save button', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    api.getRequestContentLogging.mockResolvedValue(loggingFixture)
    api.getUsageRetention.mockResolvedValue(retentionFixture)
    api.getStreamDeltaBatching.mockResolvedValue(batchingFixture)
    api.getModelRouteWhitelist.mockResolvedValue(whitelistFixture)

    render(
      <QueryClientProvider client={client}>
        <LocaleProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={['/settings/requests']}>
              <SettingsPage />
            </MemoryRouter>
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks(8)

    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-REQUESTS',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('requests')
    expect(
      screen.getByRole('heading', { name: copy.settingsUsage }),
    ).toBeTruthy()

    for (const id of [
      'settings-request-content-logging-mode',
      'settings-raw-retention-days',
      'settings-approval-retention-days',
      'settings-model-route-whitelist-enabled',
      'settings-stream-delta-batching-enabled',
      'settings-flush-window-ms',
      'settings-max-buffer-chars',
      'settings-max-buffer-bytes',
      'settings-flush-on-line-break',
      'settings-flush-on-sentence-end',
    ]) {
      expect(document.getElementById(id), id).not.toBeNull()
    }
    expect(document.querySelectorAll('[data-settings-card]').length).toBe(3)
    expect(screen.queryByRole('button', { name: copy.saved })).toBeNull()
    expect(screen.queryByRole('button', { name: /^Save$/ })).toBeNull()

    expect(
      (
        document.getElementById(
          'settings-raw-retention-days',
        ) as HTMLInputElement
      ).value,
    ).toBe('7')
    expect(
      (
        document.getElementById(
          'settings-approval-retention-days',
        ) as HTMLInputElement
      ).value,
    ).toBe('30')
    expect(
      (document.getElementById('settings-max-buffer-chars') as HTMLInputElement)
        .value,
    ).toBe('32768')
    expect(
      (
        document.getElementById(
          'settings-request-content-logging-mode',
        ) as HTMLElement
      ).textContent,
    ).toContain(copy.contentLoggingModeNormalizedAndRaw)
    expect(api.setStreamDeltaBatching).not.toHaveBeenCalled()
  })

  it('disables both retention controls while either writes and spins only the claimant', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    render(
      <QueryClientProvider client={client}>
        <LocaleProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={['/settings/requests']}>
              <SettingsPage />
            </MemoryRouter>
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks(8)

    const loggingWrite = deferred<RequestContentLoggingResponse>()
    api.setRequestContentLogging.mockReturnValueOnce(loggingWrite.promise)
    store().setRequestContentLoggingMode('normalized_only')
    await advance(250)

    expect(
      (
        document.getElementById(
          'settings-raw-retention-days',
        ) as HTMLInputElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        document.getElementById(
          'settings-approval-retention-days',
        ) as HTMLInputElement
      ).disabled,
    ).toBe(true)
    expect(screen.getAllByRole('status', { name: copy.loading })).toHaveLength(
      1,
    )

    loggingWrite.resolve({ mode: 'normalized_only', raw_retention_days: 7 })
    await flushMicrotasks(8)

    expect(
      (
        document.getElementById(
          'settings-raw-retention-days',
        ) as HTMLInputElement
      ).disabled,
    ).toBe(false)

    const retentionWrite = deferred<UsageRetentionSettings>()
    api.setUsageRetention.mockReturnValueOnce(retentionWrite.promise)
    store().setApprovalRetentionDays(45)
    await advance(250)

    expect(screen.getAllByRole('status', { name: copy.loading })).toHaveLength(
      1,
    )
    const approvalLabel = screen.getByText(copy.approvalRetentionDays)
    expect(
      within(approvalLabel.closest('label') as HTMLElement).getAllByRole(
        'status',
        { name: copy.loading },
      ),
    ).toHaveLength(1)

    retentionWrite.resolve({ ...retentionFixture, approval_retention_days: 45 })
    await flushMicrotasks(8)
  })

  it('shows a retry control only for the failed section', async () => {
    useSessionStore.setState({ me: admin })
    api.setStreamDeltaBatching.mockRejectedValueOnce(new Error('503'))
    renderPage()
    await flushMicrotasks(8)

    expect(screen.queryByRole('button', { name: copy.retry })).toBeNull()

    store().setStreamDeltaBatching({ flush_window_ms: 500 })
    await advance(300)

    expect(store().failed.streamDeltaBatching).toBe(true)
    expect(screen.getAllByRole('button', { name: copy.retry })).toHaveLength(1)
  })

  it('disposes the debounce on leave so nothing writes after unmount', async () => {
    useSessionStore.setState({ me: admin })
    const view = renderPage()
    await flushMicrotasks(8)

    store().setApprovalRetentionDays(46)
    view.unmount()
    await advance(5000)

    expect(api.setUsageRetention).not.toHaveBeenCalled()
    expect(store().usageRetention.approval_retention_days).toBe(46)
  })

  it('keeps an ordinary user out of the requests route', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()
    await flushMicrotasks(4)

    expect(
      document.querySelector(
        '[data-foundation-route-id="R-SETTINGS-REQUESTS"]',
      ),
    ).toBeNull()
    expect(api.getRequestContentLogging).not.toHaveBeenCalled()
  })

  it('disables the scoped refresh while a draft is dirty and refreshes without writing', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    api.getRequestContentLogging.mockResolvedValue(loggingFixture)
    api.getUsageRetention.mockResolvedValue(retentionFixture)
    api.getStreamDeltaBatching.mockResolvedValue(batchingFixture)
    api.getModelRouteWhitelist.mockResolvedValue(whitelistFixture)
    api.setUsageRetention.mockResolvedValue({
      ...retentionFixture,
      approval_retention_days: 45,
    })
    render(
      <QueryClientProvider client={client}>
        <LocaleProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={['/settings/requests']}>
              <SettingsPage />
            </MemoryRouter>
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks(8)

    const refreshButton = document.getElementById(
      'settings-requests-refresh',
    ) as HTMLButtonElement
    expect(refreshButton.disabled).toBe(false)

    await act(async () => {
      store().setApprovalRetentionDays(45)
    })
    expect(refreshButton.disabled).toBe(true)

    await advance(250)
    expect(api.setUsageRetention).toHaveBeenCalledTimes(1)
    expect(refreshButton.disabled).toBe(false)

    fireEvent.click(refreshButton)
    await flushMicrotasks(8)
    expect(api.getRequestContentLogging).toHaveBeenCalledTimes(2)
    expect(api.setRequestContentLogging).not.toHaveBeenCalled()
  })
})
