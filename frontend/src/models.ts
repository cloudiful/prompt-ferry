import type {
  ApprovalStatusFilter,
  ConversationEndpointOverride,
  RequestRecordCategory,
  RequestRecordDetail,
  RequestRecordListRow,
  RequestRecordState,
  SessionRouteOptionsResponse,
  UsageClearScope,
} from './generated/admin-api'

export type NewUserForm = {
  login_name: string
  password: string
  display_name: string
  is_admin: boolean
}

export type EndpointForm = {
  endpoint_id: string
  scope: 'admin' | 'user'
  owner_user_id: number | null
  name: string
  base_url: string
  api_key: string
  primary_api_key_saved: boolean
  api_keys: EndpointApiKeyForm[]
  key_lb_enabled: boolean
  protocol_mode: 'auto' | 'manual'
  native_api_override:
    'anthropic_messages' | 'responses' | 'chat' | 'realtime' | null
  daily_max_requests: number | null
  monthly_max_requests: number | null
  enabled: boolean
}

export type EndpointApiKeyForm = {
  key_label: string
  api_key: string
  has_saved_key: boolean
  enabled: boolean
}

export type ModelRouteTargetForm = {
  endpoint_id: string
  enabled: boolean
  upstream_model: string
  responses_continuation_policy: 'force_passthrough' | 'force_replay'
}

export type StreamDeltaBatchingForm = {
  enabled: boolean
  flush_window_ms: number
  max_buffer_chars: number
  max_buffer_bytes: number
  flush_on_line_break: boolean
  flush_on_sentence_end: boolean
}

export type ModelRouteForm = {
  rule_id: string
  scope: 'admin' | 'user'
  owner_user_id: number | null
  model_pattern: string
  routing_strategy: 'client_key_rendezvous' | 'responses_session_affinity'
  session_affinity_lock_after_turns: number
  daily_max_requests: number | null
  monthly_max_requests: number | null
  enabled: boolean
  targets: ModelRouteTargetForm[]
}

export type McpForm = {
  server_id: string
  scope: 'admin' | 'user'
  owner_user_id: number | null
  name: string
  aggregate_naming_mode: 'qualified_only' | 'passthrough_preferred'
  transport: 'http' | 'stdio'
  url: string
  command: string
  args_text: string
  bearer_tokens: string[]
  http_headers_text: string
  env_text: string
  tool_filter_mode: 'blacklist' | 'whitelist'
  allowed_tools: string[]
  disabled_tools: string[]
  disabled_resources: string[]
  daily_max_requests: number | null
  monthly_max_requests: number | null
  enabled: boolean
  timeout_ms: number
}

export type RequestRecordClearForm = {
  scope: UsageClearScope
  user_id: number | null
  start_at: string
  end_at: string
  delete_all: boolean
}

export type NavigationSection =
  | 'api-keys'
  | 'available-models'
  | 'users'
  | 'endpoints'
  | 'mcp'
  | 'relays'
  | 'redaction'
  | 'request-records'
  | 'approvals'
  | 'settings'

export type RequestRecordFilterModel = {
  global: { value: string | null; matchMode: string }
  request_date: { value: string | null; matchMode: string }
  user_key: { value: string | null; matchMode: string }
  model_key: { value: string | null; matchMode: string }
  request_state: { value: RequestRecordState | null; matchMode: string }
  redaction_applied: { value: boolean | null; matchMode: string }
  endpoint_id?: { value: string | null; matchMode: string }
  mcp_server_id?: { value: string | null; matchMode: string }
  mcp_bearer_token_slot?: { value: number | null; matchMode: string }
}

export type ApprovalFilter = ApprovalStatusFilter
export type RequestRecordBucketGranularity = 'minute' | 'hour' | 'day'
export type RequestRecordCategoryTab = RequestRecordCategory

export type Option<T = string> = {
  label: string
  value: T
}

export type RequestRecordRowView = RequestRecordListRow & {
  actor: string
  request_date: string
  upstream_label: string
  is_session_recognized: boolean
  is_first_turn: boolean
  target: string
  model_key: string
  session_short_id: string
  session_state: 'recognized' | 'unrecognized'
  user_key: string
}

export type RequestRecordDetailView = RequestRecordDetail & {
  actor: string
  client_installation_short?: string | null
  conversation_source_label: string
  request_date: string
  upstream_label: string
  is_session_recognized: boolean
  is_first_turn: boolean
  target: string
  model_key: string
  request_user_agent?: string | null
  session_short_id: string
  session_state: 'recognized' | 'unrecognized'
  user_key: string
}

export type SessionRouteOptionsView = SessionRouteOptionsResponse & {
  current_upstream_label: string
}

export type ConversationEndpointOverrideView = ConversationEndpointOverride

export type {
  McpServerListItemView,
  McpServerWorkspaceView,
} from './models/mcp'

export type {
  UsageDetailWorkspaceView,
  UsageFacetOptionsView,
  UsageWorkspaceView,
} from './models/usage'

export type {
  EndpointListItemView,
  EndpointsWorkspaceView,
  ModelRouteListItemView,
} from './models/endpoints'

export type { UserListItemView, UsersWorkspaceView } from './models/users'

export type { ApiKeyItemView, ApiKeysWorkspaceView } from './models/api-keys'

export type { SettingsWorkspaceView } from './models/settings'

export type {
  RedactionOptionView,
  RedactionPreviewStatView,
  RedactionRuleOptionView,
  RedactionWorkspaceView,
} from './models/redaction'
