import { onScopeDispose, ref, watch, type Ref } from 'vue'
import type { EndpointOAuthStatusResponse } from '@/generated/admin-api'
import { formatApiError } from '@/api'
import { getEndpointOAuthStatus } from '@/api/endpoint-oauth'

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

// Issue #1004 P1: the editor shows the login state only. The subscription
// quota lives on the standalone quota surface, so the editor no longer
// fetches or renders it.
export function useEndpointOAuthStatus(options: Options) {
  const status = ref<EndpointOAuthStatusResponse | null>(null)
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

  async function applyRefreshedStatus(
    next: EndpointOAuthStatusResponse,
    context: EndpointOAuthStatusContext,
  ): Promise<void> {
    if (!isCurrent(context)) return
    readGeneration += 1
    applyStatus(next, context)
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
    refreshCurrentStatus,
    refreshStatusFor,
    status,
    statusError,
  }
}
