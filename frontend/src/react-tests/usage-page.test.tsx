import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({
  clearRequestRecords: vi.fn(),
  deleteConversationEndpointOverride: vi.fn(),
  listRequestRecords: vi.fn(),
  listUserOptions: vi.fn(),
  requestRecordDetail: vi.fn(),
  requestRecordFacets: vi.fn(),
  requestRecordFull: vi.fn(),
  requestRecordOverview: vi.fn(),
  requestRecordResetSessionAffinity: vi.fn(),
  requestRecordSessionRouteOptions: vi.fn(),
  setConversationEndpointOverride: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { ThemeProvider } from '@/lib/theme'
import { useSessionStore } from '@/lib/session'
import { useUsageStore } from '@/lib/usage'
import { useUsageDetailStore } from '@/lib/usage-detail'
import { getPendingActions } from '@/lib/async-action'
import { UsagePage } from '@/pages/UsagePage'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

/** A promise the test releases on its own. */
type Deferred<T> = {
  promise: Promise<T>
  resolve: (value: T) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

/** The payload the shared fixture resolves with. */
type UsageFixturePayload = { data: unknown }

/**
 * The payload the shared `beforeEach` fixture resolves with. The fixture mock
 * is read through its own implementation, so the payload is the real one and
 * the mock call counter stays untouched.
 */
async function fixturePayload(mock: {
  getMockImplementation: () => (() => Promise<UsageFixturePayload>) | undefined
}): Promise<UsageFixturePayload> {
  const implementation = mock.getMockImplementation()
  if (!implementation) {
    throw new Error('the shared fixture mock has no implementation')
  }
  return implementation()
}

/** The safe counters and store scalars a failure is reported with. */
type UsagePageDiagnostics = {
  stage: string
  counts: Record<string, number>
  store: {
    recordsLoading: boolean
    facetsLoading: boolean
    overviewLoading: boolean
    pageLoading: boolean
    overviewAvailable: boolean
    records: number
    total: number
  }
}

function usagePageDiagnostics(stage: string): UsagePageDiagnostics {
  const state = useUsageStore.getState()
  return {
    stage,
    counts: {
      listRequestRecords: api.listRequestRecords.mock.calls.length,
      requestRecordFacets: api.requestRecordFacets.mock.calls.length,
      requestRecordOverview: api.requestRecordOverview.mock.calls.length,
      listUserOptions: api.listUserOptions.mock.calls.length,
      requestRecordDetail: api.requestRecordDetail.mock.calls.length,
      requestRecordFull: api.requestRecordFull.mock.calls.length,
      requestRecordSessionRouteOptions:
        api.requestRecordSessionRouteOptions.mock.calls.length,
      requestRecordResetSessionAffinity:
        api.requestRecordResetSessionAffinity.mock.calls.length,
      deleteConversationEndpointOverride:
        api.deleteConversationEndpointOverride.mock.calls.length,
      setConversationEndpointOverride:
        api.setConversationEndpointOverride.mock.calls.length,
      clearRequestRecords: api.clearRequestRecords.mock.calls.length,
    },
    store: {
      recordsLoading: state.recordsLoading,
      facetsLoading: state.facetsLoading,
      overviewLoading: state.overviewLoading,
      pageLoading: state.pageLoading,
      overviewAvailable: state.overview !== null,
      records: state.records.length,
      total: state.total,
    },
  }
}

/**
 * Attaches the stage, the SDK call counts and the store scalars to the
 * original failure and rethrows that same error. The assertion, its expected
 * and received values and its stack are kept.
 */
function attachUsagePageDiagnostics(cause: unknown, stage: string): unknown {
  if (!(cause instanceof Error)) return cause
  cause.message = `${cause.message}\nusage-page diagnostics: ${JSON.stringify(
    usagePageDiagnostics(stage),
  )}`
  return cause
}

beforeEach(() => {
  localStorage.clear()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  Element.prototype.scrollIntoView = vi.fn()
  vi.clearAllMocks()
  useSessionStore.setState({
    me: {
      display_name: 'Fixture Admin',
      is_admin: true,
      login_name: 'admin',
      user_id: 1,
    },
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useUsageStore.getState().resetIdentity(null)
  useUsageDetailStore.getState().reset()
  api.listRequestRecords.mockResolvedValue({
    data: {
      first: 0,
      records: [
        {
          client_key_label: 'primary',
          conversation_source: 'fixture',
          created_at: '2026-03-04T23:30:00.000Z',
          has_full_request: true,
          has_parent: false,
          path: '/v1/responses',
          record_id: 1,
          redaction: {
            applied: false,
            fields: [],
            findings_count: 0,
            replacements_count: 0,
            types: [],
          },
          request_category: 'ai',
          request_id: 'req-1',
          request_state: 'completed',
          route_selection_reason: 'default',
          storage_sanitized: false,
          storage_sanitized_nul_count: 0,
        },
      ],
      rows: 25,
      total: 1,
    },
  })
  api.requestRecordFacets.mockResolvedValue({
    data: { dates: [], models: [], states: [], users: [] },
  })
  api.requestRecordOverview.mockResolvedValue({
    data: {
      breakdown: [],
      summary: {
        error_count: 0,
        method_count: 0,
        request_count: 1,
        success_count: 1,
        success_rate: 1,
        tokens: {
          cache_read_tokens: 0,
          cache_write_tokens: 0,
          input_tokens: 0,
          output_tokens: 0,
          total_tokens: 0,
        },
      },
      trend: [],
      trend_granularity: 'hour',
    },
  })
  api.listUserOptions.mockResolvedValue({ data: { users: [] } })
  api.clearRequestRecords.mockResolvedValue({
    data: { deleted: 2, deleted_prompt_blocks: 2, protected_by_billing: 0 },
  })
  api.requestRecordDetail.mockResolvedValue({
    data: {
      conversation_id: 'conversation-1',
      conversation_source: 'fixture',
      created_at: '2026-03-04T23:30:00.000Z',
      has_full_request: false,
      has_parent: false,
      http_request_compressed: false,
      path: '/v1/responses',
      record_id: 1,
      redaction: {
        applied: false,
        fields: [],
        findings_count: 0,
        replacements_count: 0,
        types: [],
      },
      request_category: 'ai',
      request_has_previous_response_id: false,
      request_id: 'req-1',
      request_state: 'completed',
      request_storage_mode: 'normalized',
      response_capture_truncated: false,
      route_selection_reason: 'default',
      storage_sanitized: false,
      storage_sanitized_nul_count: 0,
    },
  })
  api.requestRecordSessionRouteOptions.mockResolvedValue({
    data: {
      affinity: { state: 'unbound' },
      conversation_id: 'conversation-1',
      options: [],
      override_endpoint_id: null,
    },
  })
})

function renderPage(path = '/request-records/ai') {
  const client = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={client}>
      <LocaleProvider>
        <ThemeProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={[path]}>
              <Routes>
                <Route
                  element={<UsagePage />}
                  path="/request-records/:category"
                />
              </Routes>
            </MemoryRouter>
          </NotifierProvider>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, client }
}

describe('usage page integration', () => {
  it('loads each read once on mount and shows the overview', async () => {
    // The stage names the gate a failure happened in, so a captured failure
    // says which observation was still open.
    let stage = 'mount'
    try {
      renderPage()
      stage = 'three mount reads issued'
      await waitFor(() => {
        expect(api.listRequestRecords).toHaveBeenCalled()
        expect(api.requestRecordFacets).toHaveBeenCalled()
        expect(api.requestRecordOverview).toHaveBeenCalled()
      })
      stage = 'settled overview committed'
      await waitFor(() => {
        const state = useUsageStore.getState()
        expect(state.overview).not.toBeNull()
        expect(state.pageLoading).toBe(false)
      })
      stage = 'route id rendered'
      await waitFor(() =>
        expect(
          document.querySelector('[data-foundation-route-id]'),
        ).toBeTruthy(),
      )
      stage = 'route id asserted'
      expect(
        document
          .querySelector('[data-foundation-route-id]')
          ?.getAttribute('data-foundation-route-id'),
      ).toBe('R-USAGE-AI')
      stage = 'exact read counts'
      expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
      expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
      expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
      stage = 'no unrelated reads'
      // The mount reads the usage overview only; the detail, session and
      // maintenance reads stay untouched until an operator asks for them.
      expect(api.requestRecordDetail).not.toHaveBeenCalled()
      expect(api.requestRecordFull).not.toHaveBeenCalled()
      expect(api.requestRecordSessionRouteOptions).not.toHaveBeenCalled()
      expect(api.requestRecordResetSessionAffinity).not.toHaveBeenCalled()
      expect(api.deleteConversationEndpointOverride).not.toHaveBeenCalled()
      expect(api.setConversationEndpointOverride).not.toHaveBeenCalled()
      expect(api.clearRequestRecords).not.toHaveBeenCalled()
      stage = 'overview heading rendered'
      expect(
        screen.getByRole('heading', { name: copy.aiRequests }),
      ).toBeTruthy()
      stage = 'ten overview metrics rendered'
      await waitFor(() =>
        expect(document.querySelectorAll('[data-usage-metric]')).toHaveLength(
          10,
        ),
      )
    } catch (cause) {
      throw attachUsagePageDiagnostics(cause, stage)
    }
  })

  /** The three mount reads are issued once each and none of them repeats. */
  function expectSingleMountReads(): void {
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
  }

  /** The mount issues no detail, session or maintenance read. */
  function expectNoMaintenanceOrDetailReads(): void {
    expect(api.requestRecordDetail).not.toHaveBeenCalled()
    expect(api.requestRecordFull).not.toHaveBeenCalled()
    expect(api.requestRecordSessionRouteOptions).not.toHaveBeenCalled()
    expect(api.requestRecordResetSessionAffinity).not.toHaveBeenCalled()
    expect(api.deleteConversationEndpointOverride).not.toHaveBeenCalled()
    expect(api.setConversationEndpointOverride).not.toHaveBeenCalled()
    expect(api.clearRequestRecords).not.toHaveBeenCalled()
  }

  it('settles the mount reads on their own when the facets read resolves first', async () => {
    const recordsPayload = await fixturePayload(api.listRequestRecords)
    const facetsPayload = await fixturePayload(api.requestRecordFacets)
    const overviewPayload = await fixturePayload(api.requestRecordOverview)

    const records = deferred<UsageFixturePayload>()
    const facets = deferred<UsageFixturePayload>()
    const overview = deferred<UsageFixturePayload>()
    api.listRequestRecords.mockImplementation(() => records.promise)
    api.requestRecordFacets.mockImplementation(() => facets.promise)
    api.requestRecordOverview.mockImplementation(() => overview.promise)

    renderPage()

    // Every mount read is issued before any of them is released.
    await waitFor(() => {
      expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
      expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
      expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    })

    // The facets read settles first, on its own.
    await act(async () => {
      facets.resolve(facetsPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().facets).toEqual(facetsPayload.data),
    )
    expect(useUsageStore.getState().facetsLoading).toBe(false)
    expect(useUsageStore.getState().overview).toBeNull()
    expect(useUsageStore.getState().overviewLoading).toBe(true)
    expect(useUsageStore.getState().recordsLoading).toBe(true)
    expectSingleMountReads()

    // The records read settles next, still without the overview.
    await act(async () => {
      records.resolve(recordsPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().records).toHaveLength(1),
    )
    expect(useUsageStore.getState().total).toBe(1)
    expect(useUsageStore.getState().recordsLoading).toBe(false)
    expect(useUsageStore.getState().overview).toBeNull()
    expect(useUsageStore.getState().overviewLoading).toBe(true)
    expectSingleMountReads()

    // The overview settles last and the page shows the overview.
    await act(async () => {
      overview.resolve(overviewPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().overview).not.toBeNull(),
    )
    expect(useUsageStore.getState().overviewLoading).toBe(false)
    expect(useUsageStore.getState().pageLoading).toBe(false)
    expectSingleMountReads()
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)
    expectNoMaintenanceOrDetailReads()
    expect(screen.getByRole('heading', { name: copy.aiRequests })).toBeTruthy()
    await waitFor(() =>
      expect(document.querySelectorAll('[data-usage-metric]')).toHaveLength(10),
    )
  })

  it('settles the mount reads on their own when the overview read resolves first', async () => {
    const recordsPayload = await fixturePayload(api.listRequestRecords)
    const facetsPayload = await fixturePayload(api.requestRecordFacets)
    const overviewPayload = await fixturePayload(api.requestRecordOverview)

    const records = deferred<UsageFixturePayload>()
    const facets = deferred<UsageFixturePayload>()
    const overview = deferred<UsageFixturePayload>()
    api.listRequestRecords.mockImplementation(() => records.promise)
    api.requestRecordFacets.mockImplementation(() => facets.promise)
    api.requestRecordOverview.mockImplementation(() => overview.promise)

    renderPage()

    // Every mount read is issued before any of them is released.
    await waitFor(() => {
      expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
      expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
      expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    })

    // The overview read settles first, on its own.
    await act(async () => {
      overview.resolve(overviewPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().overview).not.toBeNull(),
    )
    expect(useUsageStore.getState().overviewLoading).toBe(false)
    expect(useUsageStore.getState().recordsLoading).toBe(true)
    expect(useUsageStore.getState().facetsLoading).toBe(true)
    expect(useUsageStore.getState().records).toHaveLength(0)
    expectSingleMountReads()

    // The records read settles next, still without the facets.
    await act(async () => {
      records.resolve(recordsPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().records).toHaveLength(1),
    )
    expect(useUsageStore.getState().total).toBe(1)
    expect(useUsageStore.getState().recordsLoading).toBe(false)
    expect(useUsageStore.getState().facetsLoading).toBe(true)
    expectSingleMountReads()

    // The facets read settles last and the mount load is settled.
    await act(async () => {
      facets.resolve(facetsPayload)
    })
    await waitFor(() =>
      expect(useUsageStore.getState().facets).toEqual(facetsPayload.data),
    )
    expect(useUsageStore.getState().facetsLoading).toBe(false)
    expect(useUsageStore.getState().pageLoading).toBe(false)
    expectSingleMountReads()
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)
    expectNoMaintenanceOrDetailReads()
    expect(screen.getByRole('heading', { name: copy.aiRequests })).toBeTruthy()
    await waitFor(() =>
      expect(document.querySelectorAll('[data-usage-metric]')).toHaveLength(10),
    )
  })

  it('keeps the mode local without a request', async () => {
    renderPage()
    await waitFor(() => expect(api.listRequestRecords).toHaveBeenCalled())
    api.listRequestRecords.mockClear()
    api.requestRecordFacets.mockClear()
    api.requestRecordOverview.mockClear()

    fireEvent.click(screen.getByRole('button', { name: copy.recordsMode }))
    expect(screen.getByRole('table')).toBeTruthy()
    expect(api.listRequestRecords).not.toHaveBeenCalled()
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
  })

  it('collapses the record filters by default and keeps the draft and active filters', async () => {
    renderPage()
    await waitFor(() => expect(api.listRequestRecords).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: copy.recordsMode }))

    const toggle = screen.getByRole('button', { name: copy.advancedFilters })
    const panel = document.querySelector(
      '[data-usage-filter-panel]',
    ) as HTMLElement
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(toggle.getAttribute('aria-controls')).toBe(panel.id)
    expect(panel.hasAttribute('hidden')).toBe(true)
    expect(screen.getByRole('button', { name: copy.clearHistory })).toBeTruthy()

    fireEvent.click(toggle)
    expect(toggle.getAttribute('aria-expanded')).toBe('true')
    expect(panel.hasAttribute('hidden')).toBe(false)

    const input = screen.getByPlaceholderText(copy.search) as HTMLInputElement
    fireEvent.change(input, { target: { value: 'draft needle' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() =>
      expect(toggle.getAttribute('aria-label')).toBe(
        copy.advancedFiltersActive.replace('{count}', '1'),
      ),
    )
    expect(document.querySelector('[data-usage-active-filters]')).toBeTruthy()

    fireEvent.click(toggle)
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(panel.hasAttribute('hidden')).toBe(true)
    expect(input.value).toBe('draft needle')
    expect(toggle.getAttribute('aria-label')).toBe(
      copy.advancedFiltersActive.replace('{count}', '1'),
    )
  })

  it('loads the MCP category from the route without refreshing twice', async () => {
    renderPage('/request-records/mcp')
    await waitFor(() => expect(api.requestRecordOverview).toHaveBeenCalled())
    expect(
      document
        .querySelector('[data-foundation-route-id]')
        ?.getAttribute('data-foundation-route-id'),
    ).toBe('R-USAGE-MCP')
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(api.requestRecordOverview.mock.calls[0]?.[0]?.query).toMatchObject({
      request_category: 'mcp',
    })
  })

  it('opens the AI detail from the records table and closes it cleanly', async () => {
    renderPage()
    await waitFor(() => expect(api.listRequestRecords).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: copy.recordsMode }))
    await waitFor(() =>
      expect(
        document.querySelector('[data-usage-record-row="1"]'),
      ).toBeTruthy(),
    )
    fireEvent.click(screen.getByRole('button', { name: copy.viewDetails }))
    await waitFor(() =>
      expect(api.requestRecordDetail).toHaveBeenCalledTimes(1),
    )
    await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy())
    expect(
      screen.getByRole('heading', { name: copy.requestDetails }),
    ).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(useUsageDetailStore.getState().detail).toBeNull()
  })

  it('closes the clear dialog only for the submitted form and refreshes once', async () => {
    renderPage()
    await waitFor(() => expect(api.listRequestRecords).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: copy.recordsMode }))
    api.listRequestRecords.mockClear()
    api.requestRecordFacets.mockClear()
    api.requestRecordOverview.mockClear()

    fireEvent.click(screen.getByRole('button', { name: copy.clearHistory }))
    const dialog = await screen.findByRole('dialog')
    expect(api.clearRequestRecords).not.toHaveBeenCalled()
    fireEvent.click(
      screen.getByRole('button', { name: copy.clearHistorySubmit }),
    )
    await waitFor(() =>
      expect(api.clearRequestRecords).toHaveBeenCalledTimes(1),
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(getPendingActions()['usage:maintenance']).toBeUndefined()
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(document.body.textContent).toContain(`${copy.clearHistorySubmit}: 2`)
  })
})
