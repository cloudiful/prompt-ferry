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

export function createRequestRecordRowView(
  record: RequestRecordListRow,
): RequestRecordRowView {
  const upstreamLabel =
    record.mcp_server_name ||
    record.endpoint_name ||
    record.endpoint_id ||
    record.path
  const sessionRecognized = Boolean(record.conversation_id)
  const firstTurn = sessionRecognized && (record.conversation_seq ?? 1) <= 1
  return {
    ...record,
    actor: record.user_login_name || '-',
    is_first_turn: firstTurn,
    is_session_recognized: sessionRecognized,
    model_key: record.model || '-',
    request_date: record.created_at.slice(0, 10),
    session_state: sessionRecognized ? 'recognized' : 'unrecognized',
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: upstreamLabel,
    upstream_label: upstreamLabel,
    user_key: record.user_login_name || '-',
  }
}

export function createRequestRecordDetailView(
  record: RequestRecordDetail,
): RequestRecordDetailView {
  const recordWithUserAgent = record as RequestRecordDetail & {
    request_user_agent?: string | null
  }
  const upstreamLabel =
    record.mcp_server_name ||
    record.endpoint_name ||
    record.endpoint_id ||
    record.path
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
    request_date: record.created_at.slice(0, 10),
    request_user_agent: recordWithUserAgent.request_user_agent ?? null,
    session_state: sessionRecognized ? 'recognized' : 'unrecognized',
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: upstreamLabel,
    upstream_label: upstreamLabel,
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
