import type { ModelRouteTargetForm } from '@/models'
import type { EndpointForm } from '@/models'
import type { NativeApi } from '@/generated/admin-api'

// Issue #644: the configured free-form service-tier override is a
// provider-agnostic best-effort passthrough on every HTTP JSON protocol.
// `realtime` carries WebSocket frames instead of the common JSON request body,
// and `embeddings` has no `service_tier` field at all; `auto` is resolved to a
// JSON protocol before forwarding. Mirrors the runtime
// `supports_service_tier_for` gate in `src/db/types/endpoints.rs`. The
// caller-supplied compatibility bit from issue #637 is a separate,
// provider-scoped layer and is not part of this gate.
export function supportsServiceTierFor(protocol: NativeApi): boolean {
  return protocol !== 'realtime' && protocol !== 'embeddings'
}

// Issue #644: the protocol an endpoint form resolves to. `auto` stays `auto`
// (resolved per request by the runtime); a manual selection keeps its
// override and falls back to Responses, mirroring the request-side fallback.
export function endpointFormProtocol(
  form: Pick<EndpointForm, 'protocol_mode' | 'native_api_override'>,
): NativeApi {
  if (form.protocol_mode === 'auto') return 'auto'
  return form.native_api_override ?? 'responses'
}

// Issue #1032: every protocol axis an endpoint or target can name. Anything
// unknown normalizes to `auto` so a legacy payload never becomes a guessed
// protocol.
const NATIVE_API_AXIS: readonly NativeApi[] = [
  'auto',
  'anthropic_messages',
  'chat',
  'embeddings',
  'responses',
  'realtime',
]

export function normalizeNativeApiAxis(value: unknown): NativeApi {
  return NATIVE_API_AXIS.includes(value as NativeApi)
    ? (value as NativeApi)
    : 'auto'
}

// Issue #1032: the embeddings protocol is the only one that carries the
// per-target dimension override. A chat/responses/anthropic protocol keeps its
// own options and never shows a dimension field.
export function supportsEmbeddingDimensionOverrideFor(
  protocol: NativeApi,
): boolean {
  return protocol === 'embeddings'
}

// Issue #1032: chat-only settings (thinking effort, thinking downgrade,
// compact mode, developer->system normalize, service tier) have no meaning for
// an embeddings target and stay hidden/inactive there. `auto` keeps them: the
// UI never guesses a protocol from the model name.
export function supportsChatAdaptationFor(protocol: NativeApi): boolean {
  return protocol !== 'embeddings'
}

// Issue #1032: the effective protocol of one target. An explicit target
// protocol wins; `auto` inherits the endpoint's protocol; an `auto` endpoint
// leaves the operation to the runtime request path, so the UI stays on the
// chat controls and never infers a protocol from the model name.
export function effectiveTargetProtocol(
  targetNativeApi: unknown,
  endpointProtocol: NativeApi,
): NativeApi {
  const target = normalizeNativeApiAxis(targetNativeApi)
  return target === 'auto' ? normalizeNativeApiAxis(endpointProtocol) : target
}

// Issue #1032: whether the effective target is an embeddings target. Kept next
// to `effectiveTargetProtocol` so dialog and settings page agree on the same
// axis.
export function isEffectiveEmbeddingsTarget(
  target: ModelRouteTargetForm | null | undefined,
  endpointProtocol: NativeApi,
): boolean {
  return (
    effectiveTargetProtocol(target?.native_api, endpointProtocol) ===
    'embeddings'
  )
}
