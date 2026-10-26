import { create } from 'zustand'
import type { QueryClient } from '@tanstack/react-query'
import type {
  BillingChargeDetailResponse,
  BillingChargePageResponse,
  BillingChargeResponse,
  BillingPriceRulePageResponse,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
  ProviderEndpoint,
  User,
} from '@/generated/admin-api'
import { listUserOptions } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
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
} from '@/api/billing'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useSessionStore } from './session'
import { readStorage, writeStorage } from '@/storage'
import { normalizePageSize, pageSizeStorageKey } from '@/models/pagination'
import { capturePayload, recordActionKey } from '@/stores/record-action-types'
import {
  BILLING_CHARGE_PAGE_SIZE_FALLBACK,
  BILLING_CHARGE_PAGE_SIZE_KEY,
  BILLING_PAGE_SIZE_OPTIONS,
  BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK,
  BILLING_PRICE_RULE_PAGE_SIZE_KEY,
  BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  billingFilterQuery,
  billingFilterSignature,
  billingLastPageFirst,
  billingPriceRuleRequest,
  currentMonthDate,
  endOfDate,
  startOfDate,
  type BillingChargeFilters,
  type BillingPriceRuleForm,
} from './billing-model'

export const BILLING_RULE_PREFIX = 'billingrule'
export const BILLING_REPRICE_KEY = 'billing:reprice'

export type BillingIdentity = {
  user_id: number | null
  is_admin: boolean
}

export function billingPriceRuleActionKey(priceRuleId: string | null): string {
  return recordActionKey(BILLING_RULE_PREFIX, priceRuleId)
}

export function billingIdentityKey(identity: BillingIdentity): string {
  return `${identity.user_id ?? ''}:${identity.is_admin ? 'admin' : 'user'}`
}

export const BILLING_SUMMARY_NAMESPACE = ['billing', 'summary'] as const
export const BILLING_CHARGES_NAMESPACE = ['billing', 'charges'] as const
export const BILLING_PRICE_RULES_NAMESPACE = ['billing', 'price-rules'] as const
export const BILLING_ENDPOINTS_NAMESPACE = ['billing', 'endpoints'] as const
export const BILLING_USER_OPTIONS_NAMESPACE = [
  'billing',
  'user-options',
] as const

export function billingSummaryQueryKey(
  identity: BillingIdentity,
  filters: BillingChargeFilters,
) {
  return [
    ...BILLING_SUMMARY_NAMESPACE,
    billingIdentityKey(identity),
    billingFilterSignature(filters),
  ] as const
}

export function billingChargesQueryKey(
  identity: BillingIdentity,
  filters: BillingChargeFilters,
  first: number,
  rows: number,
) {
  return [
    ...BILLING_CHARGES_NAMESPACE,
    billingIdentityKey(identity),
    billingFilterSignature(filters),
    first,
    rows,
  ] as const
}

export function billingPriceRulesQueryKey(
  identity: BillingIdentity,
  first: number,
  rows: number,
) {
  return [
    ...BILLING_PRICE_RULES_NAMESPACE,
    billingIdentityKey(identity),
    first,
    rows,
  ] as const
}

export function billingEndpointsQueryKey(identity: BillingIdentity) {
  return [...BILLING_ENDPOINTS_NAMESPACE, billingIdentityKey(identity)] as const
}

export function billingUserOptionsQueryKey(identity: BillingIdentity) {
  return [
    ...BILLING_USER_OPTIONS_NAMESPACE,
    billingIdentityKey(identity),
  ] as const
}

export function billingChargeDetailQueryKey(
  identity: BillingIdentity,
  chargeId: number,
) {
  return [
    'billing',
    'charge-detail',
    billingIdentityKey(identity),
    chargeId,
  ] as const
}

export async function loadBillingSummary(
  filters: BillingChargeFilters,
): Promise<BillingSummaryResponse> {
  return fetchBillingSummary(billingFilterQuery(filters))
}

export async function loadBillingCharges(
  filters: BillingChargeFilters,
  first: number,
  rows: number,
): Promise<BillingChargePageResponse> {
  return fetchBillingCharges(billingFilterQuery(filters), first, rows)
}

export async function loadBillingPriceRules(
  first: number,
  rows: number,
): Promise<BillingPriceRulePageResponse> {
  return fetchBillingPriceRules(first, rows)
}

export async function loadBillingEndpoints(): Promise<ProviderEndpoint[]> {
  return (await fetchBillingEndpoints()).endpoints
}

export async function loadBillingUserOptions(): Promise<User[]> {
  const response = expectData(await listUserOptions<true>(withData()))
  return response.users
}

export async function loadBillingChargeDetail(
  chargeId: number,
): Promise<BillingChargeDetailResponse> {
  return fetchBillingChargeDetail(chargeId)
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  namespace: readonly unknown[],
  load: () => Promise<T>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: namespace,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({ queryKey, queryFn: load, staleTime: 0 })
}

export async function refreshBillingSummary(
  queryClient: QueryClient,
  identity: BillingIdentity,
  filters: BillingChargeFilters,
): Promise<BillingSummaryResponse> {
  return fetchFresh(
    queryClient,
    billingSummaryQueryKey(identity, filters),
    BILLING_SUMMARY_NAMESPACE,
    () => loadBillingSummary(filters),
  )
}

export async function refreshBillingCharges(
  queryClient: QueryClient,
  identity: BillingIdentity,
  filters: BillingChargeFilters,
  first: number,
  rows: number,
): Promise<BillingChargePageResponse> {
  let requestedFirst = first
  while (true) {
    const page = await fetchFresh(
      queryClient,
      billingChargesQueryKey(identity, filters, requestedFirst, rows),
      BILLING_CHARGES_NAMESPACE,
      () => loadBillingCharges(filters, requestedFirst, rows),
    )
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.charges.length === 0 &&
      page.total > 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst = billingLastPageFirst(page.total, pageRows)
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}

export async function refreshBillingPriceRules(
  queryClient: QueryClient,
  identity: BillingIdentity,
  first: number,
  rows: number,
): Promise<BillingPriceRulePageResponse> {
  let requestedFirst = first
  while (true) {
    const page = await fetchFresh(
      queryClient,
      billingPriceRulesQueryKey(identity, requestedFirst, rows),
      BILLING_PRICE_RULES_NAMESPACE,
      () => loadBillingPriceRules(requestedFirst, rows),
    )
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.rules.length === 0 &&
      page.total > 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst = billingLastPageFirst(page.total, pageRows)
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}

export async function refreshBillingEndpoints(
  queryClient: QueryClient,
  identity: BillingIdentity,
): Promise<ProviderEndpoint[]> {
  return fetchFresh(
    queryClient,
    billingEndpointsQueryKey(identity),
    BILLING_ENDPOINTS_NAMESPACE,
    loadBillingEndpoints,
  )
}

export async function refreshBillingUserOptions(
  queryClient: QueryClient,
  identity: BillingIdentity,
): Promise<User[]> {
  return fetchFresh(
    queryClient,
    billingUserOptionsQueryKey(identity),
    BILLING_USER_OPTIONS_NAMESPACE,
    loadBillingUserOptions,
  )
}

const chargePageSizeKey = pageSizeStorageKey(BILLING_CHARGE_PAGE_SIZE_KEY)
const priceRulePageSizeKey = pageSizeStorageKey(
  BILLING_PRICE_RULE_PAGE_SIZE_KEY,
)

function initialChargePageSize(): number {
  return normalizePageSize(
    readStorage(chargePageSizeKey),
    BILLING_CHARGE_PAGE_SIZE_FALLBACK,
    BILLING_PAGE_SIZE_OPTIONS,
  )
}

function initialPriceRulePageSize(): number {
  return normalizePageSize(
    readStorage(priceRulePageSizeKey),
    BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK,
    BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  )
}

export type BillingState = {
  identityKey: string | null
  identityUserId: number | null
  isAdmin: boolean
  filters: BillingChargeFilters
  periodStart: string
  periodEnd: string
  summary: BillingSummaryResponse | null
  summaryLoading: boolean
  charges: BillingChargeResponse[]
  chargesTotal: number
  chargesFirst: number
  chargesRows: number
  chargesLoading: boolean
  priceRules: BillingPriceRuleResponse[]
  priceRuleTotal: number
  priceRuleFirst: number
  priceRuleRows: number
  priceRulesLoading: boolean
  endpoints: ProviderEndpoint[]
  endpointsLoading: boolean
  userOptions: User[]
  userOptionsLoading: boolean
  detail: BillingChargeDetailResponse | null
  detailChargeId: number | null
  detailLoading: boolean
  priceRuleDraft: BillingPriceRuleForm | null
  initializeIdentity: (identity: BillingIdentity) => void
  resetIdentity: (identity: BillingIdentity | null) => void
  configureFilters: (filters: BillingChargeFilters) => void
  configurePeriod: (start: string, end: string) => void
  applyFilters: (
    queryClient: QueryClient,
    filters: BillingChargeFilters,
  ) => Promise<void>
  applyPeriod: (
    queryClient: QueryClient,
    start: string,
    end: string,
  ) => Promise<void>
  loadSummary: (queryClient: QueryClient) => Promise<void>
  loadChargesPage: (
    queryClient: QueryClient,
    first?: number,
    rows?: number,
  ) => Promise<void>
  loadPriceRulesPage: (
    queryClient: QueryClient,
    first?: number,
    rows?: number,
  ) => Promise<void>
  loadEndpoints: (queryClient: QueryClient) => Promise<void>
  loadUserOptions: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setChargePage: (
    queryClient: QueryClient,
    first: number,
    rows: number,
  ) => Promise<void>
  setPriceRulePage: (
    queryClient: QueryClient,
    first: number,
    rows: number,
  ) => Promise<void>
  openDetail: (chargeId: number) => Promise<void>
  closeDetail: () => void
  setPriceRuleDraft: (form: BillingPriceRuleForm | null) => void
  savePriceRule: (
    queryClient: QueryClient,
    form: BillingPriceRuleForm,
  ) => Promise<AsyncActionOutcome<void>>
  removePriceRule: (
    queryClient: QueryClient,
    priceRuleId: string,
  ) => Promise<AsyncActionOutcome<void>>
  togglePriceRule: (
    queryClient: QueryClient,
    rule: BillingPriceRuleResponse,
  ) => Promise<AsyncActionOutcome<void>>
  reprice: (queryClient: QueryClient) => Promise<AsyncActionOutcome<number>>
  fetchCsvText: (kind: 'details' | 'monthly') => Promise<string>
}

let summaryGeneration = 0
let chargesGeneration = 0
let priceRulesGeneration = 0
let endpointsGeneration = 0
let userOptionsGeneration = 0
let detailGeneration = 0

export const useBillingStore = create<BillingState>((set, get) => ({
  identityKey: null,
  identityUserId: null,
  isAdmin: false,
  filters: {},
  periodStart: currentMonthDate(),
  periodEnd: '',
  summary: null,
  summaryLoading: false,
  charges: [],
  chargesTotal: 0,
  chargesFirst: 0,
  chargesRows: initialChargePageSize(),
  chargesLoading: false,
  priceRules: [],
  priceRuleTotal: 0,
  priceRuleFirst: 0,
  priceRuleRows: initialPriceRulePageSize(),
  priceRulesLoading: false,
  endpoints: [],
  endpointsLoading: false,
  userOptions: [],
  userOptionsLoading: false,
  detail: null,
  detailChargeId: null,
  detailLoading: false,
  priceRuleDraft: null,

  initializeIdentity(identity) {
    if (get().identityKey === billingIdentityKey(identity)) return
    get().resetIdentity(identity)
  },

  resetIdentity(identity) {
    const key = identity ? billingIdentityKey(identity) : null
    ++summaryGeneration
    ++chargesGeneration
    ++priceRulesGeneration
    ++endpointsGeneration
    ++userOptionsGeneration
    ++detailGeneration
    set({
      identityKey: key,
      identityUserId: identity?.user_id ?? null,
      isAdmin: identity?.is_admin === true,
      filters: {},
      periodStart: currentMonthDate(),
      periodEnd: '',
      summary: null,
      summaryLoading: false,
      charges: [],
      chargesTotal: 0,
      chargesFirst: 0,
      chargesRows: initialChargePageSize(),
      chargesLoading: false,
      priceRules: [],
      priceRuleTotal: 0,
      priceRuleFirst: 0,
      priceRuleRows: initialPriceRulePageSize(),
      priceRulesLoading: false,
      endpoints: [],
      endpointsLoading: false,
      userOptions: [],
      userOptionsLoading: false,
      detail: null,
      detailChargeId: null,
      detailLoading: false,
      priceRuleDraft: null,
    })
  },

  configureFilters(filters) {
    set({ filters: { ...filters } })
  },

  configurePeriod(start, end) {
    const periodStart = start || currentMonthDate()
    set({
      periodStart,
      periodEnd: end,
      filters: {
        ...get().filters,
        start_at: startOfDate(periodStart),
        end_at: endOfDate(end),
      },
    })
  },

  async applyFilters(queryClient, filters) {
    set({ filters: { ...filters }, chargesFirst: 0 })
    await Promise.all([
      get().loadSummary(queryClient),
      get().loadChargesPage(queryClient, 0, get().chargesRows),
    ])
  },

  async applyPeriod(queryClient, start, end) {
    get().configurePeriod(start, end)
    set({ chargesFirst: 0 })
    await Promise.all([
      get().loadSummary(queryClient),
      get().loadChargesPage(queryClient, 0, get().chargesRows),
    ])
  },

  async loadSummary(queryClient) {
    const state = get()
    if (state.identityKey === null) return
    const identity: BillingIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const filters = { ...state.filters }
    const generation = ++summaryGeneration
    set({ summaryLoading: true })
    try {
      const summary = await refreshBillingSummary(
        queryClient,
        identity,
        filters,
      )
      if (generation !== summaryGeneration) return
      set({ summary, summaryLoading: false })
    } catch (cause) {
      if (generation === summaryGeneration) set({ summaryLoading: false })
      throw cause
    }
  },

  async loadChargesPage(queryClient, first, rows) {
    const state = get()
    if (state.identityKey === null) return
    const identity: BillingIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const filters = { ...state.filters }
    const nextFirst = first ?? state.chargesFirst
    const nextRows = rows ?? state.chargesRows
    const generation = ++chargesGeneration
    set({ chargesLoading: true })
    try {
      const page = await refreshBillingCharges(
        queryClient,
        identity,
        filters,
        nextFirst,
        nextRows,
      )
      if (generation !== chargesGeneration) return
      set({
        charges: page.charges,
        chargesTotal: page.total,
        chargesFirst: page.first,
        chargesRows: page.rows,
        chargesLoading: false,
      })
    } catch (cause) {
      if (generation === chargesGeneration) set({ chargesLoading: false })
      throw cause
    }
  },

  async loadPriceRulesPage(queryClient, first, rows) {
    const state = get()
    if (state.identityKey === null || !state.isAdmin) return
    const identity: BillingIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const nextFirst = first ?? state.priceRuleFirst
    const nextRows = rows ?? state.priceRuleRows
    const generation = ++priceRulesGeneration
    set({ priceRulesLoading: true })
    try {
      const page = await refreshBillingPriceRules(
        queryClient,
        identity,
        nextFirst,
        nextRows,
      )
      if (generation !== priceRulesGeneration) return
      set({
        priceRules: page.rules,
        priceRuleTotal: page.total,
        priceRuleFirst: page.first,
        priceRuleRows: page.rows,
        priceRulesLoading: false,
      })
    } catch (cause) {
      if (generation === priceRulesGeneration) set({ priceRulesLoading: false })
      throw cause
    }
  },

  async loadEndpoints(queryClient) {
    const state = get()
    if (state.identityKey === null || !state.isAdmin) return
    const identity: BillingIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++endpointsGeneration
    set({ endpointsLoading: true })
    try {
      const endpoints = await refreshBillingEndpoints(queryClient, identity)
      if (generation !== endpointsGeneration) return
      set({ endpoints, endpointsLoading: false })
    } catch (cause) {
      if (generation === endpointsGeneration) set({ endpointsLoading: false })
      throw cause
    }
  },

  async loadUserOptions(queryClient) {
    const state = get()
    if (state.identityKey === null || !state.isAdmin) return
    const identity: BillingIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++userOptionsGeneration
    set({ userOptionsLoading: true })
    try {
      const userOptions = await refreshBillingUserOptions(queryClient, identity)
      if (generation !== userOptionsGeneration) return
      set({ userOptions, userOptionsLoading: false })
    } catch (cause) {
      if (generation === userOptionsGeneration)
        set({ userOptionsLoading: false })
      throw cause
    }
  },

  async refresh(queryClient) {
    const state = get()
    if (state.identityKey === null) return
    const tasks: Promise<unknown>[] = [
      get().loadSummary(queryClient),
      get().loadChargesPage(queryClient),
    ]
    if (state.isAdmin) {
      tasks.push(
        get().loadPriceRulesPage(queryClient),
        get().loadEndpoints(queryClient),
        get().loadUserOptions(queryClient),
      )
    }
    await Promise.all(tasks)
  },

  async setChargePage(queryClient, first, rows) {
    const nextRows = BILLING_PAGE_SIZE_OPTIONS.includes(rows)
      ? rows
      : get().chargesRows
    const nextFirst = Math.max(0, Math.floor(first / nextRows) * nextRows)
    writeStorage(chargePageSizeKey, String(nextRows))
    set({ chargesFirst: nextFirst, chargesRows: nextRows })
    await get().loadChargesPage(queryClient, nextFirst, nextRows)
  },

  async setPriceRulePage(queryClient, first, rows) {
    const nextRows = BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS.includes(rows)
      ? rows
      : get().priceRuleRows
    const nextFirst = Math.max(0, Math.floor(first / nextRows) * nextRows)
    writeStorage(priceRulePageSizeKey, String(nextRows))
    set({ priceRuleFirst: nextFirst, priceRuleRows: nextRows })
    await get().loadPriceRulesPage(queryClient, nextFirst, nextRows)
  },

  async openDetail(chargeId) {
    const generation = ++detailGeneration
    set({ detail: null, detailChargeId: chargeId, detailLoading: true })
    try {
      const detail = await loadBillingChargeDetail(chargeId)
      if (generation !== detailGeneration) return
      set({ detail, detailLoading: false })
    } catch (cause) {
      if (generation === detailGeneration) set({ detailLoading: false })
      throw cause
    }
  },

  closeDetail() {
    ++detailGeneration
    set({ detail: null, detailChargeId: null, detailLoading: false })
  },

  setPriceRuleDraft(form) {
    set({ priceRuleDraft: form })
  },

  async savePriceRule(queryClient, form) {
    const state = get()
    const priceRuleId = form.price_rule_id ?? null
    const captured = capturePayload(billingPriceRuleRequest(form))
    const draft = state.priceRuleDraft
    return runAsyncAction(
      billingPriceRuleActionKey(priceRuleId),
      async () => {
        if (priceRuleId) await updatePriceRule(priceRuleId, captured)
        else await createPriceRule(captured)
        await Promise.all([
          get().loadPriceRulesPage(queryClient),
          get().loadSummary(queryClient),
        ])
        if (draft && get().priceRuleDraft === draft) {
          set({ priceRuleDraft: null })
        }
      },
      'save',
    )
  },

  async removePriceRule(queryClient, priceRuleId) {
    return runAsyncAction(
      billingPriceRuleActionKey(priceRuleId),
      async () => {
        await deletePriceRule(priceRuleId)
        await Promise.all([
          get().loadPriceRulesPage(queryClient),
          get().loadSummary(queryClient),
          get().loadChargesPage(queryClient),
        ])
      },
      'delete',
    )
  },

  async togglePriceRule(queryClient, rule) {
    const enabled = !rule.enabled
    return runAsyncAction(
      billingPriceRuleActionKey(rule.price_rule_id),
      async () => {
        await setPriceRuleEnabled(rule.price_rule_id, enabled)
        await get().loadPriceRulesPage(queryClient)
      },
      'toggle',
    )
  },

  async reprice(queryClient) {
    return runAsyncAction(
      BILLING_REPRICE_KEY,
      async () => {
        const result = await repriceUnpriced()
        await Promise.all([
          get().loadSummary(queryClient),
          get().loadChargesPage(queryClient),
          get().loadPriceRulesPage(queryClient),
        ])
        return result.repriced
      },
      'reprice',
    )
  },

  async fetchCsvText(kind) {
    return fetchBillingCsv(kind, billingFilterQuery(get().filters))
  },
}))

useSessionStore.subscribe((state, previous) => {
  const currentKey = billingIdentityKey({
    user_id: state.me?.user_id ?? null,
    is_admin: state.me?.is_admin === true,
  })
  const previousKey = billingIdentityKey({
    user_id: previous.me?.user_id ?? null,
    is_admin: previous.me?.is_admin === true,
  })
  if (currentKey !== previousKey) {
    useBillingStore
      .getState()
      .resetIdentity(
        state.me
          ? { user_id: state.me.user_id, is_admin: state.me.is_admin === true }
          : null,
      )
  }
})
