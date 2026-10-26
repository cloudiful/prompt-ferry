import { QueryClient } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  clearRequestRecords: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  listRequestRecords: vi.fn(),
  requestRecordFacets: vi.fn(),
  requestRecordOverview: vi.fn(),
  listUserOptions: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { getPendingActions } from '@/lib/async-action'
import { useSessionStore } from '@/lib/session'
import { useUsageStore } from '@/lib/usage'
import { isSameUsageClearForm } from '@/lib/usage-clear'
import { createDefaultUsageClearForm } from '@/lib/usage-clear'
import { useUsageDetailStore } from '@/lib/usage-detail'

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'admin',
  user_id: 1,
}
const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

function row(recordId: number, extra: Record<string, unknown> = {}) {
  return { record_id: recordId, request_id: `req-${recordId}`, ...extra }
}

function page(
  overrides: {
    records?: unknown[]
    first?: number
    rows?: number
    total?: number
  } = {},
) {
  return {
    records: [],
    first: 0,
    rows: 25,
    total: 0,
    ...overrides,
  }
}

function facets(overrides: Record<string, unknown> = {}) {
  return { dates: [], users: [], models: [], ...overrides }
}

function overview(overrides: Record<string, unknown> = {}) {
  return {
    breakdown: [],
    summary: {
      error_count: 0,
      method_count: 0,
      request_count: 0,
      success_count: 0,
      success_rate: 0,
      tokens: {
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
      },
    },
    trend: [],
    trend_granularity: 'hour',
    ...overrides,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}

function queryClient() {
  return new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
}

function lastListQuery() {
  return api.listRequestRecords.mock.calls.at(-1)?.[0]?.query
}

function listQueryCalls() {
  return api.listRequestRecords.mock.calls.map((call) => call[0]?.query)
}

const store = () => useUsageStore.getState()

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  api.listRequestRecords.mockImplementation(async (options: any) => {
    const query = options?.query ?? {}
    return page({
      records: [row(1)],
      first: query.first ?? 0,
      rows: query.rows ?? 25,
      total: 1,
    })
  })
  api.requestRecordFacets.mockResolvedValue(facets())
  api.requestRecordOverview.mockResolvedValue(overview())
  api.listUserOptions.mockResolvedValue({ users: [admin, operator] })
  api.clearRequestRecords.mockResolvedValue({
    data: { deleted: 3, deleted_prompt_blocks: 3, protected_by_billing: 1 },
  })
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useUsageStore.getState().resetIdentity(null)
})

afterEach(() => {
  useSessionStore.setState({ me: null })
  useUsageStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('usage store defaults and reads', () => {
  it('reads the default records, facets, overview and admin options once', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().refreshAll(client)

    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(lastListQuery()).toMatchObject({
      first: 0,
      rows: 25,
      request_category: 'ai',
      mcp_scope: 'service',
      sort_field: 'created_at',
      sort_order: -1,
      range: '24h',
      search: undefined,
      user: undefined,
    })
    expect(api.requestRecordFacets.mock.calls[0]?.[0]?.query).toMatchObject({
      request_category: 'ai',
      mcp_scope: 'service',
    })
    expect(api.requestRecordOverview.mock.calls[0]?.[0]?.query).toMatchObject({
      request_category: 'ai',
      perspective: 'model',
      mcp_scope: 'service',
      range: '24h',
    })
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)
    expect(store().records).toHaveLength(1)
    expect(store().pageLoading).toBe(false)
  })

  it('never reads the admin user options for an ordinary user', async () => {
    useSessionStore.setState({ me: operator })
    const client = queryClient()

    await store().refreshAll(client)

    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(store().userOptions).toEqual([])
    await store().loadUserOptions(client)
    expect(api.listUserOptions).not.toHaveBeenCalled()
  })
})

describe('usage store latest-wins', () => {
  it('keeps the newer list result when an older read settles late', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const older = deferred<unknown>()
    api.listRequestRecords.mockReturnValueOnce(older.promise)

    const pending = store().refreshRecords(client)
    api.listRequestRecords.mockResolvedValue(
      page({ records: [row(2)], total: 1 }),
    )
    await store().setFilters(client, { search: 'needle' })

    older.resolve(page({ records: [row(1), row(3)], total: 9 }))
    await pending

    expect(store().records.map((entry) => entry.record_id)).toEqual([2])
    expect(store().total).toBe(1)
    expect(store().recordsLoading).toBe(false)
  })

  it('drops a superseded list failure without clearing the newer rows', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const older = deferred<unknown>()
    api.listRequestRecords.mockReturnValueOnce(older.promise)

    const pending = store().refreshRecords(client)
    api.listRequestRecords.mockResolvedValue(
      page({ records: [row(4)], total: 1 }),
    )
    await store().setFilters(client, { search: 'needle' })

    older.reject(new Error('stale list read'))
    await expect(pending).resolves.toBeUndefined()

    expect(store().records.map((entry) => entry.record_id)).toEqual([4])
    expect(store().recordsLoading).toBe(false)
  })

  it('keeps the newer facet options when an older read settles late', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const older = deferred<unknown>()
    api.requestRecordFacets.mockReturnValueOnce(older.promise)

    const pending = store().refreshFacets(client)
    api.requestRecordFacets.mockResolvedValue(facets({ users: ['newer'] }))
    await store().setCategory(client, 'mcp')

    older.resolve(facets({ users: ['older'] }))
    await pending

    expect(store().facets.users).toEqual(['newer'])
  })

  it('drops a superseded overview result', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const older = deferred<unknown>()
    api.requestRecordOverview.mockReturnValueOnce(older.promise)

    const pending = store().refreshOverview(client)
    api.requestRecordOverview.mockResolvedValue(
      overview({ trend_granularity: 'day' }),
    )
    await store().setRange(client, '7d')

    older.resolve(overview({ trend_granularity: 'hour' }))
    await pending

    expect(store().overview?.trend_granularity).toBe('day')
    expect(store().overviewLoading).toBe(false)
  })

  it('drops an overview result whose query is no longer current', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const older = deferred<unknown>()
    api.requestRecordOverview.mockReturnValueOnce(older.promise)

    const pending = store().refreshOverview(client)
    // Change the range without refreshing so the captured query goes stale.
    useUsageStore.setState({ range: '30d' })
    older.resolve(overview({ request_count: 1 } as never))
    await pending

    expect(store().overview).toBeNull()
  })
})

describe('usage store paging and correction', () => {
  it('corrects an empty page past the end to the last page with rows', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    api.listRequestRecords.mockImplementation(async (options: any) => {
      if (options?.query?.first === 75) {
        return page({ first: 75, rows: 25, total: 51 })
      }
      return page({
        records: [row(51)],
        first: options?.query?.first ?? 0,
        rows: 25,
        total: 51,
      })
    })

    await store().refreshRecords(client, 75, 25)

    const queries = listQueryCalls()
    expect(queries).toHaveLength(2)
    expect(queries[0]?.first).toBe(75)
    expect(queries[1]?.first).toBe(50)
    expect(store().first).toBe(50)
    expect(store().records.map((entry) => entry.record_id)).toEqual([51])
  })

  it('persists the page size and reads the requested page', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setPage(client, 10, 50)

    expect(
      localStorage.getItem('prompt-ferry:table-page-size:request-records'),
    ).toBe('50')
    expect(lastListQuery()).toMatchObject({ first: 10, rows: 50 })
    expect(store().rows).toBe(50)
  })
})

describe('usage store transitions', () => {
  it('resets the scope, page and filters on a category change and keeps the rest', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().setRange(client, '7d')
    await store().setFilters(client, { search: 'needle', model: 'minimax-m2' })
    await store().setPage(client, 25, 50)
    await store().setSort(client, 'total_tokens')
    vi.clearAllMocks()

    await store().setCategory(client, 'mcp')

    expect(store().category).toBe('mcp')
    expect(store().scope).toBe('service')
    expect(store().first).toBe(0)
    expect(store().filters).toEqual({
      search: null,
      user: null,
      model: null,
      client_key_id: null,
      endpoint_id: null,
      mcp_server_id: null,
      mcp_bearer_token_slot: null,
      request_state: null,
      redaction_applied: null,
    })
    expect(store().range).toBe('7d')
    expect(store().rows).toBe(50)
    expect(store().sortField).toBe('total_tokens')
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(lastListQuery()).toMatchObject({
      request_category: 'mcp',
      mcp_scope: 'service',
      first: 0,
      rows: 50,
      sort_field: 'total_tokens',
      range: '7d',
      search: undefined,
    })
    expect(
      api.requestRecordOverview.mock.calls[0]?.[0]?.query.perspective,
    ).toBe('model')
  })

  it('clears the service filters that a gateway scope cannot match', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().setCategory(client, 'mcp')
    useUsageStore.setState({
      filters: {
        ...store().filters,
        mcp_server_id: 'mcp-context7',
        mcp_bearer_token_slot: 0,
        model: 'minimax-m2',
        user: 'operator',
      },
      first: 25,
    })
    vi.clearAllMocks()

    await store().setScope(client, 'gateway')

    expect(store().scope).toBe('gateway')
    expect(store().first).toBe(0)
    expect(store().filters.mcp_server_id).toBeNull()
    expect(store().filters.mcp_bearer_token_slot).toBeNull()
    expect(store().filters.model).toBeNull()
    expect(store().filters.user).toBe('operator')
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(lastListQuery()).toMatchObject({
      mcp_scope: 'gateway',
      first: 0,
      user: 'operator',
    })
  })

  it('refreshes only the records for a filter change', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setFilters(client, { search: 'needle' })

    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
    expect(lastListQuery()).toMatchObject({ search: 'needle' })
  })

  it('keeps the page when the sort cycles', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setPage(client, 25, 25)
    await store().setSort(client, 'total_tokens')
    expect(lastListQuery()).toMatchObject({
      first: 25,
      sort_field: 'total_tokens',
      sort_order: 1,
    })
    await store().setSort(client, 'total_tokens')
    expect(lastListQuery()).toMatchObject({
      first: 25,
      sort_field: 'total_tokens',
      sort_order: -1,
    })
    await store().setSort(client, 'total_tokens')
    expect(lastListQuery()).toMatchObject({
      first: 25,
      sort_field: 'total_tokens',
      sort_order: 0,
    })
    expect(store().first).toBe(25)
  })

  it('sends every table column backend field and keeps the current page', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setPage(client, 25, 25)
    const fields = [
      'usage_date',
      'created_at',
      'user_key',
      'client_key_label',
      'model_key',
      'target',
      'request_state',
      'duration_ms',
      'total_tokens',
    ]
    for (const field of fields) {
      await store().setSort(client, field)
      expect(lastListQuery()).toMatchObject({
        first: 25,
        sort_field: field,
        sort_order: 1,
      })
    }
  })

  it('keeps the page for a filter change and resets it for a scope change', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setPage(client, 50, 25)
    await store().setFilters(client, { request_state: 'failed' })
    expect(lastListQuery()).toMatchObject({
      first: 50,
      request_state: 'failed',
    })

    await store().setCategory(client, 'mcp')
    await store().setPage(client, 25, 25)
    await store().setScope(client, 'gateway')
    expect(lastListQuery()).toMatchObject({ first: 0, mcp_scope: 'gateway' })
  })

  it('refreshes all three reads for a range change and sends the window to facets and overview', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setRange(client, '7d')

    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).toHaveBeenCalledTimes(1)
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(lastListQuery()).toMatchObject({
      range: '7d',
      start: undefined,
      end: undefined,
    })
    const facetQuery = api.requestRecordFacets.mock.calls[0]?.[0]?.query
    expect(facetQuery.range).toBeUndefined()
    expect(typeof facetQuery.start).toBe('string')
    expect(typeof facetQuery.end).toBe('string')
    const overviewQuery = api.requestRecordOverview.mock.calls[0]?.[0]?.query
    expect(overviewQuery.range).toBe('7d')
    expect(typeof overviewQuery.start).toBe('string')
    expect(typeof overviewQuery.end).toBe('string')
  })

  it('sends the custom window bounds to every read', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()

    await store().setRange(
      client,
      'custom',
      '2026-10-01T00:00:00.000Z',
      '2026-10-02T00:00:00.000Z',
    )

    expect(lastListQuery()).toMatchObject({
      range: 'custom',
      start: '2026-10-01T00:00:00.000Z',
      end: '2026-10-02T00:00:00.000Z',
    })
    expect(api.requestRecordFacets.mock.calls[0]?.[0]?.query).toMatchObject({
      start: '2026-10-01T00:00:00.000Z',
      end: '2026-10-02T00:00:00.000Z',
    })
  })

  it('applies a drilldown to the records only and remembers the records mode', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().setPage(client, 25, 25)
    await store().setFilters(client, { search: 'needle' })
    vi.clearAllMocks()

    await store().applyDrilldown(client, {
      model: 'minimax-m2',
      endpoint_id: null,
      mcp_server_id: null,
    })

    expect(store().filters.search).toBeNull()
    expect(store().filters.model).toBe('minimax-m2')
    expect(store().first).toBe(0)
    expect(store().mode).toBe('records')
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
    expect(lastListQuery()).toMatchObject({
      first: 0,
      model: 'minimax-m2',
      search: undefined,
    })
    expect(
      JSON.parse(localStorage.getItem('request-records-overview-mode') ?? '{}'),
    ).toEqual({ ai: 'records' })
  })

  it('refreshes the overview in the background when a drilldown changes the scope', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().setCategory(client, 'mcp')
    vi.clearAllMocks()

    await store().applyDrilldown(client, {
      mcp_scope: 'gateway',
      mcp_server_id: null,
      model: null,
    })
    await Promise.resolve()

    expect(store().scope).toBe('gateway')
    expect(api.listRequestRecords).toHaveBeenCalledTimes(1)
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
  })

  it('keeps the mode local and never issues a request', async () => {
    useSessionStore.setState({ me: admin })
    store().setMode('records')
    expect(store().mode).toBe('records')
    expect(api.listRequestRecords).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
  })
})

describe('usage store perspective guard', () => {
  it('guards the perspective refresh and refreshes only the overview', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    const gate = deferred<unknown>()
    api.requestRecordOverview.mockReturnValueOnce(gate.promise)

    const first = store().setPerspective(client, 'upstream')
    const second = await store().setPerspective(client, 'upstream')
    expect(second.executed).toBe(false)

    gate.resolve(overview())
    const outcome = await first

    expect(outcome.executed).toBe(true)
    expect(store().perspective).toBe('upstream')
    expect(api.requestRecordOverview).toHaveBeenCalledTimes(1)
    expect(api.listRequestRecords).not.toHaveBeenCalled()
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
  })

  it('reports nothing when the perspective is unchanged', async () => {
    useSessionStore.setState({ me: admin })
    vi.clearAllMocks()
    const outcome = await store().setPerspective(queryClient(), 'model')
    expect(outcome.executed).toBe(false)
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
  })
})

describe('usage store identity isolation', () => {
  it('resets the view state and drops reads from the previous session', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().refreshAll(client)
    await store().setFilters(client, { search: 'needle' })
    await store().setPage(client, 25, 50)
    expect(store().records).toHaveLength(1)

    const older = deferred<unknown>()
    api.listRequestRecords.mockReturnValueOnce(older.promise)
    const pending = store().refreshRecords(client)

    useSessionStore.setState({ me: operator })
    older.resolve(page({ records: [row(9)], total: 9 }))
    await pending

    expect(store().identityKey).toBe('2:user')
    expect(store().records).toEqual([])
    expect(store().total).toBe(0)
    expect(store().first).toBe(0)
    expect(store().filters.search).toBeNull()
    expect(store().userOptions).toEqual([])
    expect(store().overview).toBeNull()
  })

  it('does not read before an identity is established', async () => {
    const client = queryClient()
    await store().refreshAll(client)
    await store().refreshRecords(client)
    await store().refreshFacets(client)
    await store().refreshOverview(client)
    expect(api.listRequestRecords).not.toHaveBeenCalled()
    expect(api.requestRecordFacets).not.toHaveBeenCalled()
    expect(api.requestRecordOverview).not.toHaveBeenCalled()
  })
})

describe('usage store detail reset hooks', () => {
  it('clears the detail resources on a category change and on an identity reset', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().refreshAll(client)
    useUsageDetailStore
      .getState()
      .initializeIdentity({ is_admin: true, user_id: 1 })
    useUsageDetailStore.setState({
      detail: { record_id: 1 } as never,
      recordId: 1,
    })

    await store().setCategory(client, 'mcp')
    expect(useUsageDetailStore.getState().detail).toBeNull()
    expect(useUsageDetailStore.getState().recordId).toBeNull()

    useUsageDetailStore.setState({
      detail: { record_id: 2 } as never,
      recordId: 2,
    })
    store().resetIdentity({ is_admin: false, user_id: 2 })
    expect(useUsageDetailStore.getState().detail).toBeNull()
    expect(useUsageDetailStore.getState().recordId).toBeNull()
  })
})

describe('usage store clear history', () => {
  it('keeps a newer form open when a stale completion reports back', () => {
    const submitted = createDefaultUsageClearForm()
    const newer = createDefaultUsageClearForm()
    expect(isSameUsageClearForm(submitted, newer)).toBe(false)
    expect(isSameUsageClearForm(submitted, submitted)).toBe(true)
  })

  it('posts the instant payload inside the maintenance guard and refreshes', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().refreshAll(client)
    const readsBefore = api.listRequestRecords.mock.calls.length

    const form = {
      ...createDefaultUsageClearForm(),
      end_at: '2026-10-02T10:00',
      scope: 'target_user' as const,
      start_at: '2026-10-01T10:00',
      user_id: 2,
    }
    const outcome = await store().clearHistory(client, form)

    expect(outcome.executed && outcome.value.deleted).toBe(3)
    expect(api.clearRequestRecords).toHaveBeenCalledTimes(1)
    const body = (api.clearRequestRecords.mock.calls[0][0] as any).body
    expect(body).toMatchObject({
      delete_all: false,
      end_at: new Date(2026, 9, 2, 10, 0, 0, 0).toISOString(),
      scope: 'target_user',
      start_at: new Date(2026, 9, 1, 10, 0, 0, 0).toISOString(),
      user_id: 2,
    })
    expect(Object.hasOwn(body, 'category')).toBe(false)
    expect(api.listRequestRecords.mock.calls.length).toBe(readsBefore + 1)
  })

  it('refuses a duplicate clear while one is pending and reports nothing', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    let release!: (value: unknown) => void
    api.clearRequestRecords.mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve
      }),
    )

    const first = store().clearHistory(client, createDefaultUsageClearForm())
    expect(getPendingActions()['usage:maintenance']).toBe('clear')
    const second = await store().clearHistory(
      client,
      createDefaultUsageClearForm(),
    )
    expect(second.executed).toBe(false)
    expect(api.clearRequestRecords).toHaveBeenCalledTimes(1)

    release({
      data: { deleted: 1, deleted_prompt_blocks: 1, protected_by_billing: 0 },
    })
    const outcome = await first
    expect(outcome.executed).toBe(true)
    expect(getPendingActions()['usage:maintenance']).toBeUndefined()
  })

  it('rejects a malformed local date before any request or refresh', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().refreshAll(client)
    const readsBefore = api.listRequestRecords.mock.calls.length

    await expect(
      store().clearHistory(client, {
        ...createDefaultUsageClearForm(),
        start_at: '2026-02-30T10:00',
      }),
    ).rejects.toThrow()
    expect(api.clearRequestRecords).not.toHaveBeenCalled()
    expect(api.listRequestRecords.mock.calls.length).toBe(readsBefore)
    expect(getPendingActions()['usage:maintenance']).toBeUndefined()
  })

  it('keeps the list unchanged until the clear and its refresh finish', async () => {
    useSessionStore.setState({ me: admin })
    const client = queryClient()
    await store().refreshAll(client)
    const readsBefore = api.listRequestRecords.mock.calls.length
    let release!: (value: unknown) => void
    api.clearRequestRecords.mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve
      }),
    )

    const pending = store().clearHistory(client, createDefaultUsageClearForm())
    await Promise.resolve()
    expect(api.listRequestRecords.mock.calls.length).toBe(readsBefore)

    release({
      data: { deleted: 0, deleted_prompt_blocks: 0, protected_by_billing: 0 },
    })
    await pending
    expect(api.listRequestRecords.mock.calls.length).toBe(readsBefore + 1)
  })
})
