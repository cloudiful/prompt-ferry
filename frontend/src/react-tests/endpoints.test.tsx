import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  EndpointPageResponse,
  EndpointRequest,
  EndpointTestResponse,
  MeResponse,
  ProviderEndpoint,
  TokenPlanUsageResponse,
  User,
} from '@/generated/admin-api'
import {
  listUserOptions,
  oauthBrowserComplete,
  oauthBrowserStart,
  oauthClear,
  oauthDevicePoll,
  oauthDeviceStart,
  oauthRefresh,
  oauthStatus,
  tokenPlanUsage,
} from '@/generated/admin-api'
import {
  deleteEndpointById,
  fetchEndpointsPage,
  fetchTokenPlanUsage,
  persistEndpoint,
  runEndpointTest,
  updateEndpointEnabled,
} from '@/stores/endpoints-api'
import { EndpointsPage } from '@/pages/EndpointsPage'
import {
  endpointRecordsQueryKey,
  modelRoutePagesQueryKey,
} from '@/lib/endpoints'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { getPendingActions } from '@/lib/async-action'
import {
  __resetTokenPlanCacheForTests,
  TOKEN_PLAN_CACHE_TTL_MS,
} from '@/lib/quota-cache'
import { __resetTokenPlanForegroundForTests } from '@/lib/quota-foreground'
import { minimaxQuotaKey, usage } from './helpers/token-plan-quota-fixtures'

vi.mock('@/generated/admin-api', () => ({
  listUserOptions: vi.fn(),
  oauthBrowserComplete: vi.fn(),
  oauthBrowserStart: vi.fn(),
  oauthClear: vi.fn(),
  oauthDevicePoll: vi.fn(),
  oauthDeviceStart: vi.fn(),
  oauthRefresh: vi.fn(),
  oauthStatus: vi.fn(),
  tokenPlanUsage: vi.fn(),
}))

vi.mock('@/stores/endpoints-api', () => ({
  deleteEndpointById: vi.fn(),
  fetchEndpointsPage: vi.fn(),
  fetchTokenPlanUsage: vi.fn(),
  persistEndpoint: vi.fn(),
  runEndpointTest: vi.fn(),
  updateEndpointEnabled: vi.fn(),
}))

const api = {
  deleteEndpointById: vi.mocked(deleteEndpointById) as unknown as ReturnType<
    typeof vi.fn
  >,
  fetchEndpointsPage: vi.mocked(fetchEndpointsPage) as unknown as ReturnType<
    typeof vi.fn
  >,
  fetchTokenPlanUsage: vi.mocked(fetchTokenPlanUsage) as unknown as ReturnType<
    typeof vi.fn
  >,
  listUserOptions: vi.mocked(listUserOptions) as unknown as ReturnType<
    typeof vi.fn
  >,
  oauthBrowserComplete: vi.mocked(
    oauthBrowserComplete,
  ) as unknown as ReturnType<typeof vi.fn>,
  oauthBrowserStart: vi.mocked(oauthBrowserStart) as unknown as ReturnType<
    typeof vi.fn
  >,
  oauthClear: vi.mocked(oauthClear) as unknown as ReturnType<typeof vi.fn>,
  oauthDevicePoll: vi.mocked(oauthDevicePoll) as unknown as ReturnType<
    typeof vi.fn
  >,
  oauthDeviceStart: vi.mocked(oauthDeviceStart) as unknown as ReturnType<
    typeof vi.fn
  >,
  oauthRefresh: vi.mocked(oauthRefresh) as unknown as ReturnType<typeof vi.fn>,
  oauthStatus: vi.mocked(oauthStatus) as unknown as ReturnType<typeof vi.fn>,
  tokenPlanUsage: vi.mocked(tokenPlanUsage) as unknown as ReturnType<
    typeof vi.fn
  >,
  persistEndpoint: vi.mocked(persistEndpoint) as unknown as ReturnType<
    typeof vi.fn
  >,
  runEndpointTest: vi.mocked(runEndpointTest) as unknown as ReturnType<
    typeof vi.fn
  >,
  updateEndpointEnabled: vi.mocked(
    updateEndpointEnabled,
  ) as unknown as ReturnType<typeof vi.fn>,
}

const admin: MeResponse = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'admin',
  user_id: 1,
}

const operator: MeResponse = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

const fixtureUsers: User[] = [
  {
    created_at: '2026-10-16T12:00:00.000Z',
    display_name: 'Fixture Administrator',
    is_active: true,
    is_admin: true,
    login_name: 'admin',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 1,
  },
  {
    created_at: '2026-10-16T12:00:00.000Z',
    display_name: 'Fixture Operator',
    is_active: true,
    is_admin: false,
    login_name: 'operator',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 2,
  },
]

function makeEndpoint(
  index: number,
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    active_windows: [],
    api_keys: [
      {
        created_at: '2026-10-16T12:00:00.000Z',
        enabled: true,
        endpoint_id: `endpoint-${index}`,
        key_id: `key-${index}`,
        key_label: 'Primary',
        position: 0,
        updated_at: '2026-10-23T12:00:00.000Z',
      },
    ],
    base_url: `https://api-${index}.example.test`,
    created_at: '2026-10-16T12:00:00.000Z',
    enabled: true,
    endpoint_id: `endpoint-${index}`,
    has_admin_api_key: false,
    has_oauth_token: false,
    has_proxy_url: false,
    key_lb_enabled: false,
    mcp_enabled: false,
    name: `Upstream ${index}`,
    native_api: 'auto',
    native_api_source: 'auto',
    owner_user_id: null,
    plan: 'platform_api_key',
    provider: 'openai',
    provider_region: null,
    scope: 'admin',
    service_tier: null,
    updated_at: '2026-10-23T12:00:00.000Z',
    ...overrides,
  }
}

function endpointPage(
  endpoints: ProviderEndpoint[],
  first = 0,
  rows = 10,
  total = endpoints.length,
): EndpointPageResponse {
  return { endpoints, first, rows, total }
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

let endpointRecords: ProviderEndpoint[]

function renderPage(me: MeResponse = admin) {
  useSessionStore.setState({ me, bootstrapped: true, bootstrapping: false })
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
      mutations: { retry: false },
    },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <EndpointsPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

function endpointCard(endpointId: string): HTMLElement {
  return document.querySelector(
    `[data-endpoint-card="${endpointId}"]`,
  ) as HTMLElement
}

beforeEach(() => {
  vi.resetAllMocks()
  Element.prototype.scrollIntoView = () => undefined
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    })
  }
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
  endpointRecords = [makeEndpoint(1)]
  api.fetchEndpointsPage.mockImplementation(
    async (first: number, rows: number) =>
      endpointPage(
        endpointRecords.slice(first, first + rows),
        first,
        rows,
        endpointRecords.length,
      ),
  )
  api.listUserOptions.mockResolvedValue({ data: { users: fixtureUsers } })
  api.fetchTokenPlanUsage.mockImplementation(async () => usage('minimax', []))
  api.oauthStatus.mockImplementation(
    async (options: { path: { endpoint_id: string } }) => ({
      data: {
        endpoint_id: options.path.endpoint_id,
        expired: false,
        expires_at: null,
        has_oauth_token: false,
        plan: 'platform_api_key',
      },
    }),
  )
  api.oauthRefresh.mockResolvedValue({
    data: {
      endpoint_id: 'endpoint-1',
      expired: false,
      expires_at: null,
      has_oauth_token: true,
      plan: 'platform_api_key',
    },
  })
  api.oauthClear.mockResolvedValue(undefined)
  api.oauthDeviceStart.mockResolvedValue({
    data: {
      expires_in_seconds: 600,
      flow_id: 'fixture-device-flow',
      interval_seconds: 60,
      user_code: 'FIXTURE-DEVICE-CODE',
      verification_uri: 'https://verify.example.test/device',
    },
  })
  api.oauthDevicePoll.mockResolvedValue({
    data: { flow_id: 'fixture-device-flow', status: 'pending' },
  })
  api.oauthBrowserStart.mockResolvedValue({
    data: {
      authorize_url: 'https://authorize.example.test/oauth',
      expires_in_seconds: 600,
      flow_id: 'fixture-browser-flow',
      redirect_uri: 'https://callback.example.test/finish',
    },
  })
  api.oauthBrowserComplete.mockResolvedValue({
    data: { flow_id: 'fixture-browser-flow', status: 'pending' },
  })
  api.persistEndpoint.mockImplementation(
    async (endpointId: string | null, body: EndpointRequest) => {
      const current = endpointId
        ? endpointRecords.find((item) => item.endpoint_id === endpointId)
        : undefined
      const saved = makeEndpoint(
        current ? Number(current.endpoint_id.slice('endpoint-'.length)) : 99,
        {
          ...current,
          base_url: body.base_url,
          enabled: body.enabled ?? true,
          name: body.name,
          provider: body.provider ?? 'generic',
          provider_region: body.provider_region ?? null,
          scope: body.scope,
          owner_user_id: body.owner_user_id ?? null,
        },
      )
      endpointRecords = current
        ? endpointRecords.map((item) =>
            item.endpoint_id === endpointId ? saved : item,
          )
        : [...endpointRecords, saved]
      return saved
    },
  )
  api.deleteEndpointById.mockImplementation(async (endpointId: string) => {
    endpointRecords = endpointRecords.filter(
      (item) => item.endpoint_id !== endpointId,
    )
  })
  api.updateEndpointEnabled.mockImplementation(
    async (endpoint: ProviderEndpoint, enabled: boolean) => ({
      ...endpoint,
      enabled,
    }),
  )
  api.runEndpointTest.mockResolvedValue({
    duration_ms: 14,
    message: 'fixture probe succeeded',
    model_count: 2,
    native_api: 'responses',
    native_api_source: 'manual',
    ok: true,
    status: 200,
  } satisfies EndpointTestResponse)
})

afterEach(() => {
  localStorage.clear()
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
})

describe('upstream building block', () => {
  it('loads admin endpoints and user options once and keeps Admin API key UI absent', async () => {
    endpointRecords = [makeEndpoint(1, { has_admin_api_key: true })]
    renderPage()

    expect(await screen.findAllByText('Upstream 1')).not.toHaveLength(0)
    expect(api.fetchEndpointsPage).toHaveBeenCalledWith(0, 10)
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)

    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', { name: 'Edit' }),
    )
    const dialog = await screen.findByRole('dialog')
    expect(
      within(dialog).queryByRole('textbox', { name: /Admin API key/i }),
    ).toBeNull()
    expect(within(dialog).queryByText(/Admin API key/i)).toBeNull()
    expect(within(dialog).getByLabelText('API key 1')).toBeTruthy()
  })

  it('prefetches only eligible quota rows and shares ordered badges across table and card', async () => {
    endpointRecords = [
      makeEndpoint(1, { provider: 'minimax' }),
      makeEndpoint(2, {
        provider: 'openai',
        has_oauth_token: true,
        plan: 'chatgpt_subscription',
      }),
      makeEndpoint(3, { provider: 'openai', has_oauth_token: false }),
      makeEndpoint(4, { provider: 'generic', has_oauth_token: true }),
    ]
    api.fetchTokenPlanUsage.mockImplementation(async (endpointId: string) =>
      endpointId === 'endpoint-1'
        ? usage('minimax', [
            minimaxQuotaKey('fixture-quota-key', 'available', 40, 70),
          ])
        : usage('openai', []),
    )
    const view = renderPage()
    await screen.findAllByText('Upstream 4')

    await waitFor(() =>
      expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(2),
    )
    expect(api.fetchTokenPlanUsage.mock.calls.map(([id]) => id)).toEqual([
      'endpoint-1',
      'endpoint-2',
    ])
    expect(screen.getByRole('columnheader', { name: 'Usage' })).toBeTruthy()

    const tablePills = document.querySelector(
      '[data-endpoint-row="endpoint-1"] [data-quota-badges="table"]',
    )
    const cardPills = endpointCard('endpoint-1').querySelector(
      '[data-quota-badges="card"]',
    )
    expect(tablePills?.textContent).toBe(cardPills?.textContent)
    expect(tablePills?.textContent).toContain('Short 40%')
    expect(tablePills?.textContent).toContain('Long 70%')
    expect(tablePills?.className).toContain('flex-wrap')
    expect(
      endpointCard('endpoint-3').querySelector('[data-quota-badges]'),
    ).toBeNull()
    expect(
      within(endpointCard('endpoint-3')).queryByRole('button', {
        name: 'Token plan usage',
      }),
    ).toBeNull()
    expect(
      within(endpointCard('endpoint-4')).queryByRole('button', {
        name: 'Token plan usage',
      }),
    ).toBeNull()

    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', {
        name: 'Token plan usage',
      }),
    )
    expect(await screen.findByText('fixture-quota-key')).toBeTruthy()
    expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(2)

    view.unmount()
    document.dispatchEvent(new Event('visibilitychange'))
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('pageshow'))
    await Promise.resolve()
    expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
    __resetTokenPlanForegroundForTests()
    __resetTokenPlanCacheForTests()
  })

  it('coalesces a cold page prefetch with the dialog read through the shared cache', async () => {
    endpointRecords = [makeEndpoint(1, { provider: 'minimax' })]
    const pendingUsage = deferred<TokenPlanUsageResponse>()
    api.fetchTokenPlanUsage.mockReturnValueOnce(pendingUsage.promise)
    const view = renderPage()
    await screen.findAllByText('Upstream 1')
    await waitFor(() =>
      expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(1),
    )

    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', {
        name: 'Token plan usage',
      }),
    )
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(screen.getByText('Loading')).toBeTruthy()
    expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

    pendingUsage.resolve(
      usage('minimax', [
        minimaxQuotaKey('cold-quota-key', 'available', 25, 50),
      ]),
    )
    expect(await screen.findByText('cold-quota-key')).toBeTruthy()
    expect(api.fetchTokenPlanUsage).toHaveBeenCalledTimes(1)

    view.unmount()
    __resetTokenPlanForegroundForTests()
    __resetTokenPlanCacheForTests()
  })

  it('does not issue admin endpoint or user-option reads for a non-admin', () => {
    renderPage(operator)

    expect(api.fetchEndpointsPage).not.toHaveBeenCalled()
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(screen.queryByRole('region')).toBeNull()
  })

  it('saves the captured mapper payload and omits the untouched Admin API key', async () => {
    endpointRecords = [
      makeEndpoint(1, {
        has_admin_api_key: true,
        api_keys: [
          {
            created_at: '2026-10-16T12:00:00.000Z',
            enabled: true,
            endpoint_id: 'endpoint-1',
            key_id: 'key-1',
            key_label: 'Primary',
            position: 0,
            updated_at: '2026-10-23T12:00:00.000Z',
          },
        ],
      }),
    ]
    renderPage()
    await screen.findAllByText('Upstream 1')
    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', { name: 'Edit' }),
    )
    const dialog = await screen.findByRole('dialog')
    expect(dialog.className).toContain(
      'sm:max-w-[min(calc(100vw_-_2rem),64rem)]',
    )
    expect(dialog.className).not.toContain('max-w-4xl')
    api.persistEndpoint.mockRejectedValueOnce({
      error: { code: 'fixture_save_failed', message: 'save failed' },
    })
    fireEvent.change(document.getElementById('endpoint-name')!, {
      target: { value: '  Renamed upstream  ' },
    })
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(
      (document.getElementById('endpoint-name') as HTMLInputElement).value,
    ).toBe('  Renamed upstream  ')
    expect(api.persistEndpoint).toHaveBeenCalledTimes(1)
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )

    await waitFor(() => expect(api.persistEndpoint).toHaveBeenCalledTimes(2))
    const [endpointId, payload] = api.persistEndpoint.mock.calls[1] as [
      string | null,
      EndpointRequest,
    ]
    expect(endpointId).toBe('endpoint-1')
    expect(payload.name).toBe('Renamed upstream')
    expect(payload.api_keys).toEqual([
      {
        api_key: '',
        enabled: true,
        key_id: 'key-1',
        key_label: 'Primary',
      },
    ])
    expect(payload).not.toHaveProperty('admin_api_key')
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('holds the endpoint dialog open until its guarded save and refresh finish', async () => {
    const pendingSave = deferred<ProviderEndpoint>()
    api.persistEndpoint.mockReturnValueOnce(pendingSave.promise)
    const { queryClient } = renderPage()
    const invalidateQueries = vi.spyOn(queryClient, 'invalidateQueries')
    await screen.findAllByText('Upstream 1')
    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', { name: 'Edit' }),
    )
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )
    await waitFor(() => expect(api.persistEndpoint).toHaveBeenCalledTimes(1))

    const saveButton = within(dialog).getByRole('button', {
      name: 'Save endpoint',
    }) as HTMLButtonElement
    const cancelButton = within(dialog).getByRole('button', {
      name: 'Cancel',
    }) as HTMLButtonElement
    expect(saveButton.disabled).toBe(true)
    expect(cancelButton.disabled).toBe(true)
    fireEvent.keyDown(document, { key: 'Escape' })
    fireEvent.click(saveButton)
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(api.persistEndpoint).toHaveBeenCalledTimes(1)

    pendingSave.resolve({ ...endpointRecords[0]!, name: 'Upstream saved' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: endpointRecordsQueryKey(),
      refetchType: 'none',
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: modelRoutePagesQueryKey(),
      refetchType: 'none',
    })
  })

  it('preserves a create draft on failure and submits the selected owner on retry', async () => {
    api.persistEndpoint.mockRejectedValueOnce({
      error: { code: 'fixture_create_failed', message: 'create failed' },
    })
    renderPage()
    await screen.findAllByText('Upstream 1')
    fireEvent.click(screen.getByRole('button', { name: 'New endpoint' }))
    const dialog = await screen.findByRole('dialog')

    fireEvent.change(document.getElementById('endpoint-name')!, {
      target: { value: '  New private upstream  ' },
    })
    fireEvent.change(document.getElementById('endpoint-base-url')!, {
      target: { value: ' https://new.example.test/v1 ' },
    })
    fireEvent.change(within(dialog).getByLabelText('API key 1'), {
      target: { value: 'fixture-endpoint-secret' },
    })
    fireEvent.click(within(dialog).getByRole('combobox', { name: 'Scope' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Private' }))
    fireEvent.click(within(dialog).getByRole('combobox', { name: 'Owner' }))
    fireEvent.click(
      await screen.findByRole('option', {
        name: 'operator / Fixture Operator',
      }),
    )
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(
      (document.getElementById('endpoint-name') as HTMLInputElement).value,
    ).toBe('  New private upstream  ')
    expect(
      (within(dialog).getByLabelText('API key 1') as HTMLInputElement).type,
    ).toBe('password')
    expect(api.persistEndpoint).toHaveBeenCalledTimes(1)

    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )
    await waitFor(() => expect(api.persistEndpoint).toHaveBeenCalledTimes(2))
    const [, payload] = api.persistEndpoint.mock.calls[1] as [
      string | null,
      EndpointRequest,
    ]
    expect(payload).toMatchObject({
      base_url: 'https://new.example.test/v1',
      name: 'New private upstream',
      owner_user_id: 2,
      scope: 'user',
    })
    expect(payload.api_keys).toEqual([
      {
        api_key: 'fixture-endpoint-secret',
        enabled: true,
        key_label: '',
      },
    ])
    expect(await screen.findByText('Endpoint saved')).toBeTruthy()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('persists endpoint page size across page queries', async () => {
    renderPage()
    await screen.findAllByText('Upstream 1')
    expect(localStorage.getItem('prompt-ferry:table-page-size:endpoints')).toBe(
      '10',
    )

    fireEvent.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    expect(await screen.findByRole('option', { name: '50' })).toBeTruthy()
    fireEvent.click(screen.getByRole('option', { name: '20' }))

    await waitFor(() =>
      expect(
        localStorage.getItem('prompt-ferry:table-page-size:endpoints'),
      ).toBe('20'),
    )
    expect(api.fetchEndpointsPage).toHaveBeenCalledWith(0, 20)
  })

  it('confirms deletion and corrects the page after removing its last endpoint', async () => {
    endpointRecords = Array.from({ length: 11 }, (_, index) =>
      makeEndpoint(index + 1),
    )
    const confirm = vi.spyOn(window, 'confirm')
    confirm.mockReturnValueOnce(false).mockReturnValueOnce(true)
    renderPage()
    await screen.findAllByText('Upstream 1')
    fireEvent.click(screen.getByRole('button', { name: 'Page 2' }))
    await screen.findAllByText('Upstream 11')

    const lastCard = within(endpointCard('endpoint-11'))
    fireEvent.click(lastCard.getByRole('button', { name: 'Delete' }))
    expect(confirm).toHaveBeenCalledWith('Delete Upstream 11?')
    expect(api.deleteEndpointById).not.toHaveBeenCalled()

    fireEvent.click(lastCard.getByRole('button', { name: 'Delete' }))
    await waitFor(() =>
      expect(api.deleteEndpointById).toHaveBeenCalledWith('endpoint-11'),
    )
    expect(await screen.findAllByText('Upstream 1')).not.toHaveLength(0)
    expect(
      screen.getByRole('button', { name: 'Page 1 (current)' }),
    ).toBeTruthy()
  })

  it('shares the endpoint gate with OAuth and freezes dismissal only while the request is pending', async () => {
    const pendingStart = deferred<unknown>()
    api.oauthBrowserStart.mockReturnValueOnce(pendingStart.promise)
    renderPage()
    await screen.findAllByText('Upstream 1')
    fireEvent.click(
      within(endpointCard('endpoint-1')).getByRole('button', { name: 'Edit' }),
    )
    const dialog = await screen.findByRole('dialog')
    await waitFor(() =>
      expect(api.oauthStatus).toHaveBeenCalledWith({
        path: { endpoint_id: 'endpoint-1' },
        responseStyle: 'data',
      }),
    )
    expect(getPendingActions()).not.toHaveProperty('endpoint:endpoint-1')
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Get authorize link' }),
    )

    await waitFor(() =>
      expect(api.oauthBrowserStart).toHaveBeenCalledWith({
        path: { endpoint_id: 'endpoint-1' },
        responseStyle: 'data',
      }),
    )
    expect(getPendingActions()['endpoint:endpoint-1']).toBe(
      'oauth-browser-start',
    )
    expect(
      (
        within(dialog).getByRole('button', {
          name: 'Save endpoint',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        within(dialog).getByRole('button', {
          name: 'Cancel',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        within(endpointCard('endpoint-1'))
          .getByText('Edit')
          .closest('button') as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Save endpoint' }),
    )
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(api.persistEndpoint).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog')).toBeTruthy()

    pendingStart.resolve({
      data: {
        authorize_url: 'https://authorize.example.test/oauth',
        expires_in_seconds: 600,
        flow_id: 'fixture-browser-flow',
        redirect_uri: 'https://callback.example.test/finish',
      },
    })
    await waitFor(() =>
      expect(getPendingActions()).not.toHaveProperty('endpoint:endpoint-1'),
    )
    expect(
      await within(dialog).findByRole('textbox', { name: 'Authorize link' }),
    ).toBeTruthy()
    expect(screen.getByRole('dialog')).toBeTruthy()
  })

  it('keeps endpoint writes row-scoped and lets a separate probe run concurrently', async () => {
    endpointRecords = [makeEndpoint(1), makeEndpoint(2)]
    const pendingFirstToggle = deferred<ProviderEndpoint>()
    api.updateEndpointEnabled
      .mockReturnValueOnce(pendingFirstToggle.promise)
      .mockImplementationOnce(
        async (endpoint: ProviderEndpoint, enabled: boolean) => ({
          ...endpoint,
          enabled,
        }),
      )
    renderPage()
    await screen.findAllByText('Upstream 1')

    const firstRow = within(endpointCard('endpoint-1'))
    const firstSwitch = firstRow.getByRole('switch', {
      name: 'Toggle endpoint Upstream 1',
    })
    fireEvent.click(firstSwitch)
    await waitFor(() =>
      expect(api.updateEndpointEnabled).toHaveBeenCalledTimes(1),
    )
    expect(
      (firstRow.getByRole('button', { name: 'Edit' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    expect(
      (firstRow.getByRole('button', { name: 'Delete' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)

    const secondRow = within(endpointCard('endpoint-2'))
    fireEvent.click(
      secondRow.getByRole('switch', { name: 'Toggle endpoint Upstream 2' }),
    )
    await waitFor(() =>
      expect(api.updateEndpointEnabled).toHaveBeenCalledTimes(2),
    )

    // Nothing has been tested on this row yet, so it carries no result control.
    expect(firstRow.queryByRole('button', { name: 'Not tested' })).toBeNull()
    fireEvent.click(firstRow.getByRole('button', { name: 'Test' }))
    await waitFor(() =>
      expect(api.runEndpointTest).toHaveBeenCalledWith('endpoint-1'),
    )
    expect(
      await screen.findAllByRole('button', { name: /HTTP 200/ }),
    ).not.toHaveLength(0)

    pendingFirstToggle.resolve({ ...endpointRecords[0]!, enabled: false })
    await waitFor(() =>
      expect(
        (
          firstRow.getByRole('switch', {
            name: 'Toggle endpoint Upstream 1',
          }) as HTMLButtonElement
        ).getAttribute('aria-checked'),
      ).toBe('false'),
    )
  })
})
