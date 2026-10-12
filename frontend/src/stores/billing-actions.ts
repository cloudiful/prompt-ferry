import { computed } from 'vue'
import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  BillingPriceRuleRequest,
  BillingPriceRuleResponse,
} from '@/generated/admin-api'
import {
  createPriceRule,
  deletePriceRule,
  repriceUnpriced,
  setPriceRuleEnabled,
  updatePriceRule,
} from '../api/billing'
import {
  capturePayload,
  pendingRowsById,
  recordActionKey,
} from '@/stores/record-action-types'

export const BILLING_RULE_PREFIX = 'billingrule'
export const BILLING_REPRICE_KEY = 'billing:reprice'

export function billingPriceRuleActionKey(priceRuleId: string | null): string {
  return recordActionKey(BILLING_RULE_PREFIX, priceRuleId)
}

export type BillingActionDeps = {
  guard: AsyncActionGuard
  refreshCharges: () => Promise<void>
  refreshPriceRules: () => Promise<void>
  refreshSummary: () => Promise<void>
}

export function createBillingActions(deps: BillingActionDeps) {
  const { guard, refreshCharges, refreshPriceRules, refreshSummary } = deps

  function savePriceRule(
    body: BillingPriceRuleRequest,
    priceRuleId?: string,
  ): Promise<AsyncActionOutcome<void>> {
    const id = priceRuleId || null
    const captured = capturePayload(body)
    return guard.run(
      billingPriceRuleActionKey(id),
      async () => {
        if (id) await updatePriceRule(id, captured)
        else await createPriceRule(captured)
        await Promise.all([refreshPriceRules(), refreshSummary()])
      },
      'save',
    )
  }

  function removePriceRule(
    priceRuleId: string,
  ): Promise<AsyncActionOutcome<void>> {
    const id = priceRuleId
    return guard.run(
      billingPriceRuleActionKey(id),
      async () => {
        await deletePriceRule(id)
        await Promise.all([
          refreshPriceRules(),
          refreshSummary(),
          refreshCharges(),
        ])
      },
      'delete',
    )
  }

  function togglePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<AsyncActionOutcome<void>> {
    const id = rule.price_rule_id
    const enabled = !rule.enabled
    return guard.run(
      billingPriceRuleActionKey(id),
      async () => {
        await setPriceRuleEnabled(id, enabled)
        await refreshPriceRules()
      },
      'toggle',
    )
  }

  function reprice(): Promise<AsyncActionOutcome<number>> {
    return guard.run(
      BILLING_REPRICE_KEY,
      async () => {
        const result = await repriceUnpriced()
        await Promise.all([
          refreshSummary(),
          refreshCharges(),
          refreshPriceRules(),
        ])
        return result.repriced
      },
      'reprice',
    )
  }

  return {
    createPending: computed(() =>
      guard.isPending(billingPriceRuleActionKey(null)),
    ),
    pendingPriceRuleActions: computed(() =>
      pendingRowsById(guard.pendingActions.value, BILLING_RULE_PREFIX),
    ),
    repricePending: computed(() => guard.isPending(BILLING_REPRICE_KEY)),
    removePriceRule,
    reprice,
    savePriceRule,
    togglePriceRule,
  }
}
