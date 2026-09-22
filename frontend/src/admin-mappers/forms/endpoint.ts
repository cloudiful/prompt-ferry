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
    enabled: true,
    mcp_enabled: false,
    // Issue #368 Phase C: masked proxy default; empty + no saved means direct.
    proxy_url: '',
    has_saved_proxy_url: false,
    // Issue #392 Phase L: untouched all-day endpoint schedule.
    active_windows: [],
    active_windows_touched: false,
  }
}

export function endpointToForm(endpoint: ProviderEndpoint): EndpointForm {
  // INLINE-proxy-ui-a1 BUG fix: upstream edit showed only the modal overlay.
  // Legacy/cached payloads can omit Phase C fields or carry empty api_keys;
  // normalize everything so the dialog always has a renderable form.
  const source = (endpoint ?? {}) as Partial<ProviderEndpoint>
  const endpointApiKeys = source.api_keys ?? []
  const nativeApiOverride =
    source.native_api === 'anthropic_messages' ||
    source.native_api === 'responses' ||
    source.native_api === 'chat' ||
    source.native_api === 'realtime'
      ? source.native_api
      : null
  return {
    endpoint_id: source.endpoint_id ?? '',
    scope: source.scope === 'user' ? 'user' : 'admin',
    owner_user_id: source.owner_user_id ?? null,
    name: source.name ?? '',
    provider: source.provider ?? 'generic',
    provider_region: source.provider_region ?? null,
    service_tier: normalizeServiceTier(source.service_tier),
    base_url: source.base_url ?? '',
    api_keys: endpointApiKeys.map((key) => ({
      key_label: key.key_label ?? '',
      api_key: '',
      has_saved_key: true,
      enabled: key.enabled ?? true,
      key_id: key.key_id ?? '',
    })),
    key_lb_enabled: source.key_lb_enabled ?? false,
    protocol_mode:
      source.native_api_source === 'auto' && source.native_api === 'auto'
        ? 'auto'
        : 'manual',
    native_api_override: nativeApiOverride,
    enabled: source.enabled ?? true,
    mcp_enabled: source.mcp_enabled ?? false,
    // Issue #368 Phase C: masked proxy input; never echo the secret.
    // `has_proxy_url` is optional for legacy payloads (missing means direct).
    proxy_url: '',
    has_saved_proxy_url: source.has_proxy_url ?? false,
    // Issue #392 Phase L: copy stored windows; untouched until the schedule
    // dialog saves. Tolerate legacy payloads without the field.
    active_windows: (source.active_windows ?? []).map((window) => ({
      start: window?.start ?? '',
      end: window?.end ?? '',
      ...(Array.isArray((window as { days?: unknown })?.days)
        ? { days: [...((window as { days?: number[] }).days ?? [])] }
        : {}),
    })),
    active_windows_touched: false,
  }
}

export function endpointFormToRequest(form: EndpointForm): EndpointRequest {
  // Issue #368 Phase C: omit-when-untouched to avoid the P2 secret-wipe.
  // Non-empty means replace; empty + saved means omit (keep stored value);
  // empty + no saved means clear to direct (`""`). The request-side
  // `has_proxy_url` hint is intentionally not sent (server ignores it).
  // INLINE-proxy-ui-a1: tolerate legacy forms missing proxy fields.
  const safe = (form ?? {}) as Partial<EndpointForm>
  const trimmedProxy = (safe.proxy_url ?? '').trim()
  const proxy_url =
    trimmedProxy !== ''
      ? trimmedProxy
      : safe.has_saved_proxy_url
        ? undefined
        : ''
  const apiKeys = safe.api_keys ?? []
  // Issue #392 Phase L: omit-when-untouched for endpoint schedules.
  // Untouched omits the key (PATCH keeps the stored value); touched sends
  // the edited array (empty = all-day, sorted).
  const endpointTouched = safe.active_windows_touched ?? false
  const active_windows = endpointTouched
    ? [...(safe.active_windows ?? [])]
        .map((window) => ({
          start: (window?.start ?? '').trim(),
          end: (window?.end ?? '').trim(),
        }))
        .sort((a, b) =>
          a.start === b.start
            ? a.end.localeCompare(b.end)
            : a.start.localeCompare(b.start),
        )
    : undefined
  return {
    api_key: apiKeys[0]?.api_key ?? '',
    api_keys: apiKeys
      .map((key) => ({
        key_label: (key.key_label ?? '').trim(),
        api_key: key.api_key ?? '',
        enabled: key.enabled ?? true,
        key_id: key.key_id || undefined,
      }))
      .filter((key) => key.key_label || key.api_key),
    key_lb_enabled: safe.key_lb_enabled ?? false,
    base_url: (safe.base_url ?? '').trim(),
    enabled: safe.enabled ?? true,
    name: (safe.name ?? '').trim(),
    provider: safe.provider ?? 'generic',
    provider_region: safe.provider_region ?? null,
    service_tier: normalizeServiceTier(safe.service_tier),
    native_api_override:
      safe.protocol_mode === 'manual'
        ? (safe.native_api_override ?? null)
        : null,
    owner_user_id: safe.scope === 'user' ? (safe.owner_user_id ?? null) : null,
    protocol_mode: safe.protocol_mode === 'manual' ? 'manual' : 'auto',
    scope: safe.scope === 'user' ? 'user' : 'admin',
    mcp_enabled: safe.mcp_enabled ?? false,
    proxy_url,
    active_windows,
  }
}
