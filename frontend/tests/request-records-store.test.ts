import { expect, test } from 'bun:test'

import {
  clearUsageHistory,
  deferred,
  fetchUsageOverview,
  fetchUsageRecords,
  loadUsageStore,
  pruneUsageHistory,
} from './usage-actions.fixtures'

test('clear and prune share one gate, snapshot the form, and stay pending through refresh', async () => {
  const store = await loadUsageStore()
  const clearGate = deferred<void>()
  const refreshGate = deferred<void>()
  let submitted: unknown
  clearUsageHistory.mockImplementationOnce(async (form) => {
    submitted = form
    await clearGate.promise
    return { deleted: 2 }
  })
  fetchUsageRecords.mockImplementationOnce(async () => {
    await refreshGate.promise
    return { first: 0, rows: [], rowsPerPage: 25, total: 0 }
  })
  const form = {
    delete_all: false,
    end_at: '2026-10-12T00:00',
    scope: 'current_user',
    start_at: '2026-10-11T00:00',
    user_id: null,
  }

  const clearing = store.clearHistory(form as never)
  form.scope = 'all_users'
  expect(store.maintenancePending).toBe(true)
  expect(store.maintenanceAction).toBe('clear')
  expect(await store.pruneHistory()).toEqual({ executed: false })
  expect(pruneUsageHistory).not.toHaveBeenCalled()

  clearGate.resolve()
  await Promise.resolve()
  await Promise.resolve()
  expect(submitted).toMatchObject({ scope: 'current_user' })
  expect(store.maintenancePending).toBe(true)

  refreshGate.resolve()
  await expect(clearing).resolves.toMatchObject({
    executed: true,
    value: { deleted: 2 },
  })
  expect(store.maintenancePending).toBe(false)
  expect(fetchUsageOverview).toHaveBeenCalledTimes(1)

  await expect(store.pruneHistory()).resolves.toMatchObject({
    executed: true,
    value: 1,
  })
  expect(pruneUsageHistory).toHaveBeenCalledTimes(1)
  expect(store.maintenancePending).toBe(false)
})

test('perspective writes reject a second selection and ignore a stale overview response', async () => {
  const store = await loadUsageStore()
  const oldOverview = deferred<unknown>()
  const currentOverview = deferred<unknown>()
  const previous = { summary: { request_count: 1 } }
  const latest = { summary: { request_count: 2 } }
  fetchUsageOverview
    .mockImplementationOnce(async () => oldOverview.promise as never)
    .mockImplementationOnce(async () => currentOverview.promise as never)

  const changing = store.setAiPerspective('upstream')
  expect(store.perspectivePending).toBe(true)
  expect(store.aiPerspective).toBe('upstream')
  expect(await store.setAiPerspective('model')).toEqual({ executed: false })
  expect(store.aiPerspective).toBe('upstream')

  store.range = '7d' as never
  const refreshing = store.refreshOverview()
  currentOverview.resolve(latest)
  await refreshing
  expect(store.overview).toMatchObject(latest)

  oldOverview.resolve(previous)
  await changing
  expect(store.overview).toMatchObject(latest)
  expect(store.perspectivePending).toBe(false)
})
