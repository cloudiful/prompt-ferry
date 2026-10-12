import { ref, watch, type Ref } from 'vue'
import { formatApiError } from '@/api'
import {
  clearEndpointOAuthToken,
  refreshEndpointOAuthToken,
} from '@/api/endpoint-oauth'
import type { RunEndpointOAuthAction } from '@/stores/endpoint-oauth-action-types'
import { useEndpointOAuthBrowserFlow } from '@/composables/useEndpointOAuthBrowserFlow'
import { useEndpointOAuthDeviceFlow } from '@/composables/useEndpointOAuthDeviceFlow'
import { useEndpointOAuthStatus } from '@/composables/useEndpointOAuthStatus'
import type { EndpointOAuthStatusPayload } from '@/composables/useEndpointOAuthStatus'

const refuseAction: RunEndpointOAuthAction = async () => ({ executed: false })

type Options = {
  endpointId: Readonly<Ref<string>>
  onStatus: (payload: EndpointOAuthStatusPayload) => void
  runAction?: RunEndpointOAuthAction
}

export function useEndpointOAuthActions(options: Options) {
  const runAction = options.runAction ?? refuseAction
  const statusActions = useEndpointOAuthStatus({
    endpointId: options.endpointId,
    onStatus: options.onStatus,
  })
  const guardedAction: RunEndpointOAuthAction = (endpointId, action, label) =>
    runAction(
      endpointId,
      async () => {
        statusActions.invalidateReads(endpointId)
        return await action()
      },
      label,
    )
  const deviceFlow = useEndpointOAuthDeviceFlow({
    capture: statusActions.capture,
    endpointId: options.endpointId,
    isCurrent: statusActions.isCurrent,
    refreshStatusFor: statusActions.refreshStatusFor,
    runAction: guardedAction,
  })
  const browserFlow = useEndpointOAuthBrowserFlow({
    capture: statusActions.capture,
    endpointId: options.endpointId,
    isCurrent: statusActions.isCurrent,
    refreshStatusFor: statusActions.refreshStatusFor,
    runAction: guardedAction,
  })
  const refreshing = ref(false)
  const clearing = ref(false)

  async function refreshToken(): Promise<void> {
    const context = statusActions.capture()
    let claimed = false
    try {
      await guardedAction(
        context.endpointId,
        async () => {
          claimed = true
          refreshing.value = true
          statusActions.statusError.value = ''
          const next = await refreshEndpointOAuthToken(context.endpointId)
          await statusActions.applyRefreshedStatus(next, context)
        },
        'oauth-refresh',
      )
    } catch (error) {
      if (claimed && statusActions.isCurrent(context)) {
        statusActions.statusError.value = formatApiError(error)
      }
    } finally {
      if (claimed && statusActions.isCurrent(context)) refreshing.value = false
    }
  }

  async function clearToken(): Promise<void> {
    const context = statusActions.capture()
    let claimed = false
    try {
      await guardedAction(
        context.endpointId,
        async () => {
          claimed = true
          clearing.value = true
          statusActions.statusError.value = ''
          await clearEndpointOAuthToken(context.endpointId)
          await statusActions.refreshStatusFor(context)
        },
        'oauth-clear',
      )
    } catch (error) {
      if (claimed && statusActions.isCurrent(context)) {
        statusActions.statusError.value = formatApiError(error)
      }
    } finally {
      if (claimed && statusActions.isCurrent(context)) clearing.value = false
    }
  }

  watch(options.endpointId, () => {
    refreshing.value = false
    clearing.value = false
  })

  return {
    browserFlow,
    clearToken,
    clearing,
    deviceFlow,
    quota: statusActions.quota,
    quotaError: statusActions.quotaError,
    refreshToken,
    refreshing,
    status: statusActions.status,
    statusError: statusActions.statusError,
  }
}
