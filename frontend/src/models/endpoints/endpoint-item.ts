import type {
  EndpointTestResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'

import type { EndpointListItemView } from '../endpoints'

export type EndpointViewItemLabels = {
  activeLabel: string
  disabledLabel: string
  endpointProbeIdle: string
  endpointSourceAuto: string
  endpointSourceManual: string
  nativeApiChat: string
  nativeApiAnthropicMessages: string
  nativeApiResponses: string
  nativeApiRealtime: string
  ownerLabel: string
  scopeAdmin: string
  scopeUser: string
}

export function createEndpointListItemView(
  endpoint: ProviderEndpoint,
  options: EndpointViewItemLabels & {
    testResult: EndpointTestResponse | null
    testingEndpointId: string
    togglingEndpointId: string
  },
): EndpointListItemView {
  return {
    endpoint_id: endpoint.endpoint_id,
    name: endpoint.name,
    base_url: endpoint.base_url,
    scope: endpoint.scope,
    scope_label:
      endpoint.scope === 'user' ? options.scopeUser : options.scopeAdmin,
    native_api: endpoint.native_api,
    native_api_label:
      endpoint.native_api === 'chat'
        ? options.nativeApiChat
        : endpoint.native_api === 'anthropic_messages'
          ? options.nativeApiAnthropicMessages
          : endpoint.native_api === 'realtime'
            ? options.nativeApiRealtime
            : options.nativeApiResponses,
    native_api_source: endpoint.native_api_source,
    native_api_source_label:
      endpoint.native_api_source === 'manual'
        ? options.endpointSourceManual
        : options.endpointSourceAuto,
    enabled: endpoint.enabled,
    enabled_label: endpoint.enabled
      ? options.activeLabel
      : options.disabledLabel,
    owner_label: endpoint.owner_user_id
      ? `${options.ownerLabel} ${endpoint.owner_user_id}`
      : '',
    testing: options.testingEndpointId === endpoint.endpoint_id,
    toggling: options.togglingEndpointId === endpoint.endpoint_id,
    test_message: createEndpointTestMessage(
      options.testResult,
      options.endpointProbeIdle,
    ),
    test_severity: options.testResult
      ? options.testResult.ok
        ? 'success'
        : 'error'
      : null,
  }
}

function createEndpointTestMessage(
  result: EndpointTestResponse | null,
  idleLabel: string,
): string {
  if (!result) return idleLabel
  return `HTTP ${result.status ?? '-'} / ${result.duration_ms}ms / ${result.message}`
}
