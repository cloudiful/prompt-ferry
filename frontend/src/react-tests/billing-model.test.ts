import { describe, expect, it } from 'vitest'
import {
  BILLING_CHARGE_PAGE_SIZE_FALLBACK,
  BILLING_CHARGE_PAGE_SIZE_KEY,
  BILLING_DEFAULT_CURRENCY,
  BILLING_PAGE_SIZE_OPTIONS,
  BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK,
  BILLING_PRICE_RULE_PAGE_SIZE_KEY,
  BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  billingFilterQuery,
  billingFilterSignature,
  billingLastPageFirst,
  billingPriceRuleRequest,
  currentMonthDate,
  dateInputValue,
  endOfDate,
  formatBillingAmount,
  formatBillingRate,
  newBillingPriceRuleForm,
  startOfDate,
  toDateTimeLocal,
} from '@/lib/billing-model'
import type { BillingPriceRuleResponse } from '@/generated/admin-api'

function rule(
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

describe('billing model', () => {
  it('keeps the page size contracts of the charge and price rule pagers', () => {
    expect(BILLING_PAGE_SIZE_OPTIONS).toEqual([10, 25, 50, 100])
    expect(BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS).toEqual([10, 20, 50])
    expect(BILLING_CHARGE_PAGE_SIZE_FALLBACK).toBe(25)
    expect(BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK).toBe(10)
    expect(BILLING_CHARGE_PAGE_SIZE_KEY).toBe('billing')
    expect(BILLING_PRICE_RULE_PAGE_SIZE_KEY).toBe('billing-price-rules')
    expect(BILLING_DEFAULT_CURRENCY).toBe('CNY')
  })

  it('reads the local current month and the date input value', () => {
    const now = new Date(2026, 9, 24, 13, 45)
    expect(currentMonthDate(now)).toBe('2026-10-01')
    expect(currentMonthDate(new Date(2026, 0, 31))).toBe('2026-01-01')
    expect(dateInputValue('2026-10-24T00:00:00.000Z')).toBe('2026-10-24')
    expect(dateInputValue('')).toBe('')
  })

  it('converts period boundaries with the local calendar', () => {
    expect(startOfDate('2026-10-24')).toBe(
      new Date('2026-10-24T00:00:00').toISOString(),
    )
    expect(endOfDate('2026-10-24')).toBe(
      new Date('2026-10-25T00:00:00').toISOString(),
    )
    expect(endOfDate('2026-01-31')).toBe(
      new Date('2026-02-01T00:00:00').toISOString(),
    )
    expect(startOfDate('')).toBeUndefined()
    expect(endOfDate('')).toBeUndefined()
    expect(startOfDate('not-a-date')).toBeUndefined()
    expect(endOfDate('not-a-date')).toBeUndefined()
  })

  it('keeps the exclusive end on the next local midnight across a daylight saving change', () => {
    const start = startOfDate('2026-03-08')
    const end = endOfDate('2026-03-08')
    expect(start).toBeDefined()
    expect(end).toBeDefined()
    const hours = (Date.parse(end!) - Date.parse(start!)) / 3_600_000
    expect([23, 24, 25]).toContain(hours)
    expect(end).toBe(new Date('2026-03-09T00:00:00').toISOString())
  })

  it('builds a new price rule form with the default rates and a local effective date', () => {
    const now = new Date(2026, 9, 24, 8, 5)
    const form = newBillingPriceRuleForm(undefined, now)
    expect(form).toEqual({
      effective_from: toDateTimeLocal(now),
      input_rate: '0',
      cache_read_rate: '0',
      cache_write_rate: '0',
      output_rate: '0',
      public_model: '',
    })
  })

  it('round-trips an existing price rule into the edit form', () => {
    const source = rule()
    const form = newBillingPriceRuleForm(source)
    expect(form).toEqual({
      price_rule_id: 'price-rule-1',
      public_model: 'fixture-model',
      input_rate: '1',
      cache_read_rate: '0.2',
      cache_write_rate: '0.4',
      output_rate: '3',
      effective_from: toDateTimeLocal(new Date(source.effective_from)),
    })
  })

  it('formats amounts and rates without inventing an exchange rate', () => {
    expect(formatBillingAmount(null)).toBe('-')
    expect(formatBillingAmount(undefined)).toBe('-')
    expect(formatBillingAmount('1.25')).toBe('CNY 1.25')
    expect(formatBillingAmount('1.25', 'USD')).toBe('USD 1.25')
    expect(formatBillingRate('0.5')).toBe('CNY 0.5 / M')
    expect(formatBillingRate('0.5', 'USD')).toBe('USD 0.5 / M')
  })

  it('omits undefined filters and keeps the remaining values', () => {
    expect(
      billingFilterQuery({
        user_id: 3,
        client_key_id: undefined,
        requested_model: 'fixture-model',
        endpoint_id: undefined,
        usage_status: 'known',
        pricing_status: undefined,
        request_id: 'req-1',
        start_at: '2026-10-01T00:00:00.000Z',
        end_at: undefined,
      }),
    ).toEqual({
      user_id: 3,
      requested_model: 'fixture-model',
      usage_status: 'known',
      request_id: 'req-1',
      start_at: '2026-10-01T00:00:00.000Z',
    })
  })

  it('signs the filters in a stable order for the query key', () => {
    const signature = billingFilterSignature({
      requested_model: 'fixture-model',
      user_id: 3,
    })
    expect(signature).toBe('requested_model=fixture-model&user_id=3')
    expect(
      billingFilterSignature({ user_id: 3, requested_model: 'fixture-model' }),
    ).toBe(signature)
  })

  it('normalizes the price rule request to the five writable fields', () => {
    const body = billingPriceRuleRequest({
      price_rule_id: 'price-rule-1',
      public_model: '  fixture-model  ',
      input_rate: ' 1 ',
      cache_read_rate: ' 0.2 ',
      cache_write_rate: ' 0.4 ',
      output_rate: ' 3 ',
      effective_from: '2026-10-24T08:05',
    })
    expect(body).toEqual({
      public_model: 'fixture-model',
      input_rate: '1',
      cache_read_rate: '0.2',
      cache_write_rate: '0.4',
      output_rate: '3',
      effective_from: new Date('2026-10-24T08:05').toISOString(),
    })
    expect(Object.keys(body).sort()).toEqual([
      'cache_read_rate',
      'cache_write_rate',
      'effective_from',
      'input_rate',
      'output_rate',
      'public_model',
    ])
  })

  it('computes the last valid page for an empty out-of-range page', () => {
    expect(billingLastPageFirst(0, 10)).toBe(0)
    expect(billingLastPageFirst(25, 10)).toBe(20)
    expect(billingLastPageFirst(100, 25)).toBe(75)
    expect(billingLastPageFirst(5, 10)).toBe(0)
  })
})
