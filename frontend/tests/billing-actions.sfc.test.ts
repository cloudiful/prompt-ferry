import { expect, test } from 'bun:test'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

test('billing mutations bind owner pending state to their controls', async () => {
  const [page, rules, dialog] = await Promise.all([
    source('../src/pages/BillingPage.vue'),
    source('../src/components/billing/BillingPriceRulesPanel.vue'),
    source('../src/components/billing/BillingPriceRuleDialog.vue'),
  ])

  expect(page).toContain(':pending-actions="billing.pendingPriceRuleActions"')
  expect(page).toContain(':create-pending="billing.createPending"')
  expect(page).toContain(':busy="priceRulePending"')
  expect(page).toContain(':save-pending="priceRuleSavePending"')
  expect(rules).toContain(
    "pendingActions[row.original.price_rule_id] === 'delete'",
  )
  expect(rules).toContain(
    "pendingActions[row.original.price_rule_id] === 'toggle'",
  )
  expect(rules).toContain(
    ':disabled="Boolean(pendingActions[row.original.price_rule_id])"',
  )
  expect(dialog).toContain(':dismissible="!busy"')
  expect(dialog).toContain(':close="!busy"')
  expect(dialog).toContain(':loading="savePending"')
})

test('billing details show their loading state before the request settles', async () => {
  const [page, store, dialog] = await Promise.all([
    source('../src/pages/BillingPage.vue'),
    source('../src/stores/billing.ts'),
    source('../src/components/billing/BillingChargeDetailDialog.vue'),
  ])

  expect(page).toContain('detailVisible.value = true')
  expect(page).toContain(':loading="billing.detailLoading"')
  expect(store).toContain('detail.value = null')
  expect(store).toContain('detailLoading.value = true')
  expect(dialog).toContain('v-if="loading"')
})
