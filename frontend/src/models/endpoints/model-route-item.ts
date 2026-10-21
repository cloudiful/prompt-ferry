import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
  ModelRouteTestTargetResult,
} from '@/generated/admin-api'

import type { ModelRouteListItemView } from '../endpoints'

export type ModelRouteViewItemLabels = {
  endpointTestIdle: string
  modelRouteTestNoneEligible: string
  modelRouteTestPassed: (passed: number, total: number) => string
  ownerLabel: string
  scopeAdmin: string
  scopeUser: string
}

export function createModelRouteListItemView(
  route: ModelEndpointRule,
  options: ModelRouteViewItemLabels & {
    testResult: ModelRouteTestResponse | null
    testingModelRouteId: string
    togglingModelRouteId: string
  },
): ModelRouteListItemView {
  const result = options.testResult
  return {
    rule_id: route.rule_id,
    model_pattern: route.model_pattern,
    scope: route.scope,
    scope_label:
      route.scope === 'user' ? options.scopeUser : options.scopeAdmin,
    enabled: route.enabled,
    owner_label: route.owner_user_id
      ? `${options.ownerLabel} ${route.owner_user_id}`
      : '',
    targets: route.targets.map((target) => ({
      target_id: target.target_id,
      endpoint_label: target.endpoint_name || target.endpoint_id,
      endpoint_enabled: target.endpoint_enabled,
      upstream_model: target.upstream_model ?? null,
    })),
    testing: options.testingModelRouteId === route.rule_id,
    toggling: options.togglingModelRouteId === route.rule_id,
    test_message: createModelRouteTestMessage(result, options),
    test_severity: modelRouteTestSeverity(result),
    test_results: result?.results ?? [],
  }
}

/**
 * The tone of a model-route test.
 *
 * Every tested target passing is green, some of them passing is amber, and no
 * target passing is red. A route without an eligible target is amber as well:
 * nothing was tested, so it is never a green result. An idle row stays
 * neutral.
 */
export function modelRouteTestSeverity(
  result: ModelRouteTestResponse | null,
): 'success' | 'warning' | 'error' | null {
  if (!result) return null
  if (result.total === 0) return 'warning'
  if (result.passed === result.total) return 'success'
  if (result.passed === 0) return 'error'
  return 'warning'
}

/**
 * The passed and total counts of a test, as the row shows them.
 */
export function modelRouteTestCounts(results: ModelRouteTestTargetResult[]): {
  passed: number
  total: number
} {
  return {
    passed: results.filter((result) => result.ok).length,
    total: results.length,
  }
}

function createModelRouteTestMessage(
  result: ModelRouteTestResponse | null,
  options: Pick<
    ModelRouteViewItemLabels,
    'endpointTestIdle' | 'modelRouteTestNoneEligible' | 'modelRouteTestPassed'
  >,
): string {
  if (!result) return options.endpointTestIdle
  if (result.total === 0) return options.modelRouteTestNoneEligible
  const counts = options.modelRouteTestPassed(result.passed, result.total)
  // A failed outcome carries the reason the operator needs, so it leads the
  // summary whenever not every target passed.
  const representative =
    result.results.find((entry) => !entry.ok) ??
    result.results.find((entry) => entry.preferred) ??
    result.results[0]
  if (!representative) return counts
  return `${counts} / ${representative.model ?? '-'} / ${representative.message}`
}
