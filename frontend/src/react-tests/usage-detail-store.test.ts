import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RequestRecordDetail,
  RequestRecordFullResponse,
  SessionRouteOptionsResponse,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  deleteConversationEndpointOverride: vi.fn(),
  requestRecordDetail: vi.fn(),
  requestRecordFull: vi.fn(),
  requestRecordResetSessionAffinity: vi.fn(),
  requestRecordSessionRouteOptions: vi.fn(),
  setConversationEndpointOverride: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { useUsageDetailStore, usageDetailActionKey } from '@/lib/usage-detail'
import { usageAffinityNoticeKey } from '@/lib/usage-detail-model'
import { getPendingActions } from '@/lib/async-action'

const admin = { is_admin: true, user_id: 1 }

function detail(
  overrides: Partial<RequestRecordDetail> = {},
): RequestRecordDetail {
  return {
    conversation_id: 'conversation-1',
    conversation_seq: 1,
    conversation_source: 'fixture',
    created_at: '2026-03-04T23:30:00.000Z',
    has_full_request: true,
    has_parent: false,
    http_request_compressed: false,
    path: '/v1/responses',
    record_id: 1,
    redaction: {
      applied: false,
      fields: [],
      findings_count: 0,
      replacements_count: 0,
      types: [],
    },
    request_category: 'ai',
    request_has_previous_response_id: false,
    request_id: 'req-1',
    request_raw_json: { reasoning_effort: 'low' },
    request_state: 'completed',
    request_storage_mode: 'normalized',
    response_capture_truncated: false,
    route_selection_reason: 'default',
    storage_sanitized: false,
    storage_sanitized_nul_count: 0,
    ...overrides,
  }
}

function full(
  overrides: Partial<RequestRecordFullResponse> = {},
): RequestRecordFullResponse {
  return {
    conversation_source: 'fixture',
    has_more: false,
    limit: 10,
    messages: [
      {
        block_hash: 'block-1',
        content_json: { content: 'one' },
        preview_text: 'one',
        role: 'user',
        same_as_turn: null,
      },
    ],
    next_cursor: null,
    offset: 0,
    order: 'desc',
    record_id: 1,
    rendered_text: 'one',
    request_has_previous_response_id: false,
    request_storage_mode: 'normalized',
    total_messages: 1,
    ...overrides,
  }
}

function options(
  overrides: Partial<SessionRouteOptionsResponse> = {},
): SessionRouteOptionsResponse {
  return {
    affinity: { state: 'active' },
    conversation_id: 'conversation-1',
    current_endpoint_id: 'endpoint-1',
    current_endpoint_key_id: 'key-1',
    current_endpoint_key_label: 'primary',
    options: [
      {
        endpoint_id: 'endpoint-1',
        endpoint_name: 'east-1',
        is_override: false,
        is_preferred: true,
        keys: [{ key_id: 'key-1', key_label: 'primary' }],
      },
    ],
    override_endpoint_id: null,
    override_endpoint_key_id: null,
    override_endpoint_key_label: null,
    ...overrides,
  }
}

const store = () => useUsageDetailStore.getState()

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

beforeEach(() => {
  vi.clearAllMocks()
  api.requestRecordDetail.mockImplementation(async (request: any) => ({
    data: detail({ record_id: Number(request?.path?.record_id ?? 1) }),
  }))
  api.requestRecordFull.mockImplementation(async () => ({ data: full() }))
  api.requestRecordSessionRouteOptions.mockResolvedValue({ data: options() })
  api.setConversationEndpointOverride.mockResolvedValue({
    data: {
      conversation_id: 'conversation-1',
      created_at: '2026-03-04T00:00:00.000Z',
      endpoint_id: 'endpoint-1',
      endpoint_key_id: 'key-1',
      endpoint_key_label: 'primary',
      endpoint_name: 'east-1',
      updated_at: '2026-03-04T00:00:00.000Z',
    },
  })
  api.deleteConversationEndpointOverride.mockResolvedValue({ data: undefined })
  api.requestRecordResetSessionAffinity.mockResolvedValue({
    data: { cleared: true, cleared_count: 1 },
  })
  store().initializeIdentity(admin)
})

afterEach(() => {
  store().reset()
  store().initializeIdentity(null)
})

describe('usage detail store reads', () => {
  it('loads the detail with its route options and keeps the display labels', async () => {
    const view = await store().loadDetail(1)

    expect(view?.record_id).toBe(1)
    expect(store().detail?.conversation_source_label).toBe('fixture')
    expect(store().detail?.model_display).toBe('-')
    expect(api.requestRecordSessionRouteOptions).toHaveBeenCalledTimes(1)
    expect(store().routeOptions?.current_upstream_label).toBe('east-1')
    expect(store().detailLoading).toBe(false)
    expect(store().routeOptionsLoading).toBe(false)
  })

  it('clears the route options without a read when the record has no conversation', async () => {
    api.requestRecordDetail.mockResolvedValueOnce({
      data: detail({ conversation_id: null }),
    })
    await store().loadDetail(1)

    expect(api.requestRecordSessionRouteOptions).not.toHaveBeenCalled()
    expect(store().routeOptions).toBeNull()
    expect(store().override).toBeNull()
  })

  it('keeps the newer detail when an older read settles late', async () => {
    const older = deferred<unknown>()
    api.requestRecordDetail.mockReturnValueOnce(older.promise)

    const first = store().loadDetail(1)
    await store().loadDetail(2)
    older.resolve({ data: detail({ record_id: 1, model: 'older-model' }) })
    await first

    expect(store().detail?.record_id).toBe(2)
    expect(store().detail?.model).toBeUndefined()
    expect(store().detailLoading).toBe(false)
  })

  it('reports a failed detail read and clears the loading flag', async () => {
    api.requestRecordDetail.mockRejectedValueOnce(new Error('detail failed'))

    await expect(store().loadDetail(1)).rejects.toThrow('detail failed')
    expect(store().detailLoading).toBe(false)
    expect(store().detail).toBeNull()
  })

  it('clears the route options on a failed options read', async () => {
    api.requestRecordSessionRouteOptions.mockRejectedValueOnce(
      new Error('options failed'),
    )
    await store().loadDetail(1)

    expect(store().routeOptions).toBeNull()
    expect(store().override).toBeNull()
    expect(store().routeOptionsLoading).toBe(false)
  })

  it('reads the first request-full page once with ten newest-first messages', async () => {
    await store().loadDetail(1)
    const first = store().loadRequestFull(1)
    const second = store().loadRequestFull(1)
    await Promise.all([first, second])

    expect(api.requestRecordFull).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFull.mock.calls[0]?.[0]?.query).toEqual({
      limit: 10,
      order: 'desc',
    })
    await store().loadRequestFull(1)
    expect(api.requestRecordFull).toHaveBeenCalledTimes(1)
  })

  it('appends the next cursor page and merges the metadata', async () => {
    api.requestRecordFull
      .mockResolvedValueOnce({
        data: full({
          has_more: true,
          messages: [
            {
              block_hash: 'block-2',
              content_json: {},
              preview_text: 'two',
              role: 'user',
              same_as_turn: null,
            },
          ],
          next_cursor: '1',
          rendered_text: 'two',
          total_messages: 2,
        }),
      })
      .mockResolvedValueOnce({
        data: full({
          has_more: false,
          messages: [
            {
              block_hash: 'block-1',
              content_json: {},
              preview_text: 'one',
              role: 'assistant',
              same_as_turn: null,
            },
          ],
          next_cursor: null,
          offset: 1,
          rendered_text: 'one',
          total_messages: 2,
        }),
      })
    await store().loadDetail(1)
    await store().loadRequestFull(1)
    const merged = await store().loadMoreRequestFull(1)

    expect(api.requestRecordFull.mock.calls[1]?.[0]?.query).toEqual({
      cursor: '1',
      limit: 10,
      order: 'desc',
    })
    expect(merged?.messages.map((message) => message.block_hash)).toEqual([
      'block-2',
      'block-1',
    ])
    expect(merged?.rendered_text).toBe('two\none')
    expect(merged?.has_more).toBe(false)
    expect(merged?.next_cursor).toBeNull()
    expect(store().requestFullLoadingMore).toBe(false)
  })

  it('does not load more without a cursor or while a load is in flight', async () => {
    api.requestRecordFull.mockResolvedValueOnce({
      data: full({ has_more: true, next_cursor: '1' }),
    })
    await store().loadDetail(1)
    await store().loadRequestFull(1)

    const gate = deferred<unknown>()
    api.requestRecordFull.mockReturnValueOnce(gate.promise)
    const first = store().loadMoreRequestFull(1)
    const second = await store().loadMoreRequestFull(1)
    expect(second?.messages).toHaveLength(1)
    gate.resolve({ data: full({ messages: [] }) })
    await first
    expect(api.requestRecordFull).toHaveBeenCalledTimes(2)

    api.requestRecordFull.mockResolvedValueOnce({
      data: full({ has_more: true, next_cursor: null }),
    })
    await store().loadDetail(2)
    await store().loadRequestFull(2)
    await store().loadMoreRequestFull(2)
    expect(api.requestRecordFull).toHaveBeenCalledTimes(3)
  })
})

describe('usage detail store session routing', () => {
  it('keeps the conversation key shared by save, clear and reset', async () => {
    await store().loadDetail(1)
    const gate = deferred<unknown>()
    api.setConversationEndpointOverride.mockReturnValueOnce(gate.promise)

    const saving = store().saveOverride('conversation-1', 'endpoint-1', null)
    expect(getPendingActions()['conversation:conversation-1']).toBe('save')

    const cleared = await store().clearOverride('conversation-1')
    const reset = await store().resetAffinity(1)
    expect(cleared.executed).toBe(false)
    expect(reset.executed).toBe(false)
    expect(api.deleteConversationEndpointOverride).not.toHaveBeenCalled()
    expect(api.requestRecordResetSessionAffinity).not.toHaveBeenCalled()

    api.requestRecordSessionRouteOptions.mockResolvedValueOnce({
      data: options({
        override_endpoint_id: 'endpoint-1',
        override_endpoint_key_id: null,
        override_endpoint_key_label: null,
      }),
    })
    gate.resolve({
      data: {
        conversation_id: 'conversation-1',
        created_at: '2026-03-04T00:00:00.000Z',
        endpoint_id: 'endpoint-1',
        endpoint_key_id: null,
        endpoint_key_label: null,
        endpoint_name: 'east-1',
        updated_at: '2026-03-04T00:00:00.000Z',
      },
    })
    await saving
    expect(store().override?.endpoint_id).toBe('endpoint-1')
    expect(getPendingActions()['conversation:conversation-1']).toBeUndefined()
  })

  it('lets another conversation save independently', async () => {
    await store().loadDetail(1)
    const gate = deferred<unknown>()
    api.setConversationEndpointOverride.mockReturnValueOnce(gate.promise)
    const first = store().saveOverride('conversation-1', 'endpoint-1', null)
    const other = await store().saveOverride(
      'conversation-2',
      'endpoint-1',
      null,
    )
    expect(other.executed).toBe(true)
    gate.resolve({
      data: {
        conversation_id: 'conversation-1',
        created_at: '',
        endpoint_id: 'endpoint-1',
        endpoint_key_id: null,
        endpoint_key_label: null,
        endpoint_name: 'east-1',
        updated_at: '',
      },
    })
    await first
  })

  it('does not replace the override after the conversation changed', async () => {
    await store().loadDetail(1)
    const gate = deferred<unknown>()
    api.setConversationEndpointOverride.mockReturnValueOnce(gate.promise)
    const saving = store().saveOverride('conversation-1', 'endpoint-1', null)

    api.requestRecordDetail.mockResolvedValueOnce({
      data: detail({ conversation_id: 'conversation-2', record_id: 2 }),
    })
    api.requestRecordSessionRouteOptions.mockResolvedValueOnce({
      data: options({ conversation_id: 'conversation-2' }),
    })
    await store().loadDetail(2)
    const optionsCallsAfterSwitch =
      api.requestRecordSessionRouteOptions.mock.calls.length

    gate.resolve({
      data: {
        conversation_id: 'conversation-1',
        created_at: '',
        endpoint_id: 'endpoint-1',
        endpoint_key_id: null,
        endpoint_key_label: null,
        endpoint_name: 'east-1',
        updated_at: '',
      },
    })
    await saving

    expect(store().detail?.record_id).toBe(2)
    expect(store().override).toBeNull()
    expect(api.requestRecordSessionRouteOptions).toHaveBeenCalledTimes(
      optionsCallsAfterSwitch,
    )
  })

  it('surfaces a 503 reset failure, releases the gate and allows a retry', async () => {
    await store().loadDetail(1)
    api.requestRecordResetSessionAffinity.mockRejectedValueOnce(
      new Error('affinity backend unavailable'),
    )
    await expect(store().resetAffinity(1)).rejects.toThrow(
      'affinity backend unavailable',
    )
    expect(getPendingActions()['conversation:conversation-1']).toBeUndefined()

    api.requestRecordResetSessionAffinity.mockResolvedValueOnce({
      data: { cleared: true, cleared_count: 2 },
    })
    const retry = await store().resetAffinity(1)
    expect(retry.executed && retry.value.cleared_count).toBe(2)
  })

  it('releases the gate after a failed write', async () => {
    await store().loadDetail(1)
    api.setConversationEndpointOverride.mockRejectedValueOnce(
      new Error('conflict'),
    )

    await expect(
      store().saveOverride('conversation-1', 'endpoint-1', null),
    ).rejects.toThrow('conflict')
    expect(getPendingActions()['conversation:conversation-1']).toBeUndefined()
  })

  it('resets the affinity through the record key when there is no conversation', async () => {
    api.requestRecordDetail.mockResolvedValueOnce({
      data: detail({ conversation_id: null }),
    })
    await store().loadDetail(1)

    const outcome = await store().resetAffinity(1)
    expect(outcome.executed).toBe(true)
    expect(getPendingActions()[usageDetailActionKey(null, 1)]).toBeUndefined()
    expect(api.requestRecordResetSessionAffinity).toHaveBeenCalledTimes(1)
  })

  it('refuses a reset for a record that is no longer open', async () => {
    await store().loadDetail(1)
    const outcome = await store().resetAffinity(2)
    expect(outcome.executed).toBe(false)
    expect(api.requestRecordResetSessionAffinity).not.toHaveBeenCalled()
  })

  it('reports an unbound reset without inventing a cleared count', async () => {
    api.requestRecordResetSessionAffinity.mockResolvedValueOnce({
      data: { cleared: false, cleared_count: 0 },
    })
    await store().loadDetail(1)
    const outcome = await store().resetAffinity(1)
    expect(outcome.executed && outcome.value).toEqual({
      cleared: false,
      cleared_count: 0,
    })
  })

  it('clears the override and reloads the options', async () => {
    await store().loadDetail(1)
    const before = api.requestRecordSessionRouteOptions.mock.calls.length
    const outcome = await store().clearOverride('conversation-1')

    expect(outcome.executed).toBe(true)
    expect(api.deleteConversationEndpointOverride).toHaveBeenCalledTimes(1)
    expect(api.requestRecordSessionRouteOptions).toHaveBeenCalledTimes(
      before + 1,
    )
  })

  it('clears every payload when the identity changes', async () => {
    await store().loadDetail(1)
    await store().loadRequestFull(1)
    expect(store().detail).not.toBeNull()

    store().initializeIdentity({ is_admin: false, user_id: 2 })
    expect(store().detail).toBeNull()
    expect(store().requestFull).toBeNull()
    expect(store().routeOptions).toBeNull()
    expect(store().recordId).toBeNull()
  })
})

describe('usage affinity notice', () => {
  it('reports a reset only when a binding was actually cleared', () => {
    expect(usageAffinityNoticeKey({ cleared: true, cleared_count: 2 })).toBe(
      'affinityReset',
    )
    expect(usageAffinityNoticeKey({ cleared: false, cleared_count: 0 })).toBe(
      'affinityNotBound',
    )
  })
})
