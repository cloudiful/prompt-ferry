import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ModelEndpointRule,
  ModelRoutePageResponse,
  ModelRouteRequest,
  ModelRouteTarget,
} from '@/generated/admin-api'
import { ModelRoutesPage } from '@/pages/ModelRoutesPage'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'

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

function target(
  ruleId: string,
  overrides: Partial<ModelRouteTarget> = {},
): ModelRouteTarget {
  return {
    active_windows: [],
    compact_mode: 'passthrough',
    created_at: '2026-10-23T12:00:00.000Z',
    dev_system_normalize: false,
    embedding_dimensions_override: null,
    enabled: true,
    endpoint_enabled: true,
    endpoint_id: 'ep-1',
    endpoint_name: 'Upstream One',
    has_proxy_url_override: false,
    native_api: 'auto',
    position: 0,
    rule_id: ruleId,
    service_tier: null,
    target_id: 'target-a1',
    thinking_downgrade_enabled: false,
    thinking_effort_override: null,
    updated_at: '2026-10-23T12:00:00.000Z',
    upstream_model: null,
    ...overrides,
  }
}

function route(
  ruleId: string,
  pattern: string,
  targets: ModelRouteTarget[],
): ModelEndpointRule {
  return {
    created_at: '2026-10-23T12:00:00.000Z',
    enabled: true,
    model_pattern: pattern,
    owner_user_id: null,
    routing_strategy: 'responses_session_affinity',
    rule_id: ruleId,
    scope: 'admin',
    targets,
    updated_at: '2026-10-23T12:00:00.000Z',
  }
}

let routes: ModelEndpointRule[]
let savedTargets: number

function page(first: number, rows: number): ModelRoutePageResponse {
  return {
    first,
    rows,
    total: routes.length,
    routes: routes.slice(first, first + rows),
  }
}

function renderPage() {
  useSessionStore.setState({
    me: admin,
    bootstrapped: true,
    bootstrapping: false,
  })
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

function row(ruleId: string): HTMLElement {
  const element = document.querySelector(
    `[data-model-route-row="${ruleId}"]`,
  ) as HTMLElement
  if (!element) throw new Error(`route row ${ruleId} is missing`)
  return element
}

function openTargets(ruleId: string): void {
  const trigger = row(ruleId).querySelector(
    '[data-slot="popover-trigger"]',
  ) as HTMLButtonElement
  if (!trigger) throw new Error(`targets disclosure of ${ruleId} is missing`)
  fireEvent.click(trigger)
}

function targetSwitch(targetId: string): HTMLElement {
  const element = document.querySelector(
    `[data-model-route-target="${targetId}"] [role="switch"]`,
  ) as HTMLElement
  if (!element) throw new Error(`target switch ${targetId} is missing`)
  return element
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
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
  savedTargets = 0
  routes = [
    route('rule-a', 'fixture-a-*', [
      target('rule-a', {
        active_windows: [{ start: '22:00', end: '06:00', days: [1, 3] }],
        endpoint_id: 'ep-1',
        endpoint_name: 'Upstream One',
        has_proxy_url_override: true,
        position: 0,
        target_id: 'target-a1',
        upstream_model: 'model-one',
      }),
      target('rule-a', {
        enabled: false,
        endpoint_id: 'ep-2',
        endpoint_name: 'Upstream Two',
        position: 1,
        target_id: 'target-a2',
        upstream_model: 'model-two',
      }),
    ]),
    route('rule-b', 'fixture-b-*', [
      target('rule-b', {
        endpoint_id: 'ep-3',
        endpoint_name: 'Upstream Three',
        position: 0,
        target_id: 'target-b1',
      }),
    ]),
  ]
  api.listAllEndpointRecords.mockResolvedValue([])
  api.listEndpointUsers.mockResolvedValue([])
  api.fetchEndpointModels.mockResolvedValue({ models: ['fixture-model'] })
  api.fetchModelRoutesPage.mockImplementation(
    async (first: number, rows: number) => page(first, rows),
  )
  api.persistModelRoute.mockImplementation(
    async (ruleId: string | null, body: ModelRouteRequest) => {
      const current = routes.find((item) => item.rule_id === ruleId)
      if (!current) throw new Error(`unknown route ${ruleId}`)
      savedTargets += 1
      const saved: ModelEndpointRule = {
        ...current,
        enabled: body.enabled !== false,
        model_pattern: body.model_pattern,
        owner_user_id: body.owner_user_id ?? null,
        scope: body.scope,
        targets: (body.targets ?? []).map((entry, index) => ({
          ...current.targets[index]!,
          enabled: entry.enabled !== false,
          endpoint_id: entry.endpoint_id ?? '',
          position: index,
          target_id: `target-${ruleId}-${index}-${savedTargets}`,
          upstream_model: entry.upstream_model ?? null,
        })),
      }
      routes = routes.map((item) => (item.rule_id === ruleId ? saved : item))
      return saved
    },
  )
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('model route target toggle', () => {
  it('enables one target and keeps every other route setting', async () => {
    renderPage()
    await screen.findAllByText('fixture-a-*')
    openTargets('rule-a')

    fireEvent.click(targetSwitch('target-a2'))

    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))
    const [ruleId, body] = api.persistModelRoute.mock.calls[0] as [
      string,
      ModelRouteRequest,
    ]
    expect(ruleId).toBe('rule-a')
    expect(body.model_pattern).toBe('fixture-a-*')
    expect(body.targets).toHaveLength(2)
    expect(body.targets?.[0]).toMatchObject({
      enabled: true,
      endpoint_id: 'ep-1',
      upstream_model: 'model-one',
    })
    expect(body.targets?.[1]).toMatchObject({
      enabled: true,
      endpoint_id: 'ep-2',
      upstream_model: 'model-two',
    })
    // The stored secret and schedule stay untouched: the request omits them.
    expect(body.targets?.[0]).not.toHaveProperty('proxy_url_override')
    expect(body.targets?.[0]).not.toHaveProperty('active_windows')
  })

  it('addresses the rebuilt target id after the route was saved', async () => {
    renderPage()
    await screen.findAllByText('fixture-a-*')
    openTargets('rule-a')
    fireEvent.click(targetSwitch('target-a2'))
    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))

    await waitFor(() =>
      expect(
        document.querySelector('[data-model-route-target="target-a2"]'),
      ).toBeNull(),
    )
    const rebuilt = document.querySelector(
      '[data-model-route-target^="target-rule-a-1-"] [role="switch"]',
    ) as HTMLElement
    expect(rebuilt).toBeTruthy()
    fireEvent.click(rebuilt)

    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(2))
    const [, body] = api.persistModelRoute.mock.calls[1] as [
      string,
      ModelRouteRequest,
    ]
    expect(body.targets?.[1]).toMatchObject({
      enabled: false,
      endpoint_id: 'ep-2',
      upstream_model: 'model-two',
    })
  })

  it('reopens the disclosure when every configured target is off', async () => {
    routes = [
      route('rule-a', 'fixture-a-*', [
        target('rule-a', {
          enabled: false,
          endpoint_id: 'ep-1',
          endpoint_name: 'Upstream One',
          target_id: 'target-a1',
        }),
      ]),
    ]
    renderPage()
    await screen.findAllByText('fixture-a-*')
    const disclosure = within(row('rule-a')).getByRole('button', {
      name: 'No enabled targets',
    })
    fireEvent.click(disclosure)
    const switchElement = targetSwitch('target-a1')
    expect(switchElement.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(switchElement)

    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))
    const [, body] = api.persistModelRoute.mock.calls[0] as [
      string,
      ModelRouteRequest,
    ]
    expect(body.targets?.[0]).toMatchObject({
      enabled: true,
      endpoint_id: 'ep-1',
    })
  })

  it('blocks the conflicting route writes of the same route only', async () => {
    renderPage()
    await screen.findAllByText('fixture-a-*')
    openTargets('rule-a')
    let resolveSave!: (value: ModelEndpointRule) => void
    api.persistModelRoute.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve
        }),
    )
    fireEvent.click(targetSwitch('target-a2'))

    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))
    expect(
      (
        screen.getByRole('button', {
          name: 'Edit fixture-a-*',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: 'Delete fixture-a-*',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (within(row('rule-a')).getByRole('switch') as HTMLButtonElement).disabled,
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: 'Edit fixture-b-*',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false)
    await act(async () =>
      resolveSave({
        ...routes[0]!,
        targets: routes[0]!.targets.map((entry) => ({ ...entry })),
      }),
    )
  })

  it('keeps the stored state and retries after a failed toggle', async () => {
    renderPage()
    await screen.findAllByText('fixture-a-*')
    openTargets('rule-a')
    api.persistModelRoute.mockRejectedValueOnce(
      new Error('fixture target update rejected'),
    )
    fireEvent.click(targetSwitch('target-a2'))

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('fixture target update rejected')
    expect(targetSwitch('target-a2').getAttribute('aria-checked')).toBe('false')

    fireEvent.click(targetSwitch('target-a2'))
    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(2))
  })

  it('never submits the draft of an open editor', async () => {
    renderPage()
    await screen.findAllByText('fixture-a-*')
    fireEvent.click(screen.getByRole('button', { name: 'Edit fixture-a-*' }))
    await screen.findByRole('dialog')
    fireEvent.change(screen.getByRole('textbox', { name: 'Model pattern' }), {
      target: { value: 'draft-only-pattern' },
    })

    openTargets('rule-a')
    fireEvent.click(targetSwitch('target-a2'))

    await waitFor(() => expect(api.persistModelRoute).toHaveBeenCalledTimes(1))
    const [, body] = api.persistModelRoute.mock.calls[0] as [
      string,
      ModelRouteRequest,
    ]
    expect(body.model_pattern).toBe('fixture-a-*')
  })
})
