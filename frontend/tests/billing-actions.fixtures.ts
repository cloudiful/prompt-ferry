import { mock } from 'bun:test'

export const createPriceRule = mock(async (_body: unknown) => ({}))
export const deletePriceRule = mock(async (_id: string) => {})
export const repriceUnpriced = mock(async () => ({ repriced: 3 }))
export const setPriceRuleEnabled = mock(
  async (_id: string, _enabled: boolean) => ({}),
)
export const updatePriceRule = mock(async (_id: string, _body: unknown) => ({}))

export const fetchBillingChargeDetail = mock(async (_id: number) => ({
  charge: {},
}))
export const fetchBillingCharges = mock(async () => ({
  charges: [],
  first: 0,
  rows: 25,
  total: 0,
}))
export const fetchBillingCsv = mock(async () => '')
export const fetchBillingEndpoints = mock(async () => ({ endpoints: [] }))
export const fetchBillingPriceRules = mock(async () => ({
  first: 0,
  rules: [],
  rows: 10,
  total: 0,
}))
export const fetchBillingSummary = mock(async () => ({}))

const apiModule = () => ({
  createPriceRule,
  deletePriceRule,
  fetchBillingChargeDetail,
  fetchBillingCharges,
  fetchBillingCsv,
  fetchBillingEndpoints,
  fetchBillingPriceRules,
  fetchBillingSummary,
  repriceUnpriced,
  setPriceRuleEnabled,
  updatePriceRule,
})

export async function loadBillingActions() {
  mock.module('../src/api/billing', apiModule)
  return import('../src/stores/billing-actions')
}

export async function loadBillingStore() {
  mock.module('../src/api/billing', apiModule)
  const { createPinia, setActivePinia } = await import('pinia')
  const { useBillingStore } = await import('../src/stores/billing')
  setActivePinia(createPinia())
  clearBillingMocks()
  return useBillingStore()
}

export function clearBillingMocks(): void {
  for (const fn of [
    createPriceRule,
    deletePriceRule,
    repriceUnpriced,
    setPriceRuleEnabled,
    updatePriceRule,
    fetchBillingChargeDetail,
    fetchBillingCharges,
    fetchBillingCsv,
    fetchBillingEndpoints,
    fetchBillingPriceRules,
    fetchBillingSummary,
  ]) {
    fn.mockClear()
  }
}

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}

export function freshRefreshes(
  onRefresh: () => Promise<void> = async () => {},
) {
  const refreshes = { charges: 0, priceRules: 0, summary: 0 }
  return {
    refreshes,
    deps: {
      refreshCharges: mock(async () => {
        refreshes.charges += 1
        await onRefresh()
      }),
      refreshPriceRules: mock(async () => {
        refreshes.priceRules += 1
        await onRefresh()
      }),
      refreshSummary: mock(async () => {
        refreshes.summary += 1
        await onRefresh()
      }),
    },
  }
}
