import type {
  BillingChargeResponse,
  BillingSummaryResponse,
} from '../../src/generated/admin-api'

export type BillingFilterOptions = {
  forcedUserId?: number | null
}

export function filteredCharges(
  charges: BillingChargeResponse[],
  url: URL,
  options: BillingFilterOptions = {},
): BillingChargeResponse[] {
  const fields = [
    'user_id',
    'client_key_id',
    'requested_model',
    'endpoint_id',
    'usage_status',
    'pricing_status',
    'request_id',
  ] as const
  return charges.filter((charge) => {
    if (
      options.forcedUserId !== undefined &&
      options.forcedUserId !== null &&
      charge.user_id !== options.forcedUserId
    ) {
      return false
    }
    for (const field of fields) {
      const value = url.searchParams.get(field)
      if (value && String(charge[field]) !== value) return false
    }
    const start = url.searchParams.get('start_at')
    const end = url.searchParams.get('end_at')
    if (start && Date.parse(charge.created_at) < Date.parse(start)) return false
    if (end && Date.parse(charge.created_at) >= Date.parse(end)) return false
    return true
  })
}

export function chargeSummary(
  charges: BillingChargeResponse[],
): BillingSummaryResponse {
  const group = (key: 'client_key_label' | 'requested_model') => {
    const groups = new Map<string, BillingChargeResponse[]>()
    for (const charge of charges) {
      const name = charge[key] ?? '(unknown)'
      groups.set(name, [...(groups.get(name) ?? []), charge])
    }
    return [...groups].map(([grouping_key, rows]) => ({
      grouping_key,
      request_count: rows.length,
      input_tokens: rows.reduce((n, r) => n + r.input_tokens, 0),
      output_tokens: rows.reduce((n, r) => n + r.output_tokens, 0),
      cache_read_tokens: rows.reduce((n, r) => n + r.cache_read_tokens, 0),
      cache_write_tokens: rows.reduce((n, r) => n + r.cache_write_tokens, 0),
      customer_amount: rows
        .reduce((n, r) => n + Number(r.customer_amount ?? 0), 0)
        .toFixed(6),
    }))
  }
  return {
    by_client_key: group('client_key_label'),
    by_model: group('requested_model'),
    currency: 'CNY',
    customer_amount: charges
      .reduce((n, r) => n + Number(r.customer_amount ?? 0), 0)
      .toFixed(6),
    known_count: charges.filter((r) => r.usage_status === 'known').length,
    unknown_count: charges.filter((r) => r.usage_status !== 'known').length,
    priced_count: charges.filter((r) => r.pricing_status === 'priced').length,
    unpriced_count: charges.filter((r) => r.pricing_status !== 'priced').length,
    request_count: charges.length,
  }
}

function csvCell(value: string | number | null | undefined): string {
  const text = value === null || value === undefined ? '' : String(value)
  return /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text
}

function csvRows(
  rows: Array<Array<string | number | null | undefined>>,
): string {
  return `${rows.map((row) => row.map(csvCell).join(',')).join('\n')}\n`
}

export function billingDetailsCsv(
  charges: BillingChargeResponse[],
  isAdmin: boolean,
): string {
  const header = isAdmin
    ? ['request_id', 'model', 'user', 'amount', 'currency', 'status']
    : ['request_id', 'model', 'amount', 'currency', 'status']
  const rows = charges.map((charge) =>
    isAdmin
      ? [
          charge.request_id,
          charge.requested_model ?? '',
          charge.user_login_name ?? '',
          charge.customer_amount ?? '',
          charge.currency,
          charge.pricing_status,
        ]
      : [
          charge.request_id,
          charge.requested_model ?? '',
          charge.customer_amount ?? '',
          charge.currency,
          charge.pricing_status,
        ],
  )
  return csvRows([header, ...rows])
}

export function billingMonthlyCsv(charges: BillingChargeResponse[]): string {
  const groups = new Map<
    string,
    { amount: number; model: string; month: string; requests: number }
  >()
  for (const charge of charges) {
    const month = charge.created_at.slice(0, 7)
    const model = charge.requested_model ?? '(unknown)'
    const key = `${month}|${model}`
    const group = groups.get(key) ?? { amount: 0, model, month, requests: 0 }
    group.amount += Number(charge.customer_amount ?? 0)
    group.requests += 1
    groups.set(key, group)
  }
  const rows = [...groups.values()]
    .sort((left, right) =>
      left.month === right.month
        ? left.model.localeCompare(right.model)
        : left.month.localeCompare(right.month),
    )
    .map((group) => [
      group.month,
      group.model,
      group.requests,
      group.amount.toFixed(6),
    ])
  return csvRows([['month', 'model', 'requests', 'customer_amount'], ...rows])
}

export function sortedPriceRules<
  T extends {
    created_at: string
    effective_from: string
    price_rule_id: string
    public_model: string
  },
>(rules: T[]): T[] {
  return [...rules].sort((left, right) => {
    if (left.public_model !== right.public_model) {
      return left.public_model.localeCompare(right.public_model)
    }
    if (left.effective_from !== right.effective_from) {
      return right.effective_from.localeCompare(left.effective_from)
    }
    if (left.created_at !== right.created_at) {
      return right.created_at.localeCompare(left.created_at)
    }
    return left.price_rule_id.localeCompare(right.price_rule_id)
  })
}
