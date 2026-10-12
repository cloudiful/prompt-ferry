import { expect, test } from 'bun:test'
import {
  deferred,
  deferredResponse,
  fresh,
  keyListResponse,
  listClientKeys,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
  type KeyListResponse,
} from './api-key-selection.fixtures'

test('a label commit and a delete of one key share a single gate', async () => {
  const store = fresh()
  const gate = deferred()
  meUpdateClientKey.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { enabled: true, key_id: 'k', label: 'renamed' } }
  })

  // The Enter-key commit and a delete of the same key must not interleave.
  const saving = store.saveKey('k', { label: 'renamed' })
  expect(store.isPending('clientkey:k')).toBe(true)
  expect(store.clientKeyPendingActions).toEqual({ k: 'save' })

  expect(await store.saveKey('k', { label: 'again' })).toEqual({
    executed: false,
  })
  expect(await store.removeKey('k')).toEqual({ executed: false })
  expect(meDeleteClientKey).not.toHaveBeenCalled()

  gate.resolve()
  await saving
  expect(store.isPending('clientkey:k')).toBe(false)
})

test('the create gate is scoped to the owning user', async () => {
  const store = fresh()
  const gate = deferred()
  meCreateClientKey.mockImplementationOnce(async () => {
    await gate.promise
    return {
      data: { key: { enabled: true, key_id: 'k', label: 'x' }, secret: 's' },
    }
  })

  const mine = store.createKey('first')
  expect(store.isPending('clientkey:create:0')).toBe(true)
  expect(await store.createKey('again')).toEqual({ executed: false })
  // Another user's create is an independent action.
  expect(await store.createKey('theirs', 7)).toMatchObject({ executed: true })

  gate.resolve()
  await mine
})

test('the owning user is captured before the write, so a switch cannot redirect it', async () => {
  const store = fresh()
  const gate = deferred()
  meUpdateClientKey.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { enabled: true, key_id: 'k', label: 'renamed' } }
  })

  const saving = store.saveKey('k', { label: 'renamed' })
  // The operator switches accounts while the write is in flight.
  store.selectedUserId = 7
  gate.resolve()
  await saving

  // The reload followed the user the write actually targeted.
  expect(meListClientKeys).toHaveBeenCalledTimes(1)
  expect(listClientKeys).not.toHaveBeenCalled()
})

test('a previous-user write cannot replace or clear the selected user view', async () => {
  const store = fresh()
  const gate = deferred()
  const oldOwnerReload = deferredResponse<KeyListResponse>()
  const oldOwnerReloadStarted = deferred()
  const currentUserReload = deferredResponse<KeyListResponse>()
  meUpdateClientKey.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { enabled: true, key_id: 'k', label: 'renamed' } }
  })
  listClientKeys
    .mockImplementationOnce(async () => keyListResponse(['user7-key']))
    .mockImplementationOnce(async () => currentUserReload.promise)
  meListClientKeys.mockImplementationOnce(async () => {
    oldOwnerReloadStarted.resolve()
    return oldOwnerReload.promise
  })

  const saving = store.saveKey('k', { label: 'renamed' })
  await store.changeSelectedUser(7)
  expect(store.keys.map((key) => key.key_id)).toEqual(['user7-key'])
  const selectedUserLoading = store.loadKeys(undefined, 10, 10)
  gate.resolve()
  await oldOwnerReloadStarted.promise
  expect(store.loading).toBe(true)

  oldOwnerReload.resolve(keyListResponse(['old-owner-key']))
  await saving
  expect(store.selectedUserId).toBe(7)
  expect(store.keys.map((key) => key.key_id)).toEqual(['user7-key'])
  expect(store.total).toBe(1)
  expect(store.loading).toBe(true)

  currentUserReload.resolve(keyListResponse(['user7-latest'], 10, 10, 20))
  await selectedUserLoading
  expect(store.keys.map((key) => key.key_id)).toEqual(['user7-latest'])
  expect(store.first).toBe(10)
  expect(store.rows).toBe(10)
  expect(store.total).toBe(20)
  expect(store.loading).toBe(false)
})

test('the payload is captured before the request is claimed', async () => {
  const store = fresh()
  const gate = deferred()
  meUpdateClientKey.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { enabled: true, key_id: 'k', label: 'renamed' } }
  })

  const body = { label: 'first' }
  const saving = store.saveKey('k', body)

  // An edit made while the request is in flight must not ride along with it.
  body.label = 'edited while saving'
  gate.resolve()
  await saving

  const sent = meUpdateClientKey.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect(sent.body).toEqual({ label: 'first' })
})

test('a failed write releases its key so the retry is not blocked', async () => {
  const store = fresh()
  meUpdateClientKey.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(store.saveKey('k', { label: 'x' })).rejects.toThrow('rejected')
  expect(store.isPending('clientkey:k')).toBe(false)

  await expect(store.saveKey('k', { label: 'x' })).resolves.toMatchObject({
    executed: true,
  })
})
