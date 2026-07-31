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
        responses_continuation_policy: 'force_replay',
      },
    ],
  }
}

export function createDefaultStreamDeltaBatchingForm(): StreamDeltaBatchingForm {
  return {
    enabled: false,
    flush_window_ms: 50,
    max_buffer_chars: 160,
    max_buffer_bytes: 1024,
    flush_on_line_break: true,
    flush_on_sentence_end: false,
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
      responses_continuation_policy: target.responses_continuation_policy,
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
      .map((target) => ({
        endpoint_id: target.endpoint_id,
        enabled: target.enabled,
        upstream_model: target.upstream_model.trim() || undefined,
        responses_continuation_policy: target.responses_continuation_policy,
      })),
  }
}
