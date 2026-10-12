import { expect, test } from 'bun:test'

import type { BillingPriceRuleResponse } from '../src/generated/admin-api'
import { useAsyncAction } from '../src/composables/useAsyncAction'
import {
  deferred,
  freshRefreshes,
  loadBillingActions,
  updatePriceRule,
} from './billing-actions.fixtures'

const { createBillingActions } = await loadBillingActions()

const rule = (id: string, enabled = true) =>
  ({ enabled, price_rule_id: id }) as BillingPriceRuleResponse

test('price-rule writes share a key through refresh and keep other rows usable', async () => {
  const refreshGate = deferred()
  const { deps, refreshes } = freshRefreshes(() => refreshGate.promise)
  const guard = useAsyncAction()
  const actions = createBillingActions({ guard, ...deps })
  const body = {
    public_model: 'first',
    input_rate: '1',
    cache_read_rate: '2',
    cache_write_rate: '3',
    output_rate: '4',
    effective_from: '2026-10-12T00:00:00.000Z',
  }

  const saving = actions.savePriceRule(body, 'rule-a')
  body.public_model = 'edited during save'

  expect(guard.pendingAction('billingrule:rule-a')).toBe('save')
  expect(await actions.removePriceRule('rule-a')).toEqual({ executed: false })
  expect(await actions.togglePriceRule(rule('rule-a'))).toEqual({
    executed: false,
  })
  const otherRule = actions.togglePriceRule(rule('rule-b'))
  expect(guard.isPending('billingrule:rule-b')).toBe(true)
  expect(updatePriceRule.mock.calls[0]?.[1]).toMatchObject({
    public_model: 'first',
  })

  refreshGate.resolve()
  const [saveResult, otherResult] = await Promise.all([saving, otherRule])
  expect(saveResult).toMatchObject({ executed: true })
  expect(otherResult).toMatchObject({ executed: true })
  expect(refreshes.priceRules).toBe(2)
  expect(refreshes.summary).toBe(1)
  expect(guard.isPending('billingrule:rule-a')).toBe(false)
})

test('create and reprice use independent pending slots and rejections release them', async () => {
  const refreshGate = deferred()
  const { deps } = freshRefreshes(() => refreshGate.promise)
  const guard = useAsyncAction()
  const actions = createBillingActions({ guard, ...deps })

  const creating = actions.savePriceRule({} as never)
  expect(guard.pendingAction('billingrule:create')).toBe('save')
  expect(await actions.savePriceRule({} as never)).toEqual({ executed: false })
  const repricing = actions.reprice()
  expect(guard.pendingAction('billing:reprice')).toBe('reprice')
  expect(repricing).toBeDefined()

  refreshGate.resolve()
  await expect(creating).resolves.toMatchObject({ executed: true })
  await expect(repricing).resolves.toMatchObject({
    executed: true,
    value: 3,
  })
  expect(guard.pendingAction('billingrule:create')).toBeUndefined()
  expect(guard.pendingAction('billing:reprice')).toBeUndefined()
})

test('a failed price-rule request releases its key for retry', async () => {
  const { deps } = freshRefreshes()
  const guard = useAsyncAction()
  const actions = createBillingActions({ guard, ...deps })
  updatePriceRule.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(actions.savePriceRule({} as never, 'rule-a')).rejects.toThrow(
    'rejected',
  )
  expect(guard.isPending('billingrule:rule-a')).toBe(false)
  await expect(
    actions.savePriceRule({} as never, 'rule-a'),
  ).resolves.toMatchObject({ executed: true })
})
