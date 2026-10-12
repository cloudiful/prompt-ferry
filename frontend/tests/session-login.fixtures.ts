import { mock } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

export const authLogin = mock(async (_options?: unknown) => ({
  data: undefined,
}))
export const authLogout = mock(async (_options?: unknown) => ({
  data: undefined,
}))
export const authMe = mock(async (_options?: unknown) => ({
  data: {
    display_name: 'Administrator',
    is_admin: true,
    login_name: 'admin',
    user_id: 1,
  },
}))
export const bridgeStatus = mock(async (_options?: unknown) => ({
  data: {
    configured_relays: 0,
    connected_relays: 0,
    relays: [],
    snapshot_version: 1,
  },
}))
export const relayAuthLogin = mock(async (_options?: unknown) => ({
  data: undefined,
}))
export const relayAuthLogout = mock(async (_options?: unknown) => ({
  data: undefined,
}))
export const relayAuthMe = mock(async (_options?: unknown) => ({
  data: { authenticated: true },
}))

export const apiMocks = [
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  relayAuthLogin,
  relayAuthLogout,
  relayAuthMe,
]

mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  relayAuthLogin,
  relayAuthLogout,
  relayAuthMe,
}))

export const { createPinia, setActivePinia } = await import('pinia')
export const { useSessionStore } = await import('../src/stores/session')
export const { useRelaySessionStore } =
  await import('../src/stores/relay-session')

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, reject, resolve }
}

export function freshSessions() {
  for (const api of apiMocks) api.mockReset()
  authLogin.mockImplementation(async () => ({ data: undefined }))
  authLogout.mockImplementation(async () => ({ data: undefined }))
  authMe.mockImplementation(async () => ({
    data: {
      display_name: 'Administrator',
      is_admin: true,
      login_name: 'admin',
      user_id: 1,
    },
  }))
  bridgeStatus.mockImplementation(async () => ({
    data: {
      configured_relays: 0,
      connected_relays: 0,
      relays: [],
      snapshot_version: 1,
    },
  }))
  relayAuthLogin.mockImplementation(async () => ({ data: undefined }))
  relayAuthLogout.mockImplementation(async () => ({ data: undefined }))
  relayAuthMe.mockImplementation(async () => ({
    data: { authenticated: true },
  }))
  setActivePinia(createPinia())
  return { worker: useSessionStore(), relay: useRelaySessionStore() }
}
