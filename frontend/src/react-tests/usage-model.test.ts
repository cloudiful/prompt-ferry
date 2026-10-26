import { describe, expect, it } from 'vitest'
import {
  DEFAULT_USAGE_CATEGORY,
  DEFAULT_USAGE_MODE,
  DEFAULT_USAGE_PERSPECTIVE,
  DEFAULT_USAGE_RANGE,
  DEFAULT_USAGE_ROWS,
  DEFAULT_USAGE_SCOPE,
  DEFAULT_USAGE_SORT_FIELD,
  DEFAULT_USAGE_SORT_ORDER,
  USAGE_MODE_STORAGE_KEY,
  USAGE_PAGE_SIZE_OPTIONS,
  USAGE_SORTABLE_BACKEND_FIELDS,
  USAGE_SORT_FIELDS,
  createDefaultUsageFilters,
  emptyUsageFacets,
  formatUsageTokenQuantity,
  isUsageEmptyTail,
  isZeroTokenEntry,
  loadUsageMode,
  nextUsageSortState,
  normalizeUsagePageSize,
  partitionZeroTokenEntries,
  resolveUsageRangeWindow,
  saveUsageMode,
  usageAriaSort,
  usageFacetsQuery,
  usageFiltersForScope,
  usageLastPageFirst,
  usageListQuery,
  usageListQueryKey,
  usageOverviewQuery,
  usagePageSizeStorageKey,
  usageSortFieldForColumn,
  type UsageFilters,
  type UsageModeStorage,
  type UsageQueryState,
} from '@/lib/usage-model'
import {
  createUsageRecordView,
  formatUsageOutputRate,
  usageOutputRate,
} from '@/lib/usage-records'
import {
  usageDetailGroup,
  usageDrilldownForPerspective,
  usageTrendBucketLabel,
} from '@/lib/usage-overview'

function queryState(overrides: Partial<UsageQueryState> = {}): UsageQueryState {
  return {
    category: DEFAULT_USAGE_CATEGORY,
    scope: DEFAULT_USAGE_SCOPE,
    range: DEFAULT_USAGE_RANGE,
    start: '',
    end: '',
    first: 0,
    rows: DEFAULT_USAGE_ROWS,
    sortField: DEFAULT_USAGE_SORT_FIELD,
    sortOrder: DEFAULT_USAGE_SORT_ORDER,
    filters: createDefaultUsageFilters(),
    ...overrides,
  }
}

function filters(overrides: Partial<UsageFilters> = {}): UsageFilters {
  return { ...createDefaultUsageFilters(), ...overrides }
}

function fakeStorage(initial: Record<string, string> = {}): UsageModeStorage {
  const entries = new Map(Object.entries(initial))
  return {
    getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => {
      entries.set(key, value)
    },
  }
}

describe('usage defaults', () => {
  it('starts on the AI pending-free defaults', () => {
    expect(DEFAULT_USAGE_CATEGORY).toBe('ai')
    expect(DEFAULT_USAGE_SCOPE).toBe('service')
    expect(DEFAULT_USAGE_RANGE).toBe('24h')
    expect(DEFAULT_USAGE_ROWS).toBe(25)
    expect(DEFAULT_USAGE_SORT_FIELD).toBe('created_at')
    expect(DEFAULT_USAGE_SORT_ORDER).toBe(-1)
    expect(DEFAULT_USAGE_PERSPECTIVE).toBe('model')
    expect(DEFAULT_USAGE_MODE).toBe('overview')
    expect(Object.values(createDefaultUsageFilters())).toEqual([
      null,
      null,
      null,
      null,
      null,
      null,
      null,
      null,
      null,
    ])
    expect(emptyUsageFacets()).toEqual({ dates: [], users: [], models: [] })
  })

  it('keeps the request-record page sizes and storage key', () => {
    expect(USAGE_PAGE_SIZE_OPTIONS).toEqual([10, 25, 50, 100])
    expect(usagePageSizeStorageKey).toBe(
      'prompt-ferry:table-page-size:request-records',
    )
    expect(normalizeUsagePageSize(null)).toBe(25)
    expect(normalizeUsagePageSize('50')).toBe(50)
    expect(normalizeUsagePageSize('7')).toBe(25)
  })
})

describe('usage queries', () => {
  it('builds the exact list query and omits every unset filter', () => {
    expect(usageListQuery(queryState())).toEqual({
      first: 0,
      rows: 25,
      request_category: 'ai',
      mcp_scope: 'service',
      sort_field: 'created_at',
      sort_order: -1,
      range: '24h',
      start: undefined,
      end: undefined,
      search: undefined,
      user: undefined,
      model: undefined,
      client_key_id: undefined,
      endpoint_id: undefined,
      mcp_server_id: undefined,
      mcp_bearer_token_slot: undefined,
      request_state: undefined,
      redaction_applied: undefined,
    })
  })

  it('carries the set filters and the custom window', () => {
    const query = usageListQuery(
      queryState({
        range: 'custom',
        start: '2026-10-01T00:00:00.000Z',
        end: '2026-10-02T00:00:00.000Z',
        first: 50,
        rows: 50,
        sortField: 'usage_date',
        sortOrder: 1,
        filters: filters({
          search: 'needle',
          user: 'operator',
          model: 'minimax-m2',
          client_key_id: 2,
          endpoint_id: 'ep-minimax-cn',
          mcp_server_id: 'mcp-context7',
          mcp_bearer_token_slot: 0,
          request_state: 'failed',
          redaction_applied: true,
        }),
      }),
    )
    expect(query).toMatchObject({
      first: 50,
      rows: 50,
      sort_field: 'usage_date',
      sort_order: 1,
      range: 'custom',
      start: '2026-10-01T00:00:00.000Z',
      end: '2026-10-02T00:00:00.000Z',
      search: 'needle',
      user: 'operator',
      model: 'minimax-m2',
      client_key_id: 2,
      endpoint_id: 'ep-minimax-cn',
      mcp_server_id: 'mcp-context7',
      mcp_bearer_token_slot: 0,
      request_state: 'failed',
      redaction_applied: true,
    })
  })

  it('sends only the category, the scope and the window to the facets', () => {
    const query = usageFacetsQuery(
      queryState({ category: 'mcp', scope: 'gateway' }),
      new Date('2026-10-20T12:00:00.000Z'),
    )
    expect(query).toEqual({
      request_category: 'mcp',
      mcp_scope: 'gateway',
      start: '2026-10-19T12:00:00.000Z',
      end: '2026-10-20T12:00:00.000Z',
    })
    expect(Object.keys(query).sort()).toEqual([
      'end',
      'mcp_scope',
      'request_category',
      'start',
    ])
  })

  it('sends only the category, the perspective, the scope and the window to the overview', () => {
    const now = new Date('2026-10-20T12:00:00.000Z')
    expect(usageOverviewQuery(queryState(), 'upstream', now)).toEqual({
      request_category: 'ai',
      perspective: 'upstream',
      mcp_scope: 'service',
      range: '24h',
      start: '2026-10-19T12:00:00.000Z',
      end: '2026-10-20T12:00:00.000Z',
    })
    // The MCP overview has no model/upstream choice, so it never forwards one.
    expect(
      usageOverviewQuery(queryState({ category: 'mcp' }), 'upstream', now)
        .perspective,
    ).toBe('model')
    expect(
      Object.keys(usageOverviewQuery(queryState(), 'model', now)).sort(),
    ).toEqual([
      'end',
      'mcp_scope',
      'perspective',
      'range',
      'request_category',
      'start',
    ])
  })

  it('keys every read by the identity and the exact query', () => {
    const identity = { user_id: 1, is_admin: true }
    const listKey = usageListQueryKey(identity, usageListQuery(queryState()))
    expect(listKey.slice(0, 3)).toEqual(['usage', 'records', '1:admin'])
    expect(
      usageListQueryKey(identity, usageListQuery(queryState({ first: 25 }))),
    ).not.toEqual(listKey)
    expect(
      usageListQueryKey(
        { user_id: 2, is_admin: false },
        usageListQuery(queryState()),
      ),
    ).not.toEqual(listKey)
  })
})

describe('usage range window', () => {
  const now = new Date('2026-10-20T09:30:00.000Z')

  it('resolves each preset against the same half-open window', () => {
    expect(resolveUsageRangeWindow('24h', '', '', now)).toEqual({
      start: '2026-10-19T09:30:00.000Z',
      end: '2026-10-20T09:30:00.000Z',
    })
    expect(resolveUsageRangeWindow('7d', '', '', now).start).toBe(
      '2026-10-13T09:30:00.000Z',
    )
    expect(resolveUsageRangeWindow('30d', '', '', now).start).toBe(
      '2026-09-20T09:30:00.000Z',
    )
    expect(resolveUsageRangeWindow('month', '', '', now)).toEqual({
      start: '2026-10-01T00:00:00.000Z',
      end: '2026-10-20T09:30:00.000Z',
    })
  })

  it('passes the custom bounds through unchanged', () => {
    expect(
      resolveUsageRangeWindow(
        'custom',
        '2026-10-01T00:00:00.000Z',
        '2026-10-03T00:00:00.000Z',
        now,
      ),
    ).toEqual({
      start: '2026-10-01T00:00:00.000Z',
      end: '2026-10-03T00:00:00.000Z',
    })
    expect(resolveUsageRangeWindow('custom', '', '', now)).toEqual({
      start: undefined,
      end: undefined,
    })
  })
})

describe('usage sorting', () => {
  it('maps the Date column to the real day field and Time to the instant', () => {
    expect(usageSortFieldForColumn('request_date')).toBe('usage_date')
    expect(usageSortFieldForColumn('created_at')).toBe('created_at')
    expect(usageSortFieldForColumn('user')).toBe('user_key')
    expect(usageSortFieldForColumn('model')).toBe('model_key')
    expect(usageSortFieldForColumn('target')).toBe('target')
  })

  it('only maps fields the backend whitelist accepts', () => {
    const whitelist = new Set<string>(USAGE_SORTABLE_BACKEND_FIELDS)
    for (const field of Object.values(USAGE_SORT_FIELDS)) {
      expect(whitelist.has(field)).toBe(true)
    }
    expect(whitelist.has('request_date')).toBe(false)
    expect(USAGE_SORT_FIELDS.request_date).not.toBe('request_date')
  })

  it('cycles ascending, descending, unsorted and keeps the chosen field', () => {
    const ascending = nextUsageSortState(
      { field: 'created_at', order: -1 },
      'total_tokens',
    )
    expect(ascending).toEqual({ field: 'total_tokens', order: 1 })
    const descending = nextUsageSortState(ascending, 'total_tokens')
    expect(descending).toEqual({ field: 'total_tokens', order: -1 })
    const unsorted = nextUsageSortState(descending, 'total_tokens')
    expect(unsorted).toEqual({ field: 'total_tokens', order: 0 })
    expect(nextUsageSortState(unsorted, 'total_tokens')).toEqual({
      field: 'total_tokens',
      order: 1,
    })
  })

  it('reports aria-sort only on the active column', () => {
    expect(usageAriaSort({ field: 'created_at', order: 1 }, 'created_at')).toBe(
      'ascending',
    )
    expect(
      usageAriaSort({ field: 'created_at', order: -1 }, 'created_at'),
    ).toBe('descending')
    expect(usageAriaSort({ field: 'created_at', order: 0 }, 'created_at')).toBe(
      'none',
    )
    expect(
      usageAriaSort({ field: 'created_at', order: 1 }, 'total_tokens'),
    ).toBe(undefined)
  })
})

describe('zero-token fold', () => {
  type Entry = {
    label: string
    request_count: number
    total_tokens?: number | null
  }

  const entries: Entry[] = [
    { label: 'known', request_count: 3, total_tokens: 1060 },
    { label: 'success-zero', request_count: 2, total_tokens: 0 },
    { label: 'unknown-null', request_count: 4, total_tokens: null },
    { label: 'missing', request_count: 5 },
    { label: 'mixed-zero', request_count: 6, total_tokens: 0 },
    { label: 'unknown-nan', request_count: 7, total_tokens: Number.NaN },
    { label: 'failed-zero', request_count: 8, total_tokens: 0 },
  ]

  it('folds only a real zero and never an unknown token count', () => {
    expect(isZeroTokenEntry({ total_tokens: 0 })).toBe(true)
    expect(isZeroTokenEntry({ total_tokens: 1 })).toBe(false)
    expect(isZeroTokenEntry({ total_tokens: null })).toBe(false)
    expect(isZeroTokenEntry({ total_tokens: undefined })).toBe(false)
    expect(isZeroTokenEntry({})).toBe(false)
    expect(isZeroTokenEntry({ total_tokens: Number.NaN })).toBe(false)
  })

  it('keeps visible rows first, folds every zero row after them and sums the hidden requests', () => {
    const frozen = Object.freeze(
      entries.map((entry) => Object.freeze({ ...entry })),
    )
    const snapshot = JSON.stringify(frozen)
    const { visible, folded, hiddenRequestCount } =
      partitionZeroTokenEntries(frozen)
    expect(visible.map((entry) => entry.label)).toEqual([
      'known',
      'unknown-null',
      'missing',
      'unknown-nan',
    ])
    expect(folded.map((entry) => entry.label)).toEqual([
      'success-zero',
      'mixed-zero',
      'failed-zero',
    ])
    expect(hiddenRequestCount).toBe(2 + 6 + 8)
    expect(JSON.stringify(frozen)).toBe(snapshot)
  })

  it('renders an exact zero as 0 and an unknown measurement as a dash', () => {
    expect(formatUsageTokenQuantity(0, 'en-US')).toBe('0')
    expect(formatUsageTokenQuantity(null, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(undefined, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(Number.NaN, 'en-US')).toBe('-')
    expect(formatUsageTokenQuantity(1500, 'en-US')).toBe('1.5K')
    expect(formatUsageTokenQuantity(1060, 'zh-CN')).toBe('1.1K')
  })
})

describe('usage page and scope helpers', () => {
  it('computes the last page that still holds rows', () => {
    expect(usageLastPageFirst(0, 25)).toBe(0)
    expect(usageLastPageFirst(25, 25)).toBe(0)
    expect(usageLastPageFirst(26, 25)).toBe(25)
    expect(usageLastPageFirst(51, 25)).toBe(50)
  })

  it('detects an empty tail only when the collection still has rows', () => {
    expect(isUsageEmptyTail({ records: [], total: 51, first: 75 })).toBe(true)
    expect(isUsageEmptyTail({ records: [], total: 0, first: 0 })).toBe(false)
    expect(
      isUsageEmptyTail({
        records: [{ record_id: 1 } as never],
        total: 51,
        first: 75,
      }),
    ).toBe(false)
  })

  it('drops the service filters a gateway record cannot match', () => {
    const base = filters({
      mcp_server_id: 'mcp-context7',
      mcp_bearer_token_slot: 0,
      model: 'minimax-m2',
      user: 'operator',
    })
    expect(usageFiltersForScope(base, 'mcp', 'gateway')).toEqual({
      ...base,
      mcp_server_id: null,
      mcp_bearer_token_slot: null,
      model: null,
    })
    expect(usageFiltersForScope(base, 'mcp', 'service')).toBe(base)
    expect(usageFiltersForScope(base, 'ai', 'gateway')).toBe(base)
  })
})

describe('usage mode persistence', () => {
  it('remembers the mode per category', () => {
    const storage = fakeStorage()
    expect(loadUsageMode(storage, 'ai')).toBe('overview')
    saveUsageMode(storage, 'ai', 'records')
    expect(loadUsageMode(storage, 'ai')).toBe('records')
    expect(loadUsageMode(storage, 'mcp')).toBe('overview')
    saveUsageMode(storage, 'mcp', 'overview')
    expect(loadUsageMode(storage, 'mcp')).toBe('overview')
    expect(storage.getItem?.(USAGE_MODE_STORAGE_KEY)).toBe(
      JSON.stringify({ ai: 'records', mcp: 'overview' }),
    )
  })

  it('falls back to the default mode for malformed or absent storage', () => {
    expect(loadUsageMode(null, 'ai')).toBe('overview')
    expect(
      loadUsageMode(fakeStorage({ [USAGE_MODE_STORAGE_KEY]: '{' }), 'ai'),
    ).toBe('overview')
    expect(
      loadUsageMode(
        fakeStorage({ [USAGE_MODE_STORAGE_KEY]: '"records"' }),
        'ai',
      ),
    ).toBe('overview')
    expect(() => saveUsageMode(null, 'ai', 'records')).not.toThrow()
  })
})

describe('usage overview drilldown', () => {
  const baseRow = {
    label: 'gpt-4o',
    model: 'gpt-4o',
    request_count: 1,
    request_share: 1,
    success_count: 1,
    success_rate: 1,
    tokens: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
  }

  it('filters AI model rows by model and upstream rows by endpoint', () => {
    expect(usageDrilldownForPerspective('model', 'ai', baseRow)).toEqual({
      mcp_server_id: null,
      model: 'gpt-4o',
    })
    expect(
      usageDrilldownForPerspective('upstream', 'ai', {
        ...baseRow,
        endpoint_id: 'endpoint-1',
        model: null,
      }),
    ).toEqual({ endpoint_id: 'endpoint-1' })
  })

  it('leaves a direct upstream row without a filter', () => {
    expect(
      usageDrilldownForPerspective('upstream', 'ai', {
        ...baseRow,
        endpoint_id: null,
        label: '(direct)',
      }),
    ).toBeNull()
  })

  it('maps MCP gateway, named server and name-only rows by their own scope', () => {
    expect(
      usageDrilldownForPerspective('model', 'mcp', {
        ...baseRow,
        mcp_scope: 'gateway',
        label: 'gateway',
      }),
    ).toEqual({ mcp_scope: 'gateway', mcp_server_id: null, model: null })
    expect(
      usageDrilldownForPerspective('model', 'mcp', {
        ...baseRow,
        mcp_server_id: 'server-1',
        mcp_scope: 'service',
      }),
    ).toEqual({ mcp_scope: 'service', mcp_server_id: 'server-1', model: null })
    expect(
      usageDrilldownForPerspective('model', 'mcp', {
        ...baseRow,
        label: 'legacy',
        mcp_server_id: null,
        mcp_scope: 'service',
      }),
    ).toEqual({ mcp_scope: 'service', mcp_server_id: null, model: 'legacy' })
  })
})

describe('usage overview trend labels', () => {
  it('keeps a day bucket on its logical UTC date', () => {
    const instant = (value: string) => `instant:${value}`
    const utcDate = (value: string) => `date:${value}`
    expect(
      usageTrendBucketLabel(
        'day',
        '2026-10-20T00:00:00.000Z',
        instant,
        utcDate,
      ),
    ).toBe('date:2026-10-20T00:00:00.000Z')
    expect(
      usageTrendBucketLabel(
        'hour',
        '2026-10-20T09:00:00.000Z',
        instant,
        utcDate,
      ),
    ).toBe('instant:2026-10-20T09:00:00.000Z')
    expect(
      usageTrendBucketLabel(null, '2026-10-20T09:00:00.000Z', instant, utcDate),
    ).toBe('instant:2026-10-20T09:00:00.000Z')
  })
})

describe('usage detail grouping', () => {
  const row = {
    label: 'gpt-4o',
    request_count: 2,
    request_share: 1,
    success_count: 2,
    success_rate: 1,
    tokens: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    },
    upstream_count: 2,
    upstream_breakdown: [
      {
        error_count: 0,
        error_rate: 0,
        request_count: 2,
        request_share: 1,
        total_tokens: 10,
        upstream_model: 'gpt-upstream',
      },
    ],
  }

  it('gates the model perspective on upstream_count rather than the row count', () => {
    expect(usageDetailGroup(row, 'model').show).toBe(true)
    expect(usageDetailGroup({ ...row, upstream_count: 1 }, 'model').show).toBe(
      false,
    )
  })

  it('gates the upstream perspective on more than one model row', () => {
    const models = [
      {
        error_count: 0,
        error_rate: 0,
        model: 'a',
        request_count: 1,
        request_share: 1,
        total_tokens: 1,
      },
    ]
    expect(
      usageDetailGroup({ ...row, model_breakdown: models }, 'upstream').show,
    ).toBe(false)
    expect(
      usageDetailGroup(
        { ...row, model_breakdown: [...models, { ...models[0], model: 'b' }] },
        'upstream',
      ).show,
    ).toBe(true)
  })

  it('keeps a missing metric as null instead of coercing it to zero', () => {
    const { entries } = usageDetailGroup(
      {
        ...row,
        upstream_breakdown: [
          {
            error_count: 0,
            error_rate: 0,
            request_count: 2,
            request_share: 1,
            total_tokens: 10,
            upstream_model: 'gpt-upstream',
          },
        ],
      },
      'model',
    )
    expect(entries[0].cache_rate).toBeNull()
    expect(entries[0].token_share).toBeNull()
    expect(entries[0].avg_output_tokens_per_second).toBeNull()
    expect(entries[0].total_tokens).toBe(10)
  })
})

describe('usage record row view', () => {
  function rawRow(overrides: Record<string, unknown> = {}) {
    return {
      client_key_label: 'team-key',
      conversation_id: 'abcdef1234567890',
      conversation_seq: 2,
      created_at: '2026-03-04T23:30:00.000Z',
      path: '/v1/chat/completions',
      record_id: 9,
      redaction: {
        applied: false,
        fields: [],
        findings_count: 0,
        replacements_count: 0,
        types: [],
      },
      request_category: 'ai',
      request_id: 'req-9',
      request_state: 'completed',
      user_login_name: 'operator',
      ...overrides,
    } as never
  }

  it('derives the display labels from the raw row and keeps raw fields', () => {
    const view = createUsageRecordView(
      rawRow({
        model: 'gpt-4o',
        output_tokens: 42,
        upstream_model: 'gpt-4o-mini',
      }),
    )
    expect(view.actor).toBe('operator')
    expect(view.user_key).toBe('operator')
    expect(view.model_key).toBe('gpt-4o')
    expect(view.model_display).toBe('gpt-4o (gpt-4o-mini)')
    expect(view.session_short_id).toBe('abcdef12')
    expect(view.is_session_recognized).toBe(true)
    expect(view.is_first_turn).toBe(false)
    expect(view.output_tokens).toBe(42)
    // The date is formatted at render time, not stored on the view.
    expect('request_date' in view).toBe(false)
  })

  it('dashes unknown labels and recognizes the first turn', () => {
    const view = createUsageRecordView(
      rawRow({
        client_key_label: null,
        conversation_seq: null,
        model: null,
        user_login_name: null,
      }),
    )
    expect(view.actor).toBe('-')
    expect(view.model_key).toBe('-')
    expect(view.is_first_turn).toBe(true)
    expect(view.upstream_label).toBe('/v1/chat/completions')
  })

  it('rates only a completed AI request with positive output and duration', () => {
    expect(
      usageOutputRate({
        duration_ms: 2000,
        output_tokens: 100,
        request_category: 'ai',
        request_state: 'completed',
      }),
    ).toBe(50)
    expect(
      formatUsageOutputRate({
        duration_ms: 2000,
        output_tokens: 100,
        request_category: 'ai',
        request_state: 'completed',
      }),
    ).toBe('50.0')
    for (const record of [
      {
        duration_ms: 2000,
        output_tokens: 100,
        request_category: 'mcp' as const,
        request_state: 'completed' as const,
      },
      {
        duration_ms: 2000,
        output_tokens: 100,
        request_category: 'ai' as const,
        request_state: 'failed' as const,
      },
      {
        duration_ms: null,
        output_tokens: 100,
        request_category: 'ai' as const,
        request_state: 'completed' as const,
      },
      {
        duration_ms: 0,
        output_tokens: 100,
        request_category: 'ai' as const,
        request_state: 'completed' as const,
      },
      {
        duration_ms: 2000,
        output_tokens: 0,
        request_category: 'ai' as const,
        request_state: 'completed' as const,
      },
    ]) {
      expect(usageOutputRate(record)).toBeNull()
      expect(formatUsageOutputRate(record)).toBe('-')
    }
  })
})
