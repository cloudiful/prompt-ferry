import type {
  ConversationEndpointOverride,
  ConversationEndpointOverrideRequest,
  RequestRecordCategory,
  RequestRecordFullResponse,
  RequestRecordListRow,
  RequestRecordOverviewResponse,
  RequestRecordsClearRequest,
  SessionRouteOptionsResponse,
} from '../../../src/generated/admin-api'
import {
  error,
  forbidden,
  nextId,
  ok,
  page,
  queryInt,
  queryString,
  readJson,
  requireAdmin,
  unauthorized,
  type FixtureContext,
} from '../http'
import { isoNow } from '../http'

const RECORDS = /^\/api\/v1\/admin\/request-records$/
const CLEAR = /^\/api\/v1\/admin\/request-records\/clear$/
const PRUNE = /^\/api\/v1\/admin\/request-records\/prune$/
const OVERVIEW = /^\/api\/v1\/admin\/request-records\/overview$/
const FACETS = /^\/api\/v1\/admin\/request-records\/facets$/
const SERIES = /^\/api\/v1\/admin\/request-records\/series$/
const SUMMARY = /^\/api\/v1\/admin\/request-records\/summary$/
const DETAIL = /^\/api\/v1\/admin\/request-records\/([^/]+)$/
const FULL = /^\/api\/v1\/admin\/request-records\/([^/]+)\/request-full$/
const RESET_AFFINITY =
  /^\/api\/v1\/admin\/request-records\/([^/]+)\/reset-session-affinity$/
const ROUTE_OPTIONS =
  /^\/api\/v1\/admin\/request-records\/([^/]+)\/session-route-options$/
const OVERRIDE = /^\/api\/v1\/admin\/conversations\/([^/]+)\/endpoint-override$/

const CLIENT_KEYS = [
  { key_id: 1, label: 'primary' },
  { key_id: 2, label: 'integration-key' },
] as const

/** The backend's sortable whitelist; anything else falls back to the instant. */
const SORTABLE_FIELDS = new Set([
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
])

const RANGE_WINDOW_MS: Record<string, number> = {
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
}

/** The same half-open window the client resolves, so the two cannot drift. */
function rangeWindow(url: URL): { start: number; end: number } {
  const range = queryString(url, 'range') ?? '24h'
  if (range === 'custom') {
    const start = queryString(url, 'start')
    const end = queryString(url, 'end')
    return {
      start: start ? Date.parse(start) : Number.NEGATIVE_INFINITY,
      end: end ? Date.parse(end) : Number.POSITIVE_INFINITY,
    }
  }
  const now = Date.now()
  const start =
    range === 'month'
      ? Date.UTC(new Date(now).getUTCFullYear(), new Date(now).getUTCMonth(), 1)
      : now - (RANGE_WINDOW_MS[range] ?? RANGE_WINDOW_MS['24h'])
  return { start, end: now }
}

function categoryOf(ctx: FixtureContext): RequestRecordCategory {
  return queryString(ctx.url, 'request_category') === 'mcp' ? 'mcp' : 'ai'
}

function scopeOf(ctx: FixtureContext): string {
  return queryString(ctx.url, 'mcp_scope') ?? 'service'
}

/**
 * The records the caller may see: an admin sees every record, an ordinary user
 * sees only their own, whatever filter they forge.
 */
function scopedSource(
  ctx: FixtureContext,
  category: RequestRecordCategory,
): RequestRecordListRow[] {
  const source = category === 'mcp' ? ctx.state.mcpRecords : ctx.state.aiRecords
  if (ctx.account?.isAdmin) return source
  const userId = ctx.account?.user.user_id ?? null
  return source.filter((row) => row.user_id === userId)
}

function matchesScope(
  row: RequestRecordListRow,
  category: RequestRecordCategory,
  scope: string,
): boolean {
  if (category !== 'mcp' || scope === 'all') return true
  const hasServer =
    row.mcp_server_id != null || (row.mcp_server_name ?? null) != null
  return scope === 'service' ? hasServer : !hasServer
}

function matchesFilters(row: RequestRecordListRow, url: URL): boolean {
  const search = (
    queryString(url, 'search') ?? queryString(url, 'q')
  )?.toLowerCase()
  if (search) {
    const haystack = [
      row.request_id,
      row.model,
      row.requested_model,
      row.upstream_model,
      row.mcp_server_name,
      row.mcp_operation_name,
      row.user_login_name,
    ]
      .filter(Boolean)
      .join(' ')
      .toLowerCase()
    if (!haystack.includes(search)) return false
  }
  const user = queryString(url, 'user') ?? queryString(url, 'user_login_name')
  if (user && row.user_login_name !== user) return false
  const model = queryString(url, 'model')
  if (model && row.model !== model) return false
  const endpointId = queryString(url, 'endpoint_id')
  if (endpointId && row.endpoint_id !== endpointId) return false
  const serverId = queryString(url, 'mcp_server_id')
  if (serverId && row.mcp_server_id !== serverId) return false
  const slot = queryString(url, 'mcp_bearer_token_slot')
  if (
    slot !== undefined &&
    row.mcp_bearer_token_slot !== Number.parseInt(slot, 10)
  ) {
    return false
  }
  const clientKeyId = queryString(url, 'client_key_id')
  if (clientKeyId !== undefined) {
    const label = CLIENT_KEYS.find(
      (key) => key.key_id === Number.parseInt(clientKeyId, 10),
    )?.label
    if (!label || row.client_key_label !== label) return false
  }
  const state = queryString(url, 'request_state') ?? queryString(url, 'state')
  if (state && row.request_state !== state) return false
  const redaction = queryString(url, 'redaction_applied')
  if (redaction !== undefined) {
    const applied = redaction === 'true' || redaction === '1'
    if ((row.redaction?.applied === true) !== applied) return false
  }
  return true
}

/** The records the list, the facets and the overview all share. */
function recordsFor(
  ctx: FixtureContext,
  category: RequestRecordCategory,
): RequestRecordListRow[] {
  const window = rangeWindow(ctx.url)
  const scope = scopeOf(ctx)
  return scopedSource(ctx, category)
    .filter((row) => matchesScope(row, category, scope))
    .filter((row) => {
      const at = Date.parse(row.created_at)
      return at >= window.start && at < window.end
    })
}

function sortValue(
  row: RequestRecordListRow,
  field: string,
): string | number | null | undefined {
  switch (field) {
    case 'usage_date':
      return row.created_at.slice(0, 10)
    case 'user_key':
      return row.user_login_name
    case 'model_key':
      return row.model
    case 'target':
      return row.endpoint_name ?? row.mcp_server_name
    default:
      return (row as unknown as Record<string, string | number | null>)[field]
  }
}

function sortRecords(
  rows: RequestRecordListRow[],
  url: URL,
): RequestRecordListRow[] {
  const requested = queryString(url, 'sort_field') ?? 'created_at'
  const order = queryInt(url, 'sort_order', -1)
  // An unknown field must not silently become a property sort: the backend
  // falls back to the recorded instant, descending.
  const field = SORTABLE_FIELDS.has(requested) ? requested : 'created_at'
  const direction = SORTABLE_FIELDS.has(requested) ? order : -1
  if (direction === 0) return rows
  return [...rows].sort((left, right) => {
    const leftValue = sortValue(left, field)
    const rightValue = sortValue(right, field)
    const leftNull = leftValue === null || leftValue === undefined
    const rightNull = rightValue === null || rightValue === undefined
    if (leftNull && rightNull) return left.record_id - right.record_id
    // Nullable values sort last in both directions.
    if (leftNull) return 1
    if (rightNull) return -1
    const compared =
      typeof leftValue === 'number' && typeof rightValue === 'number'
        ? leftValue - rightValue
        : String(leftValue).localeCompare(String(rightValue))
    if (compared !== 0) return direction > 0 ? compared : -compared
    return left.record_id - right.record_id
  })
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000
}

function ratio(part: number, whole: number): number {
  return whole > 0 ? round(part / whole) : 0
}

function percentile(
  values: Array<number | null | undefined>,
  fraction: number,
): number | null {
  const sorted = values
    .filter((value): value is number => typeof value === 'number')
    .sort((left, right) => left - right)
  if (sorted.length === 0) return null
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(fraction * sorted.length) - 1),
  )
  return sorted[index]
}

function averageOutputRate(rows: RequestRecordListRow[]): number | null {
  const samples = rows
    .filter(
      (row) =>
        row.ok !== false &&
        typeof row.output_tokens === 'number' &&
        row.output_tokens > 0 &&
        typeof row.duration_ms === 'number' &&
        row.duration_ms > 0,
    )
    .map(
      (row) =>
        (row.output_tokens as number) / ((row.duration_ms as number) / 1000),
    )
  if (samples.length === 0) return null
  return round(samples.reduce((sum, value) => sum + value, 0) / samples.length)
}

/**
 * Token totals for a set of rows.
 *
 * The overview contract requires non-null numbers, so an unknown measurement
 * contributes zero to the aggregate; the detail rows keep their nulls.
 */
function tokenUsageOf(rows: RequestRecordListRow[]) {
  const sum = (
    pick: (row: RequestRecordListRow) => number | null | undefined,
  ) => rows.reduce((total, row) => total + (pick(row) ?? 0), 0)
  const input = sum((row) => row.input_tokens)
  const output = sum((row) => row.output_tokens)
  const cacheRead = sum((row) => row.cache_read_tokens)
  const cacheWrite = sum((row) => row.cache_write_tokens)
  const total = sum((row) => row.total_tokens)
  return {
    cache_hit_rate: total > 0 ? ratio(cacheRead, total) : null,
    cache_rate: total > 0 ? ratio(cacheRead + cacheWrite, total) : null,
    cache_read_tokens: cacheRead,
    cache_write_tokens: cacheWrite,
    input_tokens: input,
    output_tokens: output,
    total_tokens: total,
  }
}

function detailRows(rows: RequestRecordListRow[], by: 'model' | 'upstream') {
  const groups = new Map<string, RequestRecordListRow[]>()
  for (const row of rows) {
    const key =
      by === 'model'
        ? (row.model ?? '(direct)')
        : (row.endpoint_id ?? '(direct)')
    groups.set(key, [...(groups.get(key) ?? []), row])
  }
  const windowRequests = rows.length
  const windowTokens = rows.reduce(
    (total, row) => total + (row.total_tokens ?? 0),
    0,
  )
  return [...groups.entries()].map(([key, group]) => {
    const errors = group.filter((row) => row.ok === false).length
    const tokens = group.reduce(
      (total, row) => total + (row.total_tokens ?? 0),
      0,
    )
    return {
      avg_output_tokens_per_second: averageOutputRate(group),
      cache_rate: tokenUsageOf(group).cache_rate,
      endpoint_id: by === 'upstream' ? (group[0].endpoint_id ?? null) : null,
      endpoint_name:
        by === 'upstream' ? (group[0].endpoint_name ?? null) : null,
      error_count: errors,
      error_rate: ratio(errors, group.length),
      model: by === 'model' ? key : undefined,
      request_count: group.length,
      request_share: ratio(group.length, windowRequests),
      token_share: windowTokens > 0 ? ratio(tokens, windowTokens) : null,
      total_tokens: tokens,
    }
  })
}

function breakdownRows(
  rows: RequestRecordListRow[],
  category: RequestRecordCategory,
  perspective: 'model' | 'upstream',
) {
  const groups = new Map<string, RequestRecordListRow[]>()
  for (const row of rows) {
    const key =
      category === 'mcp'
        ? (row.mcp_server_id ?? row.mcp_server_name ?? '(gateway)')
        : perspective === 'upstream'
          ? (row.endpoint_id ?? '(direct)')
          : (row.model ?? '(direct)')
    groups.set(key, [...(groups.get(key) ?? []), row])
  }
  const windowRequests = rows.length
  const windowTokens = rows.reduce(
    (total, row) => total + (row.total_tokens ?? 0),
    0,
  )
  return [...groups.entries()].map(([key, group]) => {
    const errors = group.filter((row) => row.ok === false).length
    const tokens = tokenUsageOf(group)
    const sample = group[0]
    const isUpstream = category === 'ai' && perspective === 'upstream'
    return {
      avg_output_tokens_per_second:
        category === 'ai' ? averageOutputRate(group) : null,
      endpoint_id: isUpstream ? (sample.endpoint_id ?? null) : null,
      error_count: errors,
      error_rate: ratio(errors, group.length),
      label: key,
      mcp_scope:
        category === 'mcp'
          ? sample.mcp_server_id
            ? ('service' as const)
            : ('gateway' as const)
          : null,
      mcp_server_id: category === 'mcp' ? (sample.mcp_server_id ?? null) : null,
      model:
        category === 'ai' && perspective === 'model'
          ? (sample.model ?? null)
          : null,
      model_breakdown: isUpstream ? detailRows(group, 'model') : null,
      request_count: group.length,
      request_share: ratio(group.length, windowRequests),
      server_provider_kind:
        category === 'mcp' ? (sample.server_provider_kind ?? null) : null,
      success_count: group.length - errors,
      success_rate: ratio(group.length - errors, group.length),
      token_share:
        windowTokens > 0 ? ratio(tokens.total_tokens, windowTokens) : null,
      tokens,
      upstream_breakdown:
        category === 'ai' && perspective === 'model'
          ? detailRows(group, 'upstream')
          : null,
      upstream_count:
        category === 'ai' && perspective === 'model'
          ? new Set(group.map((row) => row.endpoint_id ?? '(direct)')).size
          : null,
      usage_unit: category === 'mcp' ? 'requests' : null,
    }
  })
}

function trendRows(rows: RequestRecordListRow[], url: URL) {
  const range = queryString(url, 'range') ?? '24h'
  const granularity = range === '24h' || range === 'custom' ? 'hour' : 'day'
  const groups = new Map<string, RequestRecordListRow[]>()
  for (const row of rows) {
    const at = new Date(row.created_at)
    const bucket =
      granularity === 'hour'
        ? new Date(
            Date.UTC(
              at.getUTCFullYear(),
              at.getUTCMonth(),
              at.getUTCDate(),
              at.getUTCHours(),
            ),
          )
        : new Date(
            Date.UTC(at.getUTCFullYear(), at.getUTCMonth(), at.getUTCDate()),
          )
    const key = bucket.toISOString()
    groups.set(key, [...(groups.get(key) ?? []), row])
  }
  const trend = [...groups.entries()]
    .sort(([left], [right]) => right.localeCompare(left))
    .map(([bucketAt, group]) => {
      const errors = group.filter((row) => row.ok === false).length
      return {
        bucket_at: bucketAt,
        error_count: errors,
        error_rate: ratio(errors, group.length),
        p95_first_token_ms: percentile(
          group.map((row) => row.ttft_ms),
          0.95,
        ),
        p95_total_ms: percentile(
          group.map((row) => row.duration_ms),
          0.95,
        ),
        request_count: group.length,
        success_count: group.length - errors,
        success_rate: ratio(group.length - errors, group.length),
        tokens: tokenUsageOf(group),
      }
    })
  return { trend, granularity }
}

/**
 * The overview aggregates the same visible rows the list returns, so a scope,
 * window, category or role change cannot leave a stale aggregate behind.
 */
function overviewFor(
  ctx: FixtureContext,
  category: RequestRecordCategory,
): RequestRecordOverviewResponse {
  const rows = recordsFor(ctx, category)
  const perspective =
    queryString(ctx.url, 'perspective') === 'upstream' ? 'upstream' : 'model'
  const errors = rows.filter((row) => row.ok === false).length
  const methods = new Set(
    rows
      .map((row) => row.mcp_protocol_method)
      .filter((value): value is string => typeof value === 'string'),
  )
  const { trend, granularity } = trendRows(rows, ctx.url)
  return {
    breakdown: breakdownRows(
      rows,
      category,
      category === 'ai' ? perspective : 'model',
    ),
    summary: {
      avg_output_tokens_per_second:
        category === 'ai' ? averageOutputRate(rows) : null,
      error_count: errors,
      method_count: category === 'mcp' ? methods.size : 0,
      p95_first_token_ms: percentile(
        rows.map((row) => row.ttft_ms),
        0.95,
      ),
      p95_total_ms: percentile(
        rows.map((row) => row.duration_ms),
        0.95,
      ),
      request_count: rows.length,
      success_count: rows.length - errors,
      success_rate: ratio(rows.length - errors, rows.length),
      tokens: tokenUsageOf(rows),
    },
    trend,
    trend_granularity: granularity,
  }
}

function facetsFor(ctx: FixtureContext, category: RequestRecordCategory) {
  const rows = recordsFor(ctx, category)
  const dates = [...new Set(rows.map((row) => row.created_at.slice(0, 10)))]
  const users = [
    ...new Set(rows.map((row) => row.user_login_name).filter(Boolean)),
  ] as string[]
  const models = [
    ...new Set(rows.map((row) => row.model).filter(Boolean)),
  ] as string[]
  const states = [...new Set(rows.map((row) => row.request_state))]
  const redactions = [
    ...new Set(rows.map((row) => row.redaction?.applied === true)),
  ]
  const labels = new Set(rows.map((row) => row.client_key_label ?? ''))
  return {
    client_keys: CLIENT_KEYS.filter((key) => labels.has(key.label)).map(
      (key) => ({
        key_id: key.key_id,
        label: key.label,
        user_login_name:
          rows.find((row) => row.client_key_label === key.label)
            ?.user_login_name ?? null,
      }),
    ),
    dates,
    models,
    redactions,
    states,
    users,
  }
}

/** A record the caller may read, or `null` when it is not theirs. */
function visibleRecord(ctx: FixtureContext, recordId: number) {
  const detail = ctx.state.recordDetails.get(recordId)
  if (!detail) return null
  if (ctx.account?.isAdmin) return detail
  if (detail.user_id !== ctx.account?.user.user_id) return null
  return detail
}

function fixtureMessages(recordId: number): Array<{
  block_hash: string
  content_json: unknown
  preview_text: string
  role: string
  same_as_turn: number | null
}> {
  // Record 1 carries more than one page so the cursor/merge contract is real;
  // every other record stays a small, record-specific transcript.
  const total = recordId === 1 ? 12 : 3
  return Array.from({ length: total }, (_, index) => {
    const role = index % 2 === 0 ? 'user' : 'assistant'
    const position = index + 1
    return {
      block_hash: `fixture-${recordId}-${position}`,
      content_json: {
        content: `Fixture message ${position} for record ${recordId}.`,
        role,
      },
      preview_text: `Fixture message ${position}`,
      role,
      same_as_turn: index === 1 ? 1 : null,
    }
  })
}

export async function handleRecords(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  if (/^\/api\/v1\/settings\/redaction(?:\/[^/]+)*$/.test(pathname)) {
    return null
  }
  if (/^\/api\/v1\/admin\/billing(?:\/[^/]+)*$/.test(pathname)) {
    return null
  }

  // Every records surface below is authenticated. The role boundary is per
  // route: the list/facets/overview/detail reads are self-scoped for an
  // ordinary user, while prune, reset and the conversation override stay
  // admin-only.
  if (!ctx.account) return unauthorized()
  const adminOnly = () => requireAdmin(ctx)

  if (RECORDS.test(pathname) && method === 'GET') {
    const rows = sortRecords(
      recordsFor(ctx, categoryOf(ctx)).filter((row) =>
        matchesFilters(row, ctx.url),
      ),
      ctx.url,
    )
    return ok(
      page(
        rows,
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 20),
        'records',
      ),
    )
  }

  if (CLEAR.test(pathname) && method === 'POST') {
    const body = await readJson<RequestRecordsClearRequest>(ctx)
    // The current user may clear their own history; a target or all-user clear
    // is an admin operation.
    if (body.scope !== 'current_user' && !ctx.account.isAdmin) {
      return forbidden()
    }
    if (body.scope === 'target_user' && body.user_id == null) {
      return error('user_required', 'target user is required', 400)
    }
    if (!body.delete_all) {
      const bound = (value: string | null | undefined): number | null => {
        if (!value) return null
        if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/.test(value)) {
          return Number.NaN
        }
        return Date.parse(value)
      }
      const start = bound(body.start_at)
      const end = bound(body.end_at)
      if (Number.isNaN(start) || Number.isNaN(end)) {
        return error('invalid_range', 'bounds must be RFC3339 instants', 400)
      }
      if (start === null && end === null) {
        return error('range_required', 'a range is required', 400)
      }
      if (start !== null && end !== null && start > end) {
        return error('invalid_range', 'start must not be after end', 400)
      }
    }
    const userId =
      body.scope === 'all_users'
        ? null
        : body.scope === 'target_user'
          ? body.user_id
          : ctx.account?.user.user_id
    const keep = (row: RequestRecordListRow) => {
      if (userId != null && row.user_id !== userId) return true
      if (!body.delete_all && body.start_at && row.created_at < body.start_at)
        return true
      if (!body.delete_all && body.end_at && row.created_at > body.end_at)
        return true
      return false
    }
    const before = ctx.state.aiRecords.length + ctx.state.mcpRecords.length
    ctx.state.aiRecords = ctx.state.aiRecords.filter(keep)
    ctx.state.mcpRecords = ctx.state.mcpRecords.filter(keep)
    const removed =
      before - ctx.state.aiRecords.length - ctx.state.mcpRecords.length
    return ok({
      deleted: removed,
      deleted_prompt_blocks: removed,
      protected_by_billing: 1,
    })
  }

  if (PRUNE.test(pathname) && method === 'POST') {
    const adminMissing = adminOnly()
    if (adminMissing) return adminMissing
    return ok({ deleted: 3, protected_by_billing: 2 })
  }

  if (OVERVIEW.test(pathname) && method === 'GET') {
    return ok(overviewFor(ctx, categoryOf(ctx)))
  }

  if (FACETS.test(pathname) && method === 'GET') {
    return ok(facetsFor(ctx, categoryOf(ctx)))
  }

  if (SERIES.test(pathname) && method === 'GET') {
    return ok([
      {
        avg_duration_ms: 2400,
        avg_ttft_ms: 380,
        bucket_at: '2026-10-23T00:00:00.000Z',
        cache_rate: 0.4,
        cached_tokens: 160,
        error_count: 2,
        error_rate: 0.2,
        input_tokens: 1600,
        output_tokens: 520,
        request_count: 10,
        total_tokens: 2120,
      },
    ])
  }

  if (SUMMARY.test(pathname) && method === 'GET') {
    const rows = recordsFor(ctx, categoryOf(ctx))
    const total = rows.reduce((sum, row) => sum + (row.total_tokens ?? 0), 0)
    return ok({
      avg_duration_ms: 2400,
      cache_rate: 0.4,
      cached_tokens: 160,
      error_count: rows.filter((row) => row.ok === false).length,
      input_tokens: 800,
      output_tokens: 260,
      request_count: rows.length,
      success_count: rows.filter((row) => row.ok !== false).length,
      total_tokens: total,
    })
  }

  const fullId = FULL.exec(pathname)?.[1]
  if (fullId && method === 'GET') {
    const recordId = Number.parseInt(fullId, 10)
    const detail = visibleRecord(ctx, recordId)
    if (!detail) return error('not_found', 'fixture record not found', 404)
    if (!detail.has_full_request) {
      return error(
        'full_request_unavailable',
        'fixture record has no stored request',
        404,
      )
    }
    const order = queryString(ctx.url, 'order') === 'asc' ? 'asc' : 'desc'
    const limit = Math.max(1, Math.min(100, queryInt(ctx.url, 'limit', 10)))
    const cursor = queryString(ctx.url, 'cursor')
    // A parseable cursor wins over the offset, matching the backend contract.
    const cursorOffset = cursor && /^\d+$/.test(cursor) ? Number(cursor) : null
    const offset = Math.max(0, cursorOffset ?? queryInt(ctx.url, 'offset', 0))
    const all = fixtureMessages(recordId)
    if (order === 'desc') all.reverse()
    const messages = all.slice(offset, offset + limit)
    const hasMore = offset + limit < all.length
    const response: RequestRecordFullResponse = {
      client_installation_id: detail.client_installation_id ?? null,
      conversation_id: detail.conversation_id ?? null,
      conversation_source: detail.conversation_source,
      has_more: hasMore,
      limit,
      messages,
      next_cursor: hasMore ? String(offset + limit) : null,
      normalized_item_count: detail.normalized_item_count ?? null,
      offset,
      order,
      record_id: recordId,
      rendered_text: []
        .concat(all.map((message) => message.preview_text))
        .filter((text) => text.length > 0)
        .join('\n'),
      request_has_previous_response_id: false,
      request_previous_response_id: null,
      request_previous_response_parent_found: null,
      request_raw_json: detail.request_raw_json,
      request_storage_mode: detail.request_storage_mode,
      session_header_id: detail.session_header_id ?? null,
      session_parent_id: detail.session_parent_id ?? null,
      total_messages: all.length,
    }
    return ok(response)
  }

  const resetId = RESET_AFFINITY.exec(pathname)?.[1]
  if (resetId && method === 'POST') {
    const adminMissing = adminOnly()
    if (adminMissing) return adminMissing
    const recordId = Number.parseInt(resetId, 10)
    const detail = visibleRecord(ctx, recordId)
    if (!detail) return error('not_found', 'fixture record not found', 404)
    if (!detail.conversation_id) {
      return error('no_conversation_id', 'record has no conversation', 400)
    }
    const route = ctx.state.recordRouteOptions.get(recordId)
    if (route?.affinityState === 'unbound') {
      return ok({ cleared: false, cleared_count: 0 })
    }
    return ok({ cleared: true, cleared_count: 1 })
  }

  const optionsId = ROUTE_OPTIONS.exec(pathname)?.[1]
  if (optionsId && method === 'GET') {
    const recordId = Number.parseInt(optionsId, 10)
    const detail = visibleRecord(ctx, recordId)
    if (!detail) return error('not_found', 'fixture record not found', 404)
    if (!detail.conversation_id) {
      return error('no_conversation_id', 'record has no conversation', 400)
    }
    const conversationId = detail.conversation_id
    const route = ctx.state.recordRouteOptions.get(recordId)
    const storedOverride = ctx.state.conversationOverrides.get(conversationId)
    const overrideEndpointId =
      storedOverride?.endpoint_id ?? route?.overrideEndpointId ?? null
    const overrideEndpointKeyId =
      storedOverride?.endpoint_key_id ?? route?.overrideEndpointKeyId ?? null
    const overrideEndpointKeyLabel =
      storedOverride?.endpoint_key_label ??
      route?.overrideEndpointKeyLabel ??
      null
    const affinityState = route?.affinityState ?? 'active'
    const affinityEndpointId =
      affinityState === 'unbound'
        ? null
        : (route?.affinityEndpointId ?? detail.endpoint_id ?? null)
    const affinityEndpoint = ctx.state.endpoints.find(
      (endpoint) => endpoint.endpoint_id === affinityEndpointId,
    )
    const affinityKeyId =
      affinityState === 'unbound'
        ? null
        : affinityState === 'stale_key'
          ? 'key-retired'
          : (detail.endpoint_key_id ?? null)
    const affinityKeyLabel =
      affinityState === 'unbound'
        ? null
        : affinityState === 'stale_key'
          ? 'retired key'
          : (detail.endpoint_key_label ?? null)
    const response: SessionRouteOptionsResponse = {
      affinity: {
        endpoint_id: affinityEndpointId,
        endpoint_name: affinityEndpoint?.name ?? null,
        key_id: affinityKeyId,
        key_label: affinityKeyLabel,
        rule_id: affinityState === 'unbound' ? null : 'rule-gpt-codex',
        state: affinityState,
      },
      conversation_id: conversationId,
      current_endpoint_id: detail.endpoint_id ?? null,
      current_endpoint_key_id: detail.endpoint_key_id ?? null,
      current_endpoint_key_label: detail.endpoint_key_label ?? null,
      options: ctx.state.endpoints.map((endpoint) => ({
        endpoint_id: endpoint.endpoint_id,
        endpoint_name: endpoint.name,
        is_override: overrideEndpointId === endpoint.endpoint_id,
        is_preferred: endpoint.endpoint_id === detail.endpoint_id,
        keys: (endpoint.api_keys ?? []).map((key) => ({
          key_id: key.key_id,
          key_label: key.key_label,
        })),
      })),
      override_endpoint_id: overrideEndpointId,
      override_endpoint_key_id: overrideEndpointKeyId,
      override_endpoint_key_label: overrideEndpointKeyLabel,
    }
    return ok(response)
  }

  const detailId = DETAIL.exec(pathname)?.[1]
  if (detailId && method === 'GET') {
    const recordId = Number.parseInt(detailId, 10)
    const detail = visibleRecord(ctx, recordId)
    if (!detail) return error('not_found', 'fixture record not found', 404)
    return ok(detail)
  }

  const conversationId = OVERRIDE.exec(pathname)?.[1]
  if (conversationId) {
    const adminMissing = adminOnly()
    if (adminMissing) return adminMissing
    const endpoint = ctx.state.endpoints[0]
    if (method === 'GET') {
      const override = ctx.state.conversationOverrides.get(conversationId)
      if (!override)
        return error('not_found', 'no fixture override stored', 404)
      return ok(override)
    }
    if (method === 'PUT') {
      const body = await readJson<ConversationEndpointOverrideRequest>(ctx)
      const target = ctx.state.endpoints.find(
        (candidate) => candidate.endpoint_id === body.endpoint_id,
      )
      if (!target) {
        return error('invalid_endpoint', 'fixture endpoint not found', 400)
      }
      const now = isoNow()
      const key =
        body.endpoint_key_id == null
          ? undefined
          : (target.api_keys ?? []).find(
              (candidate) => candidate.key_id === body.endpoint_key_id,
            )
      if (body.endpoint_key_id != null && (!key || key.enabled === false)) {
        return error(
          'invalid_endpoint_key',
          'fixture key does not belong to the endpoint or is disabled',
          400,
        )
      }
      const override: ConversationEndpointOverride = {
        conversation_id: conversationId,
        created_at:
          ctx.state.conversationOverrides.get(conversationId)?.created_at ??
          now,
        created_by_user_id: 1,
        endpoint_id: target.endpoint_id,
        endpoint_key_id: key?.key_id ?? null,
        endpoint_key_label: key?.key_label ?? null,
        endpoint_name: target.name || endpoint.name,
        updated_at: now,
      }
      ctx.state.conversationOverrides.set(conversationId, override)
      return ok(override)
    }
    if (method === 'DELETE') {
      ctx.state.conversationOverrides.delete(conversationId)
      return new Response(null, { status: 204 })
    }
  }

  return null
}

export { nextId }
