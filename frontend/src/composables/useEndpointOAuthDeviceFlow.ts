import { onScopeDispose, ref, watch, type Ref } from 'vue'
import type { OAuthDeviceStartResponse } from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  pollEndpointDeviceLogin,
  startEndpointDeviceLogin,
} from '@/api/endpoint-oauth'
import type { RunEndpointOAuthAction } from '@/stores/endpoint-oauth-action-types'
import type { EndpointOAuthStatusContext } from '@/composables/useEndpointOAuthStatus'

type Options = {
  capture: () => EndpointOAuthStatusContext
  endpointId: Readonly<Ref<string>>
  isCurrent: (context: EndpointOAuthStatusContext) => boolean
  refreshStatusFor: (context: EndpointOAuthStatusContext) => Promise<void>
  runAction: RunEndpointOAuthAction
}

export function useEndpointOAuthDeviceFlow(options: Options) {
  const device = ref<OAuthDeviceStartResponse | null>(null)
  const starting = ref(false)
  const polling = ref(false)
  const message = ref('')
  let pollTimer: ReturnType<typeof setInterval> | null = null
  let flowGeneration = 0
  let flowEndpointId = ''

  function stopPolling(): void {
    if (pollTimer !== null) {
      clearInterval(pollTimer)
      pollTimer = null
    }
  }

  function clearFlow(): void {
    flowGeneration += 1
    stopPolling()
    flowEndpointId = ''
    device.value = null
    starting.value = false
    polling.value = false
    message.value = ''
  }

  function schedulePoll(
    context: EndpointOAuthStatusContext,
    currentFlow: number,
  ) {
    stopPolling()
    const seconds = Math.max(1, device.value?.interval_seconds ?? 5)
    pollTimer = setInterval(() => {
      void pollDevice(context, currentFlow)
    }, seconds * 1000)
  }

  async function startDevice(): Promise<void> {
    const context = options.capture()
    let currentFlow = -1
    let claimed = false
    try {
      const result = await options.runAction(
        context.endpointId,
        async () => {
          claimed = true
          currentFlow = ++flowGeneration
          stopPolling()
          device.value = null
          flowEndpointId = context.endpointId
          message.value = ''
          starting.value = true
          return await startEndpointDeviceLogin(context.endpointId)
        },
        'oauth-device-start',
      )
      if (
        !result.executed ||
        !options.isCurrent(context) ||
        currentFlow !== flowGeneration
      ) {
        return
      }
      device.value = result.value
      schedulePoll(context, currentFlow)
    } catch (error) {
      if (
        claimed &&
        options.isCurrent(context) &&
        currentFlow === flowGeneration
      ) {
        message.value = formatApiError(error)
      }
    } finally {
      if (claimed && currentFlow === flowGeneration) starting.value = false
    }
  }

  async function pollDevice(
    context = options.capture(),
    currentFlow = flowGeneration,
  ): Promise<void> {
    const flow = device.value
    if (
      !flow ||
      polling.value ||
      context.endpointId !== flowEndpointId ||
      !options.isCurrent(context) ||
      currentFlow !== flowGeneration
    ) {
      return
    }
    let claimed = false
    try {
      const result = await options.runAction(
        context.endpointId,
        async () => {
          claimed = true
          polling.value = true
          const response = await pollEndpointDeviceLogin(
            context.endpointId,
            flow.flow_id,
          )
          if (response.status === 'complete') {
            await options.refreshStatusFor(context)
          }
          return response
        },
        'oauth-device-poll',
      )
      if (!result.executed || !options.isCurrent(context)) return
      if (
        currentFlow !== flowGeneration ||
        device.value?.flow_id !== flow.flow_id
      )
        return
      if (result.value.status === 'complete') {
        stopPolling()
        device.value = null
        flowEndpointId = ''
      }
    } catch (error) {
      if (
        claimed &&
        options.isCurrent(context) &&
        currentFlow === flowGeneration &&
        device.value?.flow_id === flow.flow_id
      ) {
        stopPolling()
        device.value = null
        flowEndpointId = ''
        message.value = formatApiError(error)
      }
    } finally {
      if (claimed && currentFlow === flowGeneration) polling.value = false
    }
  }

  watch(options.endpointId, clearFlow)
  onScopeDispose(clearFlow)

  return {
    device,
    message,
    pollDevice,
    polling,
    startDevice,
    starting,
    stopPolling,
  }
}
