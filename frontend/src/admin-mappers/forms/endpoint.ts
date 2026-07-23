import type {
  EndpointRequest,
  ProviderEndpoint,
} from '../../generated/admin-api'
import type { EndpointForm } from '../../models'

export function createEmptyEndpointForm(): EndpointForm {
  return {
    endpoint_id: '',
    scope: 'admin',
    owner_user_id: null,
    name: '',
    base_url: '',
    api_key: '',
    primary_api_key_saved: false,
    api_keys: [],
    key_lb_enabled: false,
    protocol_mode: 'auto',
    native_api_override: null,
    daily_max_requests: null,
    monthly_max_requests: null,
    enabled: true,
  }
}

export function endpointToForm(endpoint: ProviderEndpoint): EndpointForm {
  const endpointApiKeys = endpoint.api_keys ?? []
  return {
    endpoint_id: endpoint.endpoint_id,
    scope: endpoint.scope === 'user' ? 'user' : 'admin',
    owner_user_id: endpoint.owner_user_id ?? null,
    name: endpoint.name,
    base_url: endpoint.base_url,
    api_key: '',
    primary_api_key_saved: endpointApiKeys.length > 0,
    api_keys: endpointApiKeys.slice(1).map((key) => ({
      key_label: key.key_label,
      api_key: '',
      has_saved_key: true,
      enabled: key.enabled,
    })),
    key_lb_enabled: endpoint.key_lb_enabled ?? false,
    protocol_mode: endpoint.native_api_source === 'manual' ? 'manual' : 'auto',
    native_api_override:
      endpoint.native_api === 'anthropic_messages' ||
      endpoint.native_api === 'responses' ||
      endpoint.native_api === 'chat' ||
      endpoint.native_api === 'realtime'
        ? endpoint.native_api
        : null,
    daily_max_requests: endpoint.daily_max_requests ?? null,
    monthly_max_requests: endpoint.monthly_max_requests ?? null,
    enabled: endpoint.enabled,
  }
}

export function endpointFormToRequest(form: EndpointForm): EndpointRequest {
  return {
    api_key: form.api_key,
    api_keys: form.api_keys
      .map((key) => ({
        key_label: key.key_label.trim(),
        api_key: key.api_key,
        enabled: key.enabled,
      }))
      .filter((key) => key.key_label || key.api_key),
    key_lb_enabled: form.key_lb_enabled,
    base_url: form.base_url.trim(),
    enabled: form.enabled,
    name: form.name.trim(),
    native_api_override:
      form.protocol_mode === 'manual' ? form.native_api_override : null,
    owner_user_id: form.scope === 'user' ? form.owner_user_id : null,
    protocol_mode: form.protocol_mode,
    scope: form.scope,
    daily_max_requests: form.daily_max_requests,
    monthly_max_requests: form.monthly_max_requests,
  }
}
