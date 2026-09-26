import { ref } from 'vue'
import type {
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { isQuotaEligible } from '@/models/endpoints/quota'
import { fetchTokenPlanUsage } from '@/stores/endpoints-api'

export function useEndpointTokenPlanUsage(
  findEndpointById: (endpointId: string) => ProviderEndpoint | null,
  onError: (cause: unknown) => void,
) {
  const visible = ref(false)
  const loading = ref(false)
  const endpointId = ref('')
  const usage = ref<TokenPlanUsageResponse | null>(null)

  async function open(nextEndpointId: string): Promise<void> {
    const endpoint = findEndpointById(nextEndpointId)
    // R2e.3: the same eligibility gate the list uses — an OpenAI endpoint
    // only opens the subscription quota once a token is stored.
    if (!endpoint || !isQuotaEligible(endpoint)) return
    endpointId.value = nextEndpointId
    usage.value = null
    visible.value = true
    loading.value = true
    try {
      usage.value = await fetchTokenPlanUsage(nextEndpointId)
    } catch (cause) {
      onError(cause)
    } finally {
      loading.value = false
    }
  }

  return {
    openTokenPlanUsage: open,
    tokenPlanUsage: usage,
    tokenPlanUsageEndpointId: endpointId,
    tokenPlanUsageLoading: loading,
    tokenPlanUsageVisible: visible,
  }
}
