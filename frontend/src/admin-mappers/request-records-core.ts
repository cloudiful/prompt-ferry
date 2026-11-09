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

/** Displays the request-time override, with the caller value when it differs. */
export function deriveReasoningEffortDisplay(
  original: string | null | undefined,
  appliedOverride: string | null | undefined,
  path: string | null | undefined,
): string | null {
  const callerValue = (original ?? '').trim()
  const supportsOverride = [
    '/v1/chat/completions',
    '/v1/responses',
    '/v1/responses/compact',
    '/v1/messages',
  ].includes(path ?? '')
  const override = supportsOverride ? (appliedOverride ?? '').trim() : ''
  if (!override) return callerValue || null
  if (!callerValue || override === callerValue) return override
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
