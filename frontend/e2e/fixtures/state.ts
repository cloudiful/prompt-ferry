import type {
  ApprovalRequest,
  BillingChargeResponse,
  BillingPriceRuleResponse,
  BridgeStatus,
  CacheAlertSettingsResponse,
  ClientKey,
  ConfigAuditEntry,
  ConversationEndpointOverride,
  EndpointSettingResponse,
  HostRole,
  LlmReviewSettings,
  ManagedRelay,
  McpProviderDescriptor,
  McpServer,
  MeResponse,
  ModelEndpointRule,
  ModelRouteWhitelistResponse,
  ProviderEndpoint,
  RawObjectStoreSettingsResponse,
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
  RelayConfigView,
  RelayHostResponse,
  RelayStatusResponse,
  RequestContentLoggingResponse,
  RequestRecordDetail,
  RequestRecordListRow,
  SessionAffinityState,
  StreamDeltaBatchingSettings,
  UsageRetentionSettings,
  User,
} from '../../src/generated/admin-api'

export type FixtureRecordRouteOptions = {
  affinityState?: SessionAffinityState
  affinityEndpointId?: string | null
  overrideEndpointId?: string | null
  overrideEndpointKeyId?: string | null
  overrideEndpointKeyLabel?: string | null
}

export type FixtureAccount = {
  user: User
  password: string
  isAdmin: boolean
}

export type FixtureState = {
  quotaStale?: boolean
  accounts: FixtureAccount[]
  sessionToken: string
  relayToken: string
  availableModels: Array<{ id: string; name: string }>
  clientKeys: ClientKey[]
  endpoints: ProviderEndpoint[]
  modelRoutes: ModelEndpointRule[]
  mcpProviders: McpProviderDescriptor[]
  mcpServers: McpServer[]
  relays: ManagedRelay[]
  bridge: BridgeStatus
  approvals: ApprovalRequest[]
  billingCharges: BillingChargeResponse[]
  billingPriceRules: BillingPriceRuleResponse[]
  billingSummary: {
    by_client_key: Array<FixtureBreakdown>
    by_model: Array<FixtureBreakdown>
    currency: string
    customer_amount: string
    known_count: number
    priced_count: number
    request_count: number
    unknown_count: number
    unpriced_count: number
  }
  aiRecords: RequestRecordListRow[]
  mcpRecords: RequestRecordListRow[]
  aiOverview: ReturnType<typeof buildOverview>
  mcpOverview: ReturnType<typeof buildOverview>
  recordDetails: Map<number, RequestRecordDetail>
  conversationOverrides: Map<string, ConversationEndpointOverride>
  recordRouteOptions: Map<number, FixtureRecordRouteOptions>
  settings: {
    requestContentLogging: RequestContentLoggingResponse
    usageRetention: UsageRetentionSettings
    streamDeltaBatching: StreamDeltaBatchingSettings
    relayIpWhitelist: { allowed_cidrs: string[]; trusted_proxy_cidrs: string[] }
    modelRouteWhitelist: ModelRouteWhitelistResponse
    llmReview: LlmReviewSettings
    endpointSetting: EndpointSettingResponse
    rawObjectStore: RawObjectStoreSettingsResponse
    cacheAlert: CacheAlertSettingsResponse
  }
  redaction: RedactionConfigSchema
  redactionByUser: Record<number, RedactionConfigSchema>
  redactionCustomStrings: RedactionCustomStringRuleRowSchema[]
  configAudit: ConfigAuditEntry[]
  /**
   * The next `created_at` the archive audit rows may use, in epoch
   * milliseconds. Each write takes the current value and adds a second, so new
   * rows are strictly newer than the seed and than each other, and the
   * newest-first order never depends on the wall clock.
   */
  configAuditClockMs: number
  relayHost: RelayHostResponse
  relayStatus: RelayStatusResponse
  relayConfig: RelayConfigView
  relayRunningRole: HostRole
  relaySavedRole: HostRole | null
  relaySavedAdminBind: string | null
  /**
   * The stored S3 credentials. Synthetic values only, kept out of the public
   * settings response: the API reports presence, never the secret itself. No
   * fixture control endpoint serialises this state.
   */
  privateStorageSecrets: {
    s3_access_key: string | null
    s3_secret_key: string | null
  }
  /**
   * The stored DingTalk alert secret. A synthetic value only, kept out of the
   * public settings response: the API reports presence, never the secret. No
   * fixture control endpoint serialises this state.
   */
  privateCacheAlertSecret: string
  nextId: number
}

type FixtureBreakdown = {
  cache_read_tokens: number
  cache_write_tokens: number
  customer_amount: string
  grouping_key: string
  input_tokens: number
  output_tokens: number
  request_count: number
}

export const FIXTURE_SESSION_COOKIE = 'pf_fixture_session'
export const FIXTURE_RELAY_COOKIE = 'pf_fixture_relay'
export const FIXTURE_ADMIN_LOGIN = 'admin'
export const FIXTURE_OPERATOR_LOGIN = 'operator'
export const FIXTURE_PASSWORD = 'fixture-password'
export const FIXTURE_RELAY_TOKEN = 'relay-fixture-token'

const AT = '2026-10-23T12:00:00.000Z'
const YESTERDAY = '2026-10-22T08:00:00.000Z'
const LAST_WEEK = '2026-10-16T08:00:00.000Z'

function createUserRedactionConfig(userId: number): RedactionConfigSchema {
  return {
    custom_strings: [
      {
        created_at: LAST_WEEK,
        match_type: 'contains',
        pattern: `private-fixture-${userId}`,
        scope: 'text',
        updated_at: AT,
      },
    ],
    enabled: userId % 2 === 0,
    rules: {
      cidr: true,
      domain: true,
      email: true,
      ip: true,
      organization: false,
      person: false,
      phone: true,
      secret: userId !== 2,
      url: true,
    },
  }
}

const adminUser: User = {
  created_at: LAST_WEEK,
  display_name: 'Fixture Administrator',
  is_active: true,
  is_admin: true,
  login_name: FIXTURE_ADMIN_LOGIN,
  updated_at: AT,
  user_id: 1,
}

const operatorUser: User = {
  created_at: LAST_WEEK,
  display_name: 'Fixture Operator',
  is_active: true,
  is_admin: false,
  login_name: FIXTURE_OPERATOR_LOGIN,
  updated_at: AT,
  user_id: 2,
}

const longNameUser: User = {
  created_at: LAST_WEEK,
  display_name:
    'Integration Operator With A Deliberately Long Display Name For Header Stress',
  is_active: false,
  is_admin: false,
  login_name: 'integration-operator-eu-west-with-a-long-login-name',
  updated_at: AT,
  user_id: 3,
}

const endpointMiniMax: ProviderEndpoint = {
  active_windows: [],
  api_keys: [
    {
      created_at: LAST_WEEK,
      enabled: true,
      endpoint_id: 'ep-minimax-cn',
      key_id: 'key-minimax-1',
      key_label: 'primary',
      position: 0,
      updated_at: AT,
    },
    {
      created_at: LAST_WEEK,
      enabled: false,
      endpoint_id: 'ep-minimax-cn',
      key_id: 'key-minimax-2',
      key_label: 'failover',
      position: 1,
      updated_at: AT,
    },
  ],
  base_url: 'https://api.minimax.chat/v1',
  created_at: LAST_WEEK,
  endpoint_id: 'ep-minimax-cn',
  enabled: true,
  has_admin_api_key: false,
  has_oauth_token: false,
  has_proxy_url: true,
  key_lb_enabled: true,
  mcp_enabled: true,
  name: 'MiniMax Platform China Production',
  native_api: 'chat',
  native_api_source: 'provider',
  owner_user_id: null,
  plan: 'platform_api_key',
  provider: 'minimax',
  provider_region: 'cn',
  scope: 'global',
  service_tier: 'preferred',
  updated_at: AT,
}

const endpointOpenAi: ProviderEndpoint = {
  active_windows: [{ start: '00:00', end: '23:59' }],
  api_keys: [],
  base_url: 'https://chatgpt.com/backend-api/codex',
  created_at: LAST_WEEK,
  endpoint_id: 'ep-openai-codex',
  enabled: false,
  has_admin_api_key: true,
  has_oauth_token: true,
  has_proxy_url: false,
  key_lb_enabled: false,
  mcp_enabled: false,
  name: 'OpenAI ChatGPT Subscription — Integration Workspace With A Long Name',
  native_api: 'responses',
  native_api_source: 'provider',
  owner_user_id: 2,
  plan: 'chatgpt_subscription',
  provider: 'openai',
  provider_region: 'global',
  scope: 'user',
  service_tier: null,
  updated_at: AT,
}

const endpointOpenRouter: ProviderEndpoint = {
  active_windows: [],
  api_keys: [
    {
      created_at: LAST_WEEK,
      enabled: true,
      endpoint_id: 'ep-openrouter',
      key_id: 'key-openrouter-1',
      key_label: 'shared',
      position: 0,
      updated_at: AT,
    },
  ],
  base_url: 'https://openrouter.ai/api/v1',
  created_at: LAST_WEEK,
  endpoint_id: 'ep-openrouter',
  enabled: true,
  has_admin_api_key: false,
  has_oauth_token: false,
  has_proxy_url: false,
  key_lb_enabled: false,
  mcp_enabled: false,
  name: 'OpenRouter Shared Pool',
  native_api: 'auto',
  native_api_source: 'detected',
  owner_user_id: null,
  plan: 'platform_api_key',
  provider: 'openrouter',
  provider_region: 'global',
  scope: 'global',
  service_tier: null,
  updated_at: AT,
}

const routePrimary: ModelEndpointRule = {
  created_at: LAST_WEEK,
  enabled: true,
  model_pattern: 'gpt-5.1-codex-preview*',
  owner_user_id: null,
  routing_strategy: 'responses_session_affinity',
  rule_id: 'rule-gpt-codex',
  scope: 'global',
  targets: [
    {
      active_windows: [],
      compact_mode: 'passthrough',
      created_at: LAST_WEEK,
      dev_system_normalize: true,
      embedding_dimensions_override: null,
      enabled: true,
      endpoint_enabled: true,
      endpoint_id: 'ep-openai-codex',
      endpoint_name: endpointOpenAi.name,
      has_proxy_url_override: false,
      native_api: 'responses',
      position: 0,
      rule_id: 'rule-gpt-codex',
      service_tier: 'priority',
      target_id: 'target-codex-primary',
      thinking_downgrade_enabled: true,
      thinking_effort_override: 'high',
      updated_at: AT,
      upstream_model:
        'gpt-5.1-codex-preview-with-an-extremely-long-upstream-model-identifier',
    },
    {
      active_windows: [],
      compact_mode: 'self_summarize',
      created_at: LAST_WEEK,
      dev_system_normalize: false,
      embedding_dimensions_override: null,
      enabled: true,
      endpoint_enabled: true,
      endpoint_id: 'ep-minimax-cn',
      endpoint_name: endpointMiniMax.name,
      has_proxy_url_override: true,
      native_api: 'chat',
      position: 1,
      rule_id: 'rule-gpt-codex',
      service_tier: null,
      target_id: 'target-codex-fallback',
      thinking_downgrade_enabled: false,
      thinking_effort_override: 'medium',
      updated_at: AT,
      upstream_model: 'minimax-m2',
    },
  ],
  updated_at: AT,
}

const routeEmbeddings: ModelEndpointRule = {
  created_at: LAST_WEEK,
  enabled: false,
  model_pattern: 'text-embedding-3-large',
  owner_user_id: 2,
  routing_strategy: 'responses_session_affinity',
  rule_id: 'rule-embeddings',
  scope: 'user',
  targets: [
    {
      active_windows: [],
      compact_mode: 'off',
      created_at: LAST_WEEK,
      dev_system_normalize: false,
      embedding_dimensions_override: 3072,
      enabled: false,
      endpoint_enabled: true,
      endpoint_id: 'ep-openrouter',
      endpoint_name: endpointOpenRouter.name,
      has_proxy_url_override: false,
      native_api: 'embeddings',
      position: 0,
      rule_id: 'rule-embeddings',
      service_tier: null,
      target_id: 'target-embeddings',
      thinking_downgrade_enabled: false,
      thinking_effort_override: null,
      updated_at: AT,
      upstream_model: null,
    },
  ],
  updated_at: AT,
}

const relayConnected: ManagedRelay = {
  bridge_encryption_mode: 'required',
  connected: true,
  created_at: LAST_WEEK,
  enabled: true,
  has_bridge_key: true,
  has_client_cert: true,
  has_client_key: true,
  has_relay_ca: true,
  last_connected_at: AT,
  last_disconnected_at: null,
  last_error: null,
  last_snapshot_version: 12,
  name: 'Edge Relay Frankfurt With A Long Operational Name',
  relay_id: 'relay-frankfurt',
  relay_url: 'wss://relay-frankfurt.example.internal:9443/bridge',
  tls_mode: 'mtls',
  updated_at: AT,
}

const relayDisconnected: ManagedRelay = {
  bridge_encryption_mode: 'off',
  connected: false,
  created_at: LAST_WEEK,
  enabled: false,
  has_bridge_key: false,
  has_client_cert: false,
  has_client_key: false,
  has_relay_ca: false,
  last_connected_at: YESTERDAY,
  last_disconnected_at: AT,
  last_error: 'worker handshake timed out after 30s',
  last_snapshot_version: 4,
  name: 'Edge Relay Singapore (staging)',
  relay_id: 'relay-singapore',
  relay_url: 'wss://relay-singapore.example.internal:9443/bridge',
  tls_mode: 'server',
  updated_at: AT,
}

const mcpServers: McpServer[] = [
  {
    aggregate_naming_mode: 'passthrough_preferred',
    allowed_tools: ['legacy-tool'],
    args: [],
    auth_mode: 'bearer',
    basic_username: null,
    bearer_tokens: [{ enabled: true, token: 'fixture-context7-token' }],
    command: null,
    created_at: LAST_WEEK,
    disabled_resources: [],
    disabled_tools: ['legacy-blocked-tool'],
    enabled: true,
    env_json: null,
    has_basic_password: false,
    has_proxy_url: false,
    http_headers_json: { 'X-Fixture-Hidden': 'round-trip' },
    lifecycle_learned_at: null,
    lifecycle_learned_for_updated_at: null,
    lifecycle_learned_mode: null,
    lifecycle_learned_protocol_version: null,
    lifecycle_manual_protocol_version: '2025-06-18',
    lifecycle_policy: 'auto',
    name: 'Context7 Documentation With A Long Server Name',
    owner_user_id: null,
    provider_kind: 'context7',
    scope: 'admin',
    server_id: 'mcp-context7',
    source_endpoint_id: null,
    timeout_ms: 27123,
    tool_filter_mode: 'blacklist',
    transport: 'http',
    updated_at: AT,
    url: 'https://mcp.context7.com/mcp',
  },
  {
    aggregate_naming_mode: 'passthrough_preferred',
    allowed_tools: ['echo', 'get_weather'],
    args: ['-y', '@modelcontextprotocol/server-everything'],
    auth_mode: 'none',
    basic_username: null,
    bearer_tokens: [],
    command: 'bunx',
    created_at: LAST_WEEK,
    disabled_resources: ['resource://internal/secret'],
    disabled_tools: ['delete_everything'],
    enabled: false,
    env_json: {
      FIXTURE_API_KEY: null,
      WORKER_API_KEY: '{env:FIXTURE_WORKER_API_KEY}',
    },
    has_basic_password: false,
    has_proxy_url: true,
    http_headers_json: null,
    lifecycle_learned_at: YESTERDAY,
    lifecycle_learned_for_updated_at: AT,
    lifecycle_learned_mode: 'manual',
    lifecycle_learned_protocol_version: '2025-03-26',
    lifecycle_manual_protocol_version: '2025-03-26',
    lifecycle_policy: 'auto',
    name: 'Local Everything Server',
    owner_user_id: 2,
    provider_kind: 'generic',
    scope: 'user',
    server_id: 'mcp-local-everything',
    source_endpoint_id: null,
    timeout_ms: 15000,
    tool_filter_mode: 'whitelist',
    transport: 'stdio',
    updated_at: YESTERDAY,
    url: null,
  },
]

const approvalPending: ApprovalRequest = {
  approval_id: 'approval-1',
  approval_status: 'pending',
  client_key_label: 'integration-key',
  created_at: AT,
  decided_at: null,
  decided_by_login_name: null,
  decided_by_user_id: null,
  model: 'gpt-5.1-codex-preview',
  path: '/v1/responses',
  request_deadline_unix_ms: Date.parse('2026-10-23T12:05:00.000Z'),
  request_id: 'req-approval-1',
  request_payload_json: { model: 'gpt-5.1-codex-preview' },
  request_preview:
    'Please summarise this long internal document and include every confidential detail verbatim.',
  review_categories: ['confidentiality', 'policy'],
  review_decision: 'review',
  review_reason:
    'Request contains a confidential marker that requires operator approval.',
  updated_at: AT,
  user_id: 2,
  user_login_name: FIXTURE_OPERATOR_LOGIN,
  wait_deadline_unix_ms: approvalWaitDeadline('approval-1'),
}

/**
 * A pending wait deadline is a live countdown, not a fixed stamp: the seeded
 * value is relative to the moment the list is read, so the operator sees the
 * remaining time actually running down.
 */
export function approvalWaitOffsetSeconds(approvalId: string): number {
  if (approvalId === 'approval-1') return 30
  if (approvalId === 'approval-fixture-1') return -60
  const match = /^approval-fixture-(\d+)$/.exec(approvalId)
  if (match) return 60 * Number(match[1])
  return 300
}

export function approvalWaitDeadline(approvalId: string): number {
  return Date.now() + approvalWaitOffsetSeconds(approvalId) * 1000
}

/** The pending order: wait deadline ascending, then created descending, then id. */
export function sortPendingApprovals(
  approvals: ApprovalRequest[],
): ApprovalRequest[] {
  return [...approvals].sort((left, right) => {
    const leftDeadline = left.wait_deadline_unix_ms
    const rightDeadline = right.wait_deadline_unix_ms
    if (leftDeadline !== rightDeadline) {
      if (leftDeadline == null) return 1
      if (rightDeadline == null) return -1
      return leftDeadline - rightDeadline
    }
    const created = Date.parse(right.created_at) - Date.parse(left.created_at)
    if (created !== 0) return created
    return left.approval_id.localeCompare(right.approval_id)
  })
}

/** The resolved order: created descending, then id descending. */
export function sortResolvedApprovals(
  approvals: ApprovalRequest[],
): ApprovalRequest[] {
  return [...approvals].sort((left, right) => {
    const created = Date.parse(right.created_at) - Date.parse(left.created_at)
    if (created !== 0) return created
    return right.approval_id.localeCompare(left.approval_id)
  })
}

const approvalResolved: ApprovalRequest = {
  approval_id: 'approval-2',
  approval_status: 'approved',
  client_key_label: 'primary',
  created_at: LAST_WEEK,
  decided_at: YESTERDAY,
  decided_by_login_name: FIXTURE_ADMIN_LOGIN,
  decided_by_user_id: 1,
  model: 'minimax-m2',
  path: '/v1/chat/completions',
  request_deadline_unix_ms: Date.parse('2026-10-16T12:05:00.000Z'),
  request_id: 'req-approval-2',
  request_payload_json: null,
  request_preview: 'Routine fixture request that was approved.',
  review_categories: [],
  review_decision: 'allow',
  review_reason: 'No policy violation detected.',
  updated_at: YESTERDAY,
  user_id: 2,
  user_login_name: FIXTURE_OPERATOR_LOGIN,
  wait_deadline_unix_ms: Date.parse('2026-10-16T12:05:00.000Z'),
}

const charge = (
  chargeId: number,
  pricingStatus: string,
  usageStatus: string,
): BillingChargeResponse => ({
  cache_read_tokens: 120,
  cache_write_tokens: 40,
  charge_id: chargeId,
  client_key_id: 10 + chargeId,
  client_key_label: chargeId % 2 === 0 ? 'primary' : 'integration-key',
  created_at: AT,
  currency: 'CNY',
  customer_amount: chargeId % 3 === 0 ? null : '0.004210',
  endpoint_id: chargeId % 2 === 0 ? 'ep-minimax-cn' : 'ep-openai-codex',
  endpoint_key_id: null,
  endpoint_name:
    chargeId % 2 === 0 ? endpointMiniMax.name : endpointOpenAi.name,
  input_tokens: 800,
  output_tokens: 260,
  pricing_status: pricingStatus,
  request_id: `req-charge-${chargeId}`,
  requested_model: chargeId % 2 === 0 ? 'minimax-m2' : 'gpt-5.1-codex-preview',
  updated_at: AT,
  upstream_model:
    chargeId % 2 === 0 ? 'minimax-m2' : 'gpt-5.1-codex-preview-2026-10-01',
  usage_status: usageStatus,
  user_id: 2,
  user_login_name: FIXTURE_OPERATOR_LOGIN,
})

export function buildOverview(
  category: 'ai' | 'mcp',
  options: { empty?: boolean; unknownTokens?: boolean } = {},
) {
  const requestCount = options.empty ? 0 : 24
  const zeroTokens = options.empty ? 0 : 1120
  return {
    breakdown: options.empty
      ? []
      : [
          {
            avg_output_tokens_per_second: category === 'ai' ? 42.5 : null,
            endpoint_id: null,
            error_count: 6,
            error_rate: 0.25,
            label: 'minimax-m2',
            mcp_scope: null,
            mcp_server_id: null,
            model: 'minimax-m2',
            model_breakdown: null,
            request_count: 12,
            request_share: 0.5,
            server_provider_kind: null,
            success_count: 6,
            success_rate: 0.5,
            token_share: 0.7,
            tokens: {
              cache_hit_rate: 0.35,
              cache_rate: 0.4,
              cache_read_tokens: 200,
              cache_write_tokens: 80,
              input_tokens: 700,
              output_tokens: 460,
              total_tokens: zeroTokens,
            },
            upstream_breakdown:
              category === 'ai'
                ? [
                    {
                      avg_output_tokens_per_second: 44.1,
                      cache_rate: 0.4,
                      endpoint_id: 'ep-minimax-cn',
                      endpoint_name: endpointMiniMax.name,
                      error_count: 1,
                      error_rate: 0.2,
                      request_count: 5,
                      request_share: 0.208,
                      token_share: 0.5,
                      total_tokens: 600,
                      upstream_model: 'minimax-m2',
                    },
                    {
                      avg_output_tokens_per_second: null,
                      cache_rate: null,
                      endpoint_id: 'ep-minimax-fallback',
                      endpoint_name:
                        'MiniMax Fallback Endpoint With A Deliberately Long Display Name',
                      error_count: 3,
                      error_rate: 0.6,
                      request_count: 5,
                      request_share: 0.208,
                      token_share: null,
                      total_tokens: 0,
                      upstream_model:
                        'minimax-m2-preview-with-a-very-long-model-identifier',
                    },
                    {
                      avg_output_tokens_per_second: null,
                      cache_rate: null,
                      endpoint_id: 'ep-minimax-broken',
                      endpoint_name: 'MiniMax Broken Pool (all failed)',
                      error_count: 5,
                      error_rate: 1,
                      request_count: 5,
                      request_share: 0.208,
                      token_share: 0,
                      total_tokens: 0,
                      upstream_model: 'minimax-m2',
                    },
                  ]
                : null,
            upstream_count: category === 'ai' ? 3 : null,
            usage_unit: category === 'mcp' ? 'requests' : null,
          },
          {
            avg_output_tokens_per_second: null,
            endpoint_id: null,
            error_count: 1,
            error_rate: 0.25,
            label: 'text-embedding-3-large',
            mcp_scope: null,
            mcp_server_id: null,
            model: 'text-embedding-3-large',
            model_breakdown: null,
            request_count: 4,
            request_share: 0.167,
            server_provider_kind: null,
            success_count: 3,
            success_rate: 0.75,
            token_share: 0.3,
            tokens: {
              cache_hit_rate: 0,
              cache_rate: 0,
              cache_read_tokens: 0,
              cache_write_tokens: 0,
              input_tokens: 400,
              output_tokens: 0,
              total_tokens: options.unknownTokens ? 0 : 480,
            },
            upstream_breakdown: null,
            upstream_count: null,
            usage_unit: null,
          },
        ],
    summary: {
      avg_output_tokens_per_second: options.empty ? null : 41.2,
      error_count: options.empty ? 0 : 7,
      method_count: options.empty ? 0 : 3,
      p95_first_token_ms: options.empty ? null : 420,
      p95_total_ms: options.empty ? null : 3120,
      request_count: requestCount,
      success_count: options.empty ? 0 : 17,
      success_rate: options.empty ? 0 : 0.708,
      tokens: {
        cache_hit_rate: options.empty ? null : 0.33,
        cache_rate: options.empty ? null : 0.4,
        cache_read_tokens: options.empty ? 0 : 200,
        cache_write_tokens: options.empty ? 0 : 80,
        input_tokens: options.empty ? 0 : 1100,
        output_tokens: options.empty ? 0 : 460,
        total_tokens: options.empty ? 0 : zeroTokens,
      },
    },
    trend: options.empty
      ? []
      : [
          {
            bucket_at: '2026-10-23T00:00:00.000Z',
            error_count: 2,
            error_rate: 0.2,
            p95_first_token_ms: 400,
            p95_total_ms: 2800,
            request_count: 10,
            success_count: 8,
            success_rate: 0.8,
            tokens: {
              cache_hit_rate: 0.3,
              cache_rate: 0.35,
              cache_read_tokens: 100,
              cache_write_tokens: 40,
              input_tokens: 600,
              output_tokens: 200,
              total_tokens: 620,
            },
          },
          {
            bucket_at: '2026-10-22T00:00:00.000Z',
            error_count: 5,
            error_rate: 0.357,
            p95_first_token_ms: 460,
            p95_total_ms: 3400,
            request_count: 14,
            success_count: 9,
            success_rate: 0.643,
            tokens: {
              cache_hit_rate: 0.36,
              cache_rate: 0.44,
              cache_read_tokens: 100,
              cache_write_tokens: 40,
              input_tokens: 500,
              output_tokens: 260,
              total_tokens: 500,
            },
          },
        ],
    trend_granularity: 'day',
  }
}

function record(
  recordId: number,
  category: 'ai' | 'mcp',
  overrides: Partial<RequestRecordListRow> = {},
): RequestRecordListRow {
  return {
    abort_from_state: null,
    abort_reason: null,
    abort_response_started: null,
    cache_rate: 0.4,
    cache_read_tokens: 120,
    cache_write_tokens: 40,
    cached_tokens: 160,
    client_key_label: 'integration-key',
    conversation_id: `conversation-${recordId}`,
    conversation_seq: 1,
    conversation_source: 'fixture',
    created_at: AT,
    duration_ms: 2400,
    endpoint_id: 'ep-minimax-cn',
    endpoint_name: endpointMiniMax.name,
    error_code: null,
    error_message: null,
    failure_family: null,
    has_full_request: true,
    has_parent: false,
    input_tokens: 800,
    mcp_bearer_token_slot: null,
    mcp_operation_name: null,
    mcp_protocol_method: null,
    mcp_server_id: null,
    mcp_server_name: null,
    model: 'minimax-m2',
    ok: true,
    output_tokens: 260,
    parent_event_id: null,
    path: category === 'ai' ? '/v1/responses' : '/mcp',
    record_id: recordId,
    redaction: {
      applied: true,
      fields: ['request'],
      findings_count: 2,
      replacements_count: 2,
      types: ['email'],
    },
    request_category: category,
    request_id: `req-fixture-${recordId}`,
    request_state: 'completed',
    requested_model: 'minimax-m2',
    route_selection_reason: 'default',
    server_provider_kind: null,
    status: 200,
    storage_sanitized: true,
    storage_sanitized_nul_count: 0,
    total_tokens: 1060,
    ttft_ms: 380,
    upstream_model: 'minimax-m2',
    user_id: 2,
    user_login_name: FIXTURE_OPERATOR_LOGIN,
    ...overrides,
  }
}

function buildRecordDetail(row: RequestRecordListRow): RequestRecordDetail {
  return {
    ...row,
    applied_thinking_effort_override: 'high',
    assistant_message_json: {
      content: 'Fixture assistant response.',
      role: 'assistant',
    },
    assistant_output_items_json: null,
    client_installation_id: 'fixture-installation',
    conversation_id: row.conversation_id ?? null,
    conversation_seq: row.conversation_seq ?? null,
    endpoint_key_id: 'key-minimax-1',
    endpoint_key_label: 'primary',
    has_full_request: row.has_full_request,
    http_request_compressed: true,
    http_request_compressed_bytes: 640,
    http_request_compression_ratio: 0.42,
    http_request_content_encoding: 'gzip',
    http_request_decompressed_bytes: 1520,
    mcp_bearer_token_slot: row.mcp_bearer_token_slot,
    mcp_operation_name: row.mcp_operation_name,
    mcp_protocol_method: row.mcp_protocol_method,
    mcp_server_id: row.mcp_server_id,
    mcp_server_name: row.mcp_server_name,
    normalized_item_count: 2,
    provider_response_id: `resp-${row.record_id}`,
    request_conversation_key: row.conversation_id ?? null,
    request_conversation_parent_found: false,
    request_has_previous_response_id: false,
    request_previous_response_id: null,
    request_previous_response_parent_found: null,
    request_raw_json: { model: row.requested_model, stream: true },
    request_storage_mode: 'normalized',
    request_user_agent: 'fixture-browser/1.0',
    response_capture_truncated: false,
    response_prompt: 'Fixture prompt text.',
    response_raw_body: null,
    route_selection_reason: row.route_selection_reason,
    server_provider_kind: row.server_provider_kind,
    session_header_id: 'session-fixture-1',
    session_parent_id: null,
    storage_sanitized: row.storage_sanitized,
    storage_sanitized_nul_count: 0,
    tool_call_events: [
      {
        arguments_json: { query: 'fixture' },
        arguments_preview: '{"query":"fixture"}',
        call_id: 'call-1',
        conversation_id: row.conversation_id ?? null,
        created_at: AT,
        mcp_request_event_id: null,
        parent_event_id: row.record_id,
        sequence_in_turn: 1,
        status: 'output_received',
        tool_call_event_id: row.record_id + 1000,
        tool_name: 'fixture_lookup',
        updated_at: AT,
      },
    ],
    upstream_error_body:
      row.ok === false
        ? 'upstream returned 429: fixture rate limit exceeded'
        : null,
    abort_reason: row.abort_reason,
  } as RequestRecordDetail
}

export function createState(): FixtureState {
  const aiRecordsRaw: RequestRecordListRow[] = [
    record(1, 'ai', { client_key_label: 'primary' }),
    record(2, 'ai', {
      error_code: 'upstream_rate_limited',
      error_message: 'Fixture upstream rate limit',
      failure_family: 'rate_limit',
      ok: false,
      request_state: 'failed',
      status: 429,
      total_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
    }),
    record(3, 'ai', {
      cache_rate: null,
      cached_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      upstream_model: null,
      endpoint_id: null,
      endpoint_name: null,
    }),
    record(4, 'ai', {
      input_tokens: null,
      output_tokens: null,
      total_tokens: null,
      cache_rate: null,
      cached_tokens: null,
      cache_read_tokens: null,
      cache_write_tokens: null,
    }),
    record(5, 'ai', {
      error_message: 'Aborted by downstream client',
      ok: false,
      request_state: 'aborted',
      abort_reason: 'downstream_closed',
      abort_from_state: 'upstream_processing',
      abort_response_started: true,
      status: null,
    }),
    record(6, 'ai', {
      client_key_label: 'primary',
      endpoint_id: 'ep-openai-codex',
      endpoint_name: endpointOpenAi.name,
      model: 'gpt-5.1-codex-preview',
      requested_model: 'gpt-5.1-codex-preview',
      upstream_model:
        'gpt-5.1-codex-preview-with-an-extremely-long-upstream-model-identifier',
      route_selection_reason: 'session_affinity',
    }),
    record(7, 'ai', {
      route_selection_reason: 'conversation_override',
      conversation_id: 'conversation-override',
    }),
    record(8, 'ai', {
      route_selection_reason: 'quota_failover',
      ok: false,
      request_state: 'failed',
      error_message: 'quota exhausted on preferred upstream',
      failure_family: 'quota',
      total_tokens: 0,
    }),
    record(9, 'ai', {
      redaction: {
        applied: false,
        fields: [],
        findings_count: 0,
        replacements_count: 0,
        types: [],
      },
    }),
    record(10, 'ai', {
      client_key_label: 'primary',
      user_id: 1,
      user_login_name: FIXTURE_ADMIN_LOGIN,
    }),
    record(11, 'ai'),
    record(12, 'ai'),
    record(13, 'ai'),
    // A record without a conversation: the route-options read and the affinity
    // reset both answer 400 no_conversation_id for it.
    record(14, 'ai', { conversation_id: null, conversation_seq: null }),
    ...Array.from({ length: 12 }, (_, index) => record(15 + index, 'ai')),
    // P10e fold matrix. One model spans four upstreams, so the model
    // perspective groups a positive upstream with a success-only zero-token
    // upstream, a 60%-failure mixed zero-token upstream and an all-failed
    // zero-token upstream; hidden request count is 1 + 5 + 2 = 8.
    record(27, 'ai', {
      endpoint_id: 'ep-fixture-1',
      endpoint_name: 'Fixture Upstream 1 With A Long Operational Name',
      model: 'usage-zero-fixture',
      requested_model: 'usage-zero-fixture',
      upstream_model: 'usage-zero-fixture',
      total_tokens: 1060,
    }),
    record(28, 'ai', {
      endpoint_id: 'ep-fixture-3',
      endpoint_name: 'Fixture Upstream 3 With A Long Operational Name',
      model: 'usage-zero-fixture',
      requested_model: 'usage-zero-fixture',
      upstream_model: 'usage-zero-fixture',
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    }),
    ...[29, 30, 31].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-5',
        endpoint_name: 'Fixture Upstream 5 With A Long Operational Name',
        error_code: 'upstream_unavailable',
        error_message: 'Fixture mixed zero-token failure',
        failure_family: 'upstream',
        model: 'usage-zero-fixture',
        ok: false,
        requested_model: 'usage-zero-fixture',
        request_state: 'failed',
        status: 502,
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture',
      }),
    ),
    ...[32, 33].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-5',
        endpoint_name: 'Fixture Upstream 5 With A Long Operational Name',
        model: 'usage-zero-fixture',
        requested_model: 'usage-zero-fixture',
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture',
      }),
    ),
    ...[34, 35].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-7',
        endpoint_name: 'Fixture Upstream 7 With A Long Operational Name',
        error_code: 'upstream_unavailable',
        error_message: 'Fixture all-failed zero-token failure',
        failure_family: 'upstream',
        model: 'usage-zero-fixture',
        ok: false,
        requested_model: 'usage-zero-fixture',
        request_state: 'failed',
        status: 503,
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture',
      }),
    ),
    // The upstream perspective groups by endpoint, so one endpoint holds a
    // positive model beside the same three zero-token variants; hidden request
    // count is again 1 + 5 + 2 = 8.
    record(36, 'ai', {
      endpoint_id: 'ep-fixture-1',
      endpoint_name: 'Fixture Upstream 1 With A Long Operational Name',
      model: 'usage-zero-fixture-b',
      requested_model: 'usage-zero-fixture-b',
      upstream_model: 'usage-zero-fixture-b',
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    }),
    ...[37, 38, 39].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-1',
        endpoint_name: 'Fixture Upstream 1 With A Long Operational Name',
        error_code: 'upstream_unavailable',
        error_message: 'Fixture mixed zero-token failure',
        failure_family: 'upstream',
        model: 'usage-zero-fixture-mixed',
        ok: false,
        requested_model: 'usage-zero-fixture-mixed',
        request_state: 'failed',
        status: 502,
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture-mixed',
      }),
    ),
    ...[40, 41].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-1',
        endpoint_name: 'Fixture Upstream 1 With A Long Operational Name',
        model: 'usage-zero-fixture-mixed',
        requested_model: 'usage-zero-fixture-mixed',
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture-mixed',
      }),
    ),
    ...[42, 43].map((recordId) =>
      record(recordId, 'ai', {
        endpoint_id: 'ep-fixture-1',
        endpoint_name: 'Fixture Upstream 1 With A Long Operational Name',
        error_code: 'upstream_unavailable',
        error_message: 'Fixture all-failed zero-token failure',
        failure_family: 'upstream',
        model: 'usage-zero-fixture-failed',
        ok: false,
        requested_model: 'usage-zero-fixture-failed',
        request_state: 'failed',
        status: 503,
        total_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        upstream_model: 'usage-zero-fixture-failed',
      }),
    ),
  ]

  const mcpRecordsRaw: RequestRecordListRow[] = [
    record(101, 'mcp', {
      client_key_label: 'primary',
      mcp_operation_name: 'tools/call',
      mcp_protocol_method: 'tools/call',
      mcp_server_id: 'mcp-context7',
      mcp_server_name: 'Context7 Documentation With A Long Server Name',
      mcp_bearer_token_slot: 0,
      server_provider_kind: 'context7',
      model: null,
      requested_model: null,
      path: '/mcp/context7',
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
    }),
    ...Array.from({ length: 24 }, (_, index) => record(103 + index, 'mcp')),
    record(102, 'mcp', {
      mcp_operation_name: 'resources/read',
      mcp_protocol_method: 'resources/read',
      mcp_server_id: null,
      mcp_server_name: null,
      server_provider_kind: null,
      model: null,
      requested_model: null,
      ok: false,
      request_state: 'failed',
      error_message: 'gateway rejected unmapped operation',
      path: '/mcp/gateway',
    }),
  ]

  // The synthetic records are dated relative to this state so a preset window
  // never drops the dataset, while a custom window can still be honestly empty.
  const recordBase = Date.now()
  const stampRecords = (rows: RequestRecordListRow[]): RequestRecordListRow[] =>
    rows.map((row, index) => ({
      ...row,
      created_at: new Date(recordBase - index * 60_000).toISOString(),
    }))
  const aiRecords = stampRecords(aiRecordsRaw)
  const mcpRecords = stampRecords(mcpRecordsRaw)

  const recordDetails = new Map<number, RequestRecordDetail>()
  for (const row of [...aiRecords, ...mcpRecords]) {
    recordDetails.set(row.record_id, buildRecordDetail(row))
  }

  // P10e detail variants. The reasoning seed stays inside the record's own raw
  // payload so the detail and request-full reads agree, and the Messages path
  // keeps the caller effort unmerged while the responses path shows
  // caller -> applied override. The parent session id makes the sub-session
  // badges reachable.
  const detailOverrides: Array<{
    recordId: number
    raw: Record<string, unknown>
    appliedOverride: string
    path: string
    sessionParentId: string | null
  }> = [
    {
      appliedOverride: 'high',
      path: '/v1/messages',
      raw: { reasoning: { effort: 'low' }, stream: true },
      recordId: 27,
      sessionParentId: null,
    },
    {
      appliedOverride: 'high',
      path: '/v1/responses',
      raw: { reasoning: { effort: 'low' }, stream: true },
      recordId: 28,
      sessionParentId: null,
    },
    {
      appliedOverride: 'high',
      path: '/v1/responses',
      raw: { reasoning: { effort: 'medium' }, stream: true },
      recordId: 36,
      sessionParentId: 'session-fixture-parent-36',
    },
  ]
  for (const variant of detailOverrides) {
    const existing = recordDetails.get(variant.recordId)
    if (!existing) continue
    recordDetails.set(variant.recordId, {
      ...existing,
      applied_thinking_effort_override: variant.appliedOverride,
      path: variant.path,
      request_raw_json: { model: existing.model, ...variant.raw },
      session_parent_id: variant.sessionParentId,
    })
  }

  // Synthetic session-routing states: the default read stays active, while
  // these records cover the stale, unbound and override cases without any
  // routing engine.
  const recordRouteOptions = new Map<number, FixtureRecordRouteOptions>([
    [
      6,
      {
        affinityEndpointId: 'ep-stale-retired',
        affinityState: 'stale_endpoint',
      },
    ],
    [
      7,
      {
        affinityState: 'unbound',
        overrideEndpointId: endpointOpenAi.endpoint_id,
        overrideEndpointKeyId: null,
        overrideEndpointKeyLabel: null,
      },
    ],
    [9, { affinityState: 'stale_key' }],
    [14, { affinityState: 'unbound' }],
  ])

  const endpoints = [endpointMiniMax, endpointOpenAi, endpointOpenRouter]
  const modelRoutes = [routePrimary, routeEmbeddings]

  const extraUsers: FixtureAccount[] = Array.from(
    { length: 10 },
    (_, index) => {
      const id = 10 + index
      return {
        isAdmin: false,
        password: FIXTURE_PASSWORD,
        user: {
          created_at: LAST_WEEK,
          display_name: `Fixture User ${index + 1} With A Longer Display Name`,
          is_active: index % 4 !== 3,
          is_admin: false,
          login_name: `fixture-user-${String(index + 1).padStart(2, '0')}`,
          updated_at: AT,
          user_id: id,
        },
      }
    },
  )

  const extraKeys: ClientKey[] = Array.from({ length: 10 }, (_, index) => ({
    created_at: LAST_WEEK,
    enabled: index % 3 !== 0,
    key_id: `key-extra-${index + 1}`,
    key_prefix: `pf_live_x${index + 1}`,
    label: `fixture-key-${index + 1}-with-a-long-label`,
    last_used_at: YESTERDAY,
    secret: null,
    user_id: 1,
  }))

  const extraModels = Array.from({ length: 21 }, (_, index) => ({
    id: `fixture-model-${index + 1}-with-a-long-identifier`,
    name: `Fixture Model ${index + 1}`,
  }))

  const extraEndpoints: ProviderEndpoint[] = Array.from(
    { length: 10 },
    (_, index) => ({
      active_windows: [],
      api_keys: [
        {
          created_at: LAST_WEEK,
          enabled: true,
          endpoint_id: `ep-fixture-${index + 1}`,
          key_id: `key-fixture-${index + 1}`,
          key_label: 'fixture',
          position: 0,
          updated_at: AT,
        },
      ],
      base_url: `https://upstream-${index + 1}.example.internal/v1`,
      created_at: LAST_WEEK,
      enabled: index % 2 === 0,
      endpoint_id: `ep-fixture-${index + 1}`,
      has_admin_api_key: false,
      has_oauth_token: false,
      has_proxy_url: false,
      key_lb_enabled: false,
      mcp_enabled: false,
      name: `Fixture Upstream ${index + 1} With A Long Operational Name`,
      native_api: 'auto',
      native_api_source: 'detected',
      owner_user_id: null,
      plan: 'platform_api_key',
      provider: 'generic',
      provider_region: 'global',
      scope: 'global',
      service_tier: null,
      updated_at: AT,
    }),
  )

  const extraRoutes: ModelEndpointRule[] = Array.from(
    { length: 10 },
    (_, index) => ({
      created_at: LAST_WEEK,
      enabled: true,
      model_pattern: `fixture-model-${index + 1}*`,
      owner_user_id: null,
      routing_strategy: 'responses_session_affinity',
      rule_id: `rule-fixture-${index + 1}`,
      scope: 'global',
      targets: [
        {
          active_windows: [],
          compact_mode: 'passthrough',
          created_at: LAST_WEEK,
          dev_system_normalize: false,
          embedding_dimensions_override: null,
          enabled: true,
          endpoint_enabled: true,
          endpoint_id: `ep-fixture-${index + 1}`,
          endpoint_name: `Fixture Upstream ${index + 1} With A Long Operational Name`,
          has_proxy_url_override: false,
          native_api: 'auto',
          position: 0,
          rule_id: `rule-fixture-${index + 1}`,
          service_tier: null,
          target_id: `target-fixture-${index + 1}`,
          thinking_downgrade_enabled: false,
          thinking_effort_override: null,
          updated_at: AT,
          upstream_model: null,
        },
      ],
      updated_at: AT,
    }),
  )

  const extraMcpServers: McpServer[] = Array.from(
    { length: 10 },
    (_, index) => ({
      aggregate_naming_mode: 'passthrough_preferred',
      allowed_tools: null,
      args: null,
      auth_mode: 'none',
      basic_username: null,
      bearer_tokens: [],
      command: null,
      created_at: LAST_WEEK,
      disabled_resources: null,
      disabled_tools: null,
      enabled: index % 2 === 0,
      env_json: null,
      has_basic_password: false,
      has_proxy_url: index === 0,
      http_headers_json: null,
      lifecycle_learned_at: null,
      lifecycle_learned_for_updated_at: null,
      lifecycle_learned_mode: null,
      lifecycle_learned_protocol_version: null,
      lifecycle_manual_protocol_version: null,
      lifecycle_policy: 'auto',
      name: `Fixture MCP Server ${index + 1} With A Long Name`,
      owner_user_id: null,
      provider_kind: 'generic',
      scope: 'admin',
      server_id: `mcp-fixture-${index + 1}`,
      source_endpoint_id: null,
      timeout_ms: 30000,
      tool_filter_mode: 'blacklist',
      transport: 'http',
      updated_at: AT,
      url:
        index === 0
          ? 'https://mcp-1.example.internal.with-a-deliberately-long-hostname.example.test/mcp'
          : `https://mcp-${index + 1}.example.internal/mcp`,
    }),
  )

  const extraRelays: ManagedRelay[] = Array.from(
    { length: 10 },
    (_, index) => ({
      bridge_encryption_mode: index % 2 === 0 ? 'required' : 'off',
      connected: index % 3 === 0,
      created_at: LAST_WEEK,
      enabled: index % 2 === 0,
      has_bridge_key: index % 2 === 0,
      has_client_cert: false,
      has_client_key: false,
      has_relay_ca: false,
      last_connected_at: index % 3 === 0 ? AT : null,
      last_disconnected_at: index % 3 === 0 ? null : YESTERDAY,
      last_error: index % 3 === 0 ? null : 'fixture relay not connected',
      last_snapshot_version: index % 3 === 0 ? 7 : null,
      name: `Fixture Relay ${index + 1} With A Long Operational Name`,
      relay_id: `relay-fixture-${index + 1}`,
      relay_url: `${index % 2 === 0 ? 'wss' : 'ws'}://relay-${index + 1}.example.internal:9443/bridge`,
      tls_mode: index % 2 === 0 ? 'server' : 'off',
      updated_at: AT,
    }),
  )

  const extraApprovals: ApprovalRequest[] = Array.from(
    { length: 13 },
    (_, index) => {
      const number = index + 1
      const status =
        number <= 10
          ? 'pending'
          : number === 11
            ? 'rejected'
            : number === 12
              ? 'expired'
              : 'aborted'
      const pending = status === 'pending'
      return {
        approval_id: `approval-fixture-${number}`,
        approval_status: status,
        client_key_label: `fixture-key-${number}`,
        created_at: new Date(Date.parse(AT) - number * 60_000).toISOString(),
        decided_at: pending ? null : YESTERDAY,
        decided_by_login_name: pending ? null : FIXTURE_ADMIN_LOGIN,
        decided_by_user_id: pending ? null : 1,
        model: index % 2 === 0 ? 'minimax-m2' : 'gpt-5.1-codex-preview',
        path: '/v1/chat/completions',
        request_deadline_unix_ms: Date.parse('2026-10-23T12:05:00.000Z'),
        request_id: `req-approval-fixture-${number}`,
        request_payload_json: null,
        request_preview: `Fixture approval request ${number} with a longer preview.`,
        review_categories: ['fixture'],
        review_decision: pending ? 'review' : 'allow',
        review_reason: pending
          ? 'Fixture pending approval waits for an operator decision.'
          : `Fixture approval was already ${status}.`,
        updated_at: AT,
        user_id: 2,
        user_login_name: FIXTURE_OPERATOR_LOGIN,
        wait_deadline_unix_ms: approvalWaitDeadline(
          `approval-fixture-${number}`,
        ),
      }
    },
  )

  const extraCharges: BillingChargeResponse[] = [
    charge(7, 'priced', 'known'),
    charge(8, 'priced', 'known'),
    charge(9, 'unpriced', 'unknown'),
    charge(10, 'priced', 'known'),
    charge(11, 'error', 'unknown'),
    charge(12, 'priced', 'known'),
    ...Array.from({ length: 20 }, (_, index) =>
      charge(
        index + 13,
        index % 3 === 0 ? 'unpriced' : 'priced',
        index % 3 === 0 ? 'unknown' : 'known',
      ),
    ),
  ]

  const extraPriceRules: BillingPriceRuleResponse[] = Array.from(
    { length: 10 },
    (_, index) => ({
      cache_read_rate: '0.00015',
      cache_write_rate: '0.00060',
      created_at: LAST_WEEK,
      created_by_user_id: 1,
      currency: 'CNY',
      effective_from: index === 0 ? '2026-09-01' : '2026-02-01',
      effective_to: null,
      enabled: index % 2 === 0,
      input_rate: '0.00300',
      output_rate: '0.01200',
      price_rule_id: `price-fixture-${index + 1}`,
      public_model: index === 0 ? 'minimax-m2' : `fixture-model-${index + 1}`,
      updated_at: AT,
    }),
  )

  const extraAudit: ConfigAuditEntry[] = Array.from(
    { length: 10 },
    (_, index) => ({
      action: index % 2 === 0 ? 'export' : 'import',
      actor_login_name: FIXTURE_ADMIN_LOGIN,
      actor_user_id: 1,
      archive_bytes: 40000 + index,
      audit_id: 10 + index,
      backend_kind: 'local',
      created_at: AT,
      domains: [{ name: 'endpoints', records: 1, unrecoverable_secrets: 0 }],
      error_code: null,
      error_message: null,
      format_version: 1,
      payload_fingerprint: 'd1b2c3d4e5f60718293a4b5c6d7e8f90',
      success: true,
    }),
  )

  const extraRedactionStrings: RedactionCustomStringRuleRowSchema[] =
    Array.from({ length: 10 }, (_, index) => ({
      array_index: index + 2,
      created_at: LAST_WEEK,
      match_type: 'contains',
      pattern: `fixture-secret-${index + 1}`,
      scope: index % 2 === 0 ? 'text' : 'line',
      updated_at: AT,
    }))

  const state: FixtureState = {
    accounts: [
      {
        isAdmin: true,
        password: FIXTURE_PASSWORD,
        user: adminUser,
      },
      {
        isAdmin: false,
        password: FIXTURE_PASSWORD,
        user: operatorUser,
      },
      {
        isAdmin: false,
        password: FIXTURE_PASSWORD,
        user: longNameUser,
      },
      ...extraUsers,
    ],
    sessionToken: 'fixture-worker-session',
    relayToken: 'fixture-relay-session',
    availableModels: [
      { id: 'minimax-m2', name: 'MiniMax M2' },
      {
        id: 'gpt-5.1-codex-preview-with-an-extremely-long-upstream-model-identifier',
        name: 'GPT 5.1 Codex Preview With A Deliberately Long Model Name',
      },
      { id: 'text-embedding-3-large', name: 'Text Embedding 3 Large' },
      ...extraModels,
    ],
    clientKeys: [
      {
        created_at: LAST_WEEK,
        enabled: true,
        key_id: 'key-1',
        key_prefix: 'pf_live_1',
        label: 'primary',
        last_used_at: AT,
        secret: null,
        user_id: 1,
      },
      {
        created_at: LAST_WEEK,
        enabled: false,
        key_id: 'key-2',
        key_prefix: 'pf_live_2',
        label: 'integration-key-with-a-long-label',
        last_used_at: null,
        secret: null,
        user_id: 1,
      },
      {
        created_at: LAST_WEEK,
        enabled: true,
        key_id: 'key-3',
        key_prefix: 'pf_live_3',
        label: 'operator-key',
        last_used_at: YESTERDAY,
        secret: null,
        user_id: 2,
      },
      ...extraKeys,
    ],
    endpoints: [...endpoints, ...extraEndpoints],
    modelRoutes: [...modelRoutes, ...extraRoutes],
    mcpProviders: [
      {
        auth: 'none',
        default_url: null,
        display_name: 'Generic',
        id: 'generic',
        provider_balance_supported: false,
        unit: 'requests',
      },
      {
        auth: 'none',
        default_url: null,
        display_name: 'MiniMax',
        id: 'minimax',
        provider_balance_supported: false,
        unit: 'requests',
      },
      {
        auth: 'bearer',
        default_url: 'https://mcp.context7.com/mcp',
        display_name: 'Context7',
        id: 'context7',
        provider_balance_supported: false,
        unit: 'requests',
      },
      {
        auth: 'bearer',
        default_url: 'https://mcp.firecrawl.dev/v2/mcp',
        display_name: 'Firecrawl',
        id: 'firecrawl',
        provider_balance_supported: true,
        unit: 'credits',
      },
    ],
    mcpServers: [...mcpServers, ...extraMcpServers],
    relays: [relayConnected, relayDisconnected, ...extraRelays],
    bridge: {
      configured_relays: 2,
      connected_relays: 1,
      relays: [
        {
          connected: true,
          enabled: true,
          last_error: null,
          last_snapshot_version: 12,
          relay_id: 'relay-frankfurt',
          relay_url: relayConnected.relay_url,
        },
        {
          connected: false,
          enabled: false,
          last_error: relayDisconnected.last_error ?? null,
          last_snapshot_version: 4,
          relay_id: 'relay-singapore',
          relay_url: relayDisconnected.relay_url,
        },
      ],
      snapshot_version: 12,
    },
    approvals: [approvalPending, approvalResolved, ...extraApprovals],
    billingCharges: [
      charge(1, 'priced', 'known'),
      charge(2, 'priced', 'known'),
      charge(3, 'unpriced', 'unknown'),
      charge(4, 'priced', 'known'),
      charge(5, 'error', 'unknown'),
      charge(6, 'priced', 'known'),
      ...extraCharges,
      {
        ...charge(99, 'priced', 'known'),
        user_id: 3,
        user_login_name: 'integration-operator',
      },
    ],
    billingPriceRules: [
      {
        cache_read_rate: '0.00015',
        cache_write_rate: '0.00060',
        created_at: LAST_WEEK,
        created_by_user_id: 1,
        currency: 'CNY',
        effective_from: '2026-01-01',
        effective_to: null,
        enabled: true,
        input_rate: '0.00300',
        output_rate: '0.01200',
        price_rule_id: 'price-minimax-m2',
        public_model: 'minimax-m2',
        updated_at: AT,
      },
      {
        cache_read_rate: '0.00020',
        cache_write_rate: '0.00080',
        created_at: LAST_WEEK,
        created_by_user_id: 1,
        currency: 'CNY',
        effective_from: '2026-06-01',
        effective_to: null,
        enabled: false,
        input_rate: '0.00400',
        output_rate: '0.01600',
        price_rule_id: 'price-gpt-codex',
        public_model: 'gpt-5.1-codex-preview',
        updated_at: AT,
      },
      ...extraPriceRules,
    ],
    billingSummary: {
      by_client_key: [
        {
          cache_read_tokens: 240,
          cache_write_tokens: 80,
          customer_amount: '0.009210',
          grouping_key: 'integration-key',
          input_tokens: 1600,
          output_tokens: 520,
          request_count: 4,
        },
      ],
      by_model: [
        {
          cache_read_tokens: 240,
          cache_write_tokens: 80,
          customer_amount: '0.005210',
          grouping_key: 'minimax-m2',
          input_tokens: 800,
          output_tokens: 260,
          request_count: 3,
        },
        {
          cache_read_tokens: 120,
          cache_write_tokens: 40,
          customer_amount: '0.004000',
          grouping_key: 'gpt-5.1-codex-preview',
          input_tokens: 800,
          output_tokens: 260,
          request_count: 3,
        },
      ],
      currency: 'USD',
      customer_amount: '0.009210',
      known_count: 4,
      priced_count: 4,
      request_count: 6,
      unknown_count: 2,
      unpriced_count: 1,
    },
    aiRecords,
    mcpRecords,
    aiOverview: buildOverview('ai'),
    mcpOverview: buildOverview('mcp'),
    recordDetails,
    conversationOverrides: new Map<string, ConversationEndpointOverride>(),
    recordRouteOptions,
    settings: {
      requestContentLogging: {
        mode: 'normalized_and_raw',
        raw_retention_days: 7,
      },
      usageRetention: {
        approval_retention_days: 30,
        content_retention_days: 30,
        metadata_retention_days: 90,
        raw_backend: 'local',
        raw_retention_days: 7,
        replay_enabled: true,
      },
      streamDeltaBatching: {
        enabled: true,
        flush_on_line_break: true,
        flush_on_sentence_end: true,
        flush_window_ms: 40,
        max_buffer_bytes: 65536,
        max_buffer_chars: 32768,
      },
      relayIpWhitelist: {
        allowed_cidrs: ['10.0.0.0/8', '192.168.0.0/16'],
        trusted_proxy_cidrs: ['127.0.0.1/32'],
      },
      modelRouteWhitelist: { enabled: false },
      llmReview: {
        custom_policy_text:
          'Fixture policy: never forward secrets or personal data.',
        enabled: true,
        failure_policy: 'fail_closed',
        pending_ttl_seconds: 120,
        review_api_key: '',
        review_base_url: 'https://review.example.internal',
        review_model: 'review-model',
        review_timeout_ms: 8000,
        webhook: {
          bearer_token: '',
          enabled: true,
          extra_headers: { 'X-Fixture-Header': 'enabled' },
          url: 'https://review.example.internal/webhook',
        },
      },
      endpointSetting: { endpoint_id: 'ep-minimax-cn' },
      rawObjectStore: {
        backend: 'local',
        has_s3_access_key: true,
        has_s3_secret_key: true,
        local_dir: '/var/lib/prompt-ferry/raw',
        s3_allow_http: false,
        s3_bucket: 'prompt-ferry-raw',
        s3_endpoint: 'https://s3.example.internal',
        s3_path_style: false,
        s3_prefix: 'fixture/',
        s3_region: 'eu-central-1',
      },
      cacheAlert: {
        cooldown_minutes: 15,
        dingtalk_secret: '',
        dingtalk_webhook_url:
          'https://oapi.dingtalk.com/robot/send?access_token=fixture',
        enabled: true,
        has_dingtalk_secret: true,
        min_turns: 8,
        threshold: 0.85,
        window_minutes: 30,
      },
    },
    redaction: {
      custom_strings: [
        {
          created_at: LAST_WEEK,
          match_type: 'contains',
          pattern: 'fixture-secret',
          scope: 'text',
          updated_at: AT,
        },
        {
          created_at: LAST_WEEK,
          match_type: 'regex',
          pattern: 'sk_fixture_[A-Za-z0-9]+',
          scope: 'line',
          updated_at: AT,
        },
        ...extraRedactionStrings.map(
          ({ array_index: _index, ...rule }) => rule,
        ),
      ],
      enabled: true,
      rules: {
        cidr: true,
        domain: true,
        email: true,
        ip: true,
        organization: false,
        person: false,
        phone: true,
        secret: true,
        url: true,
      },
    },
    redactionByUser: Object.fromEntries(
      [
        adminUser,
        operatorUser,
        longNameUser,
        ...extraUsers.map((account) => account.user),
      ].map((user) => [user.user_id, createUserRedactionConfig(user.user_id)]),
    ),
    redactionCustomStrings: [
      ...extraRedactionStrings,
      {
        array_index: 0,
        created_at: LAST_WEEK,
        match_type: 'contains',
        pattern: 'fixture-secret',
        scope: 'text',
        updated_at: AT,
      },
      {
        array_index: 1,
        created_at: LAST_WEEK,
        match_type: 'regex',
        pattern: 'sk_fixture_[A-Za-z0-9]+',
        scope: 'line',
        updated_at: AT,
      },
    ],
    configAuditClockMs: Date.parse(AT) + 1000,
    configAudit: [
      ...extraAudit,
      {
        action: 'export',
        actor_login_name: FIXTURE_ADMIN_LOGIN,
        actor_user_id: 1,
        archive_bytes: 48210,
        audit_id: 1,
        backend_kind: 'local',
        created_at: AT,
        domains: [
          { name: 'endpoints', records: 3, unrecoverable_secrets: 1 },
          { name: 'model_routes', records: 2, unrecoverable_secrets: 0 },
          { name: 'mcp_servers', records: 2, unrecoverable_secrets: 1 },
        ],
        error_code: null,
        error_message: null,
        format_version: 1,
        payload_fingerprint: 'a1b2c3d4e5f60718293a4b5c6d7e8f90',
        success: true,
      },
      {
        action: 'import',
        actor_login_name: FIXTURE_ADMIN_LOGIN,
        actor_user_id: 1,
        archive_bytes: 48210,
        audit_id: 2,
        backend_kind: 'local',
        created_at: YESTERDAY,
        domains: [
          { name: 'endpoints', records: 2, unrecoverable_secrets: 1 },
          { name: 'model_routes', records: 1, unrecoverable_secrets: 0 },
        ],
        error_code: 'secret_required',
        error_message: 'archive contains unrecoverable secrets',
        format_version: 1,
        payload_fingerprint: 'b1b2c3d4e5f60718293a4b5c6d7e8f90',
        success: false,
      },
    ],
    relayHost: {
      pending_role: null,
      restart_required: false,
      role: 'integrated',
    },
    relayRunningRole: 'integrated',
    relaySavedRole: null,
    relaySavedAdminBind: null,
    privateStorageSecrets: {
      s3_access_key: 'fixture-synthetic-s3-access-key',
      s3_secret_key: 'fixture-synthetic-s3-secret-key',
    },
    privateCacheAlertSecret: 'fixture-synthetic-dingtalk-secret',
    relayConfig: {
      admin_bind: '127.0.0.1:9444',
      bind: '0.0.0.0:9443',
      bridge_encryption_required: true,
      client_token_configured: true,
      request_timeout_seconds: 300,
      tls_enabled: true,
      worker_bind: '127.0.0.1:8788',
      worker_heartbeat_timeout_seconds: 45,
      worker_tls_enabled: false,
      worker_token_configured: true,
    },
    relayStatus: {
      relay: {
        admin_bind: '127.0.0.1:9444',
        bind: '0.0.0.0:9443',
        bridge_encryption_required: true,
        client_token_configured: true,
        request_timeout_seconds: 300,
        tls_enabled: true,
        worker_bind: '127.0.0.1:8788',
        worker_heartbeat_timeout_seconds: 45,
        worker_tls_enabled: false,
        worker_token_configured: true,
      },
      relay_ready: true,
      restart_required: false,
      role: 'integrated',
      worker: { config_version: 7, connected: false, connected_workers: 0 },
    },
    nextId: 1000,
  }
  return structuredClone(state)
}

export type FixtureMe = MeResponse
