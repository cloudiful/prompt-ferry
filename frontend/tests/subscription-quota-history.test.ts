import { afterEach, beforeEach, expect, mock, test } from 'bun:test'
import { nextTick, ref } from 'vue'
import type { QuotaSnapshotHistoryResponse } from '../src/generated/admin-api'

const fetchQuotaSnapshotHistory = mock(
  async (
    _endpointId: string,
    _limit?: number,
    _beforeId?: string,
  ): Promise<QuotaSnapshotHistoryResponse> => ({
    items: [],
    next_cursor: null,
    retention_days: 30,
  }),
)

const { useSubscriptionQuotaHistory } =
  await import('../src/composables/useSubscriptionQuotaHistory')

function page(
  ids: string[],
  nextCursor: string | null = null,
): QuotaSnapshotHistoryResponse {
  return {
    items: ids.map((snapshot_id) => ({
      snapshot_id,
      observed_at: '2026-10-10T12:00:00Z',
      plan_type: 'plus',
      limit_reached: false,
      windows: [],
      source: 'manual',
    })),
    next_cursor: nextCursor,
    retention_days: 30,
  }
}

beforeEach(() => {
  fetchQuotaSnapshotHistory.mockReset()
  fetchQuotaSnapshotHistory.mockImplementation(async () => page([]))
})

afterEach(() => {
  fetchQuotaSnapshotHistory.mockReset()
})

test('history pages use string cursors and de-duplicate snapshot IDs', async () => {
  fetchQuotaSnapshotHistory
    .mockResolvedValueOnce(page(['90', '80'], '80'))
    .mockResolvedValueOnce(page(['80', '70']))
  const history = useSubscriptionQuotaHistory(
    ref('endpoint-a'),
    fetchQuotaSnapshotHistory,
  )

  await history.loadFirstPage()
  await history.loadMore()

  expect(fetchQuotaSnapshotHistory.mock.calls).toEqual([
    ['endpoint-a', 50],
    ['endpoint-a', 50, '80'],
  ])
  expect(history.items.value.map((item) => item.snapshot_id)).toEqual([
    '90',
    '80',
    '70',
  ])
  expect(history.nextCursor.value).toBeNull()
})

test('history is lazy and refreshes one first page when the current observation changes', async () => {
  fetchQuotaSnapshotHistory.mockImplementation(async () => page(['50']))
  const expanded = ref(false)
  const refreshKey = ref('initial')
  const history = useSubscriptionQuotaHistory(
    ref('endpoint-a'),
    fetchQuotaSnapshotHistory,
    expanded,
    refreshKey,
  )

  await nextTick()
  expect(fetchQuotaSnapshotHistory).not.toHaveBeenCalled()
  expanded.value = true
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchQuotaSnapshotHistory).toHaveBeenCalledTimes(1)

  refreshKey.value = 'manual-refresh'
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchQuotaSnapshotHistory).toHaveBeenCalledTimes(2)
  expect(fetchQuotaSnapshotHistory.mock.calls[1]).toEqual(['endpoint-a', 50])
  expect(history.items.value.map((item) => item.snapshot_id)).toEqual(['50'])

  expanded.value = false
  await nextTick()
  expect(history.items.value).toEqual([])
  expect(history.loaded.value).toBe(false)
})

test('history failures remain distinct from empty and can be retried', async () => {
  fetchQuotaSnapshotHistory
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockResolvedValueOnce(page(['10']))
  const history = useSubscriptionQuotaHistory(
    ref('endpoint-a'),
    fetchQuotaSnapshotHistory,
  )

  await history.loadFirstPage()
  expect(history.failed.value).toBe(true)
  expect(history.loaded.value).toBe(false)
  expect(history.items.value).toEqual([])

  await history.retry()
  expect(history.failed.value).toBe(false)
  expect(history.loaded.value).toBe(true)
  expect(history.items.value.map((item) => item.snapshot_id)).toEqual(['10'])
})

test('late responses from an endpoint switch cannot replace the selected history', async () => {
  let resolveFirst!: (value: QuotaSnapshotHistoryResponse) => void
  fetchQuotaSnapshotHistory
    .mockImplementationOnce(
      () => new Promise((resolve) => (resolveFirst = resolve)),
    )
    .mockResolvedValueOnce(page(['endpoint-b-snapshot']))
  const endpointId = ref('endpoint-a')
  const history = useSubscriptionQuotaHistory(
    endpointId,
    fetchQuotaSnapshotHistory,
  )

  const firstRequest = history.loadFirstPage()
  endpointId.value = 'endpoint-b'
  await history.loadFirstPage()
  resolveFirst(page(['endpoint-a-snapshot']))
  await firstRequest

  expect(history.items.value.map((item) => item.snapshot_id)).toEqual([
    'endpoint-b-snapshot',
  ])
  expect(history.retentionDays.value).toBe(30)
})
