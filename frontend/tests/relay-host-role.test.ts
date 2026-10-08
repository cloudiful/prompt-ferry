import { beforeEach, expect, mock, test } from 'bun:test'
import {
  createRelayPatchRequest,
  createEmptyRelayForm,
} from '../src/admin-mappers/forms/relay'
import {
  createEmptyRelayHostForm,
  createRelayHostRoleRequest,
  hasHostRoleChange,
} from '../src/admin-mappers/forms/relay-host'
import {
  canManageWorker,
  isHostRestartRequired,
  isRelayReady,
  isWorkerConnected,
} from '../src/models/endpoints/capability'

function statusWith(overrides: Record<string, unknown> = {}) {
  return {
    relay: {
      admin_bind: '127.0.0.1:8790',
      bind: '127.0.0.1:19701',
      bridge_encryption_required: false,
      client_token_configured: false,
      request_timeout_seconds: 30,
      tls_enabled: false,
      worker_bind: '127.0.0.1:19702',
      worker_heartbeat_timeout_seconds: 30,
      worker_tls_enabled: false,
      worker_token_configured: false,
    },
    relay_ready: true,
    restart_required: false,
    role: 'relay',
    worker: { connected: false, connected_workers: 0 },
    ...overrides,
  } as unknown as Parameters<typeof isWorkerConnected>[0]
}

test('remote relay enabled toggle never carries a host role', () => {
  const form = {
    ...createEmptyRelayForm(),
    relay_id: 'relay-1',
    name: 'east',
    relay_url: 'wss://relay.example.com/ws/worker',
    enabled: false,
  }
  const patch = createRelayPatchRequest(form)
  expect(patch.enabled).toBe(false)
  expect('role' in patch).toBe(false)
  expect('pending_role' in patch).toBe(false)
})

test('host role request carries only the role', () => {
  const request = createRelayHostRoleRequest({
    ...createEmptyRelayHostForm(),
    role: 'worker',
  })
  expect(request).toEqual({ role: 'worker' })
  expect('enabled' in request).toBe(false)
})

test('role change is detected against the running role only', () => {
  expect(
    hasHostRoleChange(
      { role: 'relay', admin_bind: '' },
      { role: 'integrated', restart_required: false },
    ),
  ).toBe(true)
  expect(
    hasHostRoleChange(
      { role: 'integrated', admin_bind: '' },
      { role: 'integrated', restart_required: true },
    ),
  ).toBe(false)
})

test('capability helpers keep worker display scoped', () => {
  const disconnected = statusWith({
    worker: { connected: false, connected_workers: 0 },
  })
  expect(isWorkerConnected(disconnected)).toBe(false)
  expect(canManageWorker(disconnected)).toBe(false)
  expect(isRelayReady(disconnected)).toBe(true)

  const connected = statusWith({
    worker: { connected: true, connected_workers: 1, config_version: 1 },
  })
  expect(isWorkerConnected(connected)).toBe(true)
  expect(canManageWorker(connected)).toBe(true)
})

test('restart is required when any host surface reports it', () => {
  const host = { role: 'relay', restart_required: false } as const
  const statusOn = statusWith({ restart_required: true })
  const settingsOff = {
    relay: statusWith().relay,
    restart_required: false,
  } as unknown as Parameters<typeof isHostRestartRequired>[2]
  expect(isHostRestartRequired(host, statusOn, settingsOff)).toBe(true)
  expect(
    isHostRestartRequired(
      host,
      statusWith({ restart_required: false }),
      settingsOff,
    ),
  ).toBe(false)
})

// Tester additions: the relay-host store's own behavior against the generated
// SDK — the role save body, the restart OR across the three surfaces, and the
// save-then-reread agreement. The executor suite pins the capability helpers;
// these drive the store the page actually calls.

const actualAdminApi = await import('../src/generated/admin-api')

function relayViewFixture() {
  return {
    admin_bind: '127.0.0.1:8790',
    bind: '127.0.0.1:19701',
    bridge_encryption_required: false,
    client_token_configured: false,
    request_timeout_seconds: 30,
    tls_enabled: false,
    worker_bind: '127.0.0.1:19702',
    worker_heartbeat_timeout_seconds: 30,
    worker_tls_enabled: false,
    worker_token_configured: false,
  }
}

const relayHostGet = mock(async () => ({
  data: { role: 'relay', pending_role: null, restart_required: false },
}))
const relayStatusGet = mock(async () => ({
  data: {
    relay: relayViewFixture(),
    relay_ready: true,
    restart_required: false,
    role: 'relay',
    worker: { connected: false, connected_workers: 0 },
  },
}))
const relaySettingsGet = mock(async () => ({
  data: { relay: relayViewFixture(), restart_required: false },
}))
const relayRolePut = mock(async () => ({
  data: { role: 'relay', pending_role: 'relay', restart_required: true },
}))
const relaySettingsPatch = mock(async () => ({
  data: { relay: relayViewFixture(), restart_required: true },
}))
const relayRestartPost = mock(async () => ({ data: undefined }))

mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  relayHost: relayHostGet,
  relayStatus: relayStatusGet,
  relayGetSettings: relaySettingsGet,
  relaySetHostRole: relayRolePut,
  relaySetSettings: relaySettingsPatch,
  relayRequestRestart: relayRestartPost,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRelayHostStore } = await import('../src/stores/relay-host')

beforeEach(() => {
  relayHostGet.mockClear()
  relayStatusGet.mockClear()
  relaySettingsGet.mockClear()
  relayRolePut.mockClear()
  relaySettingsPatch.mockClear()
  relayRestartPost.mockClear()
  relayHostGet.mockImplementation(async () => ({
    data: { role: 'relay', pending_role: null, restart_required: false },
  }))
  relayStatusGet.mockImplementation(async () => ({
    data: {
      relay: relayViewFixture(),
      relay_ready: true,
      restart_required: false,
      role: 'relay',
      worker: { connected: false, connected_workers: 0 },
    },
  }))
  relaySettingsGet.mockImplementation(async () => ({
    data: { relay: relayViewFixture(), restart_required: false },
  }))
  relayRolePut.mockImplementation(async () => ({
    data: { role: 'relay', pending_role: 'relay', restart_required: true },
  }))
  relaySettingsPatch.mockImplementation(async () => ({
    data: { relay: relayViewFixture(), restart_required: true },
  }))
})

test('saving a role sends only the role and surfaces the pending restart', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()
  await store.refresh()
  expect(store.restartRequired).toBe(false)

  const saved = await store.saveRole('relay')

  expect(relayRolePut).toHaveBeenCalledTimes(1)
  const options = relayRolePut.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(options.body).toEqual({ role: 'relay' })
  expect('enabled' in options.body).toBe(false)
  expect('admin_bind' in options.body).toBe(false)

  // The save response alone reports restart_required, so the page banner can
  // appear before status/settings agree.
  expect(saved.restart_required).toBe(true)
  expect(store.restartRequired).toBe(true)
  expect(store.pendingRole).toBe('relay')
})

test('restartRequired is the OR of host, status, and settings surfaces', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()

  await store.refresh()
  expect(store.restartRequired).toBe(false)

  relayStatusGet.mockImplementation(async () => ({
    data: {
      relay: relayViewFixture(),
      relay_ready: true,
      restart_required: true,
      role: 'relay',
      worker: { connected: false, connected_workers: 0 },
    },
  }))
  await store.refresh()
  expect(store.restartRequired).toBe(true)

  relayStatusGet.mockImplementation(async () => ({
    data: {
      relay: relayViewFixture(),
      relay_ready: true,
      restart_required: false,
      role: 'relay',
      worker: { connected: false, connected_workers: 0 },
    },
  }))
  relaySettingsGet.mockImplementation(async () => ({
    data: { relay: relayViewFixture(), restart_required: true },
  }))
  await store.refresh()
  expect(store.restartRequired).toBe(true)

  relaySettingsGet.mockImplementation(async () => ({
    data: { relay: relayViewFixture(), restart_required: false },
  }))
  await store.refresh()
  expect(store.restartRequired).toBe(false)
})

test('saving settings rereads host and status so the surfaces agree', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()
  await store.refresh()

  await store.saveSettings('  127.0.0.1:8791  ')

  expect(relaySettingsPatch).toHaveBeenCalledTimes(1)
  const patchOptions = relaySettingsPatch.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  // The trimmed bind travels; the role does not.
  expect(patchOptions.body).toEqual({ admin_bind: '127.0.0.1:8791' })
  expect('role' in patchOptions.body).toBe(false)

  // One refresh read of host+status after the save (the initial refresh is
  // the first read of each; host/status are each read exactly twice now).
  expect(relayHostGet).toHaveBeenCalledTimes(2)
  expect(relayStatusGet).toHaveBeenCalledTimes(2)
  expect(store.settings?.restart_required).toBe(true)
  expect(store.restartRequired).toBe(true)
  expect(store.error).toBe('')
})

test('a blank settings save sends an empty body and no role key', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()
  await store.refresh()
  relayHostGet.mockClear()
  relayStatusGet.mockClear()

  await store.saveSettings('   ')

  expect(relaySettingsPatch).toHaveBeenCalledTimes(1)
  const patchOptions = relaySettingsPatch.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(patchOptions.body).toEqual({})
  expect('role' in patchOptions.body).toBe(false)
})

test('a restart request goes to the restart endpoint only', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()

  await store.requestRestart()

  expect(relayRestartPost).toHaveBeenCalledTimes(1)
  expect(relayRolePut).not.toHaveBeenCalled()
  expect(relaySettingsPatch).not.toHaveBeenCalled()
})

test('a failed role save records the error and rethrows', async () => {
  setActivePinia(createPinia())
  const store = useRelayHostStore()
  relayRolePut.mockImplementationOnce(async () => {
    throw new Error('403: loopback')
  })

  await expect(store.saveRole('worker')).rejects.toThrow('403: loopback')
  expect(store.error).toContain('403: loopback')
})
