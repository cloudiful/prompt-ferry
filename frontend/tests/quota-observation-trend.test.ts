import { expect, mock, test } from 'bun:test'
import { effectScope, nextTick, ref } from 'vue'
import type { QuotaKeyObservationTrendResponse } from '../src/generated/admin-api'

const { useKeyQuotaObservationTrend } =
  await import('../src/composables/useKeyQuotaObservationTrend')

function response(
  overrides: Partial<QuotaKeyObservationTrendResponse> = {},
): QuotaKeyObservationTrendResponse {
  return {
    bucket_count: 96,
    bucket_seconds: 900,
    failed_observations: 0,
    from: '2026-10-15T00:00:00Z',
    keys_truncated: false,
    range: '24h',
    sampled_observations: 1,
    series: [],
    series_truncated: false,
    to: '2026-10-16T00:00:00Z',
    ...overrides,
  }
}

async function settle(): Promise<void> {
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
}

test('the trend stays lazy until the chart is first activated', async () => {
  const fetchTrend = mock(async () => response())
  const active = ref(false)
  const trend = useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    ref<string | null>(null),
    ref('24h'),
    fetchTrend,
    active,
  )

  await settle()
  expect(fetchTrend).not.toHaveBeenCalled()
  expect(trend.loaded.value).toBe(false)

  active.value = true
  await settle()
  expect(fetchTrend.mock.calls).toEqual([['endpoint-a', null, '24h']])
  expect(trend.loaded.value).toBe(true)
  expect(trend.trend.value?.range).toBe('24h')
})

test('range and key changes refetch with the exact backend params', async () => {
  const fetchTrend = mock(async () => response())
  const keyId = ref<string | null>(null)
  const range = ref<'24h' | '7d' | '30d'>('24h')
  useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    keyId,
    range,
    fetchTrend,
    ref(true),
  )

  await settle()
  range.value = '7d'
  await settle()
  keyId.value = 'key-b'
  await settle()
  range.value = '30d'
  await settle()

  expect(fetchTrend.mock.calls).toEqual([
    ['endpoint-a', null, '24h'],
    ['endpoint-a', null, '7d'],
    ['endpoint-a', 'key-b', '7d'],
    ['endpoint-a', 'key-b', '30d'],
  ])
})

test('a late response cannot replace a newer selection', async () => {
  let resolveFirst!: (value: QuotaKeyObservationTrendResponse) => void
  const fetchTrend = mock(
    (
      _endpointId: string,
      _keyId: string | null,
      range: '24h' | '7d' | '30d',
    ) =>
      range === '7d'
        ? Promise.resolve(response({ range: '7d' }))
        : new Promise<QuotaKeyObservationTrendResponse>((resolve) => {
            resolveFirst = resolve
          }),
  )
  const range = ref<'24h' | '7d' | '30d'>('24h')
  const trend = useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    ref<string | null>(null),
    range,
    fetchTrend,
    ref(true),
  )

  await settle()
  range.value = '7d'
  await settle()
  expect(trend.trend.value?.range).toBe('7d')

  resolveFirst(response({ range: '24h' }))
  await settle()
  expect(trend.trend.value?.range).toBe('7d')
})

test('a stale failure cannot flag the newer request or clear its loading', async () => {
  let rejectFirst!: (error: Error) => void
  const fetchTrend = mock(
    (
      _endpointId: string,
      _keyId: string | null,
      range: '24h' | '7d' | '30d',
    ) =>
      range === '7d'
        ? Promise.resolve(response({ range: '7d' }))
        : new Promise<QuotaKeyObservationTrendResponse>((_resolve, reject) => {
            rejectFirst = reject
          }),
  )
  const range = ref<'24h' | '7d' | '30d'>('24h')
  const trend = useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    ref<string | null>(null),
    range,
    fetchTrend,
    ref(true),
  )

  await settle()
  expect(trend.loading.value).toBe(true)
  range.value = '7d'
  await settle()
  expect(trend.loading.value).toBe(false)
  expect(trend.failed.value).toBe(false)

  rejectFirst(new Error('stale failure'))
  await settle()
  expect(trend.failed.value).toBe(false)
  expect(trend.loading.value).toBe(false)
  expect(trend.trend.value?.range).toBe('7d')
})

test('a failure stays distinct from empty and can be retried', async () => {
  const fetchTrend = mock(async () => response())
  fetchTrend.mockRejectedValueOnce(new Error('unavailable'))
  const trend = useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    ref<string | null>(null),
    ref('24h'),
    fetchTrend,
    ref(true),
  )

  await settle()
  expect(trend.failed.value).toBe(true)
  expect(trend.loaded.value).toBe(false)
  expect(trend.trend.value).toBeNull()

  await trend.retry()
  await settle()
  expect(trend.failed.value).toBe(false)
  expect(trend.loaded.value).toBe(true)
})

test('an endpoint change while inactive invalidates without fetching', async () => {
  const fetchTrend = mock(async () => response())
  const endpointId = ref('endpoint-a')
  const active = ref(false)
  const trend = useKeyQuotaObservationTrend(
    endpointId,
    ref<string | null>(null),
    ref('24h'),
    fetchTrend,
    active,
  )

  await settle()
  expect(trend.trend.value).toBeNull()
  endpointId.value = 'endpoint-b'
  await settle()
  expect(fetchTrend).not.toHaveBeenCalled()
  expect(trend.trend.value).toBeNull()

  active.value = true
  await settle()
  expect(fetchTrend.mock.calls).toEqual([['endpoint-b', null, '24h']])
})

test('collapsing clears the trace and reopening refetches', async () => {
  const fetchTrend = mock(async () => response())
  const active = ref(true)
  const trend = useKeyQuotaObservationTrend(
    ref('endpoint-a'),
    ref<string | null>(null),
    ref('24h'),
    fetchTrend,
    active,
  )

  await settle()
  expect(trend.loaded.value).toBe(true)

  active.value = false
  trend.clear()
  expect(trend.trend.value).toBeNull()
  expect(trend.loaded.value).toBe(false)

  active.value = true
  await settle()
  expect(fetchTrend).toHaveBeenCalledTimes(2)
})

test('a response landing after disposal never writes into the disposed scope', async () => {
  let resolveFirst!: (value: QuotaKeyObservationTrendResponse) => void
  const fetchTrend = mock(
    () =>
      new Promise<QuotaKeyObservationTrendResponse>((resolve) => {
        resolveFirst = resolve
      }),
  )
  const scope = effectScope()
  let trend!: ReturnType<typeof useKeyQuotaObservationTrend>
  scope.run(() => {
    trend = useKeyQuotaObservationTrend(
      ref('endpoint-a'),
      ref<string | null>(null),
      ref('24h'),
      fetchTrend,
      ref(true),
    )
  })

  await settle()
  expect(trend.loading.value).toBe(true)

  scope.stop()
  resolveFirst(response({ range: '24h' }))
  await settle()
  expect(trend.trend.value).toBeNull()
  expect(trend.loaded.value).toBe(false)
})
