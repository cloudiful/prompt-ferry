import {
  requestRecordOverview,
  type RequestRecordCategory,
  type RequestRecordOverviewRange,
  type RequestRecordOverviewResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

export async function fetchRequestOverview(input: {
  requestCategory: RequestRecordCategory
  range: RequestRecordOverviewRange
  start?: string
  end?: string
}): Promise<RequestRecordOverviewResponse> {
  return expectData(
    await requestRecordOverview<true>(
      withData({
        query: {
          request_category: input.requestCategory,
          range: input.range,
          start: input.start || undefined,
          end: input.end || undefined,
        },
      }),
    ),
  )
}
