import { onScopeDispose, ref, watch, type Ref } from 'vue'
import type {
  EndpointOAuthStatusResponse,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
} from '@/api/endpoint-oauth'

export type EndpointOAuthStatusPayload = Pick<
  EndpointOAuthStatusResponse,
  'has_oauth_token' | 'plan'
>

export type EndpointOAuthStatusContext = {
  endpointId: string
  generation: number
}

type Options = {
  endpointId: Readonly<Ref<string>>
  onStatus: (payload: EndpointOAuthStatusPayload) => void
}

export function useEndpointOAuthStatus(options: Options) {
  const status = ref<EndpointOAuthStatusResponse | null>(null)
  const quota = ref<TokenPlanUsageResponse | null>(null)
  const quotaError = ref('')
  const statusError = ref('')
  let generation = 0
  let readGeneration = 0

  function capture(): EndpointOAuthStatusContext {
    return { endpointId: options.endpointId.value, generation }
  }

  function isCurrent(context: EndpointOAuthStatusContext): boolean {
    return (
      options.endpointId.value === context.endpointId &&
      generation === context.generation
    )
  }

  function invalidateReads(endpointId: string): void {
    if (options.endpointId.value === endpointId) readGeneration += 1
  }

  function applyStatus(
    next: EndpointOAuthStatusResponse,
    context: EndpointOAuthStatusContext,
  ): boolean {
    if (!isCurrent(context)) return false
    status.value = next
    options.onStatus({
      has_oauth_token: next.has_oauth_token,
      plan: next.plan,
    })
    return true
  }

  async function loadQuota(
    context: EndpointOAuthStatusContext,
    requestGeneration: number,
  ): Promise<void> {
    if (!isCurrent(context) || requestGeneration !== readGeneration) return
    quotaError.value = ''
    try {
      const next = await getEndpointTokenPlanUsage(context.endpointId)
      if (isCurrent(context) && requestGeneration === readGeneration) {
        quota.value = next
      }
    } catch (error) {
      if (!isCurrent(context) || requestGeneration !== readGeneration) return
      quota.value = null
      quotaError.value = formatApiError(error)
    }
  }

  async function applyRefreshedStatus(
    next: EndpointOAuthStatusResponse,
    context: EndpointOAuthStatusContext,
  ): Promise<void> {
    if (!isCurrent(context)) return
    const requestGeneration = ++readGeneration
    if (!applyStatus(next, context)) return
    if (next.has_oauth_token) {
      await loadQuota(context, requestGeneration)
    } else if (isCurrent(context) && requestGeneration === readGeneration) {
      quota.value = null
      quotaError.value = ''
    }
  }

  async function refreshStatusFor(
    context: EndpointOAuthStatusContext,
  ): Promise<void> {
    if (!isCurrent(context)) return
    const requestGeneration = ++readGeneration
    statusError.value = ''
    try {
      const next = await getEndpointOAuthStatus(context.endpointId)
      if (
        !isCurrent(context) ||
        requestGeneration !== readGeneration ||
        !applyStatus(next, context)
      ) {
        return
      }
      if (next.has_oauth_token) {
        await loadQuota(context, requestGeneration)
      } else if (isCurrent(context) && requestGeneration === readGeneration) {
        quota.value = null
        quotaError.value = ''
      }
    } catch (error) {
      if (isCurrent(context) && requestGeneration === readGeneration) {
        statusError.value = formatApiError(error)
      }
    }
  }

  async function refreshCurrentStatus(): Promise<void> {
    await refreshStatusFor(capture())
  }

  watch(
    options.endpointId,
    (endpointId) => {
      generation += 1
      readGeneration += 1
      status.value = null
      quota.value = null
      quotaError.value = ''
      statusError.value = ''
      if (endpointId) void refreshCurrentStatus()
    },
    { immediate: true },
  )
  onScopeDispose(() => {
    generation += 1
    readGeneration += 1
  })

  return {
    applyRefreshedStatus,
    capture,
    invalidateReads,
    isCurrent,
    loadQuota,
    quota,
    quotaError,
    refreshCurrentStatus,
    refreshStatusFor,
    status,
    statusError,
  }
}
