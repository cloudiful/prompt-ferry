import { expect, mock, test } from 'bun:test'
import { isRelayHostPath } from '../src/nav'

test('only the host path bypasses the Worker session guard', () => {
  expect(isRelayHostPath('/relay-host')).toBe(true)
  expect(isRelayHostPath('/relay-host/')).toBe(true)
  expect(isRelayHostPath('/relays')).toBe(false)
  expect(isRelayHostPath('/api-keys')).toBe(false)
  expect(isRelayHostPath('/login')).toBe(false)
  expect(isRelayHostPath('/relay-host-page')).toBe(false)
})

const actualAdminApi = await import('../src/generated/admin-api')

// The relay-token session authenticates with the host-local management token.
// It must never be read as a Worker `session.me`/`isAdmin` identity: the
// guard above is the only place the two sessions meet, and it keeps the
// Worker session untouched for host paths.
const relayAuthMe = mock(async () => ({ data: { authenticated: true } }))
const relayAuthLogin = mock(async () => ({ data: undefined }))
const relayAuthLogout = mock(async () => ({ data: undefined }))

// Keep every other generated export intact: `mock.module` is process-global
// and would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  relayAuthLogin,
  relayAuthLogout,
  relayAuthMe,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRelaySessionStore } = await import('../src/stores/relay-session')

test('relay login sends the management token, not Worker credentials', async () => {
  setActivePinia(createPinia())
  relayAuthLogin.mockClear()
  relayAuthMe.mockClear()
  const store = useRelaySessionStore()

  await store.login('host-token-123')

  expect(relayAuthLogin).toHaveBeenCalledTimes(1)
  const options = relayAuthLogin.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(options.body).toEqual({ admin_token: 'host-token-123' })
  expect(store.authenticated).toBe(true)
})

test('relay logout never touches the Worker session store', async () => {
  setActivePinia(createPinia())
  const relay = useRelaySessionStore()
  relay.authenticated = true
  await relay.logout()
  expect(relay.authenticated).toBe(false)

  // The Worker session module keeps its own identity; importing it here only
  // proves the relay store did not mutate it.
  const { useSessionStore } = await import('../src/stores/session')
  const worker = useSessionStore()
  expect(worker.me).toBeNull()
  expect(worker.isAdmin).toBe(false)
})

// Tester additions: the nav/route boundary the guard serves. The host entry is
// reachable without a Worker admin identity, and the Worker-admin `/relays`
// page keeps its gate at both the nav and route-meta level.
const { navItems, visibleNavItems } = await import('../src/nav')

test('the host nav entry is visible to non-admins while /relays stays admin-only', () => {
  const hostItem = navItems.find((item) => item.section === 'relay-host')
  const relaysItem = navItems.find((item) => item.section === 'relays')
  expect(hostItem?.adminOnly ?? false).toBe(false)
  expect(relaysItem?.adminOnly).toBe(true)

  const userSections = visibleNavItems(false).map((item) => item.section)
  expect(userSections).toContain('relay-host')
  expect(userSections).not.toContain('relays')
  expect(userSections).not.toContain('users')

  const adminSections = visibleNavItems(true).map((item) => item.section)
  expect(adminSections).toContain('relay-host')
  expect(adminSections).toContain('relays')
})

test('route meta keeps /relays behind the admin gate and /relay-host open', async () => {
  // The router module reads window through vue-router's web history; shim the
  // pieces resolve() touches (resolve performs no navigation and no guards).
  const listeners: Array<() => void> = []
  ;(globalThis as Record<string, unknown>).window = {
    location: {
      href: 'http://localhost/',
      pathname: '/',
      search: '',
      hash: '',
    },
    history: { state: null, pushState() {}, replaceState() {} },
    addEventListener: () => listeners.push(() => {}),
    removeEventListener: () => {},
  }
  ;(globalThis as Record<string, unknown>).location = (
    globalThis as { window: { location: unknown } }
  ).window.location
  ;(globalThis as Record<string, unknown>).history = (
    globalThis as { window: { history: unknown } }
  ).window.history

  const { router } = await import('../src/router')
  expect(router.resolve('/relays').meta.adminOnly).toBe(true)
  expect(router.resolve('/relay-host').meta.adminOnly).toBe(false)
  expect(router.resolve('/relay-host/').meta.adminOnly).toBe(false)
  expect(router.resolve('/users').meta.adminOnly).toBe(true)
  expect(router.resolve('/api-keys').meta.adminOnly).toBe(false)
  expect(router.resolve('/relays').matched.length).toBeGreaterThan(0)
  expect(router.resolve('/relay-host').matched.length).toBeGreaterThan(0)
})
