import { computed, type Ref } from 'vue'

import type {
  BillingPriceRuleRequest,
  BillingPriceRuleResponse,
} from '@/generated/admin-api'
import type { BillingPriceRuleForm } from '@/models/billing'
import type { useBillingStore } from '@/stores/billing'
import { billingPriceRuleActionKey } from '@/stores/billing-actions'
import type { useLocale } from '@/composables/useLocale'

export type BillingStore = ReturnType<typeof useBillingStore>
type Translate = ReturnType<typeof useLocale>['t']

export function useBillingPageActions(options: {
  billing: BillingStore
  priceRuleForm: Ref<BillingPriceRuleForm>
  priceRuleVisible: Ref<boolean>
  confirm: (message: string) => boolean
  notifyApiError: (cause: unknown) => void
  notifySuccess: (message: string) => void
  t: Translate
}) {
  const {
    billing,
    confirm,
    notifyApiError,
    notifySuccess,
    priceRuleForm,
    priceRuleVisible,
    t,
  } = options

  const priceRulePending = computed(() => {
    const id = priceRuleForm.value.price_rule_id
    return billing.actionPending(billingPriceRuleActionKey(id ?? null))
  })
  const priceRuleSavePending = computed(() => {
    const id = priceRuleForm.value.price_rule_id
    return id
      ? billing.pendingPriceRuleActions[id] === 'save'
      : billing.createPending
  })

  async function savePriceRule(): Promise<void> {
    const formIdentity = priceRuleForm.value
    const form = { ...formIdentity }
    const priceRuleId = form.price_rule_id
    try {
      const body: BillingPriceRuleRequest = {
        public_model: form.public_model.trim(),
        input_rate: form.input_rate.trim(),
        cache_read_rate: form.cache_read_rate.trim(),
        cache_write_rate: form.cache_write_rate.trim(),
        output_rate: form.output_rate.trim(),
        effective_from: new Date(form.effective_from).toISOString(),
      }
      const outcome = await billing.savePriceRule(body, priceRuleId)
      if (!outcome.executed) return
      if (priceRuleForm.value === formIdentity) priceRuleVisible.value = false
      notifySuccess(priceRuleId ? t('priceRuleUpdated') : t('priceRuleCreated'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function togglePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    try {
      const outcome = await billing.togglePriceRule(rule)
      if (!outcome.executed) return
      notifySuccess(
        rule.enabled ? t('priceRuleDisabled') : t('priceRuleEnabled'),
      )
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function reprice(): Promise<void> {
    try {
      const outcome = await billing.reprice()
      if (outcome.executed) {
        notifySuccess(t('repriced', { count: outcome.value }))
      }
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deletePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    if (billing.actionPending(billingPriceRuleActionKey(rule.price_rule_id))) {
      return
    }
    if (!confirm(t('priceRuleDeleteConfirm'))) return
    try {
      const outcome = await billing.removePriceRule(rule.price_rule_id)
      if (outcome.executed) notifySuccess(t('priceRuleDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return {
    deletePriceRule,
    priceRulePending,
    priceRuleSavePending,
    reprice,
    savePriceRule,
    togglePriceRule,
  }
}
