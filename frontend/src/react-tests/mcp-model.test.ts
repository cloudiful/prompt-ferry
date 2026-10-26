import { describe, expect, it } from 'vitest'
import type { McpServer, McpTestResponse } from '@/generated/admin-api'
import {
  canDisableMcpBearerToken,
  canRemoveMcpBearerToken,
  createMcpWorkspaceView,
} from '@/models/mcp'

function server(overrides: Partial<McpServer> = {}): McpServer {
  return {
    aggregate_naming_mode: 'passthrough_preferred',
    allowed_tools: [],
    args: [],
    auth_mode: 'none',
    bearer_tokens: [],
    created_at: '2026-10-24T00:00:00.000Z',
    disabled_resources: [],
    disabled_tools: [],
    enabled: true,
    env_json: {},
    has_basic_password: false,
    has_proxy_url: false,
    http_headers_json: {},
    lifecycle_manual_protocol_version: null,
    lifecycle_policy: 'auto',
    name: 'Fixture MCP',
    owner_user_id: null,
    provider_kind: 'context7',
    scope: 'admin',
    server_id: 'mcp-one',
    source_endpoint_id: null,
    timeout_ms: 30000,
    tool_filter_mode: 'blacklist',
    transport: 'http',
    updated_at: '2026-10-24T00:00:00.000Z',
    url: 'https://mcp.context7.com/mcp',
    ...overrides,
  }
}

const labels = {
  aggregateNamingModePassthroughPreferredShort: 'Raw preferred',
  aggregateNamingModeQualifiedOnlyShort: 'Alias only',
  managedMinimax: 'MiniMax Token Plan',
  privateScope: 'Private',
  publicScope: 'Public',
  providerGenericLabel: 'Generic',
  providerLabels: { context7: 'Context7' },
  hostedProviderIds: ['context7'],
}

const testResult: McpTestResponse = {
  duration_ms: 5,
  message: 'fixture succeeded',
  ok: true,
  prompt_count: 0,
  prompts: [],
  resource_count: 0,
  resources: [],
  tool_count: 0,
  tools: [],
}

describe('MCP view and bearer-token predicates', () => {
  it('projects a server and its latest test without changing the server', () => {
    const item = server({
      name: 'MCP with a long fixture endpoint',
      server_id: 'ctx',
    })
    const view = createMcpWorkspaceView([item], {
      labels,
      testResults: { ctx: testResult },
      total: 12,
    })

    expect(view.total_count).toBe(12)
    expect(view.list_items[0]).toMatchObject({
      server: item,
      server_id: 'ctx',
      name: 'MCP with a long fixture endpoint',
      provider_label: 'Context7',
      show_provider_badge: true,
      endpoint_label: 'https://mcp.context7.com/mcp',
      test_message: 'fixture succeeded',
      test_ok: true,
    })
    expect(item).toEqual(
      server({ name: 'MCP with a long fixture endpoint', server_id: 'ctx' }),
    )
  })

  it('keeps the last enabled token active while allowing empty and redundant rows to be removed', () => {
    const onlyActive = [{ token: 'fixture-token', enabled: true }]
    expect(canDisableMcpBearerToken(onlyActive, 0)).toBe(false)
    expect(canRemoveMcpBearerToken(onlyActive, 0)).toBe(true)

    const withAlternate = [
      { token: 'first', enabled: true },
      { token: 'second', enabled: true },
      { token: '', enabled: true },
    ]
    expect(canDisableMcpBearerToken(withAlternate, 0)).toBe(true)
    expect(canRemoveMcpBearerToken(withAlternate, 0)).toBe(true)
    expect(canRemoveMcpBearerToken(withAlternate, 2)).toBe(true)

    const disabledAlternate = [
      { token: 'first', enabled: true },
      { token: 'second', enabled: false },
    ]
    expect(canRemoveMcpBearerToken(disabledAlternate, 0)).toBe(false)
    expect(canDisableMcpBearerToken(disabledAlternate, 0)).toBe(false)
  })
})
