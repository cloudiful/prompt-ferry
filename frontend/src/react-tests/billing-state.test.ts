import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  BillingChargeDetailResponse,
  BillingChargePageResponse,
  BillingPriceRulePageResponse,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
  User,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  billingSummary: vi.fn(),
  listBillingCharges: vi.fn(),
  billingChargeDetail: vi.fn(),
  listBillingPriceRules: vi.fn(),
  createBillingPriceRule: vi.fn(),
  updateBillingPriceRule: vi.fn(),
  deleteBillingPriceRule: vi.fn(),
  patchBillingPriceRule: vi.fn(),
  repriceBilling: vi.fn(),
  exportBilling: vi.fn(),
  listEndpoints: vi.fn(),
  listUserOptions: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import {
  BILLING_REPRICE_KEY,
  billingChargesQueryKey,
  billingPriceRuleActionKey,
  billingPriceRulesQueryKey,
  billingSummaryQueryKey,
  useBillingStore,
} from '@/lib/billing'
import { createAppQueryClient } from '@/lib/query'
import { useSessionStore } from '@/lib/session'

const admin = {
  display_name: 'Fixture Administrator',
  user_id: 1,
  login_name: 'admin',
  is_admin: true,
}
const operator = {
  display_name: 'Fixture Operator',
  user_id: 2,
  login_name: 'operator',
  is_admin: false,
}

const userOptions: User[] = [
  {
    created_at: '2026-10-16T08:00:00.000Z',
    display_name: 'Fixture Operator',
    is_active: true,
    is_admin: false,
    login_name: 'operator',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 2,
  },
]

function summary(
  overrides: Partial<BillingSummaryResponse> = {},
): BillingSummaryResponse {
  return {
    by_client_key: [],
    by_model: [],
    currency: 'CNY',
    customer_amount: '1.00',
    known_count: 1,
    priced_count: 1,
    request_count: 1,
    unknown_count: 0,
    unpriced_count: 0,
    ...overrides,
  }
}

function charges(
  overrides: Partial<BillingChargePageResponse> = {},
): BillingChargePageResponse {
  return { charges: [], first: 0, rows: 10, total: 0, ...overrides }
}

function priceRules(
  overrides: Partial<BillingPriceRulePageResponse> = {},
): BillingPriceRulePageResponse {
  return { first: 0, rows: 10, rules: [], total: 0, ...overrides }
}

function priceRule(
  overrides: Partial<BillingPriceRuleResponse> = {},
): BillingPriceRuleResponse {
  return {
    cache_read_rate: '0.2',
    cache_write_rate: '0.4',
    created_at: '2026-10-01T00:00:00.000Z',
    currency: 'CNY',
    effective_from: '2026-10-05T02:30:00.000Z',
    enabled: true,
    input_rate: '1',
    output_rate: '3',
    price_rule_id: 'price-rule-1',
    public_model: 'fixture-model',
    updated_at: '2026-10-02T00:00:00.000Z',
    ...overrides,
  }
}

function detail(chargeId: number): BillingChargeDetailResponse {
  return {
    charge: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      charge_id: chargeId,
      created_at: '2026-10-20T08:00:00.000Z',
      currency: 'CNY',
      customer_amount: '1.00',
      input_tokens: 10,
      output_tokens: 5,
      pricing_status: 'priced',
      request_id: `req-${chargeId}`,
      updated_at: '2026-10-20T08:00:00.000Z',
      usage_status: 'known',
    },
    lines: [],
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

let queryClient: ReturnType<typeof createAppQueryClient>

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  queryClient = createAppQueryClient()
  api.billingSummary.mockResolvedValue(summary())
  api.listBillingCharges.mockImplementation(async (options) =>
    charges({
      first: Number(options?.query?.first ?? 0),
      rows: Number(options?.query?.rows ?? 10),
    }),
  )
  api.listBillingPriceRules.mockImplementation(async (options) =>
    priceRules({
      first: Number(options?.query?.first ?? 0),
      rows: Number(options?.query?.rows ?? 10),
    }),
  )
  api.listEndpoints.mockResolvedValue({
    endpoints: [],
    first: 0,
    rows: 500,
    total: 0,
  })
  api.listUserOptions.mockResolvedValue({ users: userOptions })
  api.billingChargeDetail.mockResolvedValue(detail(1))
  api.createBillingPriceRule.mockResolvedValue(priceRule())
  api.updateBillingPriceRule.mockResolvedValue(priceRule())
  api.patchBillingPriceRule.mockResolvedValue(priceRule())
  api.deleteBillingPriceRule.mockResolvedValue(undefined)
  api.repriceBilling.mockResolvedValue({ repriced: 3 })
  api.exportBilling.mockResolvedValue('csv-body')
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useBillingStore.getState().resetIdentity(null)
})

describe('billing state', () => {
  it('loads the summary, charges, and admin reads for an admin only', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })

    await useBillingStore.getState().refresh(queryClient)

    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(api.listEndpoints).toHaveBeenCalledTimes(1)
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)
    expect(useBillingStore.getState().userOptions).toHaveLength(1)
    expect(useBillingStore.getState().summary?.request_count).toBe(1)
  })

  it('never requests the admin reads for an ordinary user', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: operator.user_id,
      is_admin: false,
    })

    await useBillingStore.getState().refresh(queryClient)

    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    expect(api.listEndpoints).not.toHaveBeenCalled()
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(useBillingStore.getState().priceRules).toEqual([])
    expect(useBillingStore.getState().endpoints).toEqual([])
    expect(useBillingStore.getState().userOptions).toEqual([])
  })

  it('keys the reads by identity, filters, and page', async () => {
    const identity = { user_id: admin.user_id, is_admin: true }
    useBillingStore.getState().resetIdentity(identity)

    await useBillingStore.getState().refresh(queryClient)

    expect(
      queryClient.getQueryData(
        billingSummaryQueryKey(identity, useBillingStore.getState().filters),
      ),
    ).toBeDefined()
    expect(
      queryClient.getQueryData(
        billingChargesQueryKey(
          identity,
          useBillingStore.getState().filters,
          0,
          useBillingStore.getState().chargesRows,
        ),
      ),
    ).toBeDefined()
    expect(
      queryClient.getQueryData(billingPriceRulesQueryKey(identity, 0, 10)),
    ).toBeDefined()
    expect(billingSummaryQueryKey(identity, { user_id: 2 })).not.toEqual(
      billingSummaryQueryKey(identity, {}),
    )
    expect(billingChargesQueryKey(identity, {}, 0, 10)).not.toEqual(
      billingPriceRulesQueryKey(identity, 0, 10),
    )
  })

  it('keeps the charge pager and the price rule pager independent', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.listBillingCharges.mockClear()
    api.listBillingPriceRules.mockClear()

    await useBillingStore.getState().setChargePage(queryClient, 25, 25)

    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { first: 25, rows: 25 },
    })
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    expect(useBillingStore.getState().chargesFirst).toBe(25)
    expect(useBillingStore.getState().chargesRows).toBe(25)
    expect(useBillingStore.getState().priceRuleFirst).toBe(0)
    expect(useBillingStore.getState().priceRuleRows).toBe(10)

    api.listBillingCharges.mockClear()
    await useBillingStore.getState().setPriceRulePage(queryClient, 20, 20)

    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules.mock.calls[0]?.[0]).toMatchObject({
      query: { first: 20, rows: 20 },
    })
    expect(api.listBillingCharges).not.toHaveBeenCalled()
    expect(useBillingStore.getState().priceRuleFirst).toBe(20)
    expect(useBillingStore.getState().chargesFirst).toBe(25)
  })

  it('persists both page sizes under their own storage keys', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })

    await useBillingStore.getState().setChargePage(queryClient, 0, 50)
    await useBillingStore.getState().setPriceRulePage(queryClient, 0, 20)

    expect(localStorage.getItem('prompt-ferry:table-page-size:billing')).toBe(
      '50',
    )
    expect(
      localStorage.getItem('prompt-ferry:table-page-size:billing-price-rules'),
    ).toBe('20')
  })

  it('corrects an empty out-of-range charge page and rule page', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    api.listBillingCharges.mockResolvedValueOnce(
      charges({ charges: [], first: 30, rows: 10, total: 25 }),
    )
    api.listBillingCharges.mockResolvedValueOnce(
      charges({ charges: [], first: 20, rows: 10, total: 25 }),
    )

    await useBillingStore.getState().loadChargesPage(queryClient, 30, 10)

    expect(api.listBillingCharges).toHaveBeenCalledTimes(2)
    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { first: 30, rows: 10 },
    })
    expect(api.listBillingCharges.mock.calls[1]?.[0]).toMatchObject({
      query: { first: 20, rows: 10 },
    })
    expect(useBillingStore.getState().chargesFirst).toBe(20)
  })

  it('applies filters and the period to the summary and the charges only', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()
    api.listBillingPriceRules.mockClear()
    api.listEndpoints.mockClear()

    await useBillingStore
      .getState()
      .applyFilters(queryClient, { request_id: 'req-1', user_id: 2 })

    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    expect(api.listEndpoints).not.toHaveBeenCalled()
    expect(api.billingSummary.mock.calls[0]?.[0]).toMatchObject({
      query: { request_id: 'req-1', user_id: 2 },
    })
    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { request_id: 'req-1', user_id: 2, first: 0 },
    })
    expect(useBillingStore.getState().chargesFirst).toBe(0)

    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()
    await useBillingStore.getState().applyPeriod(queryClient, '', '')

    expect(api.billingSummary.mock.calls[0]?.[0]).toMatchObject({
      query: { request_id: 'req-1', user_id: 2 },
    })
    const periodQuery = api.listBillingCharges.mock.calls[0]?.[0]?.query
    expect(periodQuery?.end_at).toBeUndefined()
    expect(periodQuery?.start_at).toBe(
      new Date(
        `${useBillingStore.getState().periodStart}T00:00:00`,
      ).toISOString(),
    )
  })

  it('keeps the request id filter across a period change', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    useBillingStore.getState().configureFilters({ request_id: 'req-9' })

    await useBillingStore.getState().applyPeriod(queryClient, '2026-10-01', '')

    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { request_id: 'req-9' },
    })
  })

  it('keeps the newest charge page and ignores a late failure', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    const first = deferred<BillingChargePageResponse>()
    const second = deferred<BillingChargePageResponse>()
    api.listBillingCharges.mockReturnValueOnce(first.promise)
    api.listBillingCharges.mockReturnValueOnce(second.promise)

    const firstLoad = useBillingStore
      .getState()
      .loadChargesPage(queryClient, 0, 10)
    const secondLoad = useBillingStore
      .getState()
      .loadChargesPage(queryClient, 10, 10)
    second.resolve(charges({ charges: [], first: 10, rows: 10, total: 30 }))
    await secondLoad
    first.reject(new Error('late failure'))
    await expect(firstLoad).rejects.toThrow('late failure')

    expect(useBillingStore.getState().chargesFirst).toBe(10)
    expect(useBillingStore.getState().chargesLoading).toBe(false)
  })

  it('does not let a stale failure clear the running load', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    const first = deferred<BillingChargePageResponse>()
    const second = deferred<BillingChargePageResponse>()
    api.listBillingCharges.mockReturnValueOnce(first.promise)
    api.listBillingCharges.mockReturnValueOnce(second.promise)

    const firstLoad = useBillingStore
      .getState()
      .loadChargesPage(queryClient, 0, 10)
    const secondLoad = useBillingStore
      .getState()
      .loadChargesPage(queryClient, 10, 10)
    first.reject(new Error('stale failure'))
    await expect(firstLoad).rejects.toThrow('stale failure')

    expect(useBillingStore.getState().chargesLoading).toBe(true)

    second.resolve(charges({ charges: [], first: 10, rows: 10, total: 30 }))
    await secondLoad
    expect(useBillingStore.getState().chargesLoading).toBe(false)
  })

  it('opens a charge detail with a version guard and clears it on failure', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    const first = deferred<BillingChargeDetailResponse>()
    const second = deferred<BillingChargeDetailResponse>()
    api.billingChargeDetail.mockReturnValueOnce(first.promise)
    api.billingChargeDetail.mockReturnValueOnce(second.promise)

    const firstOpen = useBillingStore.getState().openDetail(1)
    const secondOpen = useBillingStore.getState().openDetail(2)
    expect(useBillingStore.getState().detail).toBeNull()
    expect(useBillingStore.getState().detailLoading).toBe(true)
    expect(useBillingStore.getState().detailChargeId).toBe(2)

    second.resolve(detail(2))
    await secondOpen
    first.resolve(detail(1))
    await firstOpen

    expect(useBillingStore.getState().detail?.charge.charge_id).toBe(2)
    expect(useBillingStore.getState().detailLoading).toBe(false)

    api.billingChargeDetail.mockRejectedValueOnce(new Error('detail failed'))
    await expect(useBillingStore.getState().openDetail(3)).rejects.toThrow(
      'detail failed',
    )
    expect(useBillingStore.getState().detailLoading).toBe(false)
    expect(useBillingStore.getState().detail).toBeNull()
  })

  it('creates a price rule with the captured body and refreshes the rules and the summary', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.listBillingPriceRules.mockClear()
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()
    const draft = {
      public_model: '  fixture-model  ',
      input_rate: ' 1 ',
      cache_read_rate: ' 0.2 ',
      cache_write_rate: ' 0.4 ',
      output_rate: ' 3 ',
      effective_from: '2026-10-24T08:05',
    }
    useBillingStore.getState().setPriceRuleDraft(draft)

    const pending = deferred<BillingPriceRuleResponse>()
    api.createBillingPriceRule.mockReturnValueOnce(pending.promise)
    const save = useBillingStore.getState().savePriceRule(queryClient, draft)
    useBillingStore.getState().setPriceRuleDraft({ ...draft })
    pending.resolve(priceRule())
    const outcome = await save

    expect(outcome.executed).toBe(true)
    expect(api.createBillingPriceRule).toHaveBeenCalledTimes(1)
    expect(api.createBillingPriceRule.mock.calls[0]?.[0]).toMatchObject({
      body: {
        public_model: 'fixture-model',
        input_rate: '1',
        cache_read_rate: '0.2',
        cache_write_rate: '0.4',
        output_rate: '3',
        effective_from: new Date('2026-10-24T08:05').toISOString(),
      },
    })
    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).not.toHaveBeenCalled()
    expect(useBillingStore.getState().priceRuleDraft).not.toBeNull()
  })

  it('refuses a duplicate rule action and keeps another rule independent', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    const pending = deferred<BillingPriceRuleResponse>()
    api.updateBillingPriceRule.mockReturnValueOnce(pending.promise)
    const form = {
      price_rule_id: 'price-rule-1',
      public_model: 'fixture-model',
      input_rate: '1',
      cache_read_rate: '0.2',
      cache_write_rate: '0.4',
      output_rate: '3',
      effective_from: '2026-10-24T08:05',
    }

    const first = useBillingStore.getState().savePriceRule(queryClient, form)
    const refused = await useBillingStore
      .getState()
      .savePriceRule(queryClient, form)

    expect(refused).toEqual({ executed: false })
    expect(api.updateBillingPriceRule).toHaveBeenCalledTimes(1)

    const other = await useBillingStore.getState().savePriceRule(queryClient, {
      ...form,
      price_rule_id: 'price-rule-2',
    })
    expect(other.executed).toBe(true)
    expect(api.updateBillingPriceRule).toHaveBeenCalledTimes(2)

    pending.resolve(priceRule())
    await first
    expect(billingPriceRuleActionKey('price-rule-1')).toBe(
      'billingrule:price-rule-1',
    )
    expect(billingPriceRuleActionKey(null)).toBe('billingrule:create')
  })

  it('deletes with the rules, summary, and charges refresh set and toggles with the rules only', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.listBillingPriceRules.mockClear()
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()

    const deleted = await useBillingStore
      .getState()
      .removePriceRule(queryClient, 'price-rule-1')

    expect(deleted.executed).toBe(true)
    expect(api.deleteBillingPriceRule).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)

    api.listBillingPriceRules.mockClear()
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()

    const toggled = await useBillingStore
      .getState()
      .togglePriceRule(queryClient, priceRule())

    expect(toggled.executed).toBe(true)
    expect(api.patchBillingPriceRule).toHaveBeenCalledWith(
      expect.objectContaining({
        path: { price_rule_id: 'price-rule-1' },
        body: { enabled: false },
      }),
    )
    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(api.billingSummary).not.toHaveBeenCalled()
    expect(api.listBillingCharges).not.toHaveBeenCalled()
  })

  it('reprices with the 10000 limit and refreshes the summary, charges, and rules', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.listBillingPriceRules.mockClear()
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()

    const outcome = await useBillingStore.getState().reprice(queryClient)

    expect(outcome).toEqual({ executed: true, value: 3 })
    expect(api.repriceBilling).toHaveBeenCalledWith(
      expect.objectContaining({ body: { limit: 10_000 } }),
    )
    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1)
    expect(BILLING_REPRICE_KEY).toBe('billing:reprice')
  })

  it('keeps the newest filter response and discards a late older one', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    const older = deferred<BillingChargePageResponse>()
    const newer = deferred<BillingChargePageResponse>()
    api.listBillingCharges.mockReturnValueOnce(older.promise)
    api.listBillingCharges.mockReturnValueOnce(newer.promise)

    const olderLoad = useBillingStore
      .getState()
      .applyFilters(queryClient, { request_id: 'req-old' })
    const newerLoad = useBillingStore
      .getState()
      .applyFilters(queryClient, { request_id: 'req-new' })
    newer.resolve(charges({ charges: [], first: 0, rows: 25, total: 5 }))
    await newerLoad
    older.resolve(charges({ charges: [], first: 0, rows: 25, total: 90 }))
    await olderLoad

    expect(useBillingStore.getState().filters).toEqual({
      request_id: 'req-new',
    })
    expect(useBillingStore.getState().chargesTotal).toBe(5)
  })

  it('holds the rule key through the deferred refresh of a write', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    const refreshPending = deferred<BillingPriceRulePageResponse>()
    api.listBillingPriceRules.mockReturnValueOnce(refreshPending.promise)
    const form = {
      price_rule_id: 'price-rule-1',
      public_model: 'fixture-model',
      input_rate: '1',
      cache_read_rate: '0.2',
      cache_write_rate: '0.4',
      output_rate: '3',
      effective_from: '2026-10-24T08:05',
    }

    const save = useBillingStore.getState().savePriceRule(queryClient, form)
    await vi.waitFor(() =>
      expect(api.listBillingPriceRules).toHaveBeenCalledTimes(1),
    )
    const refused = await useBillingStore
      .getState()
      .savePriceRule(queryClient, form)

    expect(refused).toEqual({ executed: false })
    expect(api.updateBillingPriceRule).toHaveBeenCalledTimes(1)

    refreshPending.resolve(priceRules())
    await save
  })

  it('rejects the write when its refresh fails and releases the key', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    api.listBillingPriceRules.mockRejectedValueOnce(
      new Error('rules refresh failed'),
    )

    await expect(
      useBillingStore.getState().togglePriceRule(queryClient, priceRule()),
    ).rejects.toThrow('rules refresh failed')

    const retry = await useBillingStore
      .getState()
      .togglePriceRule(queryClient, priceRule())
    expect(retry.executed).toBe(true)
  })

  it('reads the export without a mutation key and with the current filters', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    useBillingStore.getState().configureFilters({ request_id: 'req-1' })

    const csv = await useBillingStore.getState().fetchCsvText('monthly')

    expect(csv).toBe('csv-body')
    expect(api.exportBilling).toHaveBeenCalledWith(
      expect.objectContaining({
        query: { kind: 'monthly', request_id: 'req-1' },
      }),
    )
  })

  it('resets the view and scopes the reads to a new worker identity', async () => {
    useBillingStore.getState().resetIdentity({
      user_id: admin.user_id,
      is_admin: true,
    })
    await useBillingStore.getState().refresh(queryClient)
    expect(useBillingStore.getState().summary).not.toBeNull()

    useSessionStore.setState({ me: operator })
    api.listBillingPriceRules.mockClear()
    api.listUserOptions.mockClear()
    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()

    expect(useBillingStore.getState()).toMatchObject({
      identityKey: '2:user',
      identityUserId: 2,
      isAdmin: false,
      summary: null,
      charges: [],
      priceRules: [],
      endpoints: [],
      userOptions: [],
      detail: null,
      priceRuleDraft: null,
    })

    await useBillingStore.getState().refresh(queryClient)
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    expect(api.listUserOptions).not.toHaveBeenCalled()
  })
})
