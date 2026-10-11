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

function usageForFetchState(
  usage: TokenPlanUsageResponse | null,
  state: ReturnType<typeof getTokenPlanUsageSnapshot>['state'],
): TokenPlanUsageResponse | null {
  if (usage === null || state !== 'stale') return usage
  return {
    ...usage,
    keys: usage.keys.map((key) => {
      const status = key.quota_status
      if (
        status?.state === 'cooling' &&
        status.cooldown_until != null &&
        Date.parse(status.cooldown_until) > Date.now()
      ) {
        return key
      }
      return {
        ...key,
        quota_status: {
          state: 'unknown',
          observed_at: status?.observed_at ?? null,
          cooldown_until: null,
          refresh_failed: status?.refresh_failed ?? false,
          scope: 'text_inference',
        },
      }
    }),
  }
}

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
    usage.value = usageForFetchState(cached.usage, cached.state)
    loading.value = cached.usage === null
    try {
      // Always go through the shared request policy, even for a cached ready
      // snapshot: a fresh entry is a zero-request warm hit, while a stale entry
      // serves the current snapshot and kicks the TTL/SWR revalidation instead
      // of returning forever on a snapshot the cache only labels `ready`.
      const result = await requestTokenPlanUsage(nextEndpointId, options)
      // A slower open for another endpoint must not overwrite the current one.
      if (endpointId.value !== nextEndpointId) return
      usage.value = usageForFetchState(result.usage, result.state)
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
