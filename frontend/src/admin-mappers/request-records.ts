import type {
  ConversationEndpointOverride,
  RequestRecordDetail,
  RequestRecordListRow,
  SessionRouteOptionsResponse,
} from '../generated/admin-api'
import type {
  ConversationEndpointOverrideView,
  RequestRecordDetailView,
  RequestRecordRowView,
  SessionRouteOptionsView,
} from '../models'

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

export function upstreamLabel(row: {
  mcp_server_name?: string | null;
  endpoint_name?: string | null;
  endpoint_id?: string | null;
  upstream_model?: string | null;
  path?: string | null;
}): string {
  return row.mcp_server_name || row.endpoint_name || row.endpoint_id || row.path || "-";
}

export function createRequestRecordRowView(
  record: RequestRecordListRow,
): RequestRecordRowView {
  const label = upstreamLabel(record);
  const sessionRecognized = Boolean(record.conversation_id)
  const firstTurn = sessionRecognized && (record.conversation_seq ?? 1) <= 1
  return {
    ...record,
    actor: record.user_login_name || '-',
    is_first_turn: firstTurn,
    is_session_recognized: sessionRecognized,
    model_key: record.model || '-',
    model_display: deriveModelDisplay(record),
    request_date: record.created_at.slice(0, 10),
    session_state: sessionRecognized ? 'recognized' : 'unrecognized',
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: label,
    upstream_label: label,
    user_key: record.user_login_name || '-',
  }
}

export function createRequestRecordDetailView(
  record: RequestRecordDetail,
): RequestRecordDetailView {
  const recordWithUserAgent = record as RequestRecordDetail & {
    request_user_agent?: string | null
  }
  const label = upstreamLabel(record);
  const sessionRecognized = Boolean(record.conversation_id)
  const firstTurn = sessionRecognized && (record.conversation_seq ?? 1) <= 1
  const installationShort = record.client_installation_id
    ? `${record.client_installation_id.slice(0, 8)}...${record.client_installation_id.slice(-6)}`
    : null
  return {
    ...record,
    actor: record.user_login_name || '-',
    client_installation_short: installationShort,
    conversation_source_label: record.conversation_source || 'none',
    is_first_turn: firstTurn,
    is_session_recognized: sessionRecognized,
    model_key: record.model || '-',
    model_display: deriveModelDisplay(record),
    request_date: record.created_at.slice(0, 10),
    request_user_agent: recordWithUserAgent.request_user_agent ?? null,
    session_state: sessionRecognized ? 'recognized' : 'unrecognized',
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: label,
    upstream_label: label,
    user_key: record.user_login_name || '-',
  }
}

export function createSessionRouteOptionsView(
  response: SessionRouteOptionsResponse,
): SessionRouteOptionsView {
  const current = response.options.find(
    (option) => option.endpoint_id === response.current_endpoint_id,
  )
  const fallback = response.options.find((option) => option.is_preferred)
  return {
    ...response,
    current_upstream_label:
      current?.endpoint_name ||
      fallback?.endpoint_name ||
      response.current_endpoint_id ||
      '-',
  }
}

export function createConversationEndpointOverrideView(
  overrideEntry: ConversationEndpointOverride,
): ConversationEndpointOverrideView {
  return overrideEntry
}
