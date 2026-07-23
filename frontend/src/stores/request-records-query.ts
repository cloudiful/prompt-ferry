import type { RequestRecordCategory } from '../generated/admin-api'
import type {
  RequestRecordBucketGranularity,
  RequestRecordFilterModel,
} from '../models'

export function createDefaultRequestRecordFilters(): RequestRecordFilterModel {
  return {
    global: { value: null, matchMode: 'contains' },
    request_date: { value: null, matchMode: 'equals' },
    user_key: { value: null, matchMode: 'equals' },
    model_key: { value: null, matchMode: 'equals' },
    request_state: { value: null, matchMode: 'equals' },
    redaction_applied: { value: null, matchMode: 'equals' },
    endpoint_id: { value: null, matchMode: 'equals' },
    mcp_server_id: { value: null, matchMode: 'equals' },
    mcp_bearer_token_slot: { value: null, matchMode: 'equals' },
  }
}

export function buildRequestRecordListQuery(input: {
  filters: RequestRecordFilterModel
  first: number
  rows: number
  requestCategory: RequestRecordCategory
  sortField: string
  sortOrder: -1 | 0 | 1
}): Record<string, string | number | boolean | undefined> {
  const { filters, first, rows, requestCategory, sortField, sortOrder } = input
  return {
    first,
    rows,
    request_category: requestCategory,
    sort_field: sortField,
    sort_order: sortOrder,
    search: filters.global.value ?? undefined,
    date: filters.request_date.value ?? undefined,
    user: filters.user_key.value ?? undefined,
    model: filters.model_key.value ?? undefined,
    endpoint_id: filters.endpoint_id?.value ?? undefined,
    mcp_server_id: filters.mcp_server_id?.value ?? undefined,
    mcp_bearer_token_slot: filters.mcp_bearer_token_slot?.value ?? undefined,
    request_state: filters.request_state.value ?? undefined,
    redaction_applied: filters.redaction_applied.value ?? undefined,
  }
}

export function resolveRequestRecordSeriesWindow(
  range: string,
  start: string,
  end: string,
): { start?: string; end?: string; limit: number } {
  if (range === 'custom') {
    return {
      start: start || undefined,
      end: end || undefined,
      limit: 200,
    }
  }
  if (range === '30d') {
    return { limit: 30 * 24 }
  }
  if (range === '7d') {
    return { limit: 7 * 24 }
  }
  return { limit: 24 }
}

export function createRequestRecordSummaryDays(range: string): number {
  if (range === '30d') return 30
  if (range === '7d') return 7
  return 1
}

export function createRequestRecordSeriesQuery(input: {
  bucket: RequestRecordBucketGranularity
  range: string
  start: string
  end: string
}): {
  bucket: RequestRecordBucketGranularity
  start?: string
  end?: string
  limit: number
} {
  const window = resolveRequestRecordSeriesWindow(
    input.range,
    input.start,
    input.end,
  )
  return {
    bucket: input.bucket,
    end: window.end,
    limit: window.limit,
    start: window.start,
  }
}
