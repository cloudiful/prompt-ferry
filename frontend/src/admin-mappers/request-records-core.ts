/**
 * The pure display helpers of a request record.
 *
 * These are shared by the Vue mapper and the React row view, so they stay free
 * of any locale, time-zone or component dependency.
 */

export function deriveModelDisplay(record: {
  model?: string | null
  requested_model?: string | null
  upstream_model?: string | null
}): string {
  const requested = record.requested_model || record.model || '-'
  const upstream = (record.upstream_model || '').trim()
  if (upstream && upstream !== requested) {
    return `${requested} (${upstream})`
  }
  return requested
}

/**
 * Issue #546: render the reasoning effort the upstream actually received.
 * The route target override is a request-time snapshot, so a later target
 * edit never rewrites the shown value. The arrow marks a real override:
 * missing caller value, empty/equal override, and Anthropic `/v1/messages`
 * (never overridden) all keep the single value.
 */
export function deriveReasoningEffortDisplay(
  original: string | null | undefined,
  appliedOverride: string | null | undefined,
  path: string | null | undefined,
): string | null {
  const callerValue = (original ?? '').trim()
  if (!callerValue) return null
  const override = (appliedOverride ?? '').trim()
  if (!override || override === callerValue || path === '/v1/messages') {
    return callerValue
  }
  return `${callerValue} → ${override}`
}

export function upstreamLabel(row: {
  mcp_server_name?: string | null
  endpoint_name?: string | null
  endpoint_id?: string | null
  upstream_model?: string | null
  path?: string | null
}): string {
  return (
    row.mcp_server_name ||
    row.endpoint_name ||
    row.endpoint_id ||
    row.path ||
    '-'
  )
}
