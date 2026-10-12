import { computed, type Ref } from 'vue'

import { expectData, withData } from '@/api'
import {
  deleteConversationEndpointOverride,
  requestRecordResetSessionAffinity,
  setConversationEndpointOverride,
} from '@/generated/admin-api'
import type {
  ConversationEndpointOverride,
  SessionAffinityResetResponse,
} from '@/generated/admin-api'
import { createConversationEndpointOverrideView } from '@/admin-mappers'
import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  ConversationEndpointOverrideView,
  RequestRecordDetailView,
  SessionRouteOptionsView,
} from '@/models'
import { recordActionKey } from './record-action-types'

const CONVERSATION_ACTION_PREFIX = 'conversation'

export function conversationActionKey(conversationId: string): string {
  return recordActionKey(CONVERSATION_ACTION_PREFIX, conversationId)
}

type DetailActionDeps = {
  guard: AsyncActionGuard
  detailRecord: Ref<RequestRecordDetailView | null>
  conversationOverride: Ref<ConversationEndpointOverrideView | null>
  loadSessionRouteOptions: (
    recordId: number,
  ) => Promise<SessionRouteOptionsView | null>
  sessionRouteOptions: Ref<SessionRouteOptionsView | null>
}

export function createRequestRecordDetailActions(deps: DetailActionDeps) {
  const {
    conversationOverride,
    detailRecord,
    guard,
    loadSessionRouteOptions,
    sessionRouteOptions,
  } = deps

  function currentConversationId(): string | null {
    return (
      detailRecord.value?.conversation_id ??
      sessionRouteOptions.value?.conversation_id ??
      null
    )
  }

  function currentAction(conversationId: string | null): string | undefined {
    return conversationId
      ? guard.pendingAction(conversationActionKey(conversationId))
      : undefined
  }

  const overrideSaving = computed(() => {
    const action = currentAction(currentConversationId())
    return action === 'save' || action === 'clear'
  })
  const affinityResetting = computed(
    () => currentAction(currentConversationId()) === 'reset',
  )

  function saveConversationOverride(
    conversationId: string,
    endpointId: string,
    endpointKeyId: string | null,
  ): Promise<AsyncActionOutcome<void>> {
    const capturedEndpointId = endpointId
    const capturedEndpointKeyId = endpointKeyId
    return guard.run(
      conversationActionKey(conversationId),
      async () => {
        const response: ConversationEndpointOverride = expectData(
          await setConversationEndpointOverride<true>(
            withData({
              path: { conversation_id: conversationId },
              body: {
                endpoint_id: capturedEndpointId,
                endpoint_key_id: capturedEndpointKeyId,
              },
            }),
          ),
        )
        if (currentConversationId() !== conversationId) return
        conversationOverride.value =
          createConversationEndpointOverrideView(response)
        const recordId = detailRecord.value?.record_id
        if (recordId !== undefined) await loadSessionRouteOptions(recordId)
      },
      'save',
    )
  }

  function clearConversationOverride(
    conversationId: string,
  ): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      conversationActionKey(conversationId),
      async () => {
        await deleteConversationEndpointOverride<true>(
          withData({ path: { conversation_id: conversationId } }),
        )
        if (currentConversationId() !== conversationId) return
        conversationOverride.value = null
        const recordId = detailRecord.value?.record_id
        if (recordId !== undefined) await loadSessionRouteOptions(recordId)
      },
      'clear',
    )
  }

  function guardedResetSessionAffinity(
    recordId: number,
  ): Promise<AsyncActionOutcome<SessionAffinityResetResponse>> {
    if (detailRecord.value?.record_id !== recordId) {
      return Promise.resolve({ executed: false })
    }
    const conversationId = currentConversationId() ?? `record:${recordId}`
    return guard.run(
      conversationActionKey(conversationId),
      async () => {
        const response = expectData(
          await requestRecordResetSessionAffinity<true>(
            withData({ path: { record_id: recordId } }),
          ),
        )
        if (detailRecord.value?.record_id === recordId) {
          await loadSessionRouteOptions(recordId)
        }
        return response
      },
      'reset',
    )
  }

  async function resetSessionAffinity(
    recordId: number,
  ): Promise<SessionAffinityResetResponse> {
    const outcome = await guardedResetSessionAffinity(recordId)
    if (!outcome.executed) {
      throw new Error('Session affinity reset is already pending')
    }
    return outcome.value
  }

  return {
    affinityResetting,
    clearConversationOverride,
    conversationAction: currentAction,
    guardedResetSessionAffinity,
    overrideSaving,
    resetSessionAffinity,
    saveConversationOverride,
  }
}
