import type {
  ModelEndpointRule,
  ModelRouteRequest,
  StreamDeltaBatchingSettings,
} from '../../generated/admin-api'
import type { ModelRouteForm, StreamDeltaBatchingForm } from '../../models'

export function createEmptyModelRouteForm(): ModelRouteForm {
  return {
    rule_id: '',
    scope: 'admin',
    owner_user_id: null,
    model_pattern: '',
    routing_strategy: 'client_key_rendezvous',
    daily_max_requests: null,
    monthly_max_requests: null,
    enabled: true,
    targets: [
      {
        endpoint_id: '',
        enabled: true,
        upstream_model: '',
        // Issue #368 Phase C: masked per-target override; empty + no saved
        // means inherit.
        proxy_url_override: '',
        has_saved_proxy_url_override: false,
      },
    ],
  }
}

export function streamDeltaBatchingToForm(
  settings: StreamDeltaBatchingSettings,
): StreamDeltaBatchingForm {
  return {
    enabled: settings.enabled,
    flush_window_ms: settings.flush_window_ms,
    max_buffer_chars: settings.max_buffer_chars,
    max_buffer_bytes: settings.max_buffer_bytes,
    flush_on_line_break: settings.flush_on_line_break,
    flush_on_sentence_end: settings.flush_on_sentence_end,
  }
}

export function streamDeltaBatchingFormToRequest(
  form: StreamDeltaBatchingForm,
): StreamDeltaBatchingSettings {
  return {
    enabled: form.enabled,
    flush_window_ms: form.flush_window_ms,
    max_buffer_chars: form.max_buffer_chars,
    max_buffer_bytes: form.max_buffer_bytes,
    flush_on_line_break: form.flush_on_line_break,
    flush_on_sentence_end: form.flush_on_sentence_end,
  }
}

export function modelRouteToForm(route: ModelEndpointRule): ModelRouteForm {
  return {
    rule_id: route.rule_id,
    scope: route.scope === 'user' ? 'user' : 'admin',
    owner_user_id: route.owner_user_id ?? null,
    model_pattern: route.model_pattern,
    routing_strategy: route.routing_strategy ?? 'client_key_rendezvous',
    daily_max_requests: route.daily_max_requests ?? null,
    monthly_max_requests: route.monthly_max_requests ?? null,
    enabled: route.enabled,
    targets: route.targets.map((target) => ({
      endpoint_id: target.endpoint_id,
      enabled: target.enabled,
      upstream_model: target.upstream_model ?? '',
      // Issue #368 Phase C: masked override; never echo the secret.
      // `has_proxy_url_override` is optional for legacy payloads.
      proxy_url_override: '',
      has_saved_proxy_url_override: target.has_proxy_url_override ?? false,
    })),
  }
}

export function modelRouteFormToRequest(
  form: ModelRouteForm,
): ModelRouteRequest {
  return {
    enabled: form.enabled,
    model_pattern: form.model_pattern.trim(),
    owner_user_id: form.scope === 'user' ? form.owner_user_id : null,
    routing_strategy: form.routing_strategy,
    scope: form.scope,
    daily_max_requests: form.daily_max_requests,
    monthly_max_requests: form.monthly_max_requests,
    targets: form.targets
      .filter((target) => target.endpoint_id.trim() !== '')
      .map((target) => {
        // Issue #368 Phase C: omit-when-untouched per target.
        // Non-empty means replace; empty + saved means omit (keep);
        // empty + no saved means clear to inherit (`""`).
        const trimmed = target.proxy_url_override.trim()
        const proxy_url_override =
          trimmed !== ''
            ? trimmed
            : target.has_saved_proxy_url_override
              ? undefined
              : ''
        return {
          endpoint_id: target.endpoint_id,
          enabled: target.enabled,
          upstream_model: target.upstream_model.trim() || undefined,
          proxy_url_override,
        }
      }),
  }
}
