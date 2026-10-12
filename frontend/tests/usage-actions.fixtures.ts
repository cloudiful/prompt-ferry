import { mock } from 'bun:test'

export const clearUsageHistory = mock(async (_form: unknown) => ({
  deleted: 2,
}))
export const fetchUsageFacets = mock(async () => ({
  dates: [],
  models: [],
  users: [],
}))
export const fetchUsageOverview = mock(async () => null)
export const fetchUsageRecords = mock(async () => ({
  first: 0,
  rows: [],
  rowsPerPage: 25,
  total: 0,
}))
export const pruneUsageHistory = mock(async () => 1)

const apiModule = () => ({
  clearUsageHistory,
  fetchUsageFacets,
  fetchUsageOverview,
  fetchUsageRecords,
  pruneUsageHistory,
})

export async function loadUsageStore() {
  mock.module('../src/stores/usage-api', apiModule)
  mock.module('../src/composables/useLocale', () => ({
    useLocale: () => ({ t: (key: string) => key }),
  }))
  const { createPinia, setActivePinia } = await import('pinia')
  const { useRequestRecordsStore } = await import('../src/stores/usage')
  setActivePinia(createPinia())
  clearUsageMocks()
  return useRequestRecordsStore()
}

export function clearUsageMocks(): void {
  for (const fn of [
    clearUsageHistory,
    fetchUsageFacets,
    fetchUsageOverview,
    fetchUsageRecords,
    pruneUsageHistory,
  ]) {
    fn.mockClear()
  }
}

export function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}
