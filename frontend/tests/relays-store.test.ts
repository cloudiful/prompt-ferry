import { expect, mock, test } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

const listRelays = mock(async () => ({
  data: {
    relays: [],
    total: 0,
    first: 0,
    rows: 10,
    connected_count: 0,
    enabled_count: 0,
  },
}))
const updateRelay = mock(async () => ({ data: { relay_id: 'relay-1' } }))

// Keep every other generated export intact: `mock.module` is process-global
// and would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  listRelays,
  updateRelay,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRelaysStore } = await import('../src/stores/relays')

test('successful refresh clears the scoped worker error', async () => {
  setActivePinia(createPinia())
  listRelays.mockClear()
  const store = useRelaysStore()
  store.lastError = 'stale'

  await store.refresh()

  expect(listRelays).toHaveBeenCalledTimes(1)
  expect(store.lastError).toBeNull()
  expect(store.relays).toEqual([])
})

test('failed refresh records a scoped error and keeps the list usable', async () => {
  setActivePinia(createPinia())
  const store = useRelaysStore()
  listRelays.mockClear()
  listRelays.mockRejectedValueOnce(new Error('worker_not_connected'))

  await expect(store.refresh()).rejects.toThrow('worker_not_connected')
  expect(store.lastError).toContain('worker_not_connected')
})

test('saving a remote relay never calls the host role endpoint', async () => {
  setActivePinia(createPinia())
  updateRelay.mockClear()
  const store = useRelaysStore()
  listRelays.mockClear()

  await store.saveRelay('relay-1', { enabled: false })

  expect(updateRelay).toHaveBeenCalledTimes(1)
  const options = updateRelay.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(options.body).toEqual({ enabled: false })
  expect('role' in options.body).toBe(false)
})

// Tester additions: the page-level save path (a full patch built by
// `createRelayPatchRequest`, exactly what RelaysPage submits) must carry no
// host-role surface, and the host-role SDK function must never be touched by
// this store.

const relaySetHostRole = mock(async () => ({ data: undefined }))
const fullActualAdminApi = await import('../src/generated/admin-api')
mock.module('../src/generated/admin-api', () => ({
  ...fullActualAdminApi,
  relaySetHostRole,
}))

test('the full page patch body carries the connection fields and no role', async () => {
  const { createRelayPatchRequest, createEmptyRelayForm } =
    await import('../src/admin-mappers/forms/relay')
  setActivePinia(createPinia())
  listRelays.mockClear()
  updateRelay.mockClear()
  relaySetHostRole.mockClear()
  const store = useRelaysStore()

  const form = {
    ...createEmptyRelayForm(),
    relay_id: 'relay-1',
    name: 'east',
    relay_url: 'wss://relay.example.com/ws/worker',
    enabled: false,
    tls_mode: 'server',
    bridge_encryption_mode: 'required',
  } as Parameters<typeof createRelayPatchRequest>[0]
  const patch = createRelayPatchRequest(form)

  await store.saveRelay('relay-1', patch)

  expect(updateRelay).toHaveBeenCalledTimes(1)
  const options = updateRelay.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(options.body.enabled).toBe(false)
  expect('role' in options.body).toBe(false)
  expect('pending_role' in options.body).toBe(false)
  // The worker-admin relay list never reaches the host role endpoint.
  expect(relaySetHostRole).not.toHaveBeenCalled()
})
