import { type QueryClient, type QueryKey } from '@tanstack/react-query'
import type {
  BillingChargePageResponse,
  BillingPriceRulePageResponse,
  BillingSummaryResponse,
  EndpointPageResponse,
  ProviderEndpoint,
  User,
  UserOptionsResponse,
} from '@/generated/admin-api'
import {
  billingSummaryOptions,
  billingSummaryQueryKey,
  listBillingChargesOptions,
  listBillingChargesQueryKey,
  listBillingPriceRulesOptions,
  listBillingPriceRulesQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { billingFilterQuery, billingLastPageFirst } from './billing-model'
import type { BillingChargeFilters } from './billing-model'
import { recordActionKey } from '@/stores/record-action-types'

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

export function billingEndpointsSelect(
  page: EndpointPageResponse,
): ProviderEndpoint[] {
  return page.endpoints
}

export function billingUserOptionsSelect(
  response: UserOptionsResponse,
): User[] {
  return response.users
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: QueryKey,
  namespaceKey: QueryKey,
  options: Record<string, unknown>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: namespaceKey,
    refetchType: 'none',
  })
  return queryClient.fetchQuery(options as never) as Promise<T>
}

export async function refreshBillingSummary(
  queryClient: QueryClient,
  filters: BillingChargeFilters,
): Promise<BillingSummaryResponse> {
  const query = billingFilterQuery(filters)
  return fetchFresh(
    queryClient,
    billingSummaryQueryKey({ query }),
    billingSummaryQueryKey(),
    {
      ...billingSummaryOptions({ query }),
      staleTime: 0,
    },
  )
}

export async function refreshBillingCharges(
  queryClient: QueryClient,
  filters: BillingChargeFilters,
  first: number,
  rows: number,
): Promise<BillingChargePageResponse> {
  const baseQuery = billingFilterQuery(filters)
  let requestedFirst = first
  while (true) {
    const query = { ...baseQuery, first: requestedFirst, rows }
    const page = await fetchFresh<BillingChargePageResponse>(
      queryClient,
      listBillingChargesQueryKey({ query }),
      listBillingChargesQueryKey(),
      {
        ...listBillingChargesOptions({ query }),
        staleTime: 0,
      },
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
  first: number,
  rows: number,
): Promise<BillingPriceRulePageResponse> {
  let requestedFirst = first
  while (true) {
    const query = { first: requestedFirst, rows }
    const page = await fetchFresh<BillingPriceRulePageResponse>(
      queryClient,
      listBillingPriceRulesQueryKey({ query }),
      listBillingPriceRulesQueryKey(),
      {
        ...listBillingPriceRulesOptions({ query }),
        staleTime: 0,
      },
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
