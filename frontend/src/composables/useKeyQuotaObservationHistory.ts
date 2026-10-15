import { ref, watch, type Ref } from 'vue'
import type {
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationHistoryResponse,
} from '@/generated/admin-api'

export type KeyHistoryFetcher = (
  endpointId: string,
  keyId: string | null,
  limit?: number,
  beforeId?: string,
) => Promise<QuotaKeyObservationHistoryResponse>

const PAGE_SIZE = 50
const RETENTION_DAYS = 30

export function useKeyQuotaObservationHistory(
  endpointId: Ref<string>,
  keyId: Ref<string | null>,
  fetchPage: KeyHistoryFetcher,
  expanded?: Ref<boolean>,
) {
  const items = ref<QuotaKeyObservationHistoryItem[]>([])
  const nextCursor = ref<string | null>(null)
  const retentionDays = ref(RETENTION_DAYS)
  const loading = ref(false)
  const loadingMore = ref(false)
  const failed = ref(false)
  const loaded = ref(false)
  let generation = 0

  function clear(): void {
    generation += 1
    items.value = []
    nextCursor.value = null
    retentionDays.value = RETENTION_DAYS
    loading.value = false
    loadingMore.value = false
    failed.value = false
    loaded.value = false
  }

  async function loadFirstPage(): Promise<void> {
    const id = endpointId.value
    const key = keyId.value
    if (!id) return
    const requestGeneration = ++generation
    items.value = []
    nextCursor.value = null
    failed.value = false
    loaded.value = false
    loadingMore.value = false
    loading.value = true
    try {
      const page = await fetchPage(id, key, PAGE_SIZE)
      if (
        requestGeneration !== generation ||
        id !== endpointId.value ||
        key !== keyId.value
      )
        return
      items.value = page.items
      nextCursor.value = page.next_cursor ?? null
      retentionDays.value = page.retention_days
      loaded.value = true
    } catch {
      if (
        requestGeneration === generation &&
        id === endpointId.value &&
        key === keyId.value
      ) {
        failed.value = true
      }
    } finally {
      if (
        requestGeneration === generation &&
        id === endpointId.value &&
        key === keyId.value
      ) {
        loading.value = false
      }
    }
  }

  async function loadMore(): Promise<void> {
    const id = endpointId.value
    const key = keyId.value
    const cursor = nextCursor.value
    if (!id || !cursor || !loaded.value || loading.value || loadingMore.value)
      return
    const requestGeneration = generation
    failed.value = false
    loadingMore.value = true
    try {
      const page = await fetchPage(id, key, PAGE_SIZE, cursor)
      if (
        requestGeneration !== generation ||
        id !== endpointId.value ||
        key !== keyId.value
      )
        return
      const seen = new Set(items.value.map((item) => item.observation_id))
      const additionalItems = page.items.filter((item) => {
        if (seen.has(item.observation_id)) return false
        seen.add(item.observation_id)
        return true
      })
      items.value = [...items.value, ...additionalItems]
      nextCursor.value = page.next_cursor ?? null
      retentionDays.value = page.retention_days
    } catch {
      if (
        requestGeneration === generation &&
        id === endpointId.value &&
        key === keyId.value
      ) {
        failed.value = true
      }
    } finally {
      if (
        requestGeneration === generation &&
        id === endpointId.value &&
        key === keyId.value
      ) {
        loadingMore.value = false
      }
    }
  }

  async function retry(): Promise<void> {
    if (items.value.length > 0 && nextCursor.value) {
      await loadMore()
      return
    }
    await loadFirstPage()
  }

  function reloadIfExpanded(): void {
    clear()
    if (endpointId.value && expanded?.value) void loadFirstPage()
  }

  watch(endpointId, reloadIfExpanded, { flush: 'sync' })
  watch(keyId, reloadIfExpanded, { flush: 'sync' })
  if (expanded) {
    watch(expanded, (isExpanded) => {
      if (isExpanded) void loadFirstPage()
      else clear()
    })
  }

  return {
    clear,
    failed,
    items,
    loaded,
    loading,
    loadingMore,
    loadFirstPage,
    loadMore,
    nextCursor,
    retentionDays,
    retry,
  }
}
