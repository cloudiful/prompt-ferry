import { afterEach, beforeEach, expect, mock, test } from 'bun:test'
import { nextTick, ref } from 'vue'
import type {
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationHistoryResponse,
} from '../src/generated/admin-api'

const fetchPage = mock(
  async (
    _endpointId: string,
    _keyId: string | null,
    _limit?: number,
    _beforeId?: string,
  ): Promise<QuotaKeyObservationHistoryResponse> => ({
    items: [],
    next_cursor: null,
    retention_days: 30,
  }),
)

const { useKeyQuotaObservationHistory } =
  await import('../src/composables/useKeyQuotaObservationHistory')

function item(
  observation_id: string,
  key_id = 'key-a',
): QuotaKeyObservationHistoryItem {
  return {
    observation_id,
    key_id,
    key_label: `label-${key_id}`,
    provider: 'minimax',
    observed_at: '2026-10-10T12:00:00Z',
    ok: true,
    status: '200',
    error_code: null,
    plan_type: null,
    limit_reached: false,
    windows: [],
    source: 'request',
  }
}

function page(
  ids: string[],
  nextCursor: string | null = null,
  keyId = 'key-a',
): QuotaKeyObservationHistoryResponse {
  return {
    items: ids.map((id) => item(id, keyId)),
    next_cursor: nextCursor,
    retention_days: 30,
  }
}

beforeEach(() => {
  fetchPage.mockReset()
  fetchPage.mockImplementation(async () => page([]))
})

afterEach(() => {
  fetchPage.mockReset()
})

test('key pages use string cursors and de-duplicate observation IDs', async () => {
  fetchPage
    .mockResolvedValueOnce(page(['90', '80'], '80'))
    .mockResolvedValueOnce(page(['80', '70']))
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
  )

  await history.loadFirstPage()
  await history.loadMore()

  expect(fetchPage.mock.calls).toEqual([
    ['endpoint-a', null, 50],
    ['endpoint-a', null, 50, '80'],
  ])
  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    '90',
    '80',
    '70',
  ])
  expect(history.nextCursor.value).toBeNull()
})

test('key filter scopes reads and reloads when the key changes', async () => {
  fetchPage.mockImplementation(async (_endpoint, keyId) =>
    keyId === 'key-b' ? page(['7'], null, 'key-b') : page(['9', '8']),
  )
  const keyId = ref<string | null>(null)
  const expanded = ref(true)
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    keyId,
    fetchPage,
    expanded,
  )

  await history.loadFirstPage()
  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    '9',
    '8',
  ])

  keyId.value = 'key-b'
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchPage.mock.calls.at(-1)).toEqual(['endpoint-a', 'key-b', 50])
  expect(history.items.value.map((row) => row.observation_id)).toEqual(['7'])
  expect(history.items.value[0].key_id).toBe('key-b')
})

test('history is lazy with zero initial fetch and refreshes on expand', async () => {
  fetchPage.mockImplementation(async () => page(['50']))
  const expanded = ref(false)
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
    expanded,
  )

  await nextTick()
  expect(fetchPage).not.toHaveBeenCalled()
  expect(history.loaded.value).toBe(false)

  expanded.value = true
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchPage).toHaveBeenCalledTimes(1)

  expanded.value = false
  await nextTick()
  expect(history.items.value).toEqual([])
  expect(history.loaded.value).toBe(false)
})

test('history failures remain distinct from empty and can be retried', async () => {
  fetchPage
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockResolvedValueOnce(page(['10']))
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
  )

  await history.loadFirstPage()
  expect(history.failed.value).toBe(true)
  expect(history.loaded.value).toBe(false)
  expect(history.items.value).toEqual([])

  await history.retry()
  expect(history.failed.value).toBe(false)
  expect(history.loaded.value).toBe(true)
  expect(history.items.value.map((row) => row.observation_id)).toEqual(['10'])
})

test('late responses from an endpoint and key switch cannot replace the selection', async () => {
  let resolveFirst!: (value: QuotaKeyObservationHistoryResponse) => void
  fetchPage
    .mockImplementationOnce(
      () => new Promise((resolve) => (resolveFirst = resolve)),
    )
    .mockResolvedValueOnce(page(['endpoint-b-snapshot'], null, 'key-b'))
  const endpointId = ref('endpoint-a')
  const keyId = ref<string | null>('key-a')
  const history = useKeyQuotaObservationHistory(endpointId, keyId, fetchPage)

  const firstRequest = history.loadFirstPage()
  endpointId.value = 'endpoint-b'
  keyId.value = 'key-b'
  await history.loadFirstPage()
  resolveFirst(page(['endpoint-a-snapshot'], null, 'key-a'))
  await firstRequest

  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    'endpoint-b-snapshot',
  ])
  expect(history.retentionDays.value).toBe(30)
})
