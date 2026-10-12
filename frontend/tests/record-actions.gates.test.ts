import { expect, test } from 'bun:test'

import { deferred, freshHarness } from './record-actions.fixtures'

/**
 * Which key excludes what, across every record-scoped domain.
 *
 * These cases are about the gate itself: which operations collide, which stay
 * independent, and how long a key is held. The completion contract — what a
 * page reports and what it may close — is `record-actions.lifecycle.test.ts`.
 */

test('mcp: a save and a delete of one server share a gate, and the probe does not', async () => {
  const h = await freshHarness()
  const mcp = h.actions as unknown as {
    removeServer: (id: string) => Promise<{ executed: boolean }>
    runTest: (id: string) => Promise<{ executed: boolean }>
    saveServer: (id: string | null, body: unknown) => Promise<unknown>
  }

  // The save is held open, so the gate is still claimed when the refused
  // delete and the independent probe both land.
  const gate = deferred()
  h.api.updateMcpServer.mockImplementationOnce(async () => {
    await gate.promise
    return { server_id: 'server-a' }
  })
  const saving = mcp.saveServer('server-a', { name: 'renamed' })
  expect(h.guard.isPending('mcp:server-a')).toBe(true)

  expect(await mcp.removeServer('server-a')).toEqual({ executed: false })
  // A refused delete reaches neither the API nor the reload.
  expect(h.api.deleteMcpServer).not.toHaveBeenCalled()
  expect(h.reloads.mcp).toBe(0)

  // The connectivity probe has its own namespace, so it never blocks a write.
  expect(await mcp.runTest('server-a')).toMatchObject({ executed: true })
  expect(h.api.testMcpServer).toHaveBeenCalledTimes(1)

  gate.resolve()
  await saving
  expect(h.guard.isPending('mcp:server-a')).toBe(false)
})

test('mcp: another server stays writable while one is being written', async () => {
  const h = await freshHarness()
  const mcp = h.actions as unknown as {
    saveServer: (id: string, body: unknown) => Promise<{ executed: boolean }>
  }

  const saving = mcp.saveServer('server-a', { name: 'first' })
  expect(await mcp.saveServer('server-b', { name: 'other' })).toMatchObject({
    executed: true,
  })
  expect(h.api.updateMcpServer).toHaveBeenCalledTimes(2)

  await saving
})

test('mcp: a create is gated under its own key', async () => {
  const h = await freshHarness()
  const mcp = h.actions as unknown as {
    saveServer: (
      id: string | null,
      body: unknown,
    ) => Promise<{ executed: boolean }>
  }

  const creating = mcp.saveServer(null, { name: 'fresh' })
  expect(h.guard.isPending('mcp:create')).toBe(true)
  expect(await mcp.saveServer(null, { name: 'again' })).toEqual({
    executed: false,
  })
  // A create is not a record the list can act on, so it never writes one.
  expect(await mcp.saveServer('server-a', { name: 'x' })).toMatchObject({
    executed: true,
  })

  await creating
})

test('users: save, delete, and password reset of one user are mutually exclusive', async () => {
  const h = await freshHarness()
  const users = h.actions as unknown as {
    removeUser: (id: number) => Promise<{ executed: boolean }>
    resetUserPassword: (
      id: number,
      pw: string,
    ) => Promise<{ executed: boolean }>
    saveUser: (id: number, body: unknown) => Promise<{ executed: boolean }>
  }

  const saving = users.saveUser(1, { display_name: 'Alice' })
  expect(await users.removeUser(1)).toEqual({ executed: false })
  expect(await users.resetUserPassword(1, 'hunter2')).toEqual({
    executed: false,
  })
  expect(h.api.deleteUser).not.toHaveBeenCalled()
  expect(h.api.resetPassword).not.toHaveBeenCalled()

  // A different user is unaffected.
  expect(await users.saveUser(2, { display_name: 'Bob' })).toMatchObject({
    executed: true,
  })

  await saving
})

test('users: a create is gated separately and the id is the record id', async () => {
  const h = await freshHarness()
  const users = h.actions as unknown as {
    createNewUser: (body: unknown) => Promise<{ executed: boolean }>
    saveUser: (id: number, body: unknown) => Promise<{ executed: boolean }>
  }

  const creating = users.createNewUser({ login_name: 'newbie' })
  expect(h.guard.isPending('user:create')).toBe(true)
  expect(await users.createNewUser({ login_name: 'other' })).toEqual({
    executed: false,
  })
  // User ids are numbers, and the key must still be a string namespace.
  expect(await users.saveUser(1, { display_name: 'Alice' })).toMatchObject({
    executed: true,
  })

  await creating
})

test('client keys: a label commit and a delete of one key share a gate', async () => {
  const h = await freshHarness()
  const keys = h.actions as unknown as {
    removeKey: (id: string) => Promise<{ executed: boolean }>
    saveKey: (id: string, body: unknown) => Promise<{ executed: boolean }>
  }

  const saving = keys.saveKey('key-a', { label: 'renamed' })
  // The Enter-key commit and a delete must not interleave.
  expect(await keys.saveKey('key-a', { label: 'again' })).toEqual({
    executed: false,
  })
  expect(await keys.removeKey('key-a')).toEqual({ executed: false })
  expect(h.api.deleteClientKey).not.toHaveBeenCalled()

  expect(await keys.saveKey('key-b', { label: 'other' })).toMatchObject({
    executed: true,
  })

  await saving
})

test('client keys: the create gate is scoped to the owning user', async () => {
  const h = await freshHarness()
  const keys = h.actions as unknown as {
    createKey: (
      label: string,
      userId?: number,
    ) => Promise<{ executed: boolean }>
  }

  // Two users' create dialogs are independent actions, so neither blocks the
  // other — only a second create for the same user does.
  const mine = keys.createKey('first', 0)
  expect(h.guard.isPending('clientkey:create:0')).toBe(true)
  expect(await keys.createKey('again', 0)).toEqual({ executed: false })
  expect(await keys.createKey('theirs', 7)).toMatchObject({ executed: true })

  await mine
})

test('relays: reconnect shares the record gate with save and delete', async () => {
  const h = await freshHarness()
  const relays = h.actions as unknown as {
    reconnectRelay: (id: string) => Promise<{ executed: boolean }>
    removeRelay: (id: string) => Promise<{ executed: boolean }>
    saveRelay: (
      id: string | null,
      body: unknown,
    ) => Promise<{ executed: boolean }>
  }

  // A reconnect is a write in its own right, not a mere refresh.
  const reconnecting = relays.reconnectRelay('relay-a')
  expect(await relays.removeRelay('relay-a')).toEqual({ executed: false })
  expect(await relays.saveRelay('relay-a', { name: 'renamed' })).toEqual({
    executed: false,
  })
  expect(h.api.deleteRelay).not.toHaveBeenCalled()
  expect(h.api.updateRelay).not.toHaveBeenCalled()

  expect(await relays.saveRelay('relay-b', { name: 'other' })).toMatchObject({
    executed: true,
  })

  await reconnecting
  expect(h.guard.isPending('relay:relay-a')).toBe(false)
})

test('relay host: role, settings, and restart share one gate', async () => {
  const h = await freshHarness()
  const host = h.actions as unknown as {
    requestRestart: () => Promise<{ executed: boolean }>
    saveRole: (role: string) => Promise<{ executed: boolean }>
    saveSettings: (bind: string) => Promise<{ executed: boolean }>
  }

  // All three end in the same process restarting, so two of them at once would
  // describe a configuration the operator never set.
  const saving = host.saveRole('relay')
  expect(await host.saveSettings('0.0.0.0:9000')).toEqual({ executed: false })
  expect(await host.requestRestart()).toEqual({ executed: false })
  expect(h.api.relaySetSettings).not.toHaveBeenCalled()
  expect(h.api.relayRequestRestart).not.toHaveBeenCalled()

  await saving
  expect(h.guard.isPending('host:local')).toBe(false)
})
