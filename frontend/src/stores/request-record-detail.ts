import { ref } from 'vue'
import {
  deleteConversationEndpointOverride,
  requestRecordDetail,
  requestRecordFull,
  requestRecordResetSessionAffinity,
  requestRecordSessionRouteOptions,
  setConversationEndpointOverride,
} from '../generated/admin-api'
import type {
  ConversationEndpointOverride,
  RequestRecordDetail,
  RequestRecordFullResponse,
  SessionRouteOptionsResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  createConversationEndpointOverrideView,
  createRequestRecordDetailView,
  createSessionRouteOptionsView,
} from '../admin-mappers'
import type {
  ConversationEndpointOverrideView,
  RequestRecordDetailView,
  SessionRouteOptionsView,
} from '../models'

export function createRequestRecordDetailState() {
  const detailLoading = ref(false)
  const requestFullLoading = ref(false)
  const routeOptionsLoading = ref(false)
  const overrideSaving = ref(false)
  const requestFull = ref<RequestRecordFullResponse | null>(null)
  const detailRecord = ref<RequestRecordDetailView | null>(null)
  const sessionRouteOptions = ref<SessionRouteOptionsView | null>(null)
  const conversationOverride = ref<ConversationEndpointOverrideView | null>(
    null,
  )

  let detailLoadVersion = 0

  function isCurrentDetailLoad(recordId: number, loadVersion: number): boolean {
    return (
      detailLoadVersion === loadVersion &&
      detailRecord.value?.record_id === recordId
    )
  }

  async function loadDetail(
    recordId: number,
  ): Promise<RequestRecordDetailView> {
    const loadVersion = ++detailLoadVersion
    detailLoading.value = true
    try {
      const detail: RequestRecordDetail = expectData(
        await requestRecordDetail<true>(
          withData({ path: { record_id: recordId } }),
        ),
      )
      const detailView = createRequestRecordDetailView(detail)
      if (detailLoadVersion !== loadVersion) {
        return detailView
      }
      detailRecord.value = detailView
      detailLoading.value = false
      void refreshDetailRelated(recordId, detail, loadVersion)
      return detailView
    } catch (cause) {
      if (detailLoadVersion === loadVersion) {
        detailLoading.value = false
      }
      throw cause
    }
  }

  async function refreshDetailRelated(
    recordId: number,
    detail: RequestRecordDetail,
    loadVersion: number,
  ): Promise<void> {
    const tasks: Promise<unknown>[] = []
    if (detail.conversation_id) {
      tasks.push(loadSessionRouteOptions(recordId, loadVersion))
    } else if (isCurrentDetailLoad(recordId, loadVersion)) {
      sessionRouteOptions.value = null
      conversationOverride.value = null
    }
    if (
      !detail.has_full_request &&
      isCurrentDetailLoad(recordId, loadVersion)
    ) {
      requestFull.value = null
    }
    await Promise.allSettled(tasks)
  }

  async function loadRequestFull(
    recordId: number,
    loadVersion?: number,
  ): Promise<RequestRecordFullResponse | null> {
    const effectiveLoadVersion = loadVersion ?? detailLoadVersion
    requestFullLoading.value = true
    try {
      const full = expectData(
        await requestRecordFull<true>(
          withData({ path: { record_id: recordId } }),
        ),
      )
      if (isCurrentDetailLoad(recordId, effectiveLoadVersion)) {
        requestFull.value = full
      }
      return full
    } finally {
      if (isCurrentDetailLoad(recordId, effectiveLoadVersion)) {
        requestFullLoading.value = false
      }
    }
  }

  async function loadSessionRouteOptions(
    recordId: number,
    loadVersion?: number,
  ): Promise<SessionRouteOptionsView | null> {
    const effectiveLoadVersion = loadVersion ?? detailLoadVersion
    routeOptionsLoading.value = true
    try {
      const response: SessionRouteOptionsResponse = expectData(
        await requestRecordSessionRouteOptions<true>(
          withData({ path: { record_id: recordId } }),
        ),
      )
      const routeOptions = createSessionRouteOptionsView(response)
      const override = response.override_endpoint_id
        ? createConversationEndpointOverrideView({
            conversation_id: response.conversation_id,
            endpoint_id: response.override_endpoint_id,
            endpoint_key_id: response.override_endpoint_key_id ?? null,
            endpoint_key_label: response.override_endpoint_key_label ?? null,
            endpoint_name:
              response.options.find(
                (option) =>
                  option.endpoint_id === response.override_endpoint_id,
              )?.endpoint_name ?? null,
            created_at: '',
            updated_at: '',
            created_by_user_id: null,
          } as ConversationEndpointOverride)
        : null
      if (isCurrentDetailLoad(recordId, effectiveLoadVersion)) {
        sessionRouteOptions.value = routeOptions
        conversationOverride.value = override
      }
      return routeOptions
    } catch (cause) {
      if (isCurrentDetailLoad(recordId, effectiveLoadVersion)) {
        sessionRouteOptions.value = null
        conversationOverride.value = null
      }
      throw cause
    } finally {
      if (isCurrentDetailLoad(recordId, effectiveLoadVersion)) {
        routeOptionsLoading.value = false
      }
    }
  }

  async function saveConversationOverride(
    conversationId: string,
    endpointId: string,
    endpointKeyId: string | null,
  ): Promise<void> {
    overrideSaving.value = true
    try {
      const response: ConversationEndpointOverride = expectData(
        await setConversationEndpointOverride<true>(
          withData({
            path: { conversation_id: conversationId },
            body: { endpoint_id: endpointId, endpoint_key_id: endpointKeyId },
          }),
        ),
      )
      conversationOverride.value =
        createConversationEndpointOverrideView(response)
      if (detailRecord.value) {
        await loadSessionRouteOptions(detailRecord.value.record_id)
      }
    } finally {
      overrideSaving.value = false
    }
  }

  async function clearConversationOverride(
    conversationId: string,
  ): Promise<void> {
    overrideSaving.value = true
    try {
      await deleteConversationEndpointOverride<true>(
        withData({
          path: { conversation_id: conversationId },
        }),
      )
      conversationOverride.value = null
      if (detailRecord.value) {
        await loadSessionRouteOptions(detailRecord.value.record_id)
      }
    } finally {
      overrideSaving.value = false
    }
  }

  async function resetSessionAffinity(recordId: number): Promise<void> {
    overrideSaving.value = true
    try {
      await requestRecordResetSessionAffinity<true>(
        withData({ path: { record_id: recordId } }),
      )
      if (detailRecord.value) {
        await loadSessionRouteOptions(detailRecord.value.record_id)
      }
    } finally {
      overrideSaving.value = false
    }
  }

  function resetDetail(): void {
    detailLoadVersion += 1
    detailLoading.value = false
    detailRecord.value = null
    requestFull.value = null
    requestFullLoading.value = false
    sessionRouteOptions.value = null
    routeOptionsLoading.value = false
    conversationOverride.value = null
  }

  return {
    clearConversationOverride,
    conversationOverride,
    detailLoading,
    detailRecord,
    loadDetail,
    loadRequestFull,
    loadSessionRouteOptions,
    overrideSaving,
    requestFull,
    requestFullLoading,
    resetDetail,
    resetSessionAffinity,
    routeOptionsLoading,
    saveConversationOverride,
    sessionRouteOptions,
  }
}
