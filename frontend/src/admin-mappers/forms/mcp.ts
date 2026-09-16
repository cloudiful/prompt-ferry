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
    provider_kind: 'generic',
    url: '',
    command_argv_text: '[]',
    auth_mode: 'none',
    bearer_tokens: [],
    basic_username: '',
    basic_password: '',
    has_basic_password: false,
    // Issue #375 Phase F: masked per-row proxy; empty + no saved means inherit.
    proxy_url: '',
    has_saved_proxy_url: false,
    http_headers_text: '{}',
    environment_variables: [],
    tool_filter_mode: 'blacklist',
    allowed_tools: [],
    disabled_tools: [],
    disabled_resources: [],
    enabled: true,
    timeout_ms: 30000,
    lifecycle_policy: 'auto',
    lifecycle_manual_protocol_version: null,
  }
}

export function mcpServerToForm(server: McpServer): McpForm {
  // INLINE-proxy-ui-a1: tolerate legacy payloads missing Phase F fields.
  const source = (server ?? {}) as Partial<McpServer>
  const authMode =
    source.auth_mode === 'bearer' || source.auth_mode === 'basic'
      ? source.auth_mode
      : 'none'
  return {
    server_id: source.server_id ?? '',
    source_endpoint_id: source.source_endpoint_id ?? null,
    scope: source.scope === 'user' ? 'user' : 'admin',
    owner_user_id: source.owner_user_id ?? null,
    name: source.name ?? '',
    aggregate_naming_mode:
      source.aggregate_naming_mode === 'qualified_only'
        ? 'qualified_only'
        : 'passthrough_preferred',
    transport:
      source.transport === 'builtin_minimax'
        ? 'builtin_minimax'
        : source.transport === 'stdio'
          ? 'stdio'
          : 'http',
    provider_kind: source.provider_kind ?? 'generic',
    url: source.url ?? '',
    command_argv_text: JSON.stringify(commandArgv(source as McpServer)),
    auth_mode: authMode,
    bearer_tokens: (source.bearer_tokens ?? []).map((value) => ({
      token: value?.token ?? '',
      enabled: value?.enabled ?? true,
    })),
    basic_username: source.basic_username ?? '',
    basic_password: '',
    has_basic_password: source.has_basic_password ?? false,
    // Issue #375 Phase F: masked proxy input; never echo the secret.
    // `has_proxy_url` is optional for legacy payloads (missing means inherit).
    proxy_url: '',
    has_saved_proxy_url: source.has_proxy_url ?? false,
    http_headers_text: JSON.stringify(
      normalizeJsonRecord(source.http_headers_json),
      null,
      2,
    ),
    environment_variables: environmentVariables(source.env_json),
    tool_filter_mode:
      source.tool_filter_mode === 'whitelist' ? 'whitelist' : 'blacklist',
    allowed_tools: normalizeStringList(source.allowed_tools),
    disabled_tools: normalizeStringList(source.disabled_tools),
    disabled_resources: normalizeStringList(source.disabled_resources),
    enabled: source.enabled ?? true,
    timeout_ms: source.timeout_ms ?? 30000,
    lifecycle_policy:
      source.lifecycle_policy === 'legacy_initialize'
        ? 'legacy_initialize'
        : 'auto',
    lifecycle_manual_protocol_version:
      source.lifecycle_manual_protocol_version ?? null,
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
  // Issue #375 Phase F: omit-when-untouched to avoid secret-wipe.
  // Non-empty means replace; empty + saved means omit (keep stored value);
  // empty + no saved means clear to inherit (`""`). The request-side
  // `has_proxy_url` hint is intentionally not sent (server ignores it).
  // INLINE-proxy-ui-a1: tolerate legacy forms missing proxy fields.
  const trimmedProxy = (form.proxy_url ?? '').trim()
  const proxy_url =
    trimmedProxy !== ''
      ? trimmedProxy
      : (form.has_saved_proxy_url ?? false)
        ? undefined
        : ''
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
    provider_kind:
      form.transport === 'http' ? form.provider_kind || 'generic' : null,
    timeout_ms: form.timeout_ms,
    tool_filter_mode: form.tool_filter_mode,
    transport: form.transport,
    url: form.transport === 'http' ? form.url.trim() : null,
    proxy_url,
    lifecycle_policy: form.lifecycle_policy,
    lifecycle_manual_protocol_version:
      form.lifecycle_manual_protocol_version?.trim() || '',
  }
}
