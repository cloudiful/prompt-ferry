import type {
  EndpointRequest,
  MinimaxServiceTier,
  ProviderEndpoint,
} from '../../generated/admin-api'
import type { EndpointForm } from '../../models'

// Backend treats omitted/unknown tiers as standard; keep the form in sync so
// legacy endpoints round-trip without changing behavior.
export function normalizeServiceTier(
  value: MinimaxServiceTier | string | null | undefined,
): MinimaxServiceTier {
  return value === 'priority' ? 'priority' : 'standard'
}

export function createEmptyEndpointForm(): EndpointForm {
  return {
    endpoint_id: '',
    scope: 'admin',
    owner_user_id: null,
    name: '',
    provider: 'generic',
    provider_region: null,
    service_tier: 'standard',
    base_url: '',
    api_keys: [
      {
        key_label: '',
        api_key: '',
        has_saved_key: false,
        enabled: true,
        key_id: '',
      },
    ],
    key_lb_enabled: false,
    protocol_mode: 'auto',
    native_api_override: null,
    daily_max_requests: null,
    monthly_max_requests: null,
    enabled: true,
    mcp_enabled: false,
    // Issue #368 Phase C: masked proxy default; empty + no saved means direct.
    proxy_url: '',
    has_saved_proxy_url: false,
  }
}

export function endpointToForm(endpoint: ProviderEndpoint): EndpointForm {
  const endpointApiKeys = endpoint.api_keys ?? []
  const nativeApiOverride =
    endpoint.native_api === 'anthropic_messages' ||
    endpoint.native_api === 'responses' ||
    endpoint.native_api === 'chat' ||
    endpoint.native_api === 'realtime'
      ? endpoint.native_api
      : null
  return {
    endpoint_id: endpoint.endpoint_id,
    scope: endpoint.scope === 'user' ? 'user' : 'admin',
    owner_user_id: endpoint.owner_user_id ?? null,
    name: endpoint.name,
    provider: endpoint.provider ?? 'generic',
    provider_region: endpoint.provider_region ?? null,
    service_tier: normalizeServiceTier(endpoint.service_tier),
    base_url: endpoint.base_url,
    api_keys: endpointApiKeys.map((key) => ({
      key_label: key.key_label,
      api_key: '',
      has_saved_key: true,
      enabled: key.enabled,
      key_id: key.key_id,
    })),
    key_lb_enabled: endpoint.key_lb_enabled ?? false,
    protocol_mode:
      endpoint.native_api_source === 'auto' && endpoint.native_api === 'auto'
        ? 'auto'
        : 'manual',
    native_api_override: nativeApiOverride,
    daily_max_requests: endpoint.daily_max_requests ?? null,
    monthly_max_requests: endpoint.monthly_max_requests ?? null,
    enabled: endpoint.enabled,
    mcp_enabled: endpoint.mcp_enabled ?? false,
    // Issue #368 Phase C: masked proxy input; never echo the secret.
    // `has_proxy_url` is optional for legacy payloads (missing means direct).
    proxy_url: '',
    has_saved_proxy_url: endpoint.has_proxy_url ?? false,
  }
}

export function endpointFormToRequest(form: EndpointForm): EndpointRequest {
  // Issue #368 Phase C: omit-when-untouched to avoid the P2 secret-wipe.
  // Non-empty means replace; empty + saved means omit (keep stored value);
  // empty + no saved means clear to direct (`""`). The request-side
  // `has_proxy_url` hint is intentionally not sent (server ignores it).
  const trimmedProxy = form.proxy_url.trim()
  const proxy_url =
    trimmedProxy !== ''
      ? trimmedProxy
      : form.has_saved_proxy_url
        ? undefined
        : ''
  return {
    api_key: form.api_keys[0]?.api_key ?? '',
    api_keys: form.api_keys
      .map((key) => ({
        key_label: key.key_label.trim(),
        api_key: key.api_key,
        enabled: key.enabled,
        key_id: key.key_id || undefined,
      }))
      .filter((key) => key.key_label || key.api_key),
    key_lb_enabled: form.key_lb_enabled,
    base_url: form.base_url.trim(),
    enabled: form.enabled,
    name: form.name.trim(),
    provider: form.provider,
    provider_region: form.provider_region,
    service_tier: normalizeServiceTier(form.service_tier),
    native_api_override:
      form.protocol_mode === 'manual' ? form.native_api_override : null,
    owner_user_id: form.scope === 'user' ? form.owner_user_id : null,
    protocol_mode: form.protocol_mode,
    scope: form.scope,
    daily_max_requests: form.daily_max_requests,
    monthly_max_requests: form.monthly_max_requests,
    mcp_enabled: form.mcp_enabled,
    proxy_url,
  }
}
