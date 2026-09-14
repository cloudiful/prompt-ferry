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
  // INLINE-proxy-ui-a1 BUG fix: tolerate legacy payloads missing targets or
  // Phase C override flags so the edit dialog always has a renderable form.
  const source = (route ?? {}) as Partial<ModelEndpointRule>
  const targets = source.targets ?? []
  return {
    rule_id: source.rule_id ?? '',
    scope: source.scope === 'user' ? 'user' : 'admin',
    owner_user_id: source.owner_user_id ?? null,
    model_pattern: source.model_pattern ?? '',
    routing_strategy: source.routing_strategy ?? 'client_key_rendezvous',
    daily_max_requests: source.daily_max_requests ?? null,
    monthly_max_requests: source.monthly_max_requests ?? null,
    enabled: source.enabled ?? true,
    targets: targets.map((target) => ({
      endpoint_id: target?.endpoint_id ?? '',
      enabled: target?.enabled ?? true,
      upstream_model: target?.upstream_model ?? '',
      // Issue #368 Phase C: masked override; never echo the secret.
      // `has_proxy_url_override` is optional for legacy payloads.
      proxy_url_override: '',
      has_saved_proxy_url_override: target?.has_proxy_url_override ?? false,
    })),
  }
}

export function modelRouteFormToRequest(
  form: ModelRouteForm,
): ModelRouteRequest {
  // INLINE-proxy-ui-a1: tolerate legacy target rows missing proxy fields.
  const safe = (form ?? {}) as Partial<ModelRouteForm>
  const targets = safe.targets ?? []
  return {
    enabled: safe.enabled ?? true,
    model_pattern: (safe.model_pattern ?? '').trim(),
    owner_user_id: safe.scope === 'user' ? (safe.owner_user_id ?? null) : null,
    routing_strategy:
      safe.routing_strategy === 'responses_session_affinity'
        ? 'responses_session_affinity'
        : 'client_key_rendezvous',
    scope: safe.scope === 'user' ? 'user' : 'admin',
    daily_max_requests: safe.daily_max_requests ?? null,
    monthly_max_requests: safe.monthly_max_requests ?? null,
    targets: targets
      .filter((target) => (target?.endpoint_id ?? '').trim() !== '')
      .map((target) => {
        // Issue #368 Phase C: omit-when-untouched per target.
        // Non-empty means replace; empty + saved means omit (keep);
        // empty + no saved means clear to inherit (`""`).
        const trimmed = (target?.proxy_url_override ?? '').trim()
        const proxy_url_override =
          trimmed !== ''
            ? trimmed
            : target?.has_saved_proxy_url_override
              ? undefined
              : ''
        return {
          endpoint_id: target?.endpoint_id ?? '',
          enabled: target?.enabled ?? true,
          upstream_model: (target?.upstream_model ?? '').trim() || undefined,
          proxy_url_override,
        }
      }),
  }
}
