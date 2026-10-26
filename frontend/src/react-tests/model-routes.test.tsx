import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ModelEndpointRule,
  ModelRoutePageResponse,
  ModelRouteRequest,
  ModelRouteTestResponse,
  ModelRouteTestTargetResult,
  ProviderEndpoint,
  User,
} from '@/generated/admin-api'
import { ModelRoutesPage } from '@/pages/ModelRoutesPage'
import { clearEndpointModelCatalogCache } from '@/lib/endpoint-models'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { modelRoutePageQueryKey } from '@/lib/model-routes'
import { useModelRouteTargetDrag } from '@/lib/use-model-route-target-drag'

const api = vi.hoisted(() => ({
  deleteModelRouteById: vi.fn(),
  fetchEndpointModels: vi.fn(),
  fetchModelRoutesPage: vi.fn(),
  listAllEndpointRecords: vi.fn(),
  listEndpointUsers: vi.fn(),
  persistModelRoute: vi.fn(),
  runModelRouteProbe: vi.fn(),
  updateModelRouteEnabled: vi.fn(),
}))

vi.mock('@/stores/endpoints-api', () => ({
  deleteModelRouteById: api.deleteModelRouteById,
  fetchEndpointModels: api.fetchEndpointModels,
  fetchModelRoutesPage: api.fetchModelRoutesPage,
  persistModelRoute: api.persistModelRoute,
  runModelRouteProbe: api.runModelRouteProbe,
  updateModelRouteEnabled: api.updateModelRouteEnabled,
}))

vi.mock('@/lib/endpoints', () => ({
  endpointRecordsQueryKey: () => ['endpoint-records'],
  endpointUserOptionsQueryKey: (id: number) => ['endpoint-user-options', id],
  listAllEndpointRecords: api.listAllEndpointRecords,
  listEndpointUsers: api.listEndpointUsers,
  modelRoutePagesQueryKey: () => ['model-routes-page'],
}))

const admin = {
  display_name: 'Admin',
  is_admin: true,
  login_name: 'admin',
  user_id: 1,
}
const operator = {
  display_name: 'Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

function endpoint(
  id: string,
  nativeApi: ProviderEndpoint['native_api'] = 'auto',
): ProviderEndpoint {
  return {
    active_windows: [],
    api_keys: [],
    base_url: `https://${id}.example.test`,
    created_at: '2026-10-23T12:00:00.000Z',
    enabled: true,
    endpoint_id: id,
    has_admin_api_key: false,
    has_oauth_token: false,
    has_proxy_url: false,
    key_lb_enabled: false,
    mcp_enabled: false,
    name: `Fixture ${id} With Long Endpoint Name`,
    native_api: nativeApi,
    native_api_source: 'manual',
    owner_user_id: null,
    plan: 'platform_api_key',
    provider: 'generic',
    provider_region: 'global',
    scope: 'global',
    service_tier: null,
    updated_at: '2026-10-23T12:00:00.000Z',
  }
}

function route(ruleId: string, pattern = 'fixture-pattern'): ModelEndpointRule {
  return {
    created_at: '2026-10-23T12:00:00.000Z',
    enabled: true,
    model_pattern: pattern,
    owner_user_id: null,
    routing_strategy: 'responses_session_affinity',
    rule_id: ruleId,
    scope: 'global',
    targets: [
      {
        active_windows: [],
        compact_mode: 'passthrough',
        created_at: '2026-10-23T12:00:00.000Z',
        dev_system_normalize: false,
        embedding_dimensions_override: null,
        enabled: true,
        endpoint_enabled: true,
        endpoint_id: 'ep-fixture-8',
        endpoint_name: 'Off-page endpoint 8',
        has_proxy_url_override: false,
        native_api: 'auto',
        position: 0,
        rule_id: ruleId,
        service_tier: null,
        target_id: 'target-1',
        thinking_downgrade_enabled: false,
        thinking_effort_override: null,
        updated_at: '2026-10-23T12:00:00.000Z',
        upstream_model: 'existing-model',
      },
    ],
    updated_at: '2026-10-23T12:00:00.000Z',
  }
}

const users: User[] = [
  {
    created_at: '2026-10-23T12:00:00.000Z',
    display_name: 'Fixture User',
    is_active: true,
    is_admin: false,
    login_name: 'fixture-user',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 2,
  },
]

let routes: ModelEndpointRule[]
let endpoints: ProviderEndpoint[]

function renderPage(me = admin) {
  useSessionStore.setState({ me, bootstrapped: true, bootstrapping: false })
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <ModelRoutesPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

function page(first: number, rows: number): ModelRoutePageResponse {
  return {
    first,
    rows,
    total: routes.length,
    routes: routes.slice(first, first + rows),
  }
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  clearEndpointModelCatalogCache()
  Element.prototype.scrollIntoView = () => undefined
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
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
  routes = [route('rule-offpage')]
  endpoints = Array.from({ length: 10 }, (_, index) =>
    endpoint(`ep-fixture-${index + 1}`, index === 7 ? 'chat' : 'auto'),
  )
  api.listAllEndpointRecords.mockResolvedValue(endpoints)
  api.listEndpointUsers.mockResolvedValue(users)
  api.fetchEndpointModels.mockResolvedValue({ models: ['fixture-model'] })
  api.fetchModelRoutesPage.mockImplementation(
    async (first: number, rows: number) => page(first, rows),
  )
  api.persistModelRoute.mockImplementation(
    async (ruleId: string | null, body: ModelRouteRequest) => {
      const current = ruleId
        ? (routes.find((item) => item.rule_id === ruleId) ?? route(ruleId))
        : route('rule-created')
      const saved: ModelEndpointRule = {
        ...current,
        enabled: body.enabled !== false,
        model_pattern: body.model_pattern,
        owner_user_id: body.owner_user_id ?? null,
        scope: body.scope,
        targets: current.targets,
      }
      routes = ruleId
        ? routes.map((item) => (item.rule_id === ruleId ? saved : item))
        : [...routes, saved]
      return saved
    },
  )
  api.deleteModelRouteById.mockImplementation(async (ruleId: string) => {
    routes = routes.filter((item) => item.rule_id !== ruleId)
  })
  api.updateModelRouteEnabled.mockImplementation(
    async (item: ModelEndpointRule, enabled: boolean) => {
      const updated = { ...item, enabled }
      routes = routes.map((candidate) =>
        candidate.rule_id === item.rule_id ? updated : candidate,
      )
      return updated
    },
  )
  api.runModelRouteProbe.mockResolvedValue({
    duration_ms: 240,
    endpoint_id: 'ep-fixture-8',
    endpoint_name: 'Off-page endpoint 8',
    message: 'fixture probe passed',
    model: 'fixture-model',
    model_pattern: 'fixture-pattern',
    ok: false,
    passed: 1,
    preferred_endpoint_id: 'ep-fixture-8',
    preferred_endpoint_name: 'Off-page endpoint 8',
    results: [
      {
        duration_ms: 100,
        endpoint_id: 'ep-fixture-8',
        endpoint_name: 'Off-page endpoint 8',
        message: 'first result',
        model: 'model-a',
        ok: true,
        preferred: true,
        status: 200,
        target_id: 'target-a',
      },
      {
        duration_ms: 140,
        endpoint_id: 'ep-fixture-9',
        endpoint_name: 'Off-page endpoint 9',
        message: 'second result',
        model: 'model-b',
        ok: false,
        preferred: false,
        status: 503,
        target_id: 'target-b',
      },
    ] satisfies ModelRouteTestTargetResult[],
    total: 2,
  } satisfies ModelRouteTestResponse)
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('ModelRoutesPage', () => {
  it('loads the route page and all endpoint records without admin reads for non-admins', async () => {
    const { container } = renderPage(operator)
    expect(
      container.querySelector(
        '[data-foundation-route-id="R-ENDPOINTS-ROUTES"]',
      ),
    ).toBeTruthy()
    expect(api.fetchModelRoutesPage).not.toHaveBeenCalled()
    expect(api.listAllEndpointRecords).not.toHaveBeenCalled()
    expect(api.listEndpointUsers).not.toHaveBeenCalled()

    cleanup()
    renderPage()
    expect(
      (await screen.findAllByText('fixture-pattern')).length,
    ).toBeGreaterThan(0)
    expect(api.listAllEndpointRecords).toHaveBeenCalledTimes(1)
    expect(api.listEndpointUsers).toHaveBeenCalledTimes(1)
    expect(api.fetchModelRoutesPage).toHaveBeenCalledWith(0, 10)
  })

  it('uses off-page endpoint protocol in the editor and submits target-associated settings', async () => {
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    const editor = await screen.findByRole('dialog')
    expect(editor.querySelector('input[list]')).toBeTruthy()
    await waitFor(() =>
      expect(api.fetchEndpointModels).toHaveBeenCalledWith(
        'ep-fixture-8',
        'chat',
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Target settings' }))
    expect(
      await screen.findByText('Fixture ep-fixture-8 With Long Endpoint Name'),
    ).toBeTruthy()
    expect(screen.queryByLabelText('Vector dimensions override')).toBeNull()
    fireEvent.click(screen.getAllByRole('button', { name: 'Cancel' }).at(-1)!)
  })

  it('drops late protocol catalogs without clearing the custom model draft', async () => {
    let resolveInitial!: (value: { models: string[] }) => void
    api.fetchEndpointModels.mockImplementation(
      (endpointId: string, nativeApi?: string) =>
        nativeApi === 'responses'
          ? Promise.resolve({ models: ['responses-only-model'] })
          : new Promise((resolve) => {
              resolveInitial = resolve
            }),
    )
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await waitFor(() =>
      expect(api.fetchEndpointModels).toHaveBeenCalledWith(
        'ep-fixture-8',
        'chat',
      ),
    )
    const modelInput = document.querySelector('input[list]') as HTMLInputElement
    fireEvent.change(modelInput, { target: { value: 'custom-draft-model' } })
    fireEvent.click(screen.getByRole('button', { name: 'Target settings' }))
    fireEvent.click(screen.getByRole('combobox', { name: 'Native API' }))
    fireEvent.click(await screen.findByRole('option', { name: 'Responses' }))
    fireEvent.click(screen.getAllByRole('button', { name: 'Cancel' })[0]!)
    await waitFor(() =>
      expect(api.fetchEndpointModels).toHaveBeenCalledWith(
        'ep-fixture-8',
        'responses',
      ),
    )
    const currentInput = document.querySelector(
      'input[list]',
    ) as HTMLInputElement
    expect(currentInput.value).toBe('custom-draft-model')
    const listId = currentInput.getAttribute('list')
    const suggestions = () =>
      Array.from(
        document.getElementById(listId!)?.querySelectorAll('option') ?? [],
      ).map((option) => option.value)
    await waitFor(() => expect(suggestions()).toEqual(['responses-only-model']))
    await act(async () => resolveInitial({ models: ['stale-chat-model'] }))
    expect(suggestions()).toEqual(['responses-only-model'])
    expect(currentInput.disabled).toBe(false)
    fireEvent.click(
      screen.getByRole('button', { name: /Delete Upstream model name/ }),
    )
    expect(
      (document.querySelector('input[list]') as HTMLInputElement).value,
    ).toBe('')
  })

  it('keeps the failed update draft, blocks dialog dismissal and retries after failure', async () => {
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    const patternInput = screen.getByRole('textbox', { name: 'Model pattern' })
    fireEvent.change(patternInput, {
      target: { value: 'fixture-pattern-updated' },
    })
    let rejectSave!: (cause: unknown) => void
    api.persistModelRoute.mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          rejectSave = reject
        }),
    )
    fireEvent.click(screen.getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))
    expect(
      (screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    await act(async () => rejectSave(new Error('fixture update rejected')))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(
      (
        screen.getByRole('textbox', {
          name: 'Model pattern',
        }) as HTMLInputElement
      ).value,
    ).toBe('fixture-pattern-updated')

    fireEvent.click(screen.getByRole('button', { name: /^Save$/ }))
    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('keeps probes available while a route write is pending and disables conflicting writes', async () => {
    renderPage()
    await screen.findAllByText('fixture-pattern')
    let resolveToggle!: (value: ModelEndpointRule) => void
    api.updateModelRouteEnabled.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveToggle = resolve
        }),
    )
    const row = document.querySelector(
      '[data-model-route-row="rule-offpage"]',
    ) as HTMLElement
    fireEvent.click(within(row).getByRole('switch'))
    await waitFor(() =>
      expect(api.updateModelRouteEnabled).toHaveBeenCalledTimes(1),
    )
    expect(
      (
        screen.getByRole('button', {
          name: 'Edit fixture-pattern',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: 'Delete fixture-pattern',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: 'Test fixture-pattern',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false)
    fireEvent.click(
      screen.getByRole('button', { name: 'Test fixture-pattern' }),
    )
    await waitFor(() =>
      expect(api.runModelRouteProbe).toHaveBeenCalledWith('rule-offpage'),
    )
    await act(async () =>
      resolveToggle({ ...route('rule-offpage'), enabled: false }),
    )
  })

  it('shows ordered probe details, confirms deletion, and patches toggles without a list reload', async () => {
    const { queryClient } = renderPage()
    await screen.findAllByText('fixture-pattern')
    const initialReads = api.fetchModelRoutesPage.mock.calls.length
    fireEvent.click(
      screen.getByRole('button', { name: 'Test fixture-pattern' }),
    )
    await waitFor(() =>
      expect(
        screen.getAllByRole('button', { name: '1/2' }).length,
      ).toBeGreaterThan(0),
    )
    fireEvent.click(screen.getAllByRole('button', { name: '1/2' })[0]!)
    const dialog = await screen.findByRole('dialog')
    const resultText =
      within(dialog).getByText('first result').parentElement?.parentElement
    expect(resultText?.textContent).toContain('Off-page endpoint 8')
    expect(within(dialog).getByText('second result')).toBeTruthy()
    fireEvent.click(
      within(dialog).getAllByRole('button', { name: 'Close' })[0]!,
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    const row = document.querySelector('[data-model-route-row="rule-offpage"]')
    expect(row).toBeTruthy()
    fireEvent.click(within(row as HTMLElement).getByRole('switch'))
    await waitFor(() =>
      expect(api.updateModelRouteEnabled).toHaveBeenCalledWith(
        expect.objectContaining({ rule_id: 'rule-offpage' }),
        false,
      ),
    )
    expect(api.fetchModelRoutesPage).toHaveBeenCalledTimes(initialReads)
    expect(queryClient.getQueryData(modelRoutePageQueryKey(0, 10))).toBeTruthy()

    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    fireEvent.click(
      screen.getByRole('button', { name: 'Delete fixture-pattern' }),
    )
    expect(confirm).toHaveBeenCalledTimes(1)
    expect(api.deleteModelRouteById).not.toHaveBeenCalled()
    confirm.mockReturnValue(true)
    fireEvent.click(
      screen.getByRole('button', { name: 'Delete fixture-pattern' }),
    )
    await waitFor(() =>
      expect(api.deleteModelRouteById).toHaveBeenCalledWith('rule-offpage'),
    )
  })

  it('creates a route with a target selected from the complete endpoint catalog', async () => {
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'New model route' }),
    )
    fireEvent.change(screen.getByRole('textbox', { name: 'Model pattern' }), {
      target: { value: 'new-route-*' },
    })
    fireEvent.click(screen.getByRole('combobox', { name: 'Endpoint 1' }))
    fireEvent.click(
      await screen.findByRole('option', {
        name: /Fixture ep-fixture-10 With Long Endpoint Name/,
      }),
    )
    fireEvent.change(
      document.querySelector('input[list]') as HTMLInputElement,
      { target: { value: 'custom-upstream-model' } },
    )
    fireEvent.click(screen.getByRole('button', { name: /^Save$/ }))
    await waitFor(() =>
      expect(api.persistModelRoute).toHaveBeenCalledWith(
        null,
        expect.objectContaining({
          model_pattern: 'new-route-*',
          scope: 'admin',
          targets: [
            expect.objectContaining({
              endpoint_id: 'ep-fixture-10',
              upstream_model: 'custom-upstream-model',
            }),
          ],
        }),
      ),
    )
  })
})

describe('useModelRouteTargetDrag', () => {
  it('moves the same target object by keyboard and cancels a removed drag source', () => {
    const first = { id: 'a' }
    const second = { id: 'b' }
    const third = { id: 'c' }
    const onReorder = vi.fn()
    const { result, rerender } = renderHook(
      ({ targets }) => useModelRouteTargetDrag(targets, onReorder),
      { initialProps: { targets: [first, second, third] } },
    )

    act(() =>
      result.current.onGripKeyDown(
        {
          key: 'ArrowDown',
          preventDefault: vi.fn(),
        } as unknown as React.KeyboardEvent,
        first,
      ),
    )
    expect(onReorder).toHaveBeenLastCalledWith([second, first, third])

    const grip = document.createElement('button')
    grip.setAttribute('data-target-drag-handle', '')
    const dataTransfer = { effectAllowed: '', setData: vi.fn(), dropEffect: '' }
    act(() =>
      result.current.onDragStart(
        {
          target: grip,
          dataTransfer,
          preventDefault: vi.fn(),
        } as unknown as React.DragEvent,
        first,
      ),
    )
    rerender({ targets: [second, third] })
    act(() =>
      result.current.onDrop(
        { dataTransfer, preventDefault: vi.fn() } as unknown as React.DragEvent,
        third,
      ),
    )
    expect(onReorder).toHaveBeenCalledTimes(1)
  })
})
