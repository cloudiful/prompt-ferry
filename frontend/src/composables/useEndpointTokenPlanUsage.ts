import { ref } from 'vue'
import type {
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { isQuotaEligible } from '@/models/endpoints/quota'
import {
  getTokenPlanUsageSnapshot,
  requestTokenPlanUsage,
} from './useTokenPlanUsageCache'

export function useEndpointTokenPlanUsage(
  findEndpointById: (endpointId: string) => ProviderEndpoint | null,
  onError: (cause: unknown) => void,
) {
  const visible = ref(false)
  const loading = ref(false)
  const endpointId = ref('')
  const usage = ref<TokenPlanUsageResponse | null>(null)

  // The dialog reads the shared usage cache instead of calling the API itself,
  // so it inherits the same TTL, negative cache, back-off and in-flight
  // coalescing as the endpoint list. The cache stays the only owner of the
  // request policy; this composable owns eligibility and the open/close state.
  async function load(
    nextEndpointId: string,
    options: { refresh?: boolean } = {},
  ): Promise<void> {
    // Synchronous read first: the list badges normally warmed this endpoint,
    // so the dialog paints the cached snapshot before any await and a stale
    // entry keeps its numbers on screen while the shared policy revalidates.
    const cached = getTokenPlanUsageSnapshot(nextEndpointId)
    usage.value = cached.usage
    // Only a cold or failed entry shows the spinner; a cached ready snapshot
    // (fresh or stale) stays visible instead of blanking to loading.
    loading.value = cached.state !== 'ready'
    try {
      // Always go through the shared request policy, even for a cached ready
      // snapshot: a fresh entry is a zero-request warm hit, while a stale entry
      // serves the current snapshot and kicks the TTL/SWR revalidation instead
      // of returning forever on a snapshot the cache only labels `ready`.
      const result = await requestTokenPlanUsage(nextEndpointId, options)
      // A slower open for another endpoint must not overwrite the current one.
      if (endpointId.value !== nextEndpointId) return
      usage.value = result.usage
      // Report only a failure this open actually awaited: a pinned negative
      // entry is replayed silently instead of re-toasting on every open.
      if (result.failed) onError(result.cause)
    } finally {
      if (endpointId.value === nextEndpointId) loading.value = false
    }
  }

  async function open(nextEndpointId: string): Promise<void> {
    const endpoint = findEndpointById(nextEndpointId)
    // R2e.3: the same eligibility gate the list uses — an OpenAI endpoint
    // only opens the subscription quota once a token is stored.
    if (!endpoint || !isQuotaEligible(endpoint)) return
    endpointId.value = nextEndpointId
    visible.value = true
    await load(nextEndpointId)
  }

  // Deliberate refresh of the open dialog: the user asked for the numbers to
  // be re-read, so the TTL is bypassed for exactly one request while the
  // current snapshot stays on screen until it lands.
  async function refresh(): Promise<void> {
    if (!endpointId.value || !visible.value) return
    await load(endpointId.value, { refresh: true })
  }

  return {
    openTokenPlanUsage: open,
    refreshTokenPlanUsage: refresh,
    tokenPlanUsage: usage,
    tokenPlanUsageEndpointId: endpointId,
    tokenPlanUsageLoading: loading,
    tokenPlanUsageVisible: visible,
  }
}
