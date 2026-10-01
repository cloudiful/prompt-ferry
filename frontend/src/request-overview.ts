import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewResponse,
  RequestRecordOverviewRange,
} from './generated/admin-api'

export type { RequestRecordOverviewResponse, RequestRecordOverviewRange }

export type RequestOverviewMode = 'overview' | 'records'

/**
 * Top-level grouping for the AI distribution table. `model` keeps the
 * historical model-first rows with an upstream hover; `upstream` groups by
 * endpoint identity and shows a per-effective-model hover.
 */
export type RequestOverviewPerspective = 'model' | 'upstream'

export const DEFAULT_REQUEST_OVERVIEW_PERSPECTIVE: RequestOverviewPerspective =
  'model'

export type RequestOverviewDrilldown = {
  endpoint_id?: string | null
  model?: string | null
  mcp_server_id?: string | null
  mcp_bearer_token_slot?: number | null
}

/**
 * Map a clicked distribution row to the record filter it should open.
 *
 * The request category decides the filter axis first so the MCP distribution is
 * unaffected by the AI perspective: MCP rows always filter by `mcp_server_id`,
 * and AI rows branch on the perspective. An upstream `(direct)` row has no
 * endpoint identity, so it returns `null` and opens no misleading unfiltered
 * record list.
 */
export function breakdownDrilldownForPerspective(
  perspective: RequestOverviewPerspective,
  category: 'ai' | 'mcp',
  row: RequestRecordOverviewBreakdownRow,
): RequestOverviewDrilldown | null {
  if (category === 'mcp') {
    return { model: null, mcp_server_id: row.mcp_server_id ?? null }
  }
  if (perspective === 'upstream') {
    return row.endpoint_id ? { endpoint_id: row.endpoint_id } : null
  }
  return { model: row.model ?? null, mcp_server_id: null }
}
