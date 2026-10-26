import { create } from 'zustand'
import type {
  ConversationEndpointOverride,
  RequestRecordDetail,
  RequestRecordFullResponse,
  SessionAffinityResetResponse,
  SessionRouteOptionsResponse,
} from '@/generated/admin-api'
import {
  deleteConversationEndpointOverride,
  requestRecordDetail,
  requestRecordFull,
  requestRecordResetSessionAffinity,
  requestRecordSessionRouteOptions,
  setConversationEndpointOverride,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import {
  createUsageDetailView,
  createUsageOverrideView,
  createUsageRouteOptionsView,
  mergeUsageRequestFull,
  type UsageConversationOverrideView,
  type UsageDetailView,
  type UsageRouteOptionsView,
} from './usage-detail-model'
import { usageIdentityKey, type UsageIdentity } from './usage-model'

export const USAGE_REQUEST_FULL_PAGE_SIZE = 10
export const USAGE_REQUEST_FULL_ORDER = 'desc'
export const USAGE_MAINTENANCE_KEY = 'usage:maintenance'

export function usageConversationActionKey(conversationId: string): string {
  return `conversation:${conversationId}`
}

export function usageRecordActionKey(recordId: number): string {
  return `record:${recordId}`
}

export function usageDetailActionKey(
  conversationId: string | null,
  recordId: number,
): string {
  return conversationId
    ? usageConversationActionKey(conversationId)
    : usageRecordActionKey(recordId)
}

export type UsageDetailState = {
  identityKey: string | null
  recordId: number | null
  detail: UsageDetailView | null
  detailLoading: boolean
  requestFull: RequestRecordFullResponse | null
  requestFullLoading: boolean
  requestFullLoadingMore: boolean
  routeOptions: UsageRouteOptionsView | null
  override: UsageConversationOverrideView | null
  routeOptionsLoading: boolean
  initializeIdentity: (identity: UsageIdentity | null) => void
  reset: () => void
  loadDetail: (recordId: number) => Promise<UsageDetailView | null>
  loadRequestFull: (
    recordId: number,
  ) => Promise<RequestRecordFullResponse | null>
  loadMoreRequestFull: (
    recordId: number,
  ) => Promise<RequestRecordFullResponse | null>
  loadRouteOptions: (recordId: number) => Promise<UsageRouteOptionsView | null>
  saveOverride: (
    conversationId: string,
    endpointId: string,
    endpointKeyId: string | null,
  ) => Promise<AsyncActionOutcome<void>>
  clearOverride: (conversationId: string) => Promise<AsyncActionOutcome<void>>
  resetAffinity: (
    recordId: number,
  ) => Promise<AsyncActionOutcome<SessionAffinityResetResponse>>
  currentConversationId: () => string | null
}

let detailGeneration = 0
let fullGeneration = 0
let routeGeneration = 0

function clearResources() {
  ++detailGeneration
  ++fullGeneration
  ++routeGeneration
  return {
    recordId: null,
    detail: null,
    detailLoading: false,
    requestFull: null,
    requestFullLoading: false,
    requestFullLoadingMore: false,
    routeOptions: null,
    override: null,
    routeOptionsLoading: false,
  }
}

export const useUsageDetailStore = create<UsageDetailState>((set, get) => ({
  identityKey: null,
  ...clearResources(),

  initializeIdentity(identity) {
    const nextKey = identity ? usageIdentityKey(identity) : null
    if (get().identityKey === nextKey) return
    set({ identityKey: nextKey, ...clearResources() })
  },

  reset() {
    set(clearResources())
  },

  currentConversationId() {
    return (
      get().detail?.conversation_id ??
      get().routeOptions?.conversation_id ??
      null
    )
  },

  async loadDetail(recordId) {
    if (get().identityKey === null) return null
    const generation = ++detailGeneration
    ++fullGeneration
    ++routeGeneration
    set({
      recordId,
      detail: null,
      detailLoading: true,
      requestFull: null,
      requestFullLoading: false,
      requestFullLoadingMore: false,
      routeOptions: null,
      override: null,
      routeOptionsLoading: false,
    })
    try {
      const detail: RequestRecordDetail = expectData(
        await requestRecordDetail<true>(
          withData({ path: { record_id: recordId } }),
        ),
      )
      if (generation !== detailGeneration) return null
      const view = createUsageDetailView(detail)
      set({ detail: view, detailLoading: false })
      const tasks: Promise<unknown>[] = []
      if (detail.conversation_id) {
        tasks.push(get().loadRouteOptions(recordId))
      }
      await Promise.allSettled(tasks)
      return view
    } catch (cause) {
      if (generation === detailGeneration) {
        set({ detailLoading: false })
      }
      throw cause
    }
  },

  async loadRequestFull(recordId) {
    if (get().identityKey === null) return null
    if (get().recordId !== recordId) return null
    if (get().requestFull || get().requestFullLoading) {
      return get().requestFull
    }
    // The first paint deliberately asks for ten newest-first messages to avoid
    // the legacy full payload; older pages load through loadMoreRequestFull.
    const generation = ++fullGeneration
    set({ requestFullLoading: true })
    try {
      const page: RequestRecordFullResponse = expectData(
        await requestRecordFull<true>(
          withData({
            path: { record_id: recordId },
            query: {
              limit: USAGE_REQUEST_FULL_PAGE_SIZE,
              order: USAGE_REQUEST_FULL_ORDER,
            },
          }),
        ),
      )
      if (generation !== fullGeneration || get().recordId !== recordId) {
        return null
      }
      set({ requestFull: page })
      return page
    } finally {
      if (generation === fullGeneration && get().recordId === recordId) {
        set({ requestFullLoading: false })
      }
    }
  },

  async loadMoreRequestFull(recordId) {
    if (get().identityKey === null) return null
    const current = get().requestFull
    if (get().recordId !== recordId) return current
    if (!current?.has_more || !current.next_cursor) return current
    if (get().requestFullLoading || get().requestFullLoadingMore) {
      return current
    }
    const generation = ++fullGeneration
    set({ requestFullLoadingMore: true })
    try {
      const page: RequestRecordFullResponse = expectData(
        await requestRecordFull<true>(
          withData({
            path: { record_id: recordId },
            query: {
              cursor: current.next_cursor,
              limit: USAGE_REQUEST_FULL_PAGE_SIZE,
              order: USAGE_REQUEST_FULL_ORDER,
            },
          }),
        ),
      )
      if (generation !== fullGeneration || get().recordId !== recordId) {
        return null
      }
      const base = get().requestFull ?? current
      const merged = mergeUsageRequestFull(base, page)
      set({ requestFull: merged })
      return merged
    } finally {
      if (generation === fullGeneration && get().recordId === recordId) {
        set({ requestFullLoadingMore: false })
      }
    }
  },

  async loadRouteOptions(recordId) {
    if (get().identityKey === null) return null
    if (get().recordId !== recordId) return null
    const generation = ++routeGeneration
    set({ routeOptionsLoading: true })
    try {
      const response: SessionRouteOptionsResponse = expectData(
        await requestRecordSessionRouteOptions<true>(
          withData({ path: { record_id: recordId } }),
        ),
      )
      if (generation !== routeGeneration || get().recordId !== recordId) {
        return null
      }
      set({
        override: createUsageOverrideView(response),
        routeOptions: createUsageRouteOptionsView(response),
      })
      return get().routeOptions
    } catch (cause) {
      if (generation === routeGeneration && get().recordId === recordId) {
        set({ override: null, routeOptions: null })
      }
      throw cause
    } finally {
      if (generation === routeGeneration && get().recordId === recordId) {
        set({ routeOptionsLoading: false })
      }
    }
  },

  async saveOverride(conversationId, endpointId, endpointKeyId) {
    const capturedEndpointId = endpointId
    const capturedEndpointKeyId = endpointKeyId
    return runAsyncAction(
      usageConversationActionKey(conversationId),
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
        if (get().currentConversationId() !== conversationId) return
        set({
          override: {
            conversation_id: response.conversation_id,
            endpoint_id: response.endpoint_id,
            endpoint_key_id: response.endpoint_key_id ?? null,
            endpoint_key_label: response.endpoint_key_label ?? null,
            endpoint_name: response.endpoint_name ?? null,
          },
        })
        const recordId = get().recordId
        if (recordId !== null) await get().loadRouteOptions(recordId)
      },
      'save',
    )
  },

  async clearOverride(conversationId) {
    return runAsyncAction(
      usageConversationActionKey(conversationId),
      async () => {
        await deleteConversationEndpointOverride<true>(
          withData({ path: { conversation_id: conversationId } }),
        )
        if (get().currentConversationId() !== conversationId) return
        set({ override: null })
        const recordId = get().recordId
        if (recordId !== null) await get().loadRouteOptions(recordId)
      },
      'clear',
    )
  },

  async resetAffinity(recordId) {
    if (get().recordId !== recordId) return { executed: false }
    const conversationId = get().currentConversationId()
    const key = usageDetailActionKey(conversationId, recordId)
    return runAsyncAction(
      key,
      async () => {
        const response: SessionAffinityResetResponse = expectData(
          await requestRecordResetSessionAffinity<true>(
            withData({ path: { record_id: recordId } }),
          ),
        )
        if (get().recordId === recordId) {
          await get().loadRouteOptions(recordId)
        }
        return response
      },
      'reset',
    )
  },
}))
