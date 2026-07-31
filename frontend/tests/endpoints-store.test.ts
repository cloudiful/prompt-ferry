import { expect, mock, test } from 'bun:test'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
})

const endpointTest = mock(async (endpointId: string) => ({
  duration_ms: 12,
  message: `tested ${endpointId}`,
  ok: true,
  status: 200,
}))

mock.module('../src/stores/endpoints-api', () => ({
  deleteEndpointById: mock(),
  deleteModelRouteById: mock(),
  fetchEndpointsPage: mock(),
  fetchModelRoutesPage: mock(),
  persistEndpoint: mock(),
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

test('endpoint test action calls the API layer once', async () => {
  setActivePinia(createPinia())
  endpointTest.mockClear()
  const store = useEndpointsStore()

  const result = await store.runEndpointTest('endpoint-1')

  expect(endpointTest).toHaveBeenCalledTimes(1)
  expect(endpointTest).toHaveBeenCalledWith('endpoint-1')
  expect(result.ok).toBe(true)
  expect(store.endpointTestResults['endpoint-1']).toEqual(result)
})
