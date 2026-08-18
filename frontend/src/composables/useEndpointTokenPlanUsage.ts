import { ref } from 'vue'
import type {
  ProviderEndpoint,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
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
    if (!endpoint || endpoint.provider !== 'minimax') return
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
