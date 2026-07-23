import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  createBillingAdjustment,
  createPriceRule,
  fetchBillingChargeDetail,
  fetchBillingCharges,
  fetchBillingCsv,
  fetchBillingEndpoints,
  fetchBillingPriceRules,
  fetchBillingSummary,
  repriceUnpriced,
  setPriceRuleEnabled,
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
import { useStoredPageSize } from '../table-pagination'

export const useBillingStore = defineStore('billing', () => {
  const summary = ref<BillingSummaryResponse | null>(null)
  const charges = ref<BillingChargeResponse[]>([])
  const total = ref(0)
  const priceRules = ref<BillingPriceRuleResponse[]>([])
  const endpoints = ref<Awaited<ReturnType<typeof fetchBillingEndpoints>>['endpoints']>([])
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
      const page = await fetchBillingCharges(filters.value, first.value, rows.value)
      charges.value = page.charges
      total.value = page.total
    } finally {
      chargesLoading.value = false
    }
  }

  async function refreshPriceRules(): Promise<void> {
    priceRulesLoading.value = true
    try {
      priceRules.value = await fetchBillingPriceRules()
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

  async function addAdjustment(
    chargeId: number,
    amount: string,
    reason: string,
  ): Promise<void> {
    await createBillingAdjustment(chargeId, { amount, reason })
    await Promise.all([refreshSummary(), refreshCharges()])
    await openDetail(chargeId)
  }

  async function savePriceRule(body: BillingPriceRuleRequest): Promise<void> {
    await createPriceRule(body)
    await refreshPriceRules()
    await refreshSummary()
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
    const url = URL.createObjectURL(new Blob([`\ufeff${csv}`], { type: 'text/csv;charset=utf-8' }))
    const link = document.createElement('a')
    link.href = url
    link.download = filename
    link.click()
    URL.revokeObjectURL(url)
  }

  return {
    addAdjustment,
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
    priceRulesLoading,
    refresh,
    refreshCharges,
    reprice,
    savePriceRule,
    summary,
    togglePriceRule,
    total,
    rows,
  }
})
