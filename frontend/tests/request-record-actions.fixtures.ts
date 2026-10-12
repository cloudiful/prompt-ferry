import { mock } from 'bun:test'
import { ref } from 'vue'

import { useAsyncAction } from '../src/composables/useAsyncAction'

const actualAdminApi = await import('../src/generated/admin-api')

export const deleteConversationEndpointOverride = mock(async () => ({
  data: null,
}))
export const requestRecordResetSessionAffinity = mock(async () => ({
  data: { cleared: true, cleared_count: 2 },
}))
export const setConversationEndpointOverride = mock(async () => ({
  data: {
    conversation_id: 'conversation-a',
    endpoint_id: 'endpoint-a',
    endpoint_key_id: 'key-a',
    endpoint_key_label: 'key A',
    endpoint_name: 'Endpoint A',
    created_at: '',
    updated_at: '',
    created_by_user_id: null,
  },
}))

function adminApiModule() {
  return {
    ...actualAdminApi,
    deleteConversationEndpointOverride,
    requestRecordResetSessionAffinity,
    setConversationEndpointOverride,
  }
}

export async function loadRequestRecordActions() {
  mock.module('../src/generated/admin-api', adminApiModule)
  return import('../src/stores/request-record-detail-actions')
}

export function clearRequestRecordMocks(): void {
  deleteConversationEndpointOverride.mockClear()
  requestRecordResetSessionAffinity.mockClear()
  setConversationEndpointOverride.mockClear()
}

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export async function freshDetailActions() {
  clearRequestRecordMocks()
  const { createRequestRecordDetailActions } = await loadRequestRecordActions()
  const guard = useAsyncAction()
  const detailRecord = ref({
    conversation_id: 'conversation-a',
    record_id: 1,
  } as never)
  const conversationOverride = ref(null as never)
  const sessionRouteOptions = ref({
    conversation_id: 'conversation-a',
  } as never)
  const reloads: number[] = []
  const actions = createRequestRecordDetailActions({
    guard,
    detailRecord,
    conversationOverride,
    sessionRouteOptions,
    loadSessionRouteOptions: async (recordId) => {
      reloads.push(recordId)
      return null
    },
  })
  return {
    actions,
    conversationOverride,
    detailRecord,
    guard,
    reloads,
    sessionRouteOptions,
  }
}
