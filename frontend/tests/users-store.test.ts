import { expect, mock, test } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

const createUser = mock(async () => ({ data: { user_id: 9 } }))
const deleteUser = mock(async () => ({ data: null }))
const listUsers = mock(async () => ({
  data: { first: 0, rows: 10, total: 0, users: [] },
}))
const resetPassword = mock(async () => ({ data: null }))
const updateUser = mock(async () => ({ data: { user_id: 1 } }))

// Keep every other generated export intact: `mock.module` is process-global and
// would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  createUser,
  deleteUser,
  listUsers,
  resetPassword,
  updateUser,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useUsersStore } = await import('../src/stores/users')

/** A gate the tests open by hand, so "in flight" is really in flight. */
function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

const allMocks = [createUser, deleteUser, listUsers, resetPassword, updateUser]

function fresh() {
  setActivePinia(createPinia())
  for (const item of allMocks) item.mockClear()
  return useUsersStore()
}

test('save, delete, and reset of one user share a single gate', async () => {
  const store = fresh()
  const gate = deferred()
  updateUser.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { user_id: 1 } }
  })

  const saving = store.saveUser(1, { display_name: 'Alice' })
  expect(store.isPending('user:1')).toBe(true)
  expect(store.userPendingActions).toEqual({ '1': 'save' })

  expect(await store.removeUser(1)).toEqual({ executed: false })
  expect(await store.resetUserPassword(1, 'hunter2')).toEqual({
    executed: false,
  })
  expect(await store.saveUser(1, { display_name: 'again' })).toEqual({
    executed: false,
  })
  // None of the refused writes reaches the API.
  expect(deleteUser).not.toHaveBeenCalled()
  expect(resetPassword).not.toHaveBeenCalled()

  gate.resolve()
  await saving
  expect(store.isPending('user:1')).toBe(false)
})

test('another user is unaffected while one is being written', async () => {
  const store = fresh()
  const gate = deferred()
  updateUser.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { user_id: 1 } }
  })

  const saving = store.saveUser(1, { display_name: 'Alice' })
  await expect(
    store.saveUser(2, { display_name: 'Bob' }),
  ).resolves.toMatchObject({
    executed: true,
  })
  expect(store.userPendingActions).toEqual({ '1': 'save' })

  gate.resolve()
  await saving
})

test('a create is gated under the create key', async () => {
  const store = fresh()
  const gate = deferred()
  createUser.mockImplementationOnce(async () => {
    await gate.promise
    return { data: { user_id: 9 } }
  })

  const creating = store.createNewUser({ login_name: 'newbie' })
  expect(store.isPending('user:create')).toBe(true)
  expect(await store.createNewUser({ login_name: 'other' })).toEqual({
    executed: false,
  })

  gate.resolve()
  await creating
})

test('a save patches both lists from the response', async () => {
  const store = fresh()
  await store.loadPage()

  const outcome = await store.saveUser(1, { display_name: 'Alice' })

  expect(outcome.executed).toBe(true)
  expect(updateUser).toHaveBeenCalledTimes(1)
})

test('a failed write releases its key so the retry is not blocked', async () => {
  const store = fresh()
  updateUser.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(store.saveUser(1, { display_name: 'x' })).rejects.toThrow(
    'rejected',
  )
  expect(store.isPending('user:1')).toBe(false)

  await expect(store.saveUser(1, { display_name: 'x' })).resolves.toMatchObject(
    {
      executed: true,
    },
  )
})
