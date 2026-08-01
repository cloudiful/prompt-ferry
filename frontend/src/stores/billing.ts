import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
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
} from '../api/billing'
import type {
  BillingChargeDetailResponse,
  BillingChargeResponse,
  BillingPriceRuleRequest,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
} from '../generated/admin-api'
import {
  BILLING_PAGE_SIZE_OPTIONS,
  type BillingChargeFilters,
  endOfDate,
  startOfDate,
} from '../models/billing'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'

export const useBillingStore = defineStore('billing', () => {
  const summary = ref<BillingSummaryResponse | null>(null)
  const charges = ref<BillingChargeResponse[]>([])
  const total = ref(0)
  const priceRules = ref<BillingPriceRuleResponse[]>([])
  const priceRuleFirst = ref(0)
  const priceRuleRows = useStoredPageSize(
    'billing-price-rules',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const priceRuleTotal = ref(0)
  const endpoints = ref<
    Awaited<ReturnType<typeof fetchBillingEndpoints>>['endpoints']
  >([])
  const filters = ref<BillingChargeFilters>({})
  const first = ref(0)
  const rows = useStoredPageSize('billing', 25, BILLING_PAGE_SIZE_OPTIONS)
  const loading = ref(false)
  const chargesLoading = ref(false)
  const detailLoading = ref(false)
  const priceRulesLoading = ref(false)
  const detail = ref<BillingChargeDetailResponse | null>(null)

  async function refresh(includeAdmin: boolean): Promise<void> {
    loading.value = true
    try {
      const tasks: Promise<unknown>[] = [refreshSummary(), refreshCharges()]
      if (includeAdmin) tasks.push(refreshPriceRules(), refreshEndpoints())
      await Promise.all(tasks)
    } finally {
      loading.value = false
    }
  }

  async function refreshSummary(): Promise<void> {
    summary.value = await fetchBillingSummary({
      ...filters.value,
    })
  }

  async function refreshCharges(
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<void> {
    first.value = nextFirst
    rows.value = nextRows
    chargesLoading.value = true
    try {
      const page = await fetchBillingCharges(
        filters.value,
        first.value,
        rows.value,
      )
      charges.value = page.charges
      total.value = page.total
      first.value = page.first
      rows.value = page.rows
      if (
        charges.value.length === 0 &&
        total.value > 0 &&
        first.value >= total.value
      ) {
        const previousFirst =
          Math.floor((total.value - 1) / rows.value) * rows.value
        await refreshCharges(previousFirst, rows.value)
      }
    } finally {
      chargesLoading.value = false
    }
  }

  async function refreshPriceRules(
    nextFirst = priceRuleFirst.value,
    nextRows = priceRuleRows.value,
  ): Promise<void> {
    priceRuleFirst.value = nextFirst
    priceRuleRows.value = nextRows
    priceRulesLoading.value = true
    try {
      const page = await fetchBillingPriceRules(
        priceRuleFirst.value,
        priceRuleRows.value,
      )
      priceRules.value = page.rules
      priceRuleTotal.value = page.total
      priceRuleFirst.value = page.first
      priceRuleRows.value = page.rows
      if (
        priceRules.value.length === 0 &&
        priceRuleTotal.value > 0 &&
        priceRuleFirst.value >= priceRuleTotal.value
      ) {
        const previousFirst =
          Math.floor((priceRuleTotal.value - 1) / priceRuleRows.value) *
          priceRuleRows.value
        await refreshPriceRules(previousFirst, priceRuleRows.value)
      }
    } finally {
      priceRulesLoading.value = false
    }
  }

  async function refreshEndpoints(): Promise<void> {
    endpoints.value = (await fetchBillingEndpoints()).endpoints
  }

  async function applyPeriod(start: string, end: string): Promise<void> {
    configurePeriod(start, end)
    first.value = 0
    await Promise.all([refreshSummary(), refreshCharges(0, rows.value)])
  }

  function configurePeriod(start: string, end: string): void {
    filters.value = {
      ...filters.value,
      start_at: startOfDate(start),
      end_at: endOfDate(end),
    }
  }

  function configureFilters(next: BillingChargeFilters): void {
    filters.value = { ...next }
  }

  async function applyFilters(next: BillingChargeFilters): Promise<void> {
    filters.value = { ...next }
    first.value = 0
    await Promise.all([refreshSummary(), refreshCharges(0, rows.value)])
  }

  async function openDetail(chargeId: number): Promise<void> {
    detailLoading.value = true
    try {
      detail.value = await fetchBillingChargeDetail(chargeId)
    } finally {
      detailLoading.value = false
    }
  }

  async function savePriceRule(
    body: BillingPriceRuleRequest,
    priceRuleId?: string,
  ): Promise<void> {
    if (priceRuleId) await updatePriceRule(priceRuleId, body)
    else await createPriceRule(body)
    await refreshPriceRules()
    await refreshSummary()
  }

  async function removePriceRule(priceRuleId: string): Promise<void> {
    await deletePriceRule(priceRuleId)
    await Promise.all([refreshPriceRules(), refreshSummary(), refreshCharges()])
  }

  async function togglePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    await setPriceRuleEnabled(rule.price_rule_id, !rule.enabled)
    await refreshPriceRules()
  }

  async function reprice(): Promise<number> {
    const result = await repriceUnpriced()
    await Promise.all([refreshSummary(), refreshCharges(), refreshPriceRules()])
    return result.repriced
  }

  async function downloadCsv(
    kind: 'details' | 'monthly',
    filename: string,
  ): Promise<void> {
    const csv = await fetchBillingCsv(kind, filters.value)
    const url = URL.createObjectURL(
      new Blob([`\ufeff${csv}`], { type: 'text/csv;charset=utf-8' }),
    )
    const link = document.createElement('a')
    link.href = url
    link.download = filename
    link.click()
    URL.revokeObjectURL(url)
  }

  return {
    applyFilters,
    applyPeriod,
    charges,
    chargesLoading,
    configurePeriod,
    configureFilters,
    detail,
    detailLoading,
    downloadCsv,
    endpoints,
    filters,
    first,
    loading,
    openDetail,
    priceRules,
    priceRuleFirst,
    priceRuleRows,
    priceRuleTotal,
    priceRulesLoading,
    refresh,
    refreshCharges,
    reprice,
    removePriceRule,
    savePriceRule,
    summary,
    togglePriceRule,
    total,
    rows,
  }
})
