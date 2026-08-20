import type {
  RequestRecordOverviewResponse,
  RequestRecordOverviewRange,
} from './generated/admin-api'

export type { RequestRecordOverviewResponse, RequestRecordOverviewRange }

export type RequestOverviewMode = 'overview' | 'records'

export type RequestOverviewDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
}
