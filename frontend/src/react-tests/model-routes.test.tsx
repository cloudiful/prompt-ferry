import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  createEvent,
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
    expect(editor.querySelector('[data-model-candidate-input]')).toBeTruthy()
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
    const modelInput = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
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
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    expect(currentInput.value).toBe('custom-draft-model')
    const suggestions = () =>
      Array.from(
        document.querySelectorAll('[data-model-candidate-option]'),
      ).map((option) => option.textContent)
    fireEvent.click(screen.getByRole('button', { name: 'Model candidates' }))
    await waitFor(() => expect(suggestions()).toEqual([]))
    expect(
      document.querySelector('[data-model-candidate-empty]')?.textContent,
    ).toBe('No data')
    await act(async () => resolveInitial({ models: ['stale-chat-model'] }))
    // The late chat response never leaks a suggestion into the responses list.
    expect(suggestions()).toEqual([])
    expect(currentInput.disabled).toBe(false)
    fireEvent.change(currentInput, { target: { value: 'responses' } })
    await waitFor(() => expect(suggestions()).toEqual(['responses-only-model']))
    fireEvent.keyDown(currentInput, { key: 'Escape' })
    expect(screen.queryByRole('listbox')).toBeNull()
    fireEvent.click(
      screen.getByRole('button', { name: /Delete Upstream model name/ }),
    )
    expect(
      (
        document.querySelector(
          '[data-model-candidate-input]',
        ) as HTMLInputElement
      ).value,
    ).toBe('')
  })

  it('opens and filters candidates while typing without committing implicitly', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())

    fireEvent.focus(input)
    expect(screen.queryByRole('listbox')).toBeNull()
    fireEvent.change(input, { target: { value: 'ALP' } })
    const list = await screen.findByRole('listbox')
    expect(
      within(list)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual(['alpha-model'])
    expect(input.value).toBe('ALP')
    expect(list.querySelector('[aria-selected="true"]')).toBeNull()
    expect(input.getAttribute('aria-activedescendant')).toBeNull()
    // Enter without an active match never commits or submits.
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(input.value).toBe('ALP')
    expect(screen.getByRole('listbox')).toBeTruthy()
    expect(api.persistModelRoute).not.toHaveBeenCalled()

    fireEvent.keyDown(input, { key: 'ArrowDown' })
    expect(input.getAttribute('aria-activedescendant')).toBe(`${list.id}-0`)
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(input.value).toBe('alpha-model')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(api.persistModelRoute).not.toHaveBeenCalled()

    fireEvent.change(input, { target: { value: 'MANUAL-DRAFT' } })
    await waitFor(() =>
      expect(
        document.querySelector('[data-model-candidate-empty]')?.textContent,
      ).toBe('No data'),
    )
    expect(input.value).toBe('MANUAL-DRAFT')
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    expect(screen.queryByRole('listbox')).toBeTruthy()
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(input.value).toBe('MANUAL-DRAFT')

    fireEvent.change(input, { target: { value: 'beta' } })
    const reopened = await screen.findByRole('listbox')
    expect(reopened.querySelector('[aria-selected="true"]')).toBeNull()
    fireEvent.click(
      within(reopened).getByRole('option', { name: 'beta-model' }),
    )
    expect(input.value).toBe('beta-model')
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('opens the candidate list from the toggle with the input focused and commits with Enter', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())
    // Start from an empty draft so the toggle offers the full catalog.
    fireEvent.change(input, { target: { value: '' } })
    await screen.findByRole('listbox')
    fireEvent.keyDown(input, { key: 'Escape' })
    const toggle = screen.getByRole('button', { name: 'Model candidates' })
    toggle.focus()
    fireEvent.keyDown(toggle, { key: 'Enter' })
    // jsdom does not synthesize the native button activation for Enter.
    fireEvent.click(toggle)
    await screen.findByRole('listbox')
    expect(document.activeElement).toBe(input)
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(input.value).toBe('alpha-model')
    expect(document.activeElement).toBe(input)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(api.persistModelRoute).not.toHaveBeenCalled()
  })

  it('reopens the candidate list from the toggle with no active option and commits the first candidate with Enter', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model', 'gamma-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())
    const toggle = screen.getByRole('button', { name: 'Model candidates' })
    fireEvent.change(input, { target: { value: '' } })
    await screen.findByRole('listbox')
    fireEvent.keyDown(input, { key: 'Escape' })

    // The pointer picks the second candidate and stays over that option.
    fireEvent.click(toggle)
    const list = await screen.findByRole('listbox')
    const options = within(list).getAllByRole('option')
    fireEvent.mouseOver(options[1]!)
    fireEvent.click(options[1]!)
    expect(input.value).toBe('beta-model')
    expect(screen.queryByRole('listbox')).toBeNull()

    // Reopening from the toggle leaves no candidate active, not even while the
    // pointer rests over an option.
    fireEvent.click(
      screen.getByRole('button', { name: /Delete Upstream model name/ }),
    )
    expect(input.value).toBe('')
    fireEvent.click(toggle)
    const reopened = await screen.findByRole('listbox')
    const reopenedOptions = within(reopened).getAllByRole('option')
    fireEvent.mouseOver(reopenedOptions[1]!)
    expect(reopened.querySelector('[aria-selected="true"]')).toBeNull()
    expect(input.getAttribute('aria-activedescendant')).toBeNull()

    // The first arrow press activates the first candidate and Enter commits it.
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    expect(input.getAttribute('aria-activedescendant')).toBe(`${reopened.id}-0`)
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(input.value).toBe('alpha-model')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(api.persistModelRoute).not.toHaveBeenCalled()
  })

  it('closes the list on Tab without selecting or blocking the tab', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())
    fireEvent.change(input, { target: { value: 'manual-tab-draft' } })
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    await screen.findByRole('listbox')
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    const tab = createEvent.keyDown(input, { key: 'Tab' })
    fireEvent(input, tab)
    expect(tab.defaultPrevented).toBe(false)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(input.value).toBe('manual-tab-draft')
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(api.persistModelRoute).not.toHaveBeenCalled()
  })

  it('returns focus to the input after Escape without closing the dialog', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())
    fireEvent.change(input, { target: { value: 'escape-draft' } })
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    await screen.findByRole('listbox')
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(input.value).toBe('escape-draft')
    expect(document.activeElement).toBe(input)
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(api.persistModelRoute).not.toHaveBeenCalled()
  })

  it('lets another control take focus without stealing it back', async () => {
    api.fetchEndpointModels.mockResolvedValue({
      models: ['alpha-model', 'beta-model'],
    })
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() => expect(api.fetchEndpointModels).toHaveBeenCalled())
    fireEvent.change(input, { target: { value: 'outside-draft' } })
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    await screen.findByRole('listbox')
    const patternInput = screen.getByRole('textbox', { name: 'Model pattern' })
    patternInput.focus()
    fireEvent.focusIn(patternInput)
    await waitFor(() => expect(screen.queryByRole('listbox')).toBeNull())
    expect(document.activeElement).toBe(patternInput)
    expect(input.value).toBe('outside-draft')
    expect(screen.getByRole('dialog')).toBeTruthy()
  })

  it('shows the loading status and a busy list until the catalog resolves', async () => {
    let resolveCatalog!: (value: { models: string[] }) => void
    api.fetchEndpointModels.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveCatalog = resolve
        }),
    )
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    await waitFor(() =>
      expect(document.querySelector('[role="status"]')).toBeTruthy(),
    )
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    const list = await screen.findByRole('listbox')
    expect(list.getAttribute('aria-busy')).toBe('true')
    expect(within(list).queryAllByRole('option')).toHaveLength(0)
    fireEvent.change(input, { target: { value: 'alpha' } })
    expect(within(list).queryAllByRole('option')).toHaveLength(0)
    await act(async () => resolveCatalog({ models: ['alpha-model'] }))
    await waitFor(() =>
      expect(within(list).getAllByRole('option')).toHaveLength(1),
    )
    expect(list.getAttribute('aria-busy')).toBe('false')
  })

  it('keeps the model field editable when the catalog fails', async () => {
    api.fetchEndpointModels.mockRejectedValue(
      new Error('fixture catalog failure'),
    )
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    expect(
      await screen.findByText('Could not load the upstream model list.'),
    ).toBeTruthy()
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    expect(input.disabled).toBe(false)
    fireEvent.change(input, { target: { value: 'manual-model' } })
    expect(input.value).toBe('manual-model')
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    const list = await screen.findByRole('listbox')
    expect(within(list).queryAllByRole('option')).toHaveLength(0)
  })

  it('turns browser autocomplete, capitalization and spellchecking off', async () => {
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    await screen.findByRole('dialog')
    const form = document.querySelector(
      '[data-slot="dialog-content"] form',
    ) as HTMLFormElement
    expect(form.getAttribute('autocomplete')).toBe('off')
    const input = document.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    expect(input.getAttribute('autocomplete')).toBe('off')
    expect(input.getAttribute('autocapitalize')).toBe('off')
    expect(input.getAttribute('spellcheck')).toBe('false')
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
      document.querySelector(
        '[data-model-candidate-input]',
      ) as HTMLInputElement,
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
  it('keeps the selected endpoint name and icon accessible in the target row', async () => {
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    const editor = await screen.findByRole('dialog')
    const dialogContent = editor
    expect(dialogContent.getAttribute('data-slot')).toBe('dialog-content')
    expect(dialogContent.className).toContain(
      'sm:max-w-[min(64rem,calc(100vw_-_2rem))]',
    )
    expect(dialogContent.className).not.toContain('max-w-5xl')
    expect(dialogContent.className).toContain('max-h-[min(94vh,58rem)]')
    expect(dialogContent.className).toContain('overflow-y-auto')
    const endpointSelect = within(editor).getByRole('combobox', {
      name: 'Endpoint 1',
    })
    const fullName = 'Fixture ep-fixture-8 With Long Endpoint Name'
    expect(endpointSelect.getAttribute('aria-label')).toBe('Endpoint 1')
    expect(endpointSelect.textContent).toContain(fullName)
    const describedBy = endpointSelect.getAttribute('aria-describedby')
    expect(describedBy).toBeTruthy()
    expect(editor.querySelector(`[id="${describedBy}"]`)?.textContent).toBe(
      fullName,
    )
    const valueNode = endpointSelect.querySelector('[data-slot="select-value"]')
    expect(valueNode?.querySelector('[role="img"]')).toBeTruthy()
    const label = valueNode?.querySelector('[data-target-endpoint-label]')
    expect(label?.textContent).toBe(fullName)
    expect(label?.className).toContain('truncate')
    expect(label?.parentElement?.className).toContain('min-w-0')
    expect(within(editor).queryByText('Endpoint', { exact: true })).toBeNull()
    expect(within(editor).queryByText('Inherit')).toBeNull()
    const modelInput = editor.querySelector(
      '[data-model-candidate-input]',
    ) as HTMLInputElement
    expect(modelInput.value).toBe('existing-model')
    fireEvent.change(modelInput, { target: { value: 'edited-long-model' } })
    expect(modelInput.value).toBe('edited-long-model')
    fireEvent.click(screen.getByRole('button', { name: /^Save$/ }))
    await waitFor(() =>
      expect(api.persistModelRoute).toHaveBeenCalledWith(
        'rule-offpage',
        expect.objectContaining({
          targets: [
            expect.objectContaining({
              endpoint_id: 'ep-fixture-8',
              upstream_model: 'edited-long-model',
            }),
          ],
        }),
      ),
    )
  })

  it('renders the endpoint placeholder and unknown endpoint ids without an icon', async () => {
    const base = route('rule-unknown')
    routes = [
      {
        ...base,
        targets: [
          {
            ...base.targets[0]!,
            endpoint_id: 'ep-removed',
            endpoint_name: 'Removed endpoint',
          },
        ],
      },
    ]
    renderPage()
    fireEvent.click(
      await screen.findByRole('button', { name: 'Edit fixture-pattern' }),
    )
    const editor = await screen.findByRole('dialog')
    const unknownSelect = within(editor).getByRole('combobox', {
      name: 'Endpoint 1',
    })
    expect(unknownSelect.textContent).toContain('ep-removed')
    expect(unknownSelect.getAttribute('aria-describedby')).toBeTruthy()
    expect(unknownSelect.querySelector('[role="img"]')).toBeNull()
    fireEvent.click(screen.getAllByRole('button', { name: 'Cancel' }).at(-1)!)
    fireEvent.click(screen.getByRole('button', { name: 'New model route' }))
    const createEditor = await screen.findByRole('dialog')
    const placeholderSelect = within(createEditor).getByRole('combobox', {
      name: 'Endpoint 1',
    })
    expect(placeholderSelect.textContent).toBe('Endpoint')
    expect(placeholderSelect.getAttribute('aria-describedby')).toBeNull()
    expect(
      placeholderSelect.querySelector('[data-target-endpoint-label]'),
    ).toBeNull()
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
