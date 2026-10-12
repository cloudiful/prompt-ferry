import { expect, test } from 'bun:test'
import {
  SELF_USER_ID,
  deferredResponse,
  fresh,
  keyListResponse,
  listClientKeys,
  meListClientKeys,
  type KeyListResponse,
} from './api-key-selection.fixtures'

test('an earlier same-owner query stays stale after switching away and back', async () => {
  const store = fresh()
  const firstOwnerPage = deferredResponse<KeyListResponse>()
  const otherOwnerPage = deferredResponse<KeyListResponse>()
  const latestOwnerPage = deferredResponse<KeyListResponse>()
  meListClientKeys
    .mockImplementationOnce(async () => firstOwnerPage.promise)
    .mockImplementationOnce(async () => latestOwnerPage.promise)
  listClientKeys.mockImplementationOnce(async () => otherOwnerPage.promise)

  const firstOwnerLoad = store.loadKeys()
  const otherOwnerLoad = store.changeSelectedUser(7)
  const latestOwnerLoad = store.changeSelectedUser(SELF_USER_ID)

  firstOwnerPage.resolve(keyListResponse(['obsolete-owner-key']))
  await firstOwnerLoad
  expect(store.loading).toBe(true)
  expect(store.keys).toEqual([])

  otherOwnerPage.resolve(keyListResponse(['other-owner-key']))
  await otherOwnerLoad
  expect(store.loading).toBe(true)
  expect(store.keys).toEqual([])

  latestOwnerPage.resolve(keyListResponse(['current-owner-key']))
  await latestOwnerLoad
  expect(store.selectedUserId).toBe(SELF_USER_ID)
  expect(store.keys.map((key) => key.key_id)).toEqual(['current-owner-key'])
  expect(store.loading).toBe(false)
})

test('a stale list failure cannot reject or clear the selected user load', async () => {
  const store = fresh()
  const obsoletePage = deferredResponse<KeyListResponse>()
  const currentPage = deferredResponse<KeyListResponse>()
  meListClientKeys.mockImplementationOnce(async () => obsoletePage.promise)
  listClientKeys.mockImplementationOnce(async () => currentPage.promise)

  const obsoleteLoad = store.loadKeys()
  const currentLoad = store.changeSelectedUser(7)
  obsoletePage.reject(new Error('obsolete request failed'))

  await expect(obsoleteLoad).resolves.toEqual([])
  expect(store.loading).toBe(true)
  expect(store.keys).toEqual([])

  currentPage.resolve(keyListResponse(['current-user-key']))
  await currentLoad
  expect(store.keys.map((key) => key.key_id)).toEqual(['current-user-key'])
  expect(store.loading).toBe(false)
})

test('a current list failure clears loading and can be retried', async () => {
  const store = fresh()
  store.selectedUserId = 7
  const failure = new Error('current request failed')
  listClientKeys
    .mockImplementationOnce(async () => {
      throw failure
    })
    .mockImplementationOnce(async () => keyListResponse(['retry-key']))

  await expect(store.loadKeys()).rejects.toBe(failure)
  expect(store.loading).toBe(false)

  await store.loadKeys()
  expect(store.keys.map((key) => key.key_id)).toEqual(['retry-key'])
  expect(store.loading).toBe(false)
})

test('the latest page query owns pagination and stale responses cannot replace it', async () => {
  const store = fresh()
  store.selectedUserId = 7
  const olderPage = deferredResponse<KeyListResponse>()
  const newerPage = deferredResponse<KeyListResponse>()
  listClientKeys
    .mockImplementationOnce(async () => olderPage.promise)
    .mockImplementationOnce(async () => newerPage.promise)

  const olderLoad = store.loadKeys(undefined, 20, 10)
  const newerLoad = store.loadKeys(undefined, 0, 20)
  const olderQuery = listClientKeys.mock.calls[0]?.[0] as {
    query: { first: number; rows: number }
  }
  const newerQuery = listClientKeys.mock.calls[1]?.[0] as {
    query: { first: number; rows: number }
  }
  expect(olderQuery.query).toEqual({ first: 20, rows: 10 })
  expect(newerQuery.query).toEqual({ first: 0, rows: 20 })
  expect(store.first).toBe(0)
  expect(store.rows).toBe(20)
  expect(store.loading).toBe(true)

  olderPage.resolve(keyListResponse(['older-page-key'], 20, 10, 60))
  await olderLoad
  expect(store.first).toBe(0)
  expect(store.rows).toBe(20)
  expect(store.keys).toEqual([])
  expect(store.loading).toBe(true)

  newerPage.resolve(keyListResponse(['newer-page-key'], 0, 20, 60))
  await newerLoad
  expect(store.keys.map((key) => key.key_id)).toEqual(['newer-page-key'])
  expect(store.total).toBe(60)
  expect(store.first).toBe(0)
  expect(store.rows).toBe(20)
  expect(store.loading).toBe(false)
})

test('an out-of-range page correction keeps the current view loading', async () => {
  const store = fresh()
  store.selectedUserId = 7
  const correctedPage = deferredResponse<KeyListResponse>()
  listClientKeys
    .mockImplementationOnce(async () => keyListResponse([], 40, 20, 30))
    .mockImplementationOnce(async () => correctedPage.promise)

  const loading = store.loadKeys(undefined, 40, 20)
  await Promise.resolve()
  await Promise.resolve()

  const correctedQuery = listClientKeys.mock.calls[1]?.[0] as {
    query: { first: number; rows: number }
  }
  expect(correctedQuery.query).toEqual({ first: 20, rows: 20 })
  expect(store.first).toBe(20)
  expect(store.rows).toBe(20)
  expect(store.loading).toBe(true)

  correctedPage.resolve(keyListResponse(['corrected-page-key'], 20, 20, 30))
  await loading
  expect(store.keys.map((key) => key.key_id)).toEqual(['corrected-page-key'])
  expect(store.total).toBe(30)
  expect(store.loading).toBe(false)
})
