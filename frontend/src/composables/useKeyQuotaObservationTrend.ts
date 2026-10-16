import { getCurrentScope, onScopeDispose, ref, watch, type Ref } from 'vue'
import type { QuotaKeyObservationTrendResponse } from '@/generated/admin-api'

// Issue #911 P2: read-only trend reader for the expanded per-key history.
// It owns no list state: the chart and the database-paged list load, fail and
// retry independently, so a slow trend never blocks a keyset page and vice
// versa. Every request is generation-guarded, so a late response can never
// replace a newer key/range/endpoint selection or clear its loading state.
export type KeyQuotaTrendRange = '24h' | '7d' | '30d'

export const KEY_QUOTA_TREND_RANGES: KeyQuotaTrendRange[] = ['24h', '7d', '30d']

export type KeyQuotaTrendFetcher = (
  endpointId: string,
  keyId: string | null,
  range: KeyQuotaTrendRange,
) => Promise<QuotaKeyObservationTrendResponse>

export function useKeyQuotaObservationTrend(
  endpointId: Ref<string>,
  keyId: Ref<string | null>,
  range: Ref<KeyQuotaTrendRange>,
  fetchTrend: KeyQuotaTrendFetcher,
  active?: Ref<boolean>,
) {
  const trend = ref<QuotaKeyObservationTrendResponse | null>(null)
  const loading = ref(false)
  const failed = ref(false)
  const loaded = ref(false)
  let generation = 0

  function clear(): void {
    generation += 1
    trend.value = null
    loading.value = false
    failed.value = false
    loaded.value = false
  }

  function isCurrent(
    requestGeneration: number,
    id: string,
    key: string | null,
    selectedRange: KeyQuotaTrendRange,
  ): boolean {
    return (
      requestGeneration === generation &&
      id === endpointId.value &&
      key === keyId.value &&
      selectedRange === range.value
    )
  }

  function isActive(): boolean {
    return active ? active.value : true
  }

  async function load(): Promise<void> {
    const id = endpointId.value
    if (!id) return
    const key = keyId.value
    const selectedRange = range.value
    const requestGeneration = ++generation
    failed.value = false
    loading.value = true
    try {
      const result = await fetchTrend(id, key, selectedRange)
      if (!isCurrent(requestGeneration, id, key, selectedRange)) return
      trend.value = result
      loaded.value = true
    } catch {
      if (isCurrent(requestGeneration, id, key, selectedRange)) {
        failed.value = true
      }
    } finally {
      if (isCurrent(requestGeneration, id, key, selectedRange)) {
        loading.value = false
      }
    }
  }

  async function retry(): Promise<void> {
    await load()
  }

  // Filter changes invalidate the current trace. While the chart is not
  // active the data is dropped but not fetched, so a later activation reads
  // the new selection lazily instead of showing the previous one.
  function reload(): void {
    clear()
    if (endpointId.value && isActive()) void load()
  }

  watch([endpointId, keyId, range], reload, { flush: 'sync' })
  if (active) {
    watch(
      active,
      (isActiveNow) => {
        if (isActiveNow && !loaded.value && !loading.value) void load()
      },
      { flush: 'sync', immediate: true },
    )
  }

  // A response that lands after the owning component is disposed must not
  // write into it; bumping the generation turns every late success, error and
  // finally into an ignored outcome.
  if (getCurrentScope()) {
    onScopeDispose(() => {
      generation += 1
    })
  }

  return {
    clear,
    failed,
    load,
    loaded,
    loading,
    reload,
    retry,
    trend,
  }
}
