import { expect, mock, test } from 'bun:test'
import { ref } from 'vue'

import { useBillingPageActions } from '../src/composables/useBillingPageActions'
import type { BillingPriceRuleForm } from '../src/models/billing'
import type { useBillingStore } from '../src/stores/billing'

function harness(savePriceRule: (body: unknown) => Promise<unknown>) {
  const form = ref<BillingPriceRuleForm>({
    cache_read_rate: '2',
    cache_write_rate: '3',
    effective_from: '2026-10-12T12:00',
    input_rate: '1',
    output_rate: '4',
    public_model: 'model-a',
  })
  const visible = ref(true)
  const notifyApiError = mock(() => {})
  const notifySuccess = mock(() => {})
  const billing = {
    actionPending: () => false,
    createPending: false,
    pendingPriceRuleActions: {},
    savePriceRule,
  } as unknown as ReturnType<typeof useBillingStore>
  const actions = useBillingPageActions({
    billing,
    confirm: () => true,
    notifyApiError,
    notifySuccess,
    priceRuleForm: form,
    priceRuleVisible: visible,
    t: (key) => key,
  })
  return { actions, form, notifyApiError, notifySuccess, visible }
}

test('a refused duplicate does not close the rule dialog or report success', async () => {
  const save = mock(async () => ({ executed: false }))
  const h = harness(save)

  await h.actions.savePriceRule()
  expect(save).toHaveBeenCalledTimes(1)
  expect(h.visible.value).toBe(true)
  expect(h.notifySuccess).not.toHaveBeenCalled()
  expect(h.notifyApiError).not.toHaveBeenCalled()
})

test('a late save reports once without closing a replacement draft', async () => {
  let resolve!: (value: unknown) => void
  const save = mock(() => new Promise<unknown>((settle) => (resolve = settle)))
  const h = harness(save)
  const saving = h.actions.savePriceRule()
  h.form.value = { ...h.form.value, public_model: 'new draft' }
  resolve({ executed: true, value: undefined })
  await saving

  expect(h.visible.value).toBe(true)
  expect(h.form.value.public_model).toBe('new draft')
  expect(h.notifySuccess).toHaveBeenCalledTimes(1)
})
