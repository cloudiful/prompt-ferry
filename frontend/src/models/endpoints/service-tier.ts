import type { EndpointProvider, NativeApi } from '@/generated/admin-api'

// Issue #637: the documented provider/protocol matrix for the free-form
// service-tier override. Mirrors `EndpointProvider::supports_service_tier_for`
// in `src/db/types/endpoints.rs`:
//   MiniMax - Chat Completions, Responses, Anthropic Messages
//   OpenAI  - Chat Completions, Responses
// `auto` means the runtime resolves the caller protocol before forwarding, so
// it stays eligible for every supported provider. Realtime and every other
// provider/protocol combination are excluded, so the UI never offers an
// override the runtime would silently ignore.
const MINIMAX_TIER_PROTOCOLS: readonly NativeApi[] = [
  'chat',
  'responses',
  'anthropic_messages',
]

const OPENAI_TIER_PROTOCOLS: readonly NativeApi[] = ['chat', 'responses']

export function supportsServiceTierProvider(
  provider: EndpointProvider | null | undefined,
): boolean {
  return provider === 'minimax' || provider === 'openai'
}

export function supportsServiceTierFor(
  provider: EndpointProvider | null | undefined,
  protocol: NativeApi,
): boolean {
  if (!supportsServiceTierProvider(provider)) return false
  if (protocol === 'auto') return true
  const protocols =
    provider === 'minimax' ? MINIMAX_TIER_PROTOCOLS : OPENAI_TIER_PROTOCOLS
  return protocols.includes(protocol)
}
