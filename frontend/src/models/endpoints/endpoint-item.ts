import type {
  EndpointCatalogRow,
  EndpointTestResponse,
  ProviderEndpoint,
} from '@/generated/admin-api'

import type { EndpointListItemView } from '../endpoints'

export type EndpointViewItemLabels = {
  endpointTestIdle: string
  endpointSourceAuto: string
  endpointSourceDetected: string
  endpointSourceManual: string
  nativeApiChat: string
  nativeApiAnthropicMessages: string
  nativeApiEmbeddings?: string
  nativeApiResponses: string
  nativeApiRealtime: string
  ownerLabel: string
  scopeAdmin: string
  scopeUser: string
}

/**
 * The distinct meta labels of an endpoint row: the scope, the protocol and the
 * protocol source. The automatic-routing phrase resolves to one label for both
 * the protocol and its source, so it is shown once.
 */
export function endpointMetaLabels(
  item: {
    native_api: string
    native_api_label: string
    native_api_source_label: string
    scope_label: string
  },
  embeddingsLabel: string,
): string[] {
  const protocolLabel =
    item.native_api === 'embeddings' ? embeddingsLabel : item.native_api_label
  const labels = [item.scope_label, protocolLabel, item.native_api_source_label]
  return labels.filter(
    (label, index) => Boolean(label) && labels.indexOf(label) === index,
  )
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
    provider: endpoint.provider,
    provider_region: endpoint.provider_region ?? null,
    // R2e.3: carry the plan axis and token presence into the list view so
    // the table/card decide quota visibility from the row alone.
    plan: endpoint.plan ?? 'platform_api_key',
    has_oauth_token: endpoint.has_oauth_token === true,
    base_url: endpoint.base_url,
    scope: endpoint.scope,
    scope_label:
      endpoint.scope === 'user' ? options.scopeUser : options.scopeAdmin,
    native_api: endpoint.native_api,
    native_api_label: nativeApiLabel(endpoint.native_api, options),
    native_api_source: endpoint.native_api_source,
    native_api_source_label: nativeApiSourceLabel(
      endpoint.native_api_source,
      options,
    ),
    enabled: endpoint.enabled,
    mcp_enabled: endpoint.mcp_enabled,
    owner_label: endpoint.owner_user_id
      ? `${options.ownerLabel} ${endpoint.owner_user_id}`
      : '',
    testing: options.testingEndpointId === endpoint.endpoint_id,
    toggling: options.togglingEndpointId === endpoint.endpoint_id,
    test_message: createEndpointTestMessage(
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

/**
 * The list view of one safe catalog row.
 *
 * The safe row carries no upstream address, no credential, no owner and no
 * OAuth state, so the view keeps those fields absent instead of inventing
 * them. The scope is the shared/owned distinction of the safe catalog, and the
 * owner never appears.
 */
export function createCatalogEndpointListItemView(
  row: EndpointCatalogRow,
  options: EndpointViewItemLabels,
): EndpointListItemView {
  return {
    endpoint_id: row.endpoint_id,
    name: row.name,
    provider: row.provider,
    provider_region: row.provider_region ?? null,
    plan: row.plan ?? 'platform_api_key',
    scope: row.visibility,
    scope_label:
      row.visibility === 'owned' ? options.scopeUser : options.scopeAdmin,
    native_api: row.native_api,
    native_api_label: nativeApiLabel(row.native_api, options),
    native_api_source: row.native_api_source,
    native_api_source_label: nativeApiSourceLabel(
      row.native_api_source,
      options,
    ),
    enabled: row.enabled,
    quota_eligible: row.quota_eligible,
    owner_label: '',
    testing: false,
    toggling: false,
    test_message: options.endpointTestIdle,
    test_severity: null,
  }
}

/**
 * The protocol label of one row, shared by the administrator rows and the
 * safe catalog rows.
 */
function nativeApiLabel(
  nativeApi: string,
  options: EndpointViewItemLabels,
): string {
  if (nativeApi === 'auto') return options.endpointSourceAuto
  if (nativeApi === 'chat') return options.nativeApiChat
  if (nativeApi === 'anthropic_messages') return options.nativeApiAnthropicMessages
  if (nativeApi === 'realtime') return options.nativeApiRealtime
  if (nativeApi === 'embeddings')
    return options.nativeApiEmbeddings ?? options.nativeApiResponses
  return options.nativeApiResponses
}

/**
 * The protocol source label of one row, shared by the administrator rows and
 * the safe catalog rows.
 */
function nativeApiSourceLabel(
  nativeApiSource: string,
  options: EndpointViewItemLabels,
): string {
  if (nativeApiSource === 'auto') return options.endpointSourceAuto
  if (nativeApiSource === 'detected') return options.endpointSourceDetected
  return options.endpointSourceManual
}

function createEndpointTestMessage(
  result: EndpointTestResponse | null,
  idleLabel: string,
): string {
  if (!result) return idleLabel
  return `HTTP ${result.status ?? '-'} / ${result.duration_ms}ms / ${result.message}`
}
