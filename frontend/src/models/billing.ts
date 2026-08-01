import type {
  BillingChargeResponse,
  BillingPriceRuleResponse,
  BillingSummaryResponse,
  EndpointPageResponse,
} from '../generated/admin-api'

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
  price_side: 'cost' | 'sale'
  public_model: string
  endpoint_id: string
  upstream_model: string
  input_rate: string
  cache_read_rate: string
  cache_write_rate: string
  output_rate: string
  effective_from: string
}

export type BillingWorkspace = {
  summary: BillingSummaryResponse | null
  charges: BillingChargeResponse[]
  total: number
  price_rules: BillingPriceRuleResponse[]
  endpoints: EndpointPageResponse['endpoints']
}

export const BILLING_PAGE_SIZE_OPTIONS = [10, 25, 50, 100]

export function currentMonthDate(): string {
  const date = new Date()
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-01`
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

export function newBillingPriceRuleForm(
  rule?: BillingPriceRuleResponse,
): BillingPriceRuleForm {
  if (rule) {
    const effectiveFrom = new Date(rule.effective_from)
    return {
      price_rule_id: rule.price_rule_id,
      price_side: rule.price_side as 'cost' | 'sale',
      public_model: rule.public_model ?? '',
      endpoint_id: rule.endpoint_id ?? '',
      upstream_model: rule.upstream_model ?? '',
      input_rate: rule.input_rate,
      cache_read_rate: rule.cache_read_rate,
      cache_write_rate: rule.cache_write_rate,
      output_rate: rule.output_rate,
      effective_from: toDateTimeLocal(effectiveFrom),
    }
  }
  return {
    price_side: 'sale',
    public_model: '',
    endpoint_id: '',
    upstream_model: '',
    input_rate: '0',
    cache_read_rate: '0',
    cache_write_rate: '0',
    output_rate: '0',
    effective_from: toDateTimeLocal(new Date()),
  }
}

function toDateTimeLocal(value: Date): string {
  const pad = (part: number): string => String(part).padStart(2, '0')
  return `${value.getFullYear()}-${pad(value.getMonth() + 1)}-${pad(value.getDate())}T${pad(value.getHours())}:${pad(value.getMinutes())}`
}

export function formatBillingAmount(
  value: string | null | undefined,
  currency = 'CNY',
): string {
  return value == null ? '-' : `${currency} ${value}`
}

export function formatBillingRate(value: string, currency = 'CNY'): string {
  return `${currency} ${value} / M`
}

export function formatBillingTime(value: string): string {
  return new Date(value).toLocaleString()
}

export function formatTokenCount(value: number): string {
  return new Intl.NumberFormat().format(value)
}
