import { mock } from 'bun:test'
import type { TokenPlanUsageResponse } from '../../src/generated/admin-api'

export interface StorageStub {
  storage: Map<string, string>
  localStorage: {
    getItem: (key: string) => string | null
    setItem: (key: string, value: string) => void
  }
}

// A fresh in-memory localStorage stub per caller: no shared map, so suites
// can never leak keys into each other. The callsite still owns installing it
// on globalThis.
export function createStorageStub(): StorageStub {
  const storage = new Map<string, string>()
  return {
    storage,
    localStorage: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
    },
  }
}

// The default empty usage payload, rebuilt per call so no test can mutate a
// shared object through the mock.
export function emptyTokenPlanUsage(): TokenPlanUsageResponse {
  return {
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }
}

// A fresh fetch mock per caller with the default empty implementation.
export function createFetchTokenPlanUsageMock() {
  return mock(async (_endpointId: string): Promise<TokenPlanUsageResponse> =>
    emptyTokenPlanUsage(),
  )
}

// Restores the default empty implementation after a mockReset, matching the
// per-file beforeEach the specs previously inlined.
export function resetFetchTokenPlanUsageMock(
  fetchTokenPlanUsage: ReturnType<typeof createFetchTokenPlanUsageMock>,
): void {
  fetchTokenPlanUsage.mockReset()
  fetchTokenPlanUsage.mockImplementation(
    async (_endpointId: string): Promise<TokenPlanUsageResponse> =>
      emptyTokenPlanUsage(),
  )
}

// The full endpoints-api stub with fresh mocks per caller. The mock.module
// call itself stays in each spec file; only this construction is shared, so
// no mock instance ever crosses a file boundary.
export function createEndpointsApiStub(
  fetchTokenPlanUsage: ReturnType<typeof createFetchTokenPlanUsageMock>,
) {
  return {
    createEndpoint: mock(async () => ({})),
    createModelRoute: mock(async () => ({})),
    deleteEndpoint: mock(async () => undefined),
    deleteEndpointById: mock(async () => undefined),
    deleteModelRoute: mock(async () => undefined),
    deleteModelRouteById: mock(async () => undefined),
    endpointFormToRequest: mock((form: unknown) => form),
    endpointToForm: mock((endpoint: unknown) => endpoint),
    expectData: mock((value: unknown) => value),
    fetchEndpointsPage: mock(async () => ({
      endpoints: [],
      first: 0,
      rows: 10,
      total: 0,
    })),
    fetchModelRoutesPage: mock(async () => ({
      first: 0,
      routes: [],
      rows: 10,
      total: 0,
    })),
    fetchTokenPlanUsage,
    listEndpoints: mock(async () => undefined),
    listModelRoutes: mock(async () => undefined),
    modelRouteFormToRequest: mock((form: unknown) => form),
    modelRouteToForm: mock((route: unknown) => route),
    persistEndpoint: mock(async () => ({})),
    persistModelRoute: mock(async () => ({})),
    runEndpointTest: mock(async () => ({
      duration_ms: 0,
      message: '',
      ok: true,
      status: 200,
    })),
    runModelRouteProbe: mock(async () => ({
      duration_ms: 0,
      endpoint_name: '',
      message: '',
      model: null,
      model_pattern: '',
      ok: true,
      preferred_endpoint_name: '',
      status: 200,
    })),
    testEndpoint: mock(async () => undefined),
    testModelRoute: mock(async () => undefined),
    tokenPlanUsage: mock(async () => undefined),
    updateEndpoint: mock(async () => ({})),
    updateEndpointEnabled: mock(async () => ({})),
    updateModelRoute: mock(async () => ({})),
    updateModelRouteEnabled: mock(async () => ({})),
    withData: mock((value: unknown) => value),
  }
}

// An identity translator for pill-label assertions, fresh per caller.
export function createTranslateStub(): TranslateFn {
  return ((key: string) => key) as unknown as TranslateFn
}
