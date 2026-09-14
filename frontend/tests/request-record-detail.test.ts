import { expect, mock, test } from 'bun:test'

const resetAffinity = mock(async () => ({
  data: { cleared: true, cleared_count: 2 },
}))
const loadRouteOptions = mock(async () => ({
  data: {
    affinity: {
      endpoint_id: null,
      endpoint_name: null,
      key_id: null,
      key_label: null,
      rule_id: null,
      state: 'unbound',
    },
    conversation_id: 'conv-1',
    current_endpoint_id: 'endpoint-1',
    current_endpoint_key_id: null,
    current_endpoint_key_label: null,
    options: [],
    override_endpoint_id: null,
    override_endpoint_key_id: null,
    override_endpoint_key_label: null,
  },
}))

const requestRecordFullMock = mock(async () => ({
  data: {
    record_id: 1,
    conversation_source: 'none',
    request_storage_mode: 'full',
    messages: [],
    rendered_text: '',
    total_messages: 0,
    has_more: false,
    next_cursor: null,
    limit: 10,
    offset: 0,
    order: 'desc',
  },
}))

mock.module('../src/generated/admin-api', () => ({
  deleteConversationEndpointOverride: mock(),
  requestRecordDetail: mock(),
  requestRecordFull: requestRecordFullMock,
  requestRecordResetSessionAffinity: resetAffinity,
  requestRecordSessionRouteOptions: loadRouteOptions,
  setConversationEndpointOverride: mock(),
}))

mock.module('../src/composables/useLocale', () => ({
  useLocale: () => ({ t: (key: string) => key }),
}))

const { createRequestRecordDetailState } =
  await import('../src/stores/request-record-detail')

function detailStateWithRecord(recordId: number) {
  const state = createRequestRecordDetailState()
  state.detailRecord.value = { record_id: recordId } as never
  return state
}

test('reset session affinity uses the current record id and reloads realtime state', async () => {
  resetAffinity.mockClear()
  loadRouteOptions.mockClear()
  const state = detailStateWithRecord(42)

  const result = await state.resetSessionAffinity(42)

  expect(resetAffinity).toHaveBeenCalledTimes(1)
  expect(resetAffinity).toHaveBeenCalledWith({
    path: { record_id: 42 },
    responseStyle: 'data',
  })
  expect(result.cleared).toBe(true)
  expect(result.cleared_count).toBe(2)
  expect(loadRouteOptions).toHaveBeenCalledTimes(1)
  expect(state.sessionRouteOptions.value?.affinity.state).toBe('unbound')
  expect(state.affinityResetting.value).toBe(false)
})

test('reset session affinity propagates API errors and clears loading', async () => {
  resetAffinity.mockImplementation(async () => {
    throw new Error('backend unavailable')
  })
  const state = detailStateWithRecord(7)

  await expect(state.resetSessionAffinity(7)).rejects.toThrow(
    'backend unavailable',
  )
  expect(state.affinityResetting.value).toBe(false)
})

test('reset session affinity parses idempotent empty results', async () => {
  resetAffinity.mockImplementation(async () => ({
    data: { cleared: false, cleared_count: 0 },
  }))
  const state = detailStateWithRecord(9)

  const result = await state.resetSessionAffinity(9)

  expect(result.cleared).toBe(false)
  expect(result.cleared_count).toBe(0)
})

function fullMessage(index: number) {
  return {
    role: 'user',
    block_hash: `hash-${index}`,
    preview_text: `message ${index}`,
    content_json: { text: `message ${index}` },
    same_as_turn: null,
  }
}

test('first paint fetches 10 newest-first messages only', async () => {
  requestRecordFullMock.mockClear()
  requestRecordFullMock.mockImplementation(
    async (options: { query?: Record<string, unknown> }) => ({
      data: {
        record_id: 11,
        conversation_source: 'none',
        request_storage_mode: 'full',
        messages: [9, 8, 7].map(fullMessage),
        rendered_text: 'newest page',
        total_messages: 25,
        has_more: true,
        next_cursor: '10',
        limit: 10,
        offset: 0,
        order: 'desc',
      },
    }),
  )
  const state = detailStateWithRecord(11)

  const full = await state.loadRequestFull(11)

  expect(requestRecordFullMock).toHaveBeenCalledTimes(1)
  const call = requestRecordFullMock.mock.calls[0][0] as {
    query?: Record<string, unknown>
  }
  expect(call.query?.['limit']).toBe(10)
  expect(call.query?.['order']).toBe('desc')
  expect(full?.messages.map((message) => message.block_hash)).toEqual([
    'hash-9',
    'hash-8',
    'hash-7',
  ])
  expect(full?.total_messages).toBe(25)
  expect(full?.has_more).toBe(true)
  expect(state.requestFullLoading.value).toBe(false)
})

test('load more appends older messages and updates the cursor', async () => {
  const state = detailStateWithRecord(12)
  state.requestFull.value = {
    record_id: 12,
    conversation_source: 'none',
    request_storage_mode: 'full',
    messages: [9, 8].map(fullMessage),
    rendered_text: 'first',
    total_messages: 4,
    has_more: true,
    next_cursor: '2',
    limit: 10,
    offset: 0,
    order: 'desc',
  } as never
  requestRecordFullMock.mockClear()
  requestRecordFullMock.mockImplementation(async () => ({
    data: {
      record_id: 12,
      conversation_source: 'none',
      request_storage_mode: 'full',
      messages: [7, 6].map(fullMessage),
      rendered_text: 'second',
      total_messages: 4,
      has_more: false,
      next_cursor: null,
      limit: 10,
      offset: 2,
      order: 'desc',
    },
  }))

  const merged = await state.loadMoreRequestFull(12)

  expect(requestRecordFullMock).toHaveBeenCalledTimes(1)
  const call = requestRecordFullMock.mock.calls[0][0] as {
    query?: Record<string, unknown>
  }
  expect(call.query?.['cursor']).toBe('2')
  expect(merged?.messages.map((message) => message.block_hash)).toEqual([
    'hash-9',
    'hash-8',
    'hash-7',
    'hash-6',
  ])
  expect(merged?.has_more).toBe(false)
  expect(merged?.next_cursor).toBeNull()
  expect(state.requestFullLoadingMore.value).toBe(false)
})
