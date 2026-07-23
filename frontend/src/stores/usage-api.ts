import {
  clearRequestRecords,
  listRequestRecords,
  pruneRequestRecords,
  requestRecordFacets,
  requestRecordSeries,
  requestRecordSummary,
} from '../generated/admin-api'
import type {
  RequestRecordBucket,
  RequestRecordCategory,
  RequestRecordFacets,
  RequestRecordSummary,
  RequestRecordsClearResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createRequestRecordRowView } from '../admin-mappers'
import { fetchRequestOverview } from '../api/request-overview'
import type {
  RequestRecordBucketGranularity,
  RequestRecordClearForm,
  RequestRecordFilterModel,
  RequestRecordRowView,
} from '../models'
import type { RequestOverviewResponse } from '../request-overview'
import {
  buildRequestRecordListQuery,
  createRequestRecordSeriesQuery,
  createRequestRecordSummaryDays,
} from './request-records-query'

export async function fetchUsageOverview(input: {
  requestCategory: RequestRecordCategory
  range: '24h' | '7d' | '30d' | 'custom'
  start: string
  end: string
}): Promise<RequestOverviewResponse | null> {
  return fetchRequestOverview({
    requestCategory: input.requestCategory,
    range: input.range,
    start: input.start || undefined,
    end: input.end || undefined,
  })
}

export async function fetchUsageSummary(
  range: string,
): Promise<RequestRecordSummary> {
  const days = createRequestRecordSummaryDays(range)
  return expectData(
    await requestRecordSummary<true>(withData({ query: { days } })),
  )
}

export async function fetchUsageSeries(input: {
  bucket: RequestRecordBucketGranularity
  requestCategory?: RequestRecordCategory
  range: string
  start: string
  end: string
}): Promise<RequestRecordBucket[]> {
  return expectData(
    await requestRecordSeries<true>(
      withData({
        query: createRequestRecordSeriesQuery(input),
      }),
    ),
  )
}

export async function fetchUsageRecords(input: {
  filters: RequestRecordFilterModel
  first: number
  rows: number
  requestCategory: RequestRecordCategory
  sortField: string
  sortOrder: -1 | 0 | 1
}): Promise<{ rows: RequestRecordRowView[]; total: number }> {
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
