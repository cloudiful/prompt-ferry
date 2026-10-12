import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import type { useRequestRecordsStore } from '@/stores/usage'

export type RequestRecordsStore = ReturnType<typeof useRequestRecordsStore>
type Translate = ReturnType<typeof useLocale>['t']

export function useSessionRoutingActions(options: {
  store: RequestRecordsStore
  confirm: (message: string) => boolean
  notifyApiError: ReturnType<typeof useNotifier>['notifyApiError']
  notifyInfo: ReturnType<typeof useNotifier>['notifyInfo']
  notifySuccess: ReturnType<typeof useNotifier>['notifySuccess']
  t: Translate
}) {
  const { confirm, notifyApiError, notifyInfo, notifySuccess, store, t } =
    options

  async function saveConversationOverride(selection: {
    endpointId: string
    endpointKeyId: string | null
  }): Promise<void> {
    const conversationId = store.sessionRouteOptions?.conversation_id
    if (!conversationId) return
    const endpointId = selection.endpointId
    const endpointKeyId = selection.endpointKeyId
    try {
      const outcome = await store.saveConversationOverride(
        conversationId,
        endpointId,
        endpointKeyId,
      )
      if (outcome.executed) notifySuccess(t('overrideSaved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function clearConversationOverride(): Promise<void> {
    const conversationId = store.sessionRouteOptions?.conversation_id
    if (!conversationId) return
    try {
      const outcome = await store.clearConversationOverride(conversationId)
      if (outcome.executed) notifySuccess(t('overrideCleared'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function resetSessionAffinity(): Promise<void> {
    const recordId = store.detailRecord?.record_id
    if (!recordId || !confirm(t('resetAffinityConfirm'))) return
    try {
      const outcome = await store.guardedResetSessionAffinity(recordId)
      if (!outcome.executed) return
      if (outcome.value.cleared_count > 0) {
        notifySuccess(t('affinityReset'))
      } else {
        notifyInfo(t('affinityNotBound'))
      }
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return {
    clearConversationOverride,
    resetSessionAffinity,
    saveConversationOverride,
  }
}
