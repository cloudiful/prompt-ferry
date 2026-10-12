import { mock } from 'bun:test'
import { effectScope, ref } from 'vue'
import type {
  EndpointOAuthStatusResponse,
  OAuthBrowserStartResponse,
  OAuthDeviceStartResponse,
  OAuthLoginResponse,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import { useAsyncAction } from '../src/composables/useAsyncAction'
import type { RunEndpointOAuthAction } from '../src/stores/endpoint-oauth-action-types'
import type { useEndpointOAuthActions } from '../src/composables/useEndpointOAuthActions'

export const getEndpointOAuthStatus = mock(
  async (endpointId: string): Promise<EndpointOAuthStatusResponse> =>
    oauthStatus({ endpoint_id: endpointId }),
)
export const getEndpointTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> =>
    ({
      keys: [],
    }) as TokenPlanUsageResponse,
)
export const startEndpointDeviceLogin = mock(
  async (_endpointId: string): Promise<OAuthDeviceStartResponse> =>
    deviceStart(),
)
export const pollEndpointDeviceLogin = mock(
  async (_endpointId: string, _flowId: string): Promise<OAuthLoginResponse> =>
    loginResponse(),
)
export const startEndpointBrowserLogin = mock(
  async (_endpointId: string): Promise<OAuthBrowserStartResponse> =>
    browserStart(),
)
export const completeEndpointBrowserLogin = mock(
  async (
    _endpointId: string,
    _flowId: string,
    _redirectUrl: string,
  ): Promise<OAuthLoginResponse> => loginResponse(),
)
export const refreshEndpointOAuthToken = mock(
  async (_endpointId: string): Promise<EndpointOAuthStatusResponse> =>
    oauthStatus({ has_oauth_token: true }),
)
export const clearEndpointOAuthToken = mock(async (_endpointId: string) => {})

export const apiMocks = [
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
  startEndpointDeviceLogin,
  pollEndpointDeviceLogin,
  startEndpointBrowserLogin,
  completeEndpointBrowserLogin,
  refreshEndpointOAuthToken,
  clearEndpointOAuthToken,
]

export function oauthStatus(
  overrides: Partial<EndpointOAuthStatusResponse> = {},
): EndpointOAuthStatusResponse {
  return {
    endpoint_id: 'endpoint-1',
    expired: false,
    has_oauth_token: false,
    plan: 'platform_api_key',
    ...overrides,
  }
}

export function deviceStart(
  overrides: Partial<OAuthDeviceStartResponse> = {},
): OAuthDeviceStartResponse {
  return {
    expires_in_seconds: 300,
    flow_id: 'device-flow-1',
    interval_seconds: 60,
    user_code: 'ABCD-EFGH',
    verification_uri: 'https://example.test/device',
    ...overrides,
  }
}

export function browserStart(
  overrides: Partial<OAuthBrowserStartResponse> = {},
): OAuthBrowserStartResponse {
  return {
    authorize_url: 'https://example.test/authorize',
    expires_in_seconds: 300,
    flow_id: 'browser-flow-1',
    redirect_uri: 'https://example.test/callback',
    ...overrides,
  }
}

export function loginResponse(
  overrides: Partial<OAuthLoginResponse> = {},
): OAuthLoginResponse {
  return { flow_id: 'flow-1', status: 'pending', ...overrides }
}

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

export async function loadOAuthActions() {
  mock.module('../src/api/endpoint-oauth', () => ({
    clearEndpointOAuthToken,
    completeEndpointBrowserLogin,
    getEndpointOAuthStatus,
    getEndpointTokenPlanUsage,
    pollEndpointDeviceLogin,
    refreshEndpointOAuthToken,
    startEndpointBrowserLogin,
    startEndpointDeviceLogin,
  }))
  return import('../src/composables/useEndpointOAuthActions')
}

export function freshOAuthHarness(
  useActions: typeof useEndpointOAuthActions,
  endpointId = 'endpoint-1',
  configureMocks: () => void = () => {},
) {
  for (const api of apiMocks) api.mockReset()
  getEndpointOAuthStatus.mockImplementation(async (id) =>
    oauthStatus({ endpoint_id: id }),
  )
  getEndpointTokenPlanUsage.mockImplementation(
    async () => ({ keys: [] }) as TokenPlanUsageResponse,
  )
  startEndpointDeviceLogin.mockImplementation(async () => deviceStart())
  pollEndpointDeviceLogin.mockImplementation(async (_id, flowId) =>
    loginResponse({ flow_id: flowId }),
  )
  startEndpointBrowserLogin.mockImplementation(async () => browserStart())
  completeEndpointBrowserLogin.mockImplementation(async (_id, flowId) =>
    loginResponse({ flow_id: flowId }),
  )
  refreshEndpointOAuthToken.mockImplementation(async (id) =>
    oauthStatus({ endpoint_id: id, has_oauth_token: true }),
  )
  clearEndpointOAuthToken.mockImplementation(async () => {})
  configureMocks()
  const guard = useAsyncAction()
  const id = ref(endpointId)
  const statusEvents: Array<{ has_oauth_token: boolean; plan: string }> = []
  const runAction: RunEndpointOAuthAction = (targetId, action, label) =>
    guard.run(`endpoint:${targetId}`, action, label)
  const scope = effectScope()
  const actions = scope.run(() =>
    useActions({
      endpointId: id,
      onStatus: (payload) => statusEvents.push(payload),
      runAction,
    }),
  )!
  return {
    actions,
    guard,
    id,
    runAction,
    scope,
    statusEvents,
    settle: async () => {
      await Promise.resolve()
      await Promise.resolve()
      await Promise.resolve()
    },
  }
}
