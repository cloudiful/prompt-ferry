import type {
  ModelEndpointRule,
  ModelRouteRequest,
  NativeApi,
  StreamDeltaBatchingSettings,
} from '../../generated/admin-api'
import type { ModelRouteForm, StreamDeltaBatchingForm } from '../../models'

const TARGET_NATIVE_APIS: readonly NativeApi[] = [
  'auto',
  'anthropic_messages',
  'chat',
  'responses',
  'realtime',
]

function normalizeTargetNativeApi(value: unknown): NativeApi {
  return TARGET_NATIVE_APIS.includes(value as NativeApi)
    ? (value as NativeApi)
    : 'auto'
}

// Issue #464: per-target thinking effort override. Only the 7-level
// allowlist survives; anything else normalizes to null (inherit).
const THINKING_EFFORTS = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const

// Issue #502 Task 5: per-target compact mode. Only the 3-level allowlist
// survives; anything else normalizes to passthrough (default).
const COMPACT_MODES = ['passthrough', 'self_summarize', 'off'] as const

function normalizeCompactMode(value: unknown): string {
  if (typeof value !== 'string') return 'passthrough'
  const trimmed = value.trim()
  return (COMPACT_MODES as readonly string[]).includes(trimmed)
    ? trimmed
    : 'passthrough'
}

function normalizeThinkingEffort(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  return (THINKING_EFFORTS as readonly string[]).includes(trimmed) ? trimmed : null
}

export function createEmptyModelRouteForm(): ModelRouteForm {
  return {
    rule_id: '',
    scope: 'admin',
    owner_user_id: null,
    model_pattern: '',
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
        // Issue #378 Phase J: untouched all-day schedule.
        active_windows: [],
        active_windows_touched: false,
        // Issue #392 Phase L: default-off normalize.
        dev_system_normalize: false,
        // Issue #464: inherit (follow caller) by default.
        thinking_effort_override: null,
        // Issue #502 Task 5: passthrough (native) by default.
        compact_mode: 'passthrough',
        // Issue #409 Phase 2: default Auto (follow caller).
        native_api: 'auto',
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
    enabled: source.enabled ?? true,
    targets: targets.map((target) => ({
      endpoint_id: target?.endpoint_id ?? '',
      enabled: target?.enabled ?? true,
      upstream_model: target?.upstream_model ?? '',
      // Issue #368 Phase C: masked override; never echo the secret.
      // `has_proxy_url_override` is optional for legacy payloads.
      proxy_url_override: '',
      has_saved_proxy_url_override: target?.has_proxy_url_override ?? false,
      // Issue #378 Phase J: copy stored windows; untouched until the
      // schedule dialog saves. Tolerate legacy payloads without the field.
      active_windows: (target?.active_windows ?? []).map((window) => ({
        start: window?.start ?? '',
        end: window?.end ?? '',
        ...(Array.isArray((window as { days?: unknown })?.days) ? { days: [...((window as { days?: number[] }).days ?? [])] } : {}),
      })),
      active_windows_touched: false,
      // Issue #392 Phase L: default-off normalize; legacy payloads miss it.
      dev_system_normalize: target?.dev_system_normalize ?? false,
      // Issue #464: thinking effort override; legacy payloads miss it
      // (means inherit). Null/empty means inherit.
      thinking_effort_override: normalizeThinkingEffort(
        (target as { thinking_effort_override?: unknown } | undefined)?.thinking_effort_override,
      ),
      // Issue #502 Task 5: compact mode; legacy payloads miss it
      // (means passthrough).
      compact_mode: normalizeCompactMode(
        (target as { compact_mode?: unknown } | undefined)?.compact_mode,
      ),
      // Issue #409 Phase 2: per-target port type; legacy payloads miss it
      // (means Auto). Round-trips Auto default.
      native_api: normalizeTargetNativeApi(
        (target as { native_api?: unknown } | undefined)?.native_api,
      ),
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
    scope: safe.scope === 'user' ? 'user' : 'admin',
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
        // Issue #378 Phase J: omit-when-untouched for schedules.
        // Untouched omits the key (PATCH keeps the stored value);
        // touched sends the edited array (empty = all-day, sorted).
        const touched = target?.active_windows_touched ?? false
        const active_windows = touched
          ? [...(target?.active_windows ?? [])]
              .map((window) => {
                const start = (window?.start ?? '').trim()
                const end = (window?.end ?? '').trim()
                const rawDays = Array.isArray((window as { days?: unknown })?.days)
                  ? ([...((window as { days?: number[] }).days ?? [])] as number[])
                  : undefined
                const days = rawDays
                  ? [...new Set(rawDays.filter((d) => Number.isInteger(d) && (d as number) >= 1 && (d as number) <= 7))].sort((a, b) => (a as number) - (b as number))
                  : undefined
                return {
                  start,
                  end,
                  ...(days && days.length > 0 && days.length < 7 ? { days } : {}),
                }
              })
              .sort((a, b) =>
                a.start === b.start
                  ? a.end.localeCompare(b.end)
                  : a.start.localeCompare(b.start),
              )
          : undefined
        return {
          endpoint_id: target?.endpoint_id ?? '',
          enabled: target?.enabled ?? true,
          upstream_model: (target?.upstream_model ?? '').trim() || undefined,
          proxy_url_override,
          active_windows,
          // Issue #392 Phase L: booleans always sent (no omit semantics).
          dev_system_normalize: target?.dev_system_normalize ?? false,
          // Issue #464: always sent (null means inherit).
          thinking_effort_override: normalizeThinkingEffort(target?.thinking_effort_override),
          // Issue #502 Task 5: always sent (passthrough default).
          compact_mode: normalizeCompactMode(target?.compact_mode),
          // Issue #409 Phase 2: always sent (Auto default follows caller;
          // explicit wins over endpoint). Normalize legacy/undefined to Auto.
          native_api: normalizeTargetNativeApi(target?.native_api),
        }
      }),
  }
}
