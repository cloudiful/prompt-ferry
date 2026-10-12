import { expect, test } from 'bun:test'

import {
  deferred,
  fetchBillingCharges,
  fetchBillingPriceRules,
  fetchBillingSummary,
  loadBillingStore,
  setPriceRuleEnabled,
  updatePriceRule,
} from './billing-actions.fixtures'

test('the billing store rejects same-rule writes while preserving other rules', async () => {
  const store = await loadBillingStore()
  const gate = deferred()
  updatePriceRule.mockImplementationOnce(async () => {
    await gate.promise
    return {} as never
  })
  const saving = store.savePriceRule({} as never, 'rule-a')

  expect(store.actionPending('billingrule:rule-a')).toBe(true)
  expect(store.pendingPriceRuleActions['rule-a']).toBe('save')
  expect(
    await store.togglePriceRule({
      enabled: true,
      price_rule_id: 'rule-a',
    } as never),
  ).toEqual({ executed: false })
  const other = store.togglePriceRule({
    enabled: true,
    price_rule_id: 'rule-b',
  } as never)
  expect(store.actionPending('billingrule:rule-b')).toBe(true)

  gate.resolve()
  await Promise.all([saving, other])
  expect(updatePriceRule).toHaveBeenCalledTimes(1)
  expect(setPriceRuleEnabled).toHaveBeenCalledTimes(1)
  expect(fetchBillingPriceRules).toHaveBeenCalled()
  expect(fetchBillingSummary).toHaveBeenCalled()
  expect(fetchBillingCharges).not.toHaveBeenCalled()
})
