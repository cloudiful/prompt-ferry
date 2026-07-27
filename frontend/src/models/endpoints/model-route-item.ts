import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
} from '@/generated/admin-api'

import type { ModelRouteListItemView } from '../endpoints'

export type ModelRouteViewItemLabels = {
  activeLabel: string
  disabledLabel: string
  endpointTestIdle: string
  ownerLabel: string
  routingStrategyClientKey: string
  routingStrategySessionAffinity: string
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
  return {
    rule_id: route.rule_id,
    model_pattern: route.model_pattern,
    scope: route.scope,
    scope_label:
      route.scope === 'user' ? options.scopeUser : options.scopeAdmin,
    enabled: route.enabled,
    enabled_label: route.enabled ? options.activeLabel : options.disabledLabel,
    owner_label: route.owner_user_id
      ? `${options.ownerLabel} ${route.owner_user_id}`
      : '',
    routing_strategy_label:
      route.routing_strategy === 'responses_session_affinity'
        ? options.routingStrategySessionAffinity
        : options.routingStrategyClientKey,
    targets: route.targets.map((target) => ({
      target_id: target.target_id,
      endpoint_label: target.endpoint_name || target.endpoint_id,
      endpoint_enabled: target.endpoint_enabled,
      upstream_model: target.upstream_model ?? null,
    })),
    testing: options.testingModelRouteId === route.rule_id,
    toggling: options.togglingModelRouteId === route.rule_id,
    test_message: createModelRouteTestMessage(
      options.testResult,
      options.endpointTestIdle,
    ),
    test_severity: options.testResult
      ? options.testResult.ok
        ? 'success'
        : 'error'
      : null,
  }
}

function createModelRouteTestMessage(
  result: ModelRouteTestResponse | null,
  idleLabel: string,
): string {
  if (!result) return idleLabel
  return `HTTP ${result.status ?? '-'} / ${result.duration_ms}ms / ${result.preferred_endpoint_name || '-'} -> ${result.endpoint_name || '-'} / ${result.model || result.model_pattern || '-'} / ${result.message}`
}
