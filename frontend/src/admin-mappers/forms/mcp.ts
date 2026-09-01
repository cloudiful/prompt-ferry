import type { McpServer, McpServerRequest } from '../../generated/admin-api'
import type { McpEnvironmentVariableForm, McpForm } from '../../models'

import {
  normalizeJsonArray,
  normalizeJsonRecord,
  normalizeStringList,
  parseJsonText,
} from '../utils'

const WORKER_ENV_REFERENCE = /^\{env:([A-Za-z_][A-Za-z0-9_]*)\}$/

function commandArgv(server: McpServer): string[] {
  const command = server.command?.trim()
  if (!command) return []
  return [
    command,
    ...normalizeJsonArray(server.args).filter(
      (value): value is string => typeof value === 'string',
    ),
  ]
}

function environmentVariables(value: unknown): McpEnvironmentVariableForm[] {
  const variables: McpEnvironmentVariableForm[] = []
  for (const [name, rawValue] of Object.entries(normalizeJsonRecord(value))) {
    if (typeof rawValue === 'string') {
      const workerMatch = rawValue.match(WORKER_ENV_REFERENCE)
      if (workerMatch) {
        variables.push({
          name,
          source: 'worker',
          value: workerMatch[1],
          has_saved_value: false,
        })
        continue
      }
    }
    variables.push({
      name,
      source: 'value',
      value: '',
      has_saved_value: true,
    })
  }
  return variables
}

function parseCommandArgv(text: string): string[] {
  const parsed = parseJsonText(
    text,
    [],
    'MCP command must be a JSON array of strings',
  )
  if (
    !Array.isArray(parsed) ||
    parsed.length === 0 ||
    !parsed.every(
      (value): value is string =>
        typeof value === 'string' && value.trim().length > 0,
    )
  ) {
    throw new Error('MCP command must be a non-empty JSON array of strings')
  }
  return (parsed as string[]).map((value) => value.trim())
}

export function createEmptyMcpForm(): McpForm {
  return {
    server_id: '',
    source_endpoint_id: null,
    scope: 'admin',
    owner_user_id: null,
    name: '',
    aggregate_naming_mode: 'passthrough_preferred',
    transport: 'http',
    url: '',
    command_argv_text: '[]',
    auth_mode: 'none',
    bearer_tokens: [],
    basic_username: '',
    basic_password: '',
    has_basic_password: false,
    http_headers_text: '{}',
    environment_variables: [],
    tool_filter_mode: 'blacklist',
    allowed_tools: [],
    disabled_tools: [],
    disabled_resources: [],
    daily_max_requests: null,
    monthly_max_requests: null,
    enabled: true,
    timeout_ms: 30000,
    lifecycle_policy: 'auto',
    lifecycle_manual_protocol_version: null,
  }
}

export function mcpServerToForm(server: McpServer): McpForm {
  const authMode =
    server.auth_mode === 'bearer' || server.auth_mode === 'basic'
      ? server.auth_mode
      : 'none'
  return {
    server_id: server.server_id,
    source_endpoint_id: server.source_endpoint_id ?? null,
    scope: server.scope === 'user' ? 'user' : 'admin',
    owner_user_id: server.owner_user_id ?? null,
    name: server.name,
    aggregate_naming_mode:
      server.aggregate_naming_mode === 'qualified_only'
        ? 'qualified_only'
        : 'passthrough_preferred',
    transport:
      server.transport === 'builtin_minimax'
        ? 'builtin_minimax'
        : server.transport === 'stdio'
          ? 'stdio'
          : 'http',
    url: server.url ?? '',
    command_argv_text: JSON.stringify(commandArgv(server)),
    auth_mode: authMode,
    bearer_tokens: server.bearer_tokens.map((value) => ({
      token: value.token,
      enabled: value.enabled,
    })),
    basic_username: server.basic_username ?? '',
    basic_password: '',
    has_basic_password: server.has_basic_password ?? false,
    http_headers_text: JSON.stringify(
      normalizeJsonRecord(server.http_headers_json),
      null,
      2,
    ),
    environment_variables: environmentVariables(server.env_json),
    tool_filter_mode:
      server.tool_filter_mode === 'whitelist' ? 'whitelist' : 'blacklist',
    allowed_tools: normalizeStringList(server.allowed_tools),
    disabled_tools: normalizeStringList(server.disabled_tools),
    disabled_resources: normalizeStringList(server.disabled_resources),
    daily_max_requests: server.daily_max_requests ?? null,
    monthly_max_requests: server.monthly_max_requests ?? null,
    enabled: server.enabled,
    timeout_ms: server.timeout_ms,
    lifecycle_policy:
      server.lifecycle_policy === 'legacy_initialize'
        ? 'legacy_initialize'
        : 'auto',
    lifecycle_manual_protocol_version:
      server.lifecycle_manual_protocol_version ?? null,
  }
}

export function mcpFormToRequest(form: McpForm): McpServerRequest {
  const commandArgv =
    form.transport === 'stdio' ? parseCommandArgv(form.command_argv_text) : []

  const envJson: Record<string, string | null> = {}
  const names = new Set<string>()
  for (const variable of form.environment_variables) {
    const name = variable.name.trim()
    const value = variable.value.trim()
    if (!name) continue
    if (names.has(name)) {
      throw new Error(`Duplicate MCP environment variable: ${name}`)
    }
    names.add(name)
    if (variable.source === 'worker') {
      if (!value) {
        throw new Error(
          `MCP environment variable ${name} requires a Worker variable name`,
        )
      }
      envJson[name] = `{env:${value}}`
      continue
    }
    if (value) {
      envJson[name] = value
    } else if (variable.has_saved_value) {
      envJson[name] = null
    } else {
      throw new Error(`MCP environment variable ${name} requires a value`)
    }
  }

  const authMode = form.transport === 'http' ? form.auth_mode : 'none'
  return {
    source_endpoint_id: form.source_endpoint_id,
    args: commandArgv.slice(1),
    allowed_tools: form.allowed_tools,
    bearer_tokens:
      form.transport === 'http' && authMode === 'bearer'
        ? form.bearer_tokens
            .map((value) => ({
              token: value.token.trim(),
              enabled: value.enabled,
            }))
            .filter((value) => value.token !== '')
        : null,
    auth_mode: authMode,
    basic_username:
      form.transport === 'http' && authMode === 'basic'
        ? form.basic_username.trim()
        : null,
    basic_password:
      form.transport === 'http' && authMode === 'basic'
        ? form.basic_password.trim()
          ? form.basic_password.trim()
          : form.has_basic_password
            ? ''
            : null
        : null,
    command: form.transport === 'stdio' ? String(commandArgv[0]).trim() : null,
    disabled_resources: form.disabled_resources,
    disabled_tools: form.disabled_tools,
    enabled: form.enabled,
    env_json: form.transport === 'stdio' ? envJson : {},
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
    lifecycle_policy: form.lifecycle_policy,
    lifecycle_manual_protocol_version:
      form.lifecycle_manual_protocol_version?.trim() || '',
  }
}
