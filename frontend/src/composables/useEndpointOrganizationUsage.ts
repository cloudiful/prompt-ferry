import { ref } from 'vue'
import type {
  EndpointProvider,
  OpenAiOrganizationUsageResponse,
} from '@/generated/admin-api'
import { formatApiError } from '@/api'
import { isOrganizationUsageEligible } from '@/models/endpoints/quota'
import { fetchOrganizationUsage } from '@/stores/endpoints-api'

export type OrganizationUsageSource = {
  endpointId: string
  provider: EndpointProvider
  has_admin_api_key?: boolean | null
}

// Issue #589 P2c: OpenAI Platform organization usage for one endpoint, read
// through its separate Admin API Key. Display-only — never routing weight,
// token-plan quota, or remaining-credit math.
export function useEndpointOrganizationUsage() {
  const loading = ref(false)
  const usage = ref<OpenAiOrganizationUsageResponse | null>(null)
  const errorMessage = ref('')

  async function load(source: OrganizationUsageSource): Promise<void> {
    // Shared gate: an OpenAI endpoint with a stored Admin API Key and a
    // saved id. Anything else leaves the surface untouched (the component
    // shows the actionable hint instead).
    if (
      !source.endpointId ||
      !isOrganizationUsageEligible({
        provider: source.provider,
        has_admin_api_key: source.has_admin_api_key,
      })
    ) {
      return
    }
    loading.value = true
    errorMessage.value = ''
    try {
      usage.value = await fetchOrganizationUsage(source.endpointId)
    } catch (cause) {
      usage.value = null
      errorMessage.value = formatApiError(cause)
    } finally {
      loading.value = false
    }
  }

  function reset(): void {
    usage.value = null
    errorMessage.value = ''
  }

  return {
    errorMessage,
    loadOrganizationUsage: load,
    organizationUsage: usage,
    organizationUsageLoading: loading,
    resetOrganizationUsage: reset,
  }
}
