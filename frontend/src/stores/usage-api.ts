import {
  clearRequestRecords,
  listRequestRecords,
  pruneRequestRecords,
  requestRecordFacets,
} from '../generated/admin-api'
import type {
  RequestRecordCategory,
  RequestRecordFacets,
  RequestRecordOverviewRange,
  RequestRecordsClearResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createRequestRecordRowView } from '../admin-mappers'
import { fetchRequestOverview } from '../api/request-overview'
import type {
  RequestRecordClearForm,
  RequestRecordFilterModel,
  RequestRecordRowView,
} from '../models'
import type { RequestRecordOverviewResponse } from '../generated/admin-api'
import { buildRequestRecordListQuery } from './request-records-query'

export async function fetchUsageOverview(input: {
  requestCategory: RequestRecordCategory
  range: RequestRecordOverviewRange
  start: string
  end: string
}): Promise<RequestRecordOverviewResponse | null> {
  return fetchRequestOverview({
    requestCategory: input.requestCategory,
    range: input.range,
    start: input.start || undefined,
    end: input.end || undefined,
  })
}

export async function fetchUsageRecords(input: {
  filters: RequestRecordFilterModel
  first: number
  rows: number
  requestCategory: RequestRecordCategory
  sortField: string
  sortOrder: -1 | 0 | 1
}): Promise<{
  rows: RequestRecordRowView[]
  total: number
  first: number
  rowsPerPage: number
}> {
  const page = expectData(
    await listRequestRecords<true>(
      withData({
        query: buildRequestRecordListQuery(input) as any,
      }),
    ),
  )
  return {
    rows: page.records.map(createRequestRecordRowView),
    total: page.total,
    first: page.first,
    rowsPerPage: page.rows,
  }
}

export async function fetchUsageFacets(
  requestCategory: RequestRecordCategory,
): Promise<RequestRecordFacets> {
  return expectData(
    await requestRecordFacets<true>(
      withData({
        query: { request_category: requestCategory },
      }),
    ),
  )
}

export async function clearUsageHistory(
  form: RequestRecordClearForm,
): Promise<RequestRecordsClearResponse> {
  return expectData(
    await clearRequestRecords<true>(
      withData({
        body: {
          scope: form.scope,
          user_id: form.user_id,
          start_at: form.delete_all ? null : form.start_at || null,
          end_at: form.delete_all ? null : form.end_at || null,
          delete_all: form.delete_all,
        },
      }),
    ),
  )
}

export async function pruneUsageHistory(): Promise<number> {
  const result = expectData(await pruneRequestRecords<true>(withData()))
  return result.deleted
}
