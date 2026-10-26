import type {
  BillingPriceRuleRequest,
  BillingPriceRuleResponse,
} from '@/generated/admin-api'

export type BillingChargeFilters = {
  user_id?: number
  client_key_id?: number
  requested_model?: string
  endpoint_id?: string
  usage_status?: string
  pricing_status?: string
  request_id?: string
  start_at?: string
  end_at?: string
}

export type BillingPriceRuleForm = {
  price_rule_id?: string
  public_model: string
  input_rate: string
  cache_read_rate: string
  cache_write_rate: string
  output_rate: string
  effective_from: string
}

export const BILLING_PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
export const BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS = [10, 20, 50]
export const BILLING_CHARGE_PAGE_SIZE_KEY = 'billing'
export const BILLING_PRICE_RULE_PAGE_SIZE_KEY = 'billing-price-rules'
export const BILLING_CHARGE_PAGE_SIZE_FALLBACK = 25
export const BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK = 10
export const BILLING_DEFAULT_CURRENCY = 'CNY'

export function currentMonthDate(now: Date = new Date()): string {
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-01`
}

export function dateInputValue(value: string): string {
  return value.slice(0, 10)
}

export function startOfDate(value: string): string | undefined {
  if (!value) return undefined
  const date = new Date(`${value}T00:00:00`)
  return Number.isNaN(date.getTime()) ? undefined : date.toISOString()
}

export function endOfDate(value: string): string | undefined {
  if (!value) return undefined
  const date = new Date(`${value}T00:00:00`)
  if (Number.isNaN(date.getTime())) return undefined
  date.setDate(date.getDate() + 1)
  return date.toISOString()
}

export function toDateTimeLocal(value: Date): string {
  const pad = (part: number): string => String(part).padStart(2, '0')
  return `${value.getFullYear()}-${pad(value.getMonth() + 1)}-${pad(value.getDate())}T${pad(value.getHours())}:${pad(value.getMinutes())}`
}

export function newBillingPriceRuleForm(
  rule?: BillingPriceRuleResponse,
  now: Date = new Date(),
): BillingPriceRuleForm {
  if (rule) {
    return {
      price_rule_id: rule.price_rule_id,
      public_model: rule.public_model,
      input_rate: rule.input_rate,
      cache_read_rate: rule.cache_read_rate,
      cache_write_rate: rule.cache_write_rate,
      output_rate: rule.output_rate,
      effective_from: toDateTimeLocal(new Date(rule.effective_from)),
    }
  }
  return {
    public_model: '',
    input_rate: '0',
    cache_read_rate: '0',
    cache_write_rate: '0',
    output_rate: '0',
    effective_from: toDateTimeLocal(now),
  }
}

export function formatBillingAmount(
  value: string | null | undefined,
  currency = BILLING_DEFAULT_CURRENCY,
): string {
  return value == null ? '-' : `${currency} ${value}`
}

export function formatBillingRate(
  value: string,
  currency = BILLING_DEFAULT_CURRENCY,
): string {
  return `${currency} ${value} / M`
}

export function billingFilterQuery(
  filters: BillingChargeFilters,
): BillingChargeFilters {
  const query: BillingChargeFilters = {}
  if (filters.user_id !== undefined) query.user_id = filters.user_id
  if (filters.client_key_id !== undefined) {
    query.client_key_id = filters.client_key_id
  }
  if (filters.requested_model !== undefined) {
    query.requested_model = filters.requested_model
  }
  if (filters.endpoint_id !== undefined) query.endpoint_id = filters.endpoint_id
  if (filters.usage_status !== undefined) {
    query.usage_status = filters.usage_status
  }
  if (filters.pricing_status !== undefined) {
    query.pricing_status = filters.pricing_status
  }
  if (filters.request_id !== undefined) query.request_id = filters.request_id
  if (filters.start_at !== undefined) query.start_at = filters.start_at
  if (filters.end_at !== undefined) query.end_at = filters.end_at
  return query
}

export function billingFilterSignature(filters: BillingChargeFilters): string {
  const query = billingFilterQuery(filters)
  return Object.keys(query)
    .sort()
    .map((key) => `${key}=${String(query[key as keyof BillingChargeFilters])}`)
    .join('&')
}

export function billingPriceRuleRequest(
  form: BillingPriceRuleForm,
): BillingPriceRuleRequest {
  return {
    public_model: form.public_model.trim(),
    input_rate: form.input_rate.trim(),
    cache_read_rate: form.cache_read_rate.trim(),
    cache_write_rate: form.cache_write_rate.trim(),
    output_rate: form.output_rate.trim(),
    effective_from: new Date(form.effective_from).toISOString(),
  }
}

export function billingLastPageFirst(total: number, rows: number): number {
  return total > 0 && rows > 0 ? Math.floor((total - 1) / rows) * rows : 0
}
