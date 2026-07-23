import type { RequestRecordCategory } from '../generated/admin-api'
import type { RequestRecordFilterModel } from '../models'

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
