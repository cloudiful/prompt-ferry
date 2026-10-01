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
 * The aggregate fields both detail shapes carry: `upstream_breakdown` (inside a
 * model row) and `model_breakdown` (inside an upstream row).
 */
export type RequestOverviewDetailEntry = {
  error_rate?: number | null
  request_count: number
  total_tokens: number
}

/**
 * Issue #34 P3: a detail row whose requests all failed or aborted and that
 * produced no tokens at all is noise in the model/upstream distribution, so it
 * is collapsed by default. A row that carries real tokens, a successful empty
 * response, or an in-flight request keeps `error_rate < 1` and stays visible;
 * totals, error counts, and the record list are untouched.
 */
export function isFailedOnlyZeroTokenEntry(
  entry: RequestOverviewDetailEntry,
): boolean {
  return entry.total_tokens === 0 && entry.error_rate === 1
}

/**
 * Split a hover detail list into the rows worth showing by default and the
 * fully failed zero-token rows, plus the request count those hidden rows stand
 * for so the detail can still report what it folded away.
 */
export function splitFailedOnlyDetailEntries<
  T extends RequestOverviewDetailEntry,
>(
  entries: readonly T[],
): {
  failedOnly: T[]
  failedRequestCount: number
  visible: T[]
} {
  const failedOnly: T[] = []
  const visible: T[] = []
  let failedRequestCount = 0
  for (const entry of entries ?? []) {
    if (isFailedOnlyZeroTokenEntry(entry)) {
      failedOnly.push(entry)
      failedRequestCount += entry.request_count ?? 0
      continue
    }
    visible.push(entry)
  }
  return { failedOnly, failedRequestCount, visible }
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
