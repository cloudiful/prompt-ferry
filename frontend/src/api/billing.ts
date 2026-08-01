import {
  billingChargeDetail,
  billingSummary,
  createBillingPriceRule,
  deleteBillingPriceRule,
  exportBilling,
  listBillingCharges,
  listBillingPriceRules,
  listEndpoints,
  patchBillingPriceRule,
  repriceBilling,
  updateBillingPriceRule,
} from '../generated/admin-api'
import type {
  BillingChargeDetailResponse,
  BillingChargePageResponse,
  BillingPriceRuleRequest,
  BillingPriceRulePageResponse,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
  EndpointPageResponse,
  BillingRepriceResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import type { BillingChargeFilters } from '../models/billing'

export async function fetchBillingSummary(
  query: BillingChargeFilters,
): Promise<BillingSummaryResponse> {
  return expectData(await billingSummary<true>(withData({ query })))
}

export async function fetchBillingCharges(
  filters: BillingChargeFilters,
  first: number,
  rows: number,
): Promise<BillingChargePageResponse> {
  return expectData(
    await listBillingCharges<true>(
      withData({
        query: { ...filters, first, rows },
      }),
    ),
  )
}

export async function fetchBillingPriceRules(
  first: number,
  rows: number,
): Promise<BillingPriceRulePageResponse> {
  return expectData(
    await listBillingPriceRules<true>(withData({ query: { first, rows } })),
  )
}

export async function fetchBillingEndpoints(): Promise<EndpointPageResponse> {
  return expectData(
    await listEndpoints<true>(withData({ query: { first: 0, rows: 500 } })),
  )
}

export async function createPriceRule(
  body: BillingPriceRuleRequest,
): Promise<BillingPriceRuleResponse> {
  return expectData(await createBillingPriceRule<true>(withData({ body })))
}

export async function updatePriceRule(
  priceRuleId: string,
  body: BillingPriceRuleRequest,
): Promise<BillingPriceRuleResponse> {
  return expectData(
    await updateBillingPriceRule<true>(
      withData({ path: { price_rule_id: priceRuleId }, body }),
    ),
  )
}

export async function deletePriceRule(priceRuleId: string): Promise<void> {
  await deleteBillingPriceRule<true>(
    withData({ path: { price_rule_id: priceRuleId } }),
  )
}

export async function setPriceRuleEnabled(
  priceRuleId: string,
  enabled: boolean,
): Promise<BillingPriceRuleResponse> {
  return expectData(
    await patchBillingPriceRule<true>(
      withData({ path: { price_rule_id: priceRuleId }, body: { enabled } }),
    ),
  )
}

export async function fetchBillingChargeDetail(
  chargeId: number,
): Promise<BillingChargeDetailResponse> {
  return expectData(
    await billingChargeDetail<true>(
      withData({ path: { charge_id: chargeId } }),
    ),
  )
}

export async function repriceUnpriced(): Promise<BillingRepriceResponse> {
  return expectData(
    await repriceBilling<true>(withData({ body: { limit: 10_000 } })),
  )
}

export async function fetchBillingCsv(
  kind: 'details' | 'monthly',
  filters: BillingChargeFilters,
): Promise<string> {
  return expectData(
    await exportBilling<true>(
      withData({ query: { kind, ...filters }, parseAs: 'text' }),
    ),
  )
}
