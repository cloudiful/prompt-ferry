import { beforeEach, expect, mock, test } from 'bun:test'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
  // Keep the property configurable so the whole suite can run in one bun process
  // alongside other tests that redefine `globalThis.localStorage`.
  configurable: true,
})

const emptyPage = { endpoints: [], first: 0, rows: 10, total: 0 }
const emptyRoutePage = { routes: [], first: 0, rows: 10, total: 0 }
const endpointTest = mock(async (endpointId: string) => ({
  duration_ms: 12,
  message: `tested ${endpointId}`,
  ok: true,
  status: 200,
}))
const deleteEndpointById = mock(async (_id: string) => {})
const fetchEndpointsPage = mock(async () => emptyPage)
const fetchModelRoutesPage = mock(async () => emptyRoutePage)
const persistEndpoint = mock(async (endpointId: string | null) => ({
  endpoint_id: endpointId ?? 'endpoint-new',
}))

mock.module('../src/stores/endpoints-api', () => ({
  deleteEndpointById,
  deleteModelRouteById: mock(),
  fetchEndpointsPage,
  fetchModelRoutesPage,
  persistEndpoint,
  persistModelRoute: mock(),
  runEndpointTest: endpointTest,
  runModelRouteProbe: mock(),
  updateEndpointEnabled: mock(),
  updateModelRouteEnabled: mock(),
}))

mock.module('../src/composables/useLocale', () => ({
  useLocale: () => ({ t: (key: string) => key }),
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useEndpointsStore } = await import('../src/stores/endpoints')

beforeEach(() => {
  setActivePinia(createPinia())
  for (const item of [
    deleteEndpointById,
    endpointTest,
    fetchEndpointsPage,
    fetchModelRoutesPage,
    persistEndpoint,
  ]) {
    item.mockClear()
  }
})

/** A gate the tests open by hand, so "while it is in flight" is real. */
function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

test('endpoint test action calls the API layer once', async () => {
  const store = useEndpointsStore()

  const result = await store.runEndpointTest('endpoint-1')

  expect(endpointTest).toHaveBeenCalledTimes(1)
  expect(endpointTest).toHaveBeenCalledWith('endpoint-1')
  expect(result.executed).toBe(true)
  expect(result.value.ok).toBe(true)
  expect(store.endpointTestResults['endpoint-1']).toEqual(result.value)
})

// Compatibility: the store keeps the surface its page calls, and the mutating
// actions now report whether they ran instead of always succeeding. The
// `delete*` entry points live on the page, not here — the store exposes the
// remove actions they call.
test('the store still exposes every action the endpoints page calls', () => {
  const store = useEndpointsStore()

  for (const action of [
    'findEndpointById',
    'findModelRouteById',
    'loadEndpoints',
    'loadModelRoutes',
    'refresh',
    'removeEndpoint',
    'removeModelRoute',
    'runEndpointTest',
    'runModelRouteTest',
    'saveEndpoint',
    'saveModelRoute',
    'toggleEndpointEnabled',
    'toggleModelRouteEnabled',
  ]) {
    expect(typeof (store as unknown as Record<string, unknown>)[action]).toBe(
      'function',
    )
  }
})

test('a save reports that it ran and reloads the workspace', async () => {
  const store = useEndpointsStore()

  const result = await store.saveEndpoint('endpoint-1', { name: 'renamed' })

  expect(result).toMatchObject({ executed: true })
  expect(persistEndpoint).toHaveBeenCalledTimes(1)
  expect(fetchEndpointsPage).toHaveBeenCalledTimes(1)
  expect(fetchModelRoutesPage).toHaveBeenCalledTimes(1)
})

test('the fetch-only busy flag is not raised by a write', async () => {
  const store = useEndpointsStore()
  const pendingSave = store.saveEndpoint('endpoint-1', { name: 'renamed' })

  expect(store.loading).toBe(false)
  expect(store.endpointPendingActions).toEqual({ 'endpoint-1': 'save' })

  await pendingSave

  expect(store.loading).toBe(false)
  expect(store.endpointPendingActions).toEqual({})
})

test('a second write of one record never reaches the API twice', async () => {
  const store = useEndpointsStore()

  const [first, second] = await Promise.all([
    store.saveEndpoint('endpoint-1', { name: 'renamed' }),
    store.saveEndpoint('endpoint-1', { name: 'renamed' }),
  ])

  expect(persistEndpoint).toHaveBeenCalledTimes(1)
  expect([first.executed, second.executed].sort()).toEqual([false, true])
})

test('a delete and a save of one record cannot overlap', async () => {
  const store = useEndpointsStore()

  const saving = store.saveEndpoint('endpoint-1', { name: 'renamed' })
  const deleting = await store.removeEndpoint('endpoint-1')

  expect(deleting).toEqual({ executed: false })
  expect(deleteEndpointById).not.toHaveBeenCalled()

  await saving
  expect(deleteEndpointById).not.toHaveBeenCalled()
})

test('a probe of one record does not block a write of it', async () => {
  const store = useEndpointsStore()

  const probing = store.runEndpointTest('endpoint-1')
  const saving = store.saveEndpoint('endpoint-1', { name: 'renamed' })

  expect((await saving).executed).toBe(true)
  expect((await probing).executed).toBe(true)
  expect(endpointTest).toHaveBeenCalledTimes(1)
  expect(persistEndpoint).toHaveBeenCalledTimes(1)
})

test('another record stays writable while one is being written', async () => {
  const store = useEndpointsStore()
  const gate = deferred()
  persistEndpoint.mockImplementationOnce(async () => {
    await gate.promise
    return { endpoint_id: 'endpoint-1' }
  })

  const saving = store.saveEndpoint('endpoint-1', { name: 'renamed' })
  expect(
    (await store.saveEndpoint('endpoint-2', { name: 'other' })).executed,
  ).toBe(true)

  // The slow record is still the only gated one.
  expect(persistEndpoint).toHaveBeenCalledTimes(2)
  expect(store.endpointPendingActions).toEqual({ 'endpoint-1': 'save' })

  gate.resolve()
  await saving
  expect(store.endpointPendingActions).toEqual({})
})

test('the store exposes the raw guard state read-only', async () => {
  const store = useEndpointsStore()
  const pending = store.saveEndpoint('endpoint-1', { name: 'renamed' })

  expect([...store.pendingActions.keys()]).toEqual(['endpoint:endpoint-1'])
  expect(store.isPending('endpoint:endpoint-1')).toBe(true)

  await pending

  expect([...store.pendingActions.keys()]).toEqual([])
  expect(store.isPending('endpoint:endpoint-1')).toBe(false)
})
