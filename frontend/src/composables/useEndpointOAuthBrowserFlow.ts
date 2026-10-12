import { onScopeDispose, ref, watch, type Ref } from 'vue'
import type { OAuthBrowserStartResponse } from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  completeEndpointBrowserLogin,
  startEndpointBrowserLogin,
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

export function useEndpointOAuthBrowserFlow(options: Options) {
  const browser = ref<OAuthBrowserStartResponse | null>(null)
  const redirectUrl = ref('')
  const busy = ref(false)
  const message = ref('')
  let flowGeneration = 0
  let flowEndpointId = ''

  function clearFlow(): void {
    flowGeneration += 1
    flowEndpointId = ''
    browser.value = null
    redirectUrl.value = ''
    busy.value = false
    message.value = ''
  }

  async function startBrowser(): Promise<void> {
    if (busy.value) return
    const context = options.capture()
    let currentFlow = -1
    let claimed = false
    try {
      const result = await options.runAction(
        context.endpointId,
        async () => {
          claimed = true
          currentFlow = ++flowGeneration
          flowEndpointId = context.endpointId
          browser.value = null
          redirectUrl.value = ''
          message.value = ''
          busy.value = true
          return await startEndpointBrowserLogin(context.endpointId)
        },
        'oauth-browser-start',
      )
      if (
        !result.executed ||
        !options.isCurrent(context) ||
        currentFlow !== flowGeneration
      ) {
        return
      }
      browser.value = result.value
    } catch (error) {
      if (
        claimed &&
        options.isCurrent(context) &&
        currentFlow === flowGeneration
      ) {
        message.value = formatApiError(error)
      }
    } finally {
      if (claimed && currentFlow === flowGeneration) busy.value = false
    }
  }

  async function completeBrowser(): Promise<void> {
    if (busy.value) return
    const context = options.capture()
    const flow = browser.value
    const capturedRedirectUrl = redirectUrl.value.trim()
    const currentFlow = flowGeneration
    if (
      !flow ||
      !capturedRedirectUrl ||
      context.endpointId !== flowEndpointId ||
      !options.isCurrent(context)
    ) {
      return
    }
    let claimed = false
    try {
      const result = await options.runAction(
        context.endpointId,
        async () => {
          claimed = true
          busy.value = true
          message.value = ''
          const response = await completeEndpointBrowserLogin(
            context.endpointId,
            flow.flow_id,
            capturedRedirectUrl,
          )
          if (response.status === 'complete') {
            await options.refreshStatusFor(context)
          }
          return response
        },
        'oauth-browser-complete',
      )
      if (
        !result.executed ||
        !options.isCurrent(context) ||
        currentFlow !== flowGeneration ||
        browser.value?.flow_id !== flow.flow_id
      ) {
        return
      }
      if (result.value.status === 'complete') clearFlow()
    } catch (error) {
      if (
        claimed &&
        options.isCurrent(context) &&
        currentFlow === flowGeneration &&
        browser.value?.flow_id === flow.flow_id
      ) {
        message.value = formatApiError(error)
      }
    } finally {
      if (claimed && currentFlow === flowGeneration) busy.value = false
    }
  }

  watch(options.endpointId, clearFlow)
  onScopeDispose(clearFlow)

  return {
    browser,
    busy,
    completeBrowser,
    message,
    redirectUrl,
    startBrowser,
  }
}
