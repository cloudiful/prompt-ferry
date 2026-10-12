import { expect, test } from 'bun:test'

import {
  clearUsageHistory,
  deferred,
  fetchUsageOverview,
  loadUsageStore,
} from './usage-actions.fixtures'

test('usage maintenance and perspective have independent keys but maintenance actions exclude each other', async () => {
  const store = await loadUsageStore()
  const clearGate = deferred<void>()
  const overviewGate = deferred<unknown>()
  clearUsageHistory.mockImplementationOnce(async () => {
    await clearGate.promise
    return { deleted: 1 }
  })
  fetchUsageOverview.mockImplementationOnce(
    async () => overviewGate.promise as never,
  )

  const clearing = store.clearHistory({} as never)
  const changingPerspective = store.setAiPerspective('upstream')
  expect(store.maintenancePending).toBe(true)
  expect(store.perspectivePending).toBe(true)
  expect(await store.pruneHistory()).toEqual({ executed: false })

  overviewGate.resolve({ summary: { request_count: 1 } })
  await changingPerspective
  expect(store.maintenancePending).toBe(true)
  expect(store.perspectivePending).toBe(false)
  clearGate.resolve()
  await clearing
  expect(store.maintenancePending).toBe(false)
})
