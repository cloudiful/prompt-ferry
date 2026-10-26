import type {
  ListRequestRecordsData,
  RequestRecordCategory,
  RequestRecordFacets,
  RequestRecordFacetsData,
  RequestRecordListRow,
  RequestRecordMcpScope,
  RequestRecordOverviewData,
  RequestRecordOverviewRange,
  RequestRecordOverviewResponse,
} from '@/generated/admin-api'
import { formatTokenQuantity } from '@/format/quantity'
import type { Locale } from '@/i18n'
import {
  REQUEST_RECORD_PAGE_SIZE_OPTIONS,
  normalizePageSize,
  pageSizeStorageKey,
} from '@/models/pagination'

export type UsageCategory = RequestRecordCategory
export type UsageScope = RequestRecordMcpScope
export type UsageRange = RequestRecordOverviewRange
export type UsagePerspective = 'model' | 'upstream'
export type UsageMode = 'overview' | 'records'
export type UsageSortOrder = -1 | 0 | 1

export const DEFAULT_USAGE_CATEGORY: UsageCategory = 'ai'
export const DEFAULT_USAGE_SCOPE: UsageScope = 'service'
export const DEFAULT_USAGE_RANGE: UsageRange = '24h'
export const DEFAULT_USAGE_PERSPECTIVE: UsagePerspective = 'model'
export const DEFAULT_USAGE_MODE: UsageMode = 'overview'
export const DEFAULT_USAGE_SORT_FIELD = 'created_at'
export const DEFAULT_USAGE_SORT_ORDER: UsageSortOrder = -1
export const DEFAULT_USAGE_ROWS = 25
export const USAGE_PAGE_SIZE_OPTIONS = REQUEST_RECORD_PAGE_SIZE_OPTIONS
export const USAGE_PAGE_SIZE_KEY = 'request-records'
export const USAGE_MODE_STORAGE_KEY = 'request-records-overview-mode'
export const USAGE_PERSPECTIVE_KEY = 'usage:perspective'

export const USAGE_LIST_NAMESPACE = ['usage', 'records'] as const
export const USAGE_FACETS_NAMESPACE = ['usage', 'facets'] as const
export const USAGE_OVERVIEW_NAMESPACE = ['usage', 'overview'] as const
export const USAGE_USER_OPTIONS_NAMESPACE = ['usage', 'user-options'] as const

/** The optional record filters, each `null` when the filter is off. */
export type UsageFilters = {
  search: string | null
  user: string | null
  model: string | null
  client_key_id: number | null
  endpoint_id: string | null
  mcp_server_id: string | null
  mcp_bearer_token_slot: number | null
  request_state: string | null
  redaction_applied: boolean | null
}

export function createDefaultUsageFilters(): UsageFilters {
  return {
    search: null,
    user: null,
    model: null,
    client_key_id: null,
    endpoint_id: null,
    mcp_server_id: null,
    mcp_bearer_token_slot: null,
    request_state: null,
    redaction_applied: null,
  }
}

export function createDefaultUsageFiltersForScope(): UsageFilters {
  return createDefaultUsageFilters()
}

export type UsageIdentity = {
  user_id: number | null
  is_admin: boolean
}

export function usageIdentityKey(identity: UsageIdentity): string {
  return `${identity.user_id ?? ''}:${identity.is_admin ? 'admin' : 'user'}`
}

/** The captured query every list read is built from. */
export type UsageQueryState = {
  category: UsageCategory
  scope: UsageScope
  range: UsageRange
  start: string
  end: string
  first: number
  rows: number
  sortField: string
  sortOrder: UsageSortOrder
  filters: UsageFilters
}

export type UsageListQuery = NonNullable<ListRequestRecordsData['query']>
export type UsageFacetsQuery = NonNullable<RequestRecordFacetsData['query']>
export type UsageOverviewQuery = NonNullable<RequestRecordOverviewData['query']>

function optionalText(value: string | null): string | undefined {
  return value === null || value === '' ? undefined : value
}

/**
 * The exact list query: the page, the category, the scope, the sort, the range
 * and only the filters that are set. A custom window sends its bounds; the
 * preset windows let the server derive them from the range.
 */
export function usageListQuery(state: UsageQueryState): UsageListQuery {
  const { filters } = state
  return {
    first: state.first,
    rows: state.rows,
    request_category: state.category,
    mcp_scope: state.scope,
    sort_field: state.sortField,
    sort_order: state.sortOrder,
    range: state.range,
    start: optionalText(state.start),
    end: optionalText(state.end),
    search: optionalText(filters.search),
    user: optionalText(filters.user),
    model: optionalText(filters.model),
    client_key_id: filters.client_key_id ?? undefined,
    endpoint_id: optionalText(filters.endpoint_id),
    mcp_server_id: optionalText(filters.mcp_server_id),
    mcp_bearer_token_slot: filters.mcp_bearer_token_slot ?? undefined,
    request_state: (optionalText(filters.request_state) ??
      undefined) as UsageListQuery['request_state'],
    redaction_applied: filters.redaction_applied ?? undefined,
  }
}

export type UsageRangeWindow = { start?: string; end?: string }

const USAGE_RANGE_WINDOW_MS: Record<
  Exclude<UsageRange, 'custom' | 'month'>,
  number
> = {
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
}

/**
 * The explicit `[start, end)` bounds a preset window stands for, mirroring the
 * backend so the facet dropdowns follow the same window as the record list. A
 * custom window keeps the picker-supplied bounds unchanged.
 */
export function resolveUsageRangeWindow(
  range: UsageRange,
  start: string,
  end: string,
  now: Date = new Date(),
): UsageRangeWindow {
  if (range === 'custom') {
    return { start: start || undefined, end: end || undefined }
  }
  const windowStart =
    range === 'month'
      ? new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1))
      : new Date(now.getTime() - USAGE_RANGE_WINDOW_MS[range])
  return { start: windowStart.toISOString(), end: now.toISOString() }
}

/** The facet query carries only the category, the scope and the window. */
export function usageFacetsQuery(
  state: Pick<
    UsageQueryState,
    'category' | 'scope' | 'range' | 'start' | 'end'
  >,
  now: Date = new Date(),
): UsageFacetsQuery {
  const window = resolveUsageRangeWindow(
    state.range,
    state.start,
    state.end,
    now,
  )
  return {
    request_category: state.category,
    mcp_scope: state.scope,
    start: window.start,
    end: window.end,
  }
}

/**
 * The overview query carries only the category, the perspective, the scope and
 * the window. The MCP overview has no model/upstream choice, so its
 * perspective is fixed instead of forwarding an inert value.
 */
export function usageOverviewQuery(
  state: Pick<
    UsageQueryState,
    'category' | 'scope' | 'range' | 'start' | 'end'
  >,
  perspective: UsagePerspective,
  now: Date = new Date(),
): UsageOverviewQuery {
  const window = resolveUsageRangeWindow(
    state.range,
    state.start,
    state.end,
    now,
  )
  return {
    request_category: state.category,
    perspective: state.category === 'mcp' ? 'model' : perspective,
    mcp_scope: state.scope,
    range: state.range,
    start: window.start,
    end: window.end,
  }
}

/** A stable cache signature: the key order is fixed by the builder above. */
export function usageQuerySignature(query: object): string {
  return JSON.stringify(query)
}

export function usageListQueryKey(
  identity: UsageIdentity,
  query: UsageListQuery,
) {
  return [
    ...USAGE_LIST_NAMESPACE,
    usageIdentityKey(identity),
    usageQuerySignature(query),
  ] as const
}

export function usageFacetsQueryKey(
  identity: UsageIdentity,
  query: UsageFacetsQuery,
) {
  return [
    ...USAGE_FACETS_NAMESPACE,
    usageIdentityKey(identity),
    usageQuerySignature(query),
  ] as const
}

export function usageOverviewQueryKey(
  identity: UsageIdentity,
  query: UsageOverviewQuery,
) {
  return [
    ...USAGE_OVERVIEW_NAMESPACE,
    usageIdentityKey(identity),
    usageQuerySignature(query),
  ] as const
}

export function usageUserOptionsQueryKey(identity: UsageIdentity) {
  return [...USAGE_USER_OPTIONS_NAMESPACE, usageIdentityKey(identity)] as const
}

export const usagePageSizeStorageKey = pageSizeStorageKey(USAGE_PAGE_SIZE_KEY)

export function normalizeUsagePageSize(value: number | string | null): number {
  return normalizePageSize(value, DEFAULT_USAGE_ROWS, USAGE_PAGE_SIZE_OPTIONS)
}

/**
 * The sortable record columns and the backend field each one sorts by.
 *
 * `Date` sorts by the derived request day (`usage_date`) and `Time` by the
 * recorded instant (`created_at`); every other column maps to a field the
 * backend whitelist accepts. A column that is not registered here is not
 * sortable, so the UI cannot send an unsupported `request_date`-style field.
 */
export const USAGE_SORT_FIELDS = {
  created_at: 'created_at',
  duration_ms: 'duration_ms',
  client_key: 'client_key_label',
  mcp_method: 'mcp_protocol_method',
  mcp_operation: 'mcp_operation_name',
  model: 'model_key',
  request_date: 'usage_date',
  request_state: 'request_state',
  status: 'status',
  target: 'target',
  total_tokens: 'total_tokens',
  ttft_ms: 'ttft_ms',
  user: 'user_key',
} as const

export type UsageSortColumn = keyof typeof USAGE_SORT_FIELDS

/** The backend whitelist, kept as data so the cycle helper stays pure. */
export const USAGE_SORTABLE_BACKEND_FIELDS = [
  'created_at',
  'usage_date',
  'user_key',
  'client_key_label',
  'model_key',
  'mcp_protocol_method',
  'mcp_operation_name',
  'target',
  'request_state',
  'status',
  'duration_ms',
  'ttft_ms',
  'total_tokens',
] as const

export function usageSortFieldForColumn(column: UsageSortColumn): string {
  return USAGE_SORT_FIELDS[column]
}

export type UsageSortState = { field: string; order: UsageSortOrder }

/**
 * Cycle a sortable column: ascending, then descending, then unsorted.
 *
 * Selecting a different column starts at ascending and keeps that field even
 * when the cycle later reaches order 0, so the inactive/unsorted request still
 * names the column the operator chose instead of inventing a default.
 */
export function nextUsageSortState(
  current: UsageSortState,
  field: string,
): UsageSortState {
  if (current.field !== field) return { field, order: 1 }
  if (current.order === 1) return { field, order: -1 }
  if (current.order === -1) return { field, order: 0 }
  return { field, order: 1 }
}

/** The `aria-sort` value for a column, or `undefined` when it is inactive. */
export function usageAriaSort(
  current: UsageSortState,
  field: string,
): 'ascending' | 'descending' | 'none' | undefined {
  if (current.field !== field) return undefined
  if (current.order === 1) return 'ascending'
  if (current.order === -1) return 'descending'
  return 'none'
}

/**
 * An entry whose token total is exactly zero.
 *
 * Only a real `0` folds. A missing, null, undefined or `NaN` total is an
 * unknown measurement and never becomes `0`.
 */
export function isZeroTokenEntry(entry: {
  total_tokens?: number | null
}): boolean {
  return entry.total_tokens === 0
}

/**
 * Split detail rows into the rows shown by default and the zero-token rows that
 * are folded behind the reveal control.
 *
 * Visible rows keep their incoming order and folded rows follow them. The
 * hidden count is the request count the folded rows stand for, so the caller
 * can report what it folded away without touching summary, breakdown or
 * filter totals.
 */
export function partitionZeroTokenEntries<
  T extends { total_tokens?: number | null; request_count: number },
>(
  entries: readonly T[],
): {
  visible: T[]
  folded: T[]
  hiddenRequestCount: number
} {
  const visible: T[] = []
  const folded: T[] = []
  let hiddenRequestCount = 0
  for (const entry of entries ?? []) {
    if (!isZeroTokenEntry(entry)) {
      visible.push(entry)
      continue
    }
    folded.push(entry)
    hiddenRequestCount += entry.request_count ?? 0
  }
  return { visible, folded, hiddenRequestCount }
}

/**
 * A token measurement for a table cell: an exact `0` renders `0`, a missing or
 * non-finite measurement renders `-`.
 */
export function formatUsageTokenQuantity(
  value: number | null | undefined,
  locale: Locale,
): string {
  if (value == null || !Number.isFinite(value)) return '-'
  return formatTokenQuantity(value, locale)
}

export type UsageModeStorage = Pick<Storage, 'getItem' | 'setItem'>

/** The overview/records mode remembered per category. */
export function loadUsageMode(
  storage: UsageModeStorage | null,
  category: UsageCategory,
): UsageMode {
  if (!storage) return DEFAULT_USAGE_MODE
  try {
    const raw = storage.getItem(USAGE_MODE_STORAGE_KEY)
    if (!raw) return DEFAULT_USAGE_MODE
    const parsed = JSON.parse(raw) as Partial<Record<UsageCategory, UsageMode>>
    return parsed[category] ?? DEFAULT_USAGE_MODE
  } catch {
    return DEFAULT_USAGE_MODE
  }
}

export function saveUsageMode(
  storage: UsageModeStorage | null,
  category: UsageCategory,
  mode: UsageMode,
): void {
  if (!storage) return
  try {
    const raw = storage.getItem(USAGE_MODE_STORAGE_KEY)
    const parsed = raw
      ? (JSON.parse(raw) as Partial<Record<UsageCategory, UsageMode>>)
      : {}
    parsed[category] = mode
    storage.setItem(USAGE_MODE_STORAGE_KEY, JSON.stringify(parsed))
  } catch {
    // Persistence is best effort; the in-memory mode stays authoritative.
  }
}

/** The first row of the last page that still holds rows. */
export function usageLastPageFirst(total: number, rows: number): number {
  return total > 0 && rows > 0 ? Math.floor((total - 1) / rows) * rows : 0
}

/** True when a read returned an empty page past the end of the collection. */
export function isUsageEmptyTail(page: {
  records: RequestRecordListRow[]
  total: number
  first: number
}): boolean {
  return page.records.length === 0 && page.total > 0 && page.first >= page.total
}

/**
 * The filters that cannot match a gateway record: a gateway row carries no MCP
 * server identity, so a service filter would hide every gateway row.
 */
export function usageFiltersForScope(
  filters: UsageFilters,
  category: UsageCategory,
  scope: UsageScope,
): UsageFilters {
  if (scope !== 'gateway' || category !== 'mcp') return filters
  return {
    ...filters,
    mcp_server_id: null,
    mcp_bearer_token_slot: null,
    model: null,
  }
}

/** The facets the future filter controls read, with a stable empty shape. */
export function emptyUsageFacets(): RequestRecordFacets {
  return { dates: [], users: [], models: [] }
}

/**
 * The overview query identity the newest result must still match.
 *
 * It is built from the stored window, not from the resolved request bounds, so
 * a refresh that starts in the same second still matches the state it read.
 */
export function usageOverviewSignature(input: {
  category: UsageCategory
  perspective: UsagePerspective
  range: UsageRange
  start: string
  end: string
}): string {
  return usageQuerySignature({
    category: input.category,
    perspective: input.perspective,
    range: input.range,
    start: input.start,
    end: input.end,
  })
}

export type { RequestRecordOverviewResponse }
