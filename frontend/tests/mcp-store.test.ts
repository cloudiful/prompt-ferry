import { expect, mock, test } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

const createMcpServer = mock(async () => ({
  data: { server_id: 'server-new' },
}))
const deleteMcpServer = mock(async () => ({ data: null }))
const listMcpServers = mock(async () => ({
  data: { first: 0, rows: 10, servers: [], total: 0 },
}))
const testMcpServer = mock(async () => ({
  data: { prompts: [], resources: [], tools: [] },
}))
const updateMcpServer = mock(async () => ({ data: { server_id: 'a' } }))

// Keep every other generated export intact: `mock.module` is process-global and
// would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  createMcpServer,
  deleteMcpServer,
  listMcpServers,
  testMcpServer,
  updateMcpServer,
}))

// The store labels its workspace view, and `useLocale` reaches vue-i18n, which
// only works inside a component setup.
mock.module('../src/composables/useLocale', () => ({
  useLocale: () => ({ t: (key: string) => key }),
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useMcpStore } = await import('../src/stores/mcp')

/** A gate the tests open by hand, so "in flight" is really in flight. */
function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

const allMocks = [
  createMcpServer,
  deleteMcpServer,
  listMcpServers,
  testMcpServer,
  updateMcpServer,
]

function fresh() {
  setActivePinia(createPinia())
  for (const item of allMocks) item.mockClear()
  return useMcpStore()
}

test('the store owns one guard shared by every server write', async () => {
  const store = fresh()
  const gate = deferred()
  updateMcpServer.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { server_id: 'a' } }
  })

  const saving = store.saveServer('a', { name: 'renamed' })

  // The key is claimed at the owner, before the request resolves.
  expect(store.isPending('mcp:a')).toBe(true)
  expect(store.mcpPendingActions).toEqual({ a: 'save' })

  // Every other write of that server is refused, and the probe is not.
  expect(await store.removeServer('a')).toEqual({ executed: false })
  expect(await store.saveServer('a', { name: 'again' })).toEqual({
    executed: false,
  })
  expect(await store.runTest('a')).toMatchObject({ executed: true })
  expect(deleteMcpServer).not.toHaveBeenCalled()

  gate.resolve()
  await saving
  expect(store.isPending('mcp:a')).toBe(false)
})

test('the store holds its key through the reload that follows the write', async () => {
  const store = fresh()
  await store.refresh()

  const saving = store.saveServer('a', { name: 'renamed' })
  // The write is done; the refreshed list has not landed yet.
  expect(updateMcpServer).toHaveBeenCalledTimes(1)
  expect(listMcpServers).toHaveBeenCalledTimes(1)
  expect(store.isPending('mcp:a')).toBe(true)

  await saving
  expect(store.isPending('mcp:a')).toBe(false)
})

test('an unrelated server stays writable while one is being written', async () => {
  const store = fresh()
  const gate = deferred()
  updateMcpServer.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { server_id: 'a' } }
  })

  const saving = store.saveServer('a', { name: 'renamed' })
  expect(await store.saveServer('b', { name: 'other' })).toMatchObject({
    executed: true,
  })
  expect(store.mcpPendingActions).toEqual({ a: 'save' })

  gate.resolve()
  await saving
})

test('a create is gated under the create key, not a row id', async () => {
  const store = fresh()
  const gate = deferred()
  createMcpServer.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { server_id: 'server-new' } }
  })

  const creating = store.saveServer(null, { name: 'fresh' })
  expect(store.isPending('mcp:create')).toBe(true)
  expect(await store.saveServer(null, { name: 'again' })).toEqual({
    executed: false,
  })
  expect(store.mcpPendingActions).toEqual({ create: 'save' })

  gate.resolve()
  await creating
})

test('a failed write releases its key so the retry is not blocked', async () => {
  const store = fresh()
  updateMcpServer.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(store.saveServer('a', { name: 'x' })).rejects.toThrow('rejected')
  expect(store.isPending('mcp:a')).toBe(false)

  await expect(store.saveServer('a', { name: 'x' })).resolves.toMatchObject({
    executed: true,
  })
})

test('the probe keeps its own namespace and records its result', async () => {
  const store = fresh()
  const gate = deferred()
  testMcpServer.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { prompts: ['p'], resources: ['r'], tools: ['t'] } }
  })

  const probing = store.runTest('a')
  expect(store.testingServerId).toBe('a')
  // The probe never writes a row, so it never appears as a gated row.
  expect(store.mcpPendingActions).toEqual({})
  // A repeat probe of the same server is refused.
  expect(await store.runTest('a')).toEqual({ executed: false })

  gate.resolve()
  const outcome = await probing
  expect(outcome.executed && outcome.value.prompts).toEqual(['p'])
  expect(store.testingServerId).toBe('')
})
