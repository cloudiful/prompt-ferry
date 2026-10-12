import { mock } from 'bun:test'
import { ref } from 'vue'

import type {
  EndpointTestResponse,
  ModelEndpointRule,
  ModelRouteTestResponse,
  ProviderEndpoint,
} from '../src/generated/admin-api'

/**
 * Shared setup for the endpoint action tests.
 *
 * The store's actions are a factory over injected slices, so they can be driven
 * on plain refs with no Pinia, router, or component.
 *
 * `mock.module` is process-global and this repo's other suites replace
 * `endpoints-api` too — one of them with a partial module — so a registration
 * made once at import time can be replaced before the action module links
 * against it. Every test file therefore calls `loadEndpointActions` itself: the
 * mock *behaviour* lives here, in one place, while the registration stays paired
 * with the import that consumes it.
 */

export const probe = (extra: Record<string, unknown> = {}) => ({
  duration_ms: 1,
  message: 'ok',
  ok: true,
  status: 200,
  ...extra,
})

export const deleteEndpointById = mock(async () => {})
export const deleteModelRouteById = mock(async () => {})
export const persistEndpoint = mock(
  async (id: string | null) =>
    ({ endpoint_id: id ?? 'endpoint-new' }) as ProviderEndpoint,
)
export const persistModelRoute = mock(
  async (id: string | null) =>
    ({ rule_id: id ?? 'rule-new' }) as ModelEndpointRule,
)
export const runEndpointTest = mock(
  async (_id: string): Promise<EndpointTestResponse> => probe(),
)
export const runModelRouteProbe = mock(
  async (_id: string): Promise<ModelRouteTestResponse> => probe(),
)
export const updateEndpointEnabled = mock(
  async (endpoint: ProviderEndpoint, enabled: boolean) => ({
    ...endpoint,
    enabled,
  }),
)
export const updateModelRouteEnabled = mock(
  async (route: ModelEndpointRule, enabled: boolean) => ({ ...route, enabled }),
)

export const apiMocks = [
  deleteEndpointById,
  deleteModelRouteById,
  persistEndpoint,
  persistModelRoute,
  runEndpointTest,
  runModelRouteProbe,
  updateEndpointEnabled,
  updateModelRouteEnabled,
]

const apiModule = () => ({
  deleteEndpointById,
  deleteModelRouteById,
  fetchEndpointsPage: mock(),
  fetchModelRoutesPage: mock(),
  fetchOrganizationUsage: mock(),
  persistEndpoint,
  persistModelRoute,
  runEndpointTest,
  runModelRouteProbe,
  updateEndpointEnabled,
  updateModelRouteEnabled,
})

/**
 * Claim `endpoints-api` and load the action module against it, in that order.
 * Call once per test file, at the top level, before anything is asserted.
 */
export async function loadEndpointActions() {
  mock.module('../src/stores/endpoints-api', apiModule)
  loaded = {
    ...(await import('../src/stores/endpoint-actions')),
    useAsyncAction: (await import('../src/composables/useAsyncAction'))
      .useAsyncAction,
  }
  return loaded
}

/** A gate the tests open by hand, so "in flight" is really in flight. */
export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export const endpoint = (id: string) =>
  ({ enabled: true, endpoint_id: id, name: id }) as ProviderEndpoint
export const route = (id: string) =>
  ({ enabled: true, model_pattern: id, rule_id: id }) as ModelEndpointRule

type PendingMap = ReadonlyMap<string, string | undefined>

/**
 * The most recently loaded action module. Both files in this family load the
 * same one; holding it here keeps every test body reading `freshHarness()`
 * rather than threading the module through every call site.
 */
let loaded: Awaited<ReturnType<typeof loadEndpointActions>> | null = null

/** The per-domain row projection, as the surfaces receive it. */
export const rows = (
  guard: { pendingActions: { value: PendingMap } },
  prefix: string,
) => loaded!.pendingRowsById(guard.pendingActions.value, prefix)

/**
 * The actions on plain refs: no store, no router, no component needed.
 * `reloadWorkspace` can be replaced to hold a save open past its request.
 *
 * The harness carries the API recorders too, so a test reads as one subject
 * (`h.api.persistEndpoint`) and the few that need a call held open can say so
 * (`holdPersist`) instead of reaching for the mock module directly.
 */
export function buildHarness(reloadWorkspace?: () => Promise<void>) {
  if (!loaded) throw new Error('call loadEndpointActions() first')
  const { createEndpointActions, useAsyncAction } = loaded
  const guard = useAsyncAction()
  const endpoints = {
    items: ref<ProviderEndpoint[]>([
      endpoint('endpoint-a'),
      endpoint('endpoint-b'),
    ]),
    testResults: ref<Record<string, EndpointTestResponse>>({}),
    testingId: ref(''),
    togglingId: ref(''),
  }
  const modelRoutes = {
    items: ref<ModelEndpointRule[]>([route('rule-a'), route('rule-b')]),
    testResults: ref<Record<string, ModelRouteTestResponse>>({}),
    testingId: ref(''),
    togglingId: ref(''),
  }
  const reloads = { modelroute: 0, workspace: 0 }
  const actions = createEndpointActions({
    endpoints,
    guard,
    modelRoutes,
    reloadModelRouting: async () => void reloads.modelroute++,
    reloadWorkspace: reloadWorkspace ?? (async () => void reloads.workspace++),
  })
  return {
    actions,
    api: {
      deleteEndpointById,
      deleteModelRouteById,
      persistEndpoint,
      persistModelRoute,
      runEndpointTest,
      runModelRouteProbe,
      updateEndpointEnabled,
      updateModelRouteEnabled,
    },
    endpoints,
    guard,
    modelRoutes,
    persistCalls: () => persistEndpoint.mock.calls.length,
    reloads,
  }
}

export type Harness = ReturnType<typeof buildHarness>
export const endpointKey = (id: string) => loaded!.endpointActionKey(id)
export const routeKey = (id: string) => loaded!.modelRouteActionKey(id)

/** Replace the next `persistEndpoint` call with one the test controls. */
export const holdPersist = (implementation: () => Promise<unknown>) =>
  persistEndpoint.mockImplementationOnce(implementation as never)

/** Replace the next `persistModelRoute` call with one the test controls. */
export const holdRoutePersist = (implementation: () => Promise<unknown>) =>
  persistModelRoute.mockImplementationOnce(implementation as never)

/** A fresh harness per test: fresh refs, a fresh guard, empty reload logs. */
export function freshHarness(reloadWorkspace?: () => Promise<void>): Harness {
  for (const item of apiMocks) item.mockClear()
  return buildHarness(reloadWorkspace)
}
