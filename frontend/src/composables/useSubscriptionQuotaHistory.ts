import { ref, watch, type Ref } from 'vue'
import type {
  QuotaSnapshotHistoryItem,
  QuotaSnapshotHistoryResponse,
} from '@/generated/admin-api'
type HistoryFetcher = (
  endpointId: string,
  limit?: number,
  beforeId?: string,
) => Promise<QuotaSnapshotHistoryResponse>

const PAGE_SIZE = 50
const RETENTION_DAYS = 30

export function useSubscriptionQuotaHistory(
  endpointId: Ref<string>,
  fetchPage: HistoryFetcher,
  expanded?: Ref<boolean>,
  refreshKey?: Ref<string>,
) {
  const items = ref<QuotaSnapshotHistoryItem[]>([])
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
    if (!id) return
    const requestGeneration = ++generation
    items.value = []
    nextCursor.value = null
    failed.value = false
    loaded.value = false
    loadingMore.value = false
    loading.value = true
    try {
      const page = await fetchPage(id, PAGE_SIZE)
      if (requestGeneration !== generation || id !== endpointId.value) return
      items.value = page.items
      nextCursor.value = page.next_cursor ?? null
      retentionDays.value = page.retention_days
      loaded.value = true
    } catch {
      if (requestGeneration === generation && id === endpointId.value) {
        failed.value = true
      }
    } finally {
      if (requestGeneration === generation && id === endpointId.value) {
        loading.value = false
      }
    }
  }

  async function loadMore(): Promise<void> {
    const id = endpointId.value
    const cursor = nextCursor.value
    if (!id || !cursor || !loaded.value || loading.value || loadingMore.value)
      return
    const requestGeneration = generation
    failed.value = false
    loadingMore.value = true
    try {
      const page = await fetchPage(id, PAGE_SIZE, cursor)
      if (requestGeneration !== generation || id !== endpointId.value) return
      const seen = new Set(items.value.map((item) => item.snapshot_id))
      const additionalItems = page.items.filter((item) => {
        if (seen.has(item.snapshot_id)) return false
        seen.add(item.snapshot_id)
        return true
      })
      items.value = [...items.value, ...additionalItems]
      nextCursor.value = page.next_cursor ?? null
      retentionDays.value = page.retention_days
    } catch {
      if (requestGeneration === generation && id === endpointId.value) {
        failed.value = true
      }
    } finally {
      if (requestGeneration === generation && id === endpointId.value) {
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

  watch(
    endpointId,
    (id) => {
      clear()
      if (id && expanded?.value) void loadFirstPage()
    },
    { flush: 'sync' },
  )
  if (expanded) {
    watch(expanded, (isExpanded) => {
      if (isExpanded) void loadFirstPage()
      else clear()
    })
  }
  if (expanded && refreshKey) {
    watch(refreshKey, (next, previous) => {
      if (
        expanded.value &&
        (loaded.value || failed.value) &&
        next !== previous
      ) {
        void loadFirstPage()
      }
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
