import type { EndpointForm } from '@/models'
import type { NativeApi } from '@/generated/admin-api'

// Issue #644: the configured free-form service-tier override is a
// provider-agnostic best-effort passthrough on every HTTP JSON protocol.
// `realtime` carries WebSocket frames instead of the common JSON request body,
// so it never advertises or injects the setting; `auto` is resolved to a JSON
// protocol before forwarding. Mirrors the runtime
// `supports_service_tier_for` gate in `src/db/types/endpoints.rs`. The
// caller-supplied compatibility bit from issue #637 is a separate,
// provider-scoped layer and is not part of this gate.
export function supportsServiceTierFor(protocol: NativeApi): boolean {
  return protocol !== 'realtime'
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
