import type { McpServer, McpServerRequest } from '../../generated/admin-api'
import type { McpForm } from '../../models'

import {
  normalizeJsonArray,
  normalizeJsonRecord,
  normalizeStringList,
  parseJsonText,
} from '../utils'

export function createEmptyMcpForm(): McpForm {
  return {
    server_id: '',
    scope: 'admin',
    owner_user_id: null,
    name: '',
    aggregate_naming_mode: 'passthrough_preferred',
    transport: 'http',
    url: '',
    command: '',
    args_text: '[]',
    bearer_tokens: [],
    http_headers_text: '{}',
    env_text: '{}',
    tool_filter_mode: 'blacklist',
    allowed_tools: [],
    disabled_tools: [],
    disabled_resources: [],
    daily_max_requests: null,
    monthly_max_requests: null,
    enabled: true,
    timeout_ms: 30000,
  }
}

export function mcpServerToForm(server: McpServer): McpForm {
  return {
    server_id: server.server_id,
    scope: server.scope === 'user' ? 'user' : 'admin',
    owner_user_id: server.owner_user_id ?? null,
    name: server.name,
    aggregate_naming_mode:
      server.aggregate_naming_mode === 'qualified_only'
        ? 'qualified_only'
        : 'passthrough_preferred',
    transport: server.transport === 'stdio' ? 'stdio' : 'http',
    url: server.url ?? '',
    command: server.command ?? '',
    args_text: JSON.stringify(normalizeJsonArray(server.args), null, 2),
    bearer_tokens: normalizeStringList(server.bearer_tokens),
    http_headers_text: JSON.stringify(
      normalizeJsonRecord(server.http_headers_json),
      null,
      2,
    ),
    env_text: JSON.stringify(normalizeJsonRecord(server.env_json), null, 2),
    tool_filter_mode:
      server.tool_filter_mode === 'whitelist' ? 'whitelist' : 'blacklist',
    allowed_tools: normalizeStringList(server.allowed_tools),
    disabled_tools: normalizeStringList(server.disabled_tools),
    disabled_resources: normalizeStringList(server.disabled_resources),
    daily_max_requests: server.daily_max_requests ?? null,
    monthly_max_requests: server.monthly_max_requests ?? null,
    enabled: server.enabled,
    timeout_ms: server.timeout_ms,
  }
}

export function mcpFormToRequest(form: McpForm): McpServerRequest {
  return {
    args: parseJsonText(form.args_text, [], 'MCP args must be a JSON array'),
    allowed_tools: form.allowed_tools,
    bearer_tokens:
      form.transport === 'http'
        ? form.bearer_tokens
            .map((value) => value.trim())
            .filter((value) => value !== '')
        : null,
    command: form.transport === 'stdio' ? form.command.trim() : null,
    disabled_resources: form.disabled_resources,
    disabled_tools: form.disabled_tools,
    enabled: form.enabled,
    env_json:
      form.transport === 'stdio'
        ? parseJsonText(form.env_text, {}, 'MCP env must be a JSON object')
        : {},
    http_headers_json:
      form.transport === 'http'
        ? parseJsonText(
            form.http_headers_text,
            {},
            'MCP HTTP headers must be a JSON object',
          )
        : {},
    name: form.name.trim(),
    owner_user_id: form.scope === 'user' ? form.owner_user_id : null,
    scope: form.scope,
    aggregate_naming_mode: form.aggregate_naming_mode,
    daily_max_requests: form.daily_max_requests,
    monthly_max_requests: form.monthly_max_requests,
    timeout_ms: form.timeout_ms,
    tool_filter_mode: form.tool_filter_mode,
    transport: form.transport,
    url: form.transport === 'http' ? form.url.trim() : null,
  }
}
