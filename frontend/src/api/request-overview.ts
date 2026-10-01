import {
  requestRecordOverview,
  type RequestRecordCategory,
  type RequestRecordOverviewRange,
  type RequestRecordOverviewResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import type { RequestOverviewPerspective } from '../request-overview'

export async function fetchRequestOverview(input: {
  requestCategory: RequestRecordCategory
  perspective: RequestOverviewPerspective
  range: RequestRecordOverviewRange
  start?: string
  end?: string
}): Promise<RequestRecordOverviewResponse> {
  return expectData(
    await requestRecordOverview<true>(
      withData({
        query: {
          request_category: input.requestCategory,
          perspective:
            input.requestCategory === 'ai' ? input.perspective : 'model',
          range: input.range,
          start: input.start || undefined,
          end: input.end || undefined,
        },
      }),
    ),
  )
}
