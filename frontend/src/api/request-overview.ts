import {
  requestRecordOverview,
  type RequestRecordCategory,
  type RequestRecordOverviewRange,
  type RequestRecordOverviewResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import type { RequestOverviewPerspective } from '../request-overview'
import type { RequestRecordMcpScope } from '../models/usage'

export async function fetchRequestOverview(input: {
  requestCategory: RequestRecordCategory
  perspective: RequestOverviewPerspective
  range: RequestRecordOverviewRange
  start?: string
  end?: string
  mcpScope?: RequestRecordMcpScope
}): Promise<RequestRecordOverviewResponse> {
  return expectData(
    await requestRecordOverview<true>(
      withData({
        query: {
          request_category: input.requestCategory,
          perspective:
            input.requestCategory === 'ai' ? input.perspective : 'model',
          mcp_scope: input.mcpScope,
          range: input.range,
          start: input.start || undefined,
          end: input.end || undefined,
        },
      }),
    ),
  )
}
