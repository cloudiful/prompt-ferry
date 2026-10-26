import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  BillingChargeDetailResponse,
  BillingChargePageResponse,
  BillingChargeResponse,
  BillingPriceRulePageResponse,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
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

import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useBillingStore } from '@/lib/billing'
import { useSessionStore } from '@/lib/session'
import { BillingPage } from '@/pages/BillingPage'

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}
const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}

function summary(
  overrides: Partial<BillingSummaryResponse> = {},
): BillingSummaryResponse {
  return {
    by_client_key: [
      {
        cache_read_tokens: 240,
        cache_write_tokens: 80,
        customer_amount: '0.009210',
        grouping_key: 'integration-key',
        input_tokens: 1600,
        output_tokens: 520,
        request_count: 4,
      },
    ],
    by_model: [],
    currency: 'CNY',
    customer_amount: '0.009210',
    known_count: 4,
    priced_count: 3,
    request_count: 4,
    unknown_count: 0,
    unpriced_count: 1,
    ...overrides,
  }
}

function charge(
  chargeId: number,
  overrides: Partial<BillingChargeResponse> = {},
): BillingChargeResponse {
  return {
    cache_read_tokens: 120,
    cache_write_tokens: 40,
    charge_id: chargeId,
    client_key_id: 11,
    client_key_label: 'integration-key',
    created_at: '2026-10-20T08:00:00.000Z',
    currency: 'CNY',
    customer_amount: '0.004210',
    endpoint_id: 'ep-minimax-cn',
    endpoint_key_id: null,
    endpoint_name: 'Fixture Minimax',
    input_tokens: 800,
    output_tokens: 260,
    pricing_status: 'priced',
    request_id: `req-charge-${chargeId}`,
    requested_model: 'minimax-m2',
    updated_at: '2026-10-20T08:00:00.000Z',
    usage_status: 'known',
    user_id: 2,
    user_login_name: 'fixture-operator',
    ...overrides,
  }
}

function charges(
  overrides: Partial<BillingChargePageResponse> = {},
): BillingChargePageResponse {
  return { charges: [], first: 0, rows: 25, total: 0, ...overrides }
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
    cache_read_rate: '0.00015',
    cache_write_rate: '0.00060',
    created_at: '2026-10-01T00:00:00.000Z',
    created_by_user_id: 1,
    currency: 'CNY',
    effective_from: '2026-10-05T02:30:00.000Z',
    effective_to: null,
    enabled: true,
    input_rate: '0.00300',
    output_rate: '0.01200',
    price_rule_id: 'price-fixture-1',
    public_model: 'fixture-model-1',
    updated_at: '2026-10-02T00:00:00.000Z',
    ...overrides,
  }
}

function detail(chargeId: number): BillingChargeDetailResponse {
  return {
    charge: charge(chargeId),
    lines: [
      {
        amount: '0.002400',
        line_id: 1,
        meter: 'input_tokens',
        price_rule_id: 'price-fixture-1',
        token_count: 800,
        unit_rate: '0.00300',
      },
    ],
  }
}

function renderPage(path = '/billing') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={[path]}>
            <BillingPage />
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

let downloads: Array<{ download: string; href: string }>
let blobParts: Blob[]

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  vi.clearAllMocks()
  downloads = []
  blobParts = []
  Object.defineProperty(URL, 'createObjectURL', {
    configurable: true,
    value: vi.fn((blob: Blob) => {
      blobParts.push(blob)
      return 'blob:fixture'
    }),
  })
  Object.defineProperty(URL, 'revokeObjectURL', {
    configurable: true,
    value: vi.fn(),
  })
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
    this: HTMLAnchorElement,
  ) {
    downloads.push({ download: this.download, href: this.href })
  })
  api.billingSummary.mockResolvedValue(summary())
  api.listBillingCharges.mockImplementation(async (options) =>
    charges({
      charges: [charge(1)],
      first: Number(options?.query?.first ?? 0),
      rows: Number(options?.query?.rows ?? 25),
      total: 26,
    }),
  )
  api.listBillingPriceRules.mockImplementation(async (options) =>
    priceRules({
      first: Number(options?.query?.first ?? 0),
      rows: Number(options?.query?.rows ?? 10),
      rules: [priceRule()],
      total: 12,
    }),
  )
  api.listEndpoints.mockResolvedValue({
    endpoints: [],
    first: 0,
    rows: 500,
    total: 0,
  })
  api.listUserOptions.mockResolvedValue({ users: [operator] })
  api.billingChargeDetail.mockResolvedValue(detail(1))
  api.createBillingPriceRule.mockResolvedValue(priceRule())
  api.updateBillingPriceRule.mockResolvedValue(priceRule())
  api.patchBillingPriceRule.mockResolvedValue(priceRule())
  api.deleteBillingPriceRule.mockResolvedValue(undefined)
  api.repriceBilling.mockResolvedValue({ repriced: 7 })
  api.exportBilling.mockResolvedValue(
    'request_id,model\nreq-charge-1,minimax-m2',
  )
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useBillingStore.getState().resetIdentity(null)
})

afterEach(() => {
  cleanup()
  useSessionStore.setState({ me: null })
  useBillingStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('billing page', () => {
  it('renders the authenticated route with the summary, the charges, and the admin price rules', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()

    expect(
      document.querySelector('[data-foundation-route-id="R-BILLING"]'),
    ).toBeTruthy()
    expect(
      (await screen.findAllByText('CNY 0.009210', { exact: false })).length,
    ).toBeGreaterThan(0)
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())
    expect(api.listBillingPriceRules).toHaveBeenCalled()
    expect(api.listUserOptions).toHaveBeenCalled()
    expect(
      screen.getByRole('button', { name: 'Reprice unpriced' }),
    ).toBeTruthy()
    expect(screen.getByText('fixture-model-1')).toBeTruthy()
  })

  it('hides the admin filters, price rules, and reprice from an ordinary user', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()

    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())
    expect(screen.queryByRole('combobox', { name: 'User' })).toBeNull()
    expect(screen.queryByRole('combobox', { name: 'Endpoint' })).toBeNull()
    expect(screen.queryByText('Price rules')).toBeNull()
    expect(
      screen.queryByRole('button', { name: 'Reprice unpriced' }),
    ).toBeNull()
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(api.listEndpoints).not.toHaveBeenCalled()
  })

  it('applies the period immediately and applies the filters to the summary and the charges only', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())

    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()
    api.listBillingPriceRules.mockClear()

    fireEvent.change(screen.getByLabelText('Start date'), {
      target: { value: '2026-10-05' },
    })
    await waitFor(() => expect(api.billingSummary).toHaveBeenCalled())
    expect(api.listBillingCharges).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
    const periodQuery = api.listBillingCharges.mock.calls[0]?.[0]?.query
    expect(periodQuery?.start_at).toBe(
      new Date('2026-10-05T00:00:00').toISOString(),
    )

    api.billingSummary.mockClear()
    api.listBillingCharges.mockClear()
    fireEvent.click(screen.getByRole('button', { name: 'Apply filters' }))
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())
    expect(api.billingSummary).toHaveBeenCalledTimes(1)
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()
  })

  it('keeps the route request id in the applied filters', async () => {
    useSessionStore.setState({ me: admin })
    renderPage('/billing?request_id=req-route-1')
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())

    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { request_id: 'req-route-1' },
    })
  })

  it('opens a charge detail with the loading state and renders the lines', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())

    fireEvent.click(
      screen.getByRole('button', { name: 'Billing details req-charge-1' }),
    )
    await screen.findByText('req-charge-1')
    expect(screen.getByText('input_tokens / 800')).toBeTruthy()
    expect(api.billingChargeDetail).toHaveBeenCalledTimes(1)
  })

  it('creates a price rule with the captured payload and closes the dialog on success', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingPriceRules).toHaveBeenCalled())

    fireEvent.click(screen.getByRole('button', { name: 'New price version' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Public model'), {
      target: { value: '  fixture-created  ' },
    })
    await act(async () => {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))
    })

    expect(api.createBillingPriceRule).toHaveBeenCalledTimes(1)
    expect(api.createBillingPriceRule.mock.calls[0]?.[0]).toMatchObject({
      body: {
        public_model: 'fixture-created',
        input_rate: '0',
        cache_read_rate: '0',
        cache_write_rate: '0',
        output_rate: '0',
      },
    })
    expect(
      Object.keys(api.createBillingPriceRule.mock.calls[0]?.[0].body).sort(),
    ).toEqual([
      'cache_read_rate',
      'cache_write_rate',
      'effective_from',
      'input_rate',
      'output_rate',
      'public_model',
    ])
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('deletes a price rule only after the native confirm and toggles it', async () => {
    const confirmSpy = vi
      .spyOn(window, 'confirm')
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true)
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingPriceRules).toHaveBeenCalled())

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Delete price version fixture-model-1',
      }),
    )
    await waitFor(() => expect(confirmSpy).toHaveBeenCalledTimes(1))
    expect(api.deleteBillingPriceRule).not.toHaveBeenCalled()

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Delete price version fixture-model-1',
      }),
    )
    await waitFor(() =>
      expect(api.deleteBillingPriceRule).toHaveBeenCalledTimes(1),
    )
    expect(api.deleteBillingPriceRule.mock.calls[0]?.[0]).toMatchObject({
      path: { price_rule_id: 'price-fixture-1' },
    })

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Disable price version fixture-model-1',
      }),
    )
    await waitFor(() => expect(api.patchBillingPriceRule).toHaveBeenCalled())
    expect(api.patchBillingPriceRule.mock.calls[0]?.[0]).toMatchObject({
      body: { enabled: false },
    })
  })

  it('keeps the price rule pager independent of the charge pager', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingPriceRules).toHaveBeenCalled())
    const chargesSection = screen
      .getByRole('heading', { name: 'Request charges' })
      .closest('section') as HTMLElement
    const rulesSection = screen
      .getByRole('heading', { name: 'Price rules' })
      .closest('section') as HTMLElement

    api.listBillingCharges.mockClear()
    api.listBillingPriceRules.mockClear()

    fireEvent.click(
      within(chargesSection).getByRole('button', { name: 'Next page' }),
    )
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())
    expect(api.listBillingCharges.mock.calls[0]?.[0]).toMatchObject({
      query: { first: 25, rows: 25 },
    })
    expect(api.listBillingPriceRules).not.toHaveBeenCalled()

    api.listBillingCharges.mockClear()
    fireEvent.click(
      within(rulesSection).getByRole('button', { name: 'Next page' }),
    )
    await waitFor(() => expect(api.listBillingPriceRules).toHaveBeenCalled())
    expect(api.listBillingPriceRules.mock.calls[0]?.[0]).toMatchObject({
      query: { first: 10, rows: 10 },
    })
    expect(api.listBillingCharges).not.toHaveBeenCalled()
  })

  it('downloads the CSV with the byte order mark and the file name', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listBillingCharges).toHaveBeenCalled())

    await act(async () => {
      fireEvent.click(
        screen.getByRole('button', { name: 'Export details CSV' }),
      )
    })

    expect(api.exportBilling).toHaveBeenCalledTimes(1)
    expect(downloads).toHaveLength(1)
    expect(downloads[0]?.download).toBe('prompt-ferry-billing-details.csv')
    expect(blobParts[0]?.type).toBe('text/csv;charset=utf-8')
    expect(blobParts[0]?.size).toBe(
      'request_id,model\nreq-charge-1,minimax-m2'.length + 3,
    )
  })
})
