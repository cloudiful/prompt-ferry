import type { RequestRecordCategory } from '../generated/admin-api'
import { client, expectData, withData } from '../api'
import type { RequestOverviewResponse } from '../request-overview'

type RequestOverviewRange = '24h' | '7d' | '30d' | 'custom'

export async function fetchRequestOverview(input: {
  requestCategory: RequestRecordCategory
  range: RequestOverviewRange
  start?: string
  end?: string
}): Promise<RequestOverviewResponse> {
  return expectData(
    await client.get(
      withData({
        url: '/api/v1/admin/request-records/overview',
        query: {
          request_category: input.requestCategory,
          range: input.range,
          start: input.start || undefined,
          end: input.end || undefined,
        },
      } as any),
    ),
  ) as RequestOverviewResponse
}
