import { expect, test } from 'bun:test'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '../src/admin-mappers/forms/model-route'
import {
  hasTargetServiceTier,
  hasTargetSettings,
} from '../src/components/endpoints/modelRouteTargetHelpers'
import type { ModelEndpointRule } from '../src/generated/admin-api'

function routeFixture(
  service_tier: string | null | undefined,
): ModelEndpointRule {
  return {
    created_at: '2026-09-04T00:00:00Z',
    enabled: true,
    model_pattern: 'test-*',
    rule_id: 'rule-1',
    scope: 'admin',
    updated_at: '2026-09-04T00:00:00Z',
    targets: [
      {
        created_at: '2026-09-04T00:00:00Z',
        enabled: true,
        endpoint_enabled: true,
        endpoint_id: 'endpoint-1',
        position: 0,
        rule_id: 'rule-1',
        service_tier,
        target_id: 'target-1',
        updated_at: '2026-09-04T00:00:00Z',
      },
    ],
  } as ModelEndpointRule
}

test('new model-route target forms default to inherit', () => {
  expect(createEmptyModelRouteForm().targets[0]?.service_tier).toBeNull()
})

test('modelRouteToForm preserves a configured target tier', () => {
  expect(
    modelRouteToForm(routeFixture('priority')).targets[0]?.service_tier,
  ).toBe('priority')
  // Free-form values survive so provider-specific vocabularies (e.g.
  // OpenAI `fast`) are never coerced.
  expect(modelRouteToForm(routeFixture('fast')).targets[0]?.service_tier).toBe(
    'fast',
  )
  expect(
    modelRouteToForm(routeFixture('standard')).targets[0]?.service_tier,
  ).toBe('standard')
})

test('modelRouteToForm inherits missing, null and blank target tiers', () => {
  expect(
    modelRouteToForm(routeFixture(null)).targets[0]?.service_tier,
  ).toBeNull()
  expect(modelRouteToForm(routeFixture('')).targets[0]?.service_tier).toBeNull()
  expect(
    modelRouteToForm(routeFixture('   ')).targets[0]?.service_tier,
  ).toBeNull()
  const rule = routeFixture('priority')
  const { service_tier, ...bareTarget } = rule.targets[0] ?? {}
  void service_tier
  const withoutTier = {
    ...rule,
    targets: [bareTarget],
  } as unknown as ModelEndpointRule
  expect(modelRouteToForm(withoutTier).targets[0]?.service_tier).toBeNull()
})

test('modelRouteFormToRequest round-trips the target tier', () => {
  // Issue #637: always sent (null means inherit) so a save or enable
  // toggle preserves a configured override instead of clearing it.
  const request = modelRouteFormToRequest(
    modelRouteToForm(routeFixture('priority')),
  )
  expect(request.targets?.[0]?.service_tier).toBe('priority')
  const blank = modelRouteToForm(routeFixture('priority'))
  blank.targets[0].service_tier = '   '
  expect(modelRouteFormToRequest(blank).targets?.[0]?.service_tier).toBeNull()
  const inherit = modelRouteFormToRequest(modelRouteToForm(routeFixture(null)))
  expect(inherit.targets?.[0]?.service_tier).toBeNull()
})

test('a configured target tier lights up the target settings gear', () => {
  const target = createEmptyModelRouteForm().targets[0]
  if (!target) throw new Error('expected a default target')
  expect(hasTargetServiceTier(target)).toBe(false)
  expect(hasTargetSettings(target)).toBe(false)

  target.service_tier = 'priority'
  expect(hasTargetServiceTier(target)).toBe(true)
  expect(hasTargetSettings(target)).toBe(true)

  // Whitespace-only is inherit, so it must not highlight the gear.
  target.service_tier = '   '
  expect(hasTargetServiceTier(target)).toBe(false)
  expect(hasTargetSettings(target)).toBe(false)

  target.service_tier = null
  expect(hasTargetSettings(target)).toBe(false)
})
