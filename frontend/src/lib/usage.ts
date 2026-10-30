import { create } from 'zustand'
import type { QueryClient } from '@tanstack/react-query'
import type {
  RequestRecordFacets,
  RequestRecordListRow,
  RequestRecordOverviewResponse,
  RequestRecordsClearResponse,
  User,
} from '@/generated/admin-api'
import {
  clearRequestRecords,
  listRequestRecords,
  listUserOptions,
  requestRecordFacets,
  requestRecordOverview,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { readStorage, writeStorage } from '@/storage'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { usageClearPayload, type UsageClearForm } from './usage-clear'
import { slidingUsageWindow, type LocalUsageRange } from './usage-range'
import { USAGE_MAINTENANCE_KEY, useUsageDetailStore } from './usage-detail'
import { useSessionStore } from './session'
import {
  DEFAULT_USAGE_CATEGORY,
  DEFAULT_USAGE_PERSPECTIVE,
  DEFAULT_USAGE_RANGE,
  DEFAULT_USAGE_ROWS,
  DEFAULT_USAGE_SCOPE,
  DEFAULT_USAGE_SORT_FIELD,
  DEFAULT_USAGE_SORT_ORDER,
  USAGE_PERSPECTIVE_KEY,
  USAGE_LIST_NAMESPACE,
  USAGE_FACETS_NAMESPACE,
  USAGE_OVERVIEW_NAMESPACE,
  USAGE_USER_OPTIONS_NAMESPACE,
  createDefaultUsageFilters,
  emptyUsageFacets,
  isUsageEmptyTail,
  loadUsageMode,
  normalizeUsagePageSize,
  nextUsageSortState,
  saveUsageMode,
  usageFacetsQuery,
  usageFacetsQueryKey,
  usageFiltersForScope,
  usageIdentityKey,
  usageLastPageFirst,
  usageListQuery,
  usageListQueryKey,
  usageOverviewQuery,
  usageOverviewQueryKey,
  usageOverviewSignature,
  usagePageSizeStorageKey,
  usageUserOptionsQueryKey,
  type UsageCategory,
  type UsageFilters,
  type UsageIdentity,
  type UsageListQuery,
  type UsageMode,
  type UsageModeStorage,
  type UsageOverviewQuery,
  type UsagePerspective,
  type UsageQueryState,
  type UsageScope,
  type UsageSortOrder,
} from './usage-model'

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  namespace: readonly unknown[],
  load: () => Promise<T>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: namespace,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({ queryKey, queryFn: load, staleTime: 0 })
}

export type UsagePage = {
  records: RequestRecordListRow[]
  total: number
  first: number
  rows: number
}

export async function loadUsagePage(query: UsageListQuery): Promise<UsagePage> {
  const page = expectData(await listRequestRecords<true>(withData({ query })))
  return {
    records: page.records,
    total: page.total,
    first: page.first,
    rows: page.rows,
  }
}

export async function loadUsageFacets(
  query: ReturnType<typeof usageFacetsQuery>,
): Promise<RequestRecordFacets> {
  return expectData(await requestRecordFacets<true>(withData({ query })))
}

export async function loadUsageOverview(
  query: UsageOverviewQuery,
): Promise<RequestRecordOverviewResponse> {
  return expectData(await requestRecordOverview<true>(withData({ query })))
}

export async function loadUsageUserOptions(): Promise<User[]> {
  return expectData(await listUserOptions<true>(withData())).users
}

const pageSizeKey = usagePageSizeStorageKey

function initialRows(): number {
  return normalizeUsagePageSize(readStorage(pageSizeKey))
}

function modeStorage(): UsageModeStorage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

export type UsageState = {
  identityKey: string | null
  identityUserId: number | null
  isAdmin: boolean
  category: UsageCategory
  scope: UsageScope
  range: LocalUsageRange
  start: string
  end: string
  filters: UsageFilters
  first: number
  rows: number
  total: number
  sortField: string
  sortOrder: UsageSortOrder
  perspective: UsagePerspective
  mode: UsageMode
  records: RequestRecordListRow[]
  facets: RequestRecordFacets
  overview: RequestRecordOverviewResponse | null
  userOptions: User[]
  recordsLoading: boolean
  facetsLoading: boolean
  overviewLoading: boolean
  pageLoading: boolean
  initializeIdentity: (identity: UsageIdentity) => void
  resetIdentity: (identity: UsageIdentity | null) => void
  queryState: () => UsageQueryState
  setMode: (mode: UsageMode, queryClient?: QueryClient) => Promise<void>
  setCategory: (
    queryClient: QueryClient,
    category: UsageCategory,
  ) => Promise<void>
  setScope: (queryClient: QueryClient, scope: UsageScope) => Promise<void>
  setFilters: (
    queryClient: QueryClient,
    patch: Partial<UsageFilters>,
  ) => Promise<void>
  setPage: (
    queryClient: QueryClient,
    first: number,
    rows: number,
  ) => Promise<void>
  setSort: (queryClient: QueryClient, field: string) => Promise<void>
  setRange: (
    queryClient: QueryClient,
    range: LocalUsageRange,
    start?: string,
    end?: string,
  ) => Promise<void>
  setPerspective: (
    queryClient: QueryClient,
    perspective: UsagePerspective,
  ) => Promise<AsyncActionOutcome<void>>
  applyDrilldown: (
    queryClient: QueryClient,
    drilldown: UsageDrilldown,
  ) => Promise<void>
  refreshRecords: (
    queryClient: QueryClient,
    first?: number,
    rows?: number,
  ) => Promise<void>
  refreshFacets: (queryClient: QueryClient) => Promise<void>
  refreshOverview: (queryClient: QueryClient) => Promise<void>
  refreshPage: (queryClient: QueryClient) => Promise<void>
  refreshAll: (queryClient: QueryClient) => Promise<void>
  loadUserOptions: (queryClient: QueryClient) => Promise<void>
  clearHistory: (
    queryClient: QueryClient,
    form: UsageClearForm,
  ) => Promise<AsyncActionOutcome<RequestRecordsClearResponse>>
}

export type UsageDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
  mcp_scope?: UsageScope | null
}

let recordsGeneration = 0
let facetsGeneration = 0
let overviewGeneration = 0
let pageGeneration = 0

export const useUsageStore = create<UsageState>((set, get) => ({
  identityKey: null,
  identityUserId: null,
  isAdmin: false,
  category: DEFAULT_USAGE_CATEGORY,
  scope: DEFAULT_USAGE_SCOPE,
  range: DEFAULT_USAGE_RANGE,
  start: '',
  end: '',
  filters: createDefaultUsageFilters(),
  first: 0,
  rows: initialRows(),
  total: 0,
  sortField: DEFAULT_USAGE_SORT_FIELD,
  sortOrder: DEFAULT_USAGE_SORT_ORDER,
  perspective: DEFAULT_USAGE_PERSPECTIVE,
  mode: loadUsageMode(modeStorage(), DEFAULT_USAGE_CATEGORY),
  records: [],
  facets: emptyUsageFacets(),
  overview: null,
  userOptions: [],
  recordsLoading: false,
  facetsLoading: false,
  overviewLoading: false,
  pageLoading: false,

  initializeIdentity(identity) {
    if (get().identityKey === usageIdentityKey(identity)) return
    get().resetIdentity(identity)
  },

  resetIdentity(identity) {
    // In-flight reads of the previous session must never commit into this one.
    useUsageDetailStore.getState().reset()
    ++recordsGeneration
    ++facetsGeneration
    ++overviewGeneration
    const category = DEFAULT_USAGE_CATEGORY
    set({
      identityKey: identity ? usageIdentityKey(identity) : null,
      identityUserId: identity?.user_id ?? null,
      isAdmin: identity?.is_admin === true,
      category,
      scope: DEFAULT_USAGE_SCOPE,
      range: DEFAULT_USAGE_RANGE,
      start: '',
      end: '',
      filters: createDefaultUsageFilters(),
      first: 0,
      rows: initialRows(),
      total: 0,
      sortField: DEFAULT_USAGE_SORT_FIELD,
      sortOrder: DEFAULT_USAGE_SORT_ORDER,
      perspective: DEFAULT_USAGE_PERSPECTIVE,
      mode: loadUsageMode(modeStorage(), category),
      records: [],
      facets: emptyUsageFacets(),
      overview: null,
      userOptions: [],
      recordsLoading: false,
      facetsLoading: false,
      overviewLoading: false,
      pageLoading: false,
    })
  },

  queryState() {
    const state = get()
    return {
      category: state.category,
      scope: state.scope,
      range:
        state.range === '1h' || state.range === '5h' ? 'custom' : state.range,
      start: state.start,
      end: state.end,
      first: state.first,
      rows: state.rows,
      sortField: state.sortField,
      sortOrder: state.sortOrder,
      filters: state.filters,
    }
  },

  async setMode(mode, queryClient) {
    const { category, range } = get()
    saveUsageMode(modeStorage(), category, mode)
    set({ mode })
    if (queryClient && (range === '1h' || range === '5h')) {
      await get().refreshAll(queryClient)
    }
  },

  async setCategory(queryClient, category) {
    if (get().category === category) return
    useUsageDetailStore.getState().reset()
    set({
      category,
      scope: DEFAULT_USAGE_SCOPE,
      first: 0,
      filters: createDefaultUsageFilters(),
      mode: loadUsageMode(modeStorage(), category),
    })
    await get().refreshAll(queryClient)
  },

  async setScope(queryClient, scope) {
    if (get().scope === scope) return
    const state = get()
    set({
      scope,
      first: 0,
      filters: usageFiltersForScope(state.filters, state.category, scope),
    })
    await get().refreshAll(queryClient)
  },

  async setFilters(queryClient, patch) {
    set({ filters: { ...get().filters, ...patch } })
    await get().refreshRecords(queryClient)
  },

  async setPage(queryClient, first, rows) {
    const nextRows = normalizeUsagePageSize(rows)
    writeStorage(pageSizeKey, String(nextRows))
    set({ rows: nextRows })
    await get().refreshRecords(queryClient, Math.max(0, first), nextRows)
  },

  async setSort(queryClient, field) {
    const state = get()
    const next = nextUsageSortState(
      { field: state.sortField, order: state.sortOrder },
      field,
    )
    // A sort keeps the page the operator is on.
    set({ sortField: next.field, sortOrder: next.order })
    await get().refreshRecords(queryClient)
  },

  async setRange(queryClient, range, start, end) {
    set({ range, start: start ?? '', end: end ?? '' })
    await get().refreshAll(queryClient)
  },

  async setPerspective(queryClient, perspective) {
    if (get().perspective === perspective) return { executed: false }
    const captured = {
      perspective,
      category: get().category,
      scope: get().scope,
      range: get().range,
      start: get().start,
      end: get().end,
    }
    return runAsyncAction(
      USAGE_PERSPECTIVE_KEY,
      async () => {
        set({ perspective: captured.perspective })
        await get().refreshOverview(queryClient)
      },
      'perspective',
    )
  },

  async applyDrilldown(queryClient, drilldown) {
    const state = get()
    const filters = {
      ...state.filters,
      search: null,
      endpoint_id: drilldown.endpoint_id ?? null,
      mcp_server_id: drilldown.mcp_server_id ?? null,
      mcp_bearer_token_slot: drilldown.mcp_bearer_token_slot ?? null,
      model: drilldown.model ?? null,
    }
    const nextScope = drilldown.mcp_scope ?? null
    const scopeChanged = nextScope !== null && nextScope !== state.scope
    set({
      filters: scopeChanged
        ? usageFiltersForScope(filters, state.category, nextScope)
        : filters,
      first: 0,
      scope: nextScope ?? state.scope,
      mode: 'records',
    })
    saveUsageMode(modeStorage(), state.category, 'records')
    // The drilldown opens the records list. A scope change also has to refresh
    // the overview so it stops serving another scope; a failed background
    // refresh keeps the previous overview and the next explicit refresh
    // reports the error.
    if (scopeChanged)
      void get()
        .refreshOverview(queryClient)
        .catch(() => {})
    await get().refreshRecords(queryClient, 0)
  },

  async refreshRecords(queryClient, first, rows) {
    const state = get()
    if (state.identityKey === null) return
    const nextRows =
      rows === undefined ? state.rows : normalizeUsagePageSize(rows)
    const nextFirst = Math.max(0, first ?? state.first)
    set({ first: nextFirst, rows: nextRows })
    const query = usageListQuery({
      ...get().queryState(),
      first: nextFirst,
      rows: nextRows,
    })
    const identity: UsageIdentity = {
      user_id: get().identityUserId,
      is_admin: get().isAdmin,
    }
    const generation = ++recordsGeneration
    set({ recordsLoading: true })
    let correction: { first: number; rows: number } | null = null
    try {
      const page = await fetchFresh(
        queryClient,
        usageListQueryKey(identity, query),
        USAGE_LIST_NAMESPACE,
        () => loadUsagePage(query),
      )
      if (generation !== recordsGeneration) return
      set({
        records: page.records,
        total: page.total,
        first: page.first,
        rows: page.rows,
      })
      if (isUsageEmptyTail(page)) {
        correction = {
          first: usageLastPageFirst(page.total, page.rows),
          rows: page.rows,
        }
      }
    } catch (cause) {
      // A superseded query reports nothing: its rows, total and error must not
      // replace the newer query's state.
      if (generation === recordsGeneration) {
        set({ recordsLoading: false })
        throw cause
      }
      return
    }
    if (generation !== recordsGeneration) return
    set({ recordsLoading: false })
    // The correction is a fresh query, so a newer query that started in the
    // meantime wins and the retry reports its own failure.
    if (correction)
      await get().refreshRecords(queryClient, correction.first, correction.rows)
  },

  async refreshFacets(queryClient) {
    const state = get()
    if (state.identityKey === null) return
    const query = usageFacetsQuery({ ...state, ...get().queryState() })
    const identity: UsageIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++facetsGeneration
    set({ facetsLoading: true })
    try {
      const facets = await fetchFresh(
        queryClient,
        usageFacetsQueryKey(identity, query),
        USAGE_FACETS_NAMESPACE,
        () => loadUsageFacets(query),
      )
      if (generation !== facetsGeneration) return
      set({ facets, facetsLoading: false })
    } catch (cause) {
      if (generation === facetsGeneration) {
        set({ facetsLoading: false })
        throw cause
      }
    }
  },

  async refreshOverview(queryClient) {
    const state = get()
    if (state.identityKey === null) return
    const query = usageOverviewQuery(
      { ...state, ...get().queryState() },
      state.perspective,
    )
    const signature = usageOverviewSignature({
      category: state.category,
      perspective: state.perspective,
      range: get().queryState().range,
      start: state.start,
      end: state.end,
    })
    const identity: UsageIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++overviewGeneration
    set({ overviewLoading: true })
    try {
      const overview = await fetchFresh(
        queryClient,
        usageOverviewQueryKey(identity, query),
        USAGE_OVERVIEW_NAMESPACE,
        () => loadUsageOverview(query),
      )
      if (generation !== overviewGeneration) return
      const current = get()
      if (
        signature !==
        usageOverviewSignature({
          category: current.category,
          perspective: current.perspective,
          range:
            current.range === '1h' || current.range === '5h'
              ? 'custom'
              : current.range,
          start: current.start,
          end: current.end,
        })
      ) {
        return
      }
      set({ overview, overviewLoading: false })
    } catch (cause) {
      if (generation === overviewGeneration) {
        set({ overviewLoading: false })
        throw cause
      }
    }
  },

  async refreshPage(queryClient) {
    const generation = ++pageGeneration
    set({ pageLoading: true })
    try {
      await Promise.all([
        get().refreshRecords(queryClient),
        get().refreshFacets(queryClient),
      ])
    } finally {
      if (generation === pageGeneration) set({ pageLoading: false })
    }
  },

  async refreshAll(queryClient) {
    const { range } = get()
    if (range === '1h' || range === '5h') {
      set(slidingUsageWindow(range, Date.now()))
    }
    const generation = ++pageGeneration
    set({ pageLoading: true })
    try {
      await Promise.all([
        get().refreshOverview(queryClient),
        get().refreshRecords(queryClient),
        get().refreshFacets(queryClient),
        get().isAdmin ? get().loadUserOptions(queryClient) : Promise.resolve(),
      ])
    } finally {
      if (generation === pageGeneration) set({ pageLoading: false })
    }
  },

  async clearHistory(queryClient, form) {
    // A malformed local date fails before the guard and before any request.
    const body = usageClearPayload(form)
    return runAsyncAction(
      USAGE_MAINTENANCE_KEY,
      async () => {
        const result: RequestRecordsClearResponse = expectData(
          await clearRequestRecords<true>(withData({ body })),
        )
        await get().refreshAll(queryClient)
        return result
      },
      'clear',
    )
  },

  async loadUserOptions(queryClient) {
    const state = get()
    // The user options are an admin filter source, never a page read for an
    // ordinary user.
    if (state.identityKey === null || !state.isAdmin) return
    const identity: UsageIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const users = await fetchFresh(
      queryClient,
      usageUserOptionsQueryKey(identity),
      USAGE_USER_OPTIONS_NAMESPACE,
      loadUsageUserOptions,
    )
    if (get().identityKey !== state.identityKey) return
    set({ userOptions: users })
  },
}))

useSessionStore.subscribe((state, previous) => {
  const currentKey = usageIdentityKey({
    user_id: state.me?.user_id ?? null,
    is_admin: state.me?.is_admin === true,
  })
  const previousKey = usageIdentityKey({
    user_id: previous.me?.user_id ?? null,
    is_admin: previous.me?.is_admin === true,
  })
  if (currentKey !== previousKey) {
    useUsageStore
      .getState()
      .resetIdentity(
        state.me
          ? { user_id: state.me.user_id, is_admin: state.me.is_admin === true }
          : null,
      )
  }
})
