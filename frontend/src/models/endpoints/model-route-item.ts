import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
  ModelRouteTestTargetResult,
} from '@/generated/admin-api'

import type {
  ModelRouteListItemView,
  ModelRouteTargetItemView,
} from '../endpoints'

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
      enabled: target.enabled,
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
 * How many target pairs a route row shows before the rest move into the
 * disclosure.
 */
export const ROUTE_TARGET_PREVIEW_LIMIT = 2

/**
 * The configured targets a route row may show: the target itself is enabled
 * and its endpoint still exists and is enabled. A target on a deleted or
 * disabled endpoint is never listed, and the configured order is kept.
 */
export function enabledRouteTargets(
  targets: readonly ModelRouteTargetItemView[],
): ModelRouteTargetItemView[] {
  return targets.filter((target) => target.enabled && target.endpoint_enabled)
}

/**
 * The pairs shown inline, at most {@link ROUTE_TARGET_PREVIEW_LIMIT}.
 */
export function routeTargetPreview(
  targets: readonly ModelRouteTargetItemView[],
): ModelRouteTargetItemView[] {
  return enabledRouteTargets(targets).slice(0, ROUTE_TARGET_PREVIEW_LIMIT)
}

/**
 * How many enabled targets the disclosure still holds.
 */
export function hiddenRouteTargetCount(
  targets: readonly ModelRouteTargetItemView[],
): number {
  return Math.max(
    0,
    enabledRouteTargets(targets).length - ROUTE_TARGET_PREVIEW_LIMIT,
  )
}

/**
 * The upstream model override of a target, or null when it has none. A blank
 * override is the same as no override.
 */
export function routeTargetOverride(
  target: ModelRouteTargetItemView,
): string | null {
  const override = target.upstream_model?.trim() ?? ''
  return override === '' ? null : override
}

/**
 * The full name of a target pair, endpoint and model override together.
 */
export function routeTargetPairLabel(target: ModelRouteTargetItemView): string {
  const override = routeTargetOverride(target)
  return override
    ? `${target.endpoint_label} · ${override}`
    : target.endpoint_label
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
