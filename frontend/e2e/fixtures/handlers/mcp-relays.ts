import type {
  ManagedRelay,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
  ManagedRelaySecretPatch,
  McpServer,
  McpServerRequest,
} from '../../../src/generated/admin-api'
import {
  error,
  nextId,
  ok,
  page,
  queryInt,
  readJson,
  requireAdmin,
  unauthorized,
  type FixtureContext,
} from '../http'
import { isoNow } from '../http'

const MCP_PROVIDERS = /^\/api\/v1\/admin\/mcp-providers$/
const MCP_SERVERS = /^\/api\/v1\/admin\/mcp-servers$/
const MCP_PATH = /^\/api\/v1\/admin\/(?:mcp-providers|mcp-servers(?:\/[^/]+)*)$/
const MCP_ONE = /^\/api\/v1\/admin\/mcp-servers\/([^/]+)$/
const MCP_CATALOG = /^\/api\/v1\/admin\/mcp-servers\/([^/]+)\/catalog$/
const MCP_TEST = /^\/api\/v1\/admin\/mcp-servers\/([^/]+)\/test$/
const RELAYS = /^\/api\/v1\/admin\/relays$/
const RELAY_ONE = /^\/api\/v1\/admin\/relays\/([^/]+)$/
const RELAY_RECONNECT = /^\/api\/v1\/admin\/relays\/([^/]+)\/reconnect$/

const RELAY_SECRET_FIELDS = [
  'relay_ca_pem',
  'client_cert_pem',
  'client_key_pem',
  'bridge_encryption_key',
] as const

const RELAY_REQUEST_FIELDS = [
  'name',
  'relay_url',
  'enabled',
  'tls_mode',
  'bridge_encryption_mode',
  ...RELAY_SECRET_FIELDS,
]

const catalogItem = (name: string) => ({
  aggregate_names: [`fixture__${name}`],
  description: `Fixture catalog entry for ${name}`,
  name,
  title: name,
})

const fixtureTools = [
  'fixture-lookup',
  'fixture-write',
  ...Array.from(
    { length: 8 },
    (_, index) => `fixture-tool-${String(index + 3).padStart(2, '0')}`,
  ),
]

function fixtureCatalog(serverId?: string) {
  const tools =
    serverId === 'mcp-fixture-1'
      ? [...fixtureTools, 'fixture-server-one-only']
      : serverId === 'mcp-fixture-3'
        ? [...fixtureTools, 'fixture-server-three-only']
        : fixtureTools
  return {
    prompts: [catalogItem('fixture-prompt')],
    resources: [catalogItem('fixture-resource')],
    tools: tools.map(catalogItem),
  }
}

function requestField<K extends keyof McpServerRequest, T>(
  body: McpServerRequest,
  key: K,
  current: T,
): T {
  return Object.hasOwn(body, key) ? (body[key] as T) : current
}

function serverVisibleToAccount(
  server: McpServer,
  ctx: FixtureContext,
): boolean {
  const account = ctx.account
  return Boolean(
    account &&
    (account.isAdmin ||
      (server.scope === 'user' &&
        server.owner_user_id === account.user.user_id)),
  )
}

function applyServerRequest(
  server: McpServer,
  body: McpServerRequest,
  serverId: string,
  ctx: FixtureContext,
): McpServer {
  const account = ctx.account
  const transport = body.transport
  const scope = account?.isAdmin ? (body.scope ?? server.scope) : 'user'
  const ownerUserId = account?.isAdmin
    ? requestField(body, 'owner_user_id', server.owner_user_id ?? null)
    : (account?.user.user_id ?? null)
  const providerKind =
    transport === 'builtin_minimax'
      ? 'minimax'
      : transport !== 'http'
        ? null
        : requestField(body, 'provider_kind', server.provider_kind ?? null)
  const hostedProvider = ctx.state.mcpProviders.find(
    (provider) =>
      provider.id === providerKind &&
      provider.default_url !== null &&
      provider.auth === 'bearer',
  )

  return {
    ...server,
    aggregate_naming_mode: requestField(
      body,
      'aggregate_naming_mode',
      server.aggregate_naming_mode,
    ),
    allowed_tools: requestField(body, 'allowed_tools', server.allowed_tools),
    args: requestField(body, 'args', server.args),
    auth_mode: hostedProvider
      ? 'bearer'
      : requestField(body, 'auth_mode', server.auth_mode),
    basic_username: requestField(
      body,
      'basic_username',
      server.basic_username ?? null,
    ),
    bearer_tokens: Array.isArray(body.bearer_tokens)
      ? body.bearer_tokens
      : server.bearer_tokens,
    command: requestField(body, 'command', server.command ?? null),
    disabled_resources: requestField(
      body,
      'disabled_resources',
      server.disabled_resources,
    ),
    disabled_tools: requestField(body, 'disabled_tools', server.disabled_tools),
    enabled: requestField(body, 'enabled', server.enabled),
    env_json: requestField(body, 'env_json', server.env_json),
    has_basic_password:
      Boolean(body.basic_password?.trim()) || server.has_basic_password,
    has_proxy_url:
      body.proxy_url === undefined || body.proxy_url === null
        ? server.has_proxy_url
        : Boolean(body.proxy_url.trim()),
    http_headers_json: requestField(
      body,
      'http_headers_json',
      server.http_headers_json,
    ),
    lifecycle_manual_protocol_version:
      requestField(
        body,
        'lifecycle_manual_protocol_version',
        server.lifecycle_manual_protocol_version ?? null,
      )?.trim() || null,
    lifecycle_policy: requestField(
      body,
      'lifecycle_policy',
      server.lifecycle_policy,
    ),
    name: body.name,
    owner_user_id: ownerUserId,
    provider_kind: providerKind,
    proxy_url: body.proxy_url ?? server.proxy_url,
    scope,
    server_id: serverId,
    timeout_ms: body.timeout_ms ?? server.timeout_ms,
    tool_filter_mode: body.tool_filter_mode ?? server.tool_filter_mode,
    transport,
    updated_at: isoNow(),
    url: hostedProvider?.default_url ?? body.url ?? server.url,
  }
}

function relayInputError(value: unknown, creating: boolean): string | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return 'relay request must be an object'
  }
  const body = value as Record<string, unknown>
  for (const key of Object.keys(body)) {
    if (
      !RELAY_REQUEST_FIELDS.includes(
        key as (typeof RELAY_REQUEST_FIELDS)[number],
      )
    ) {
      return 'relay request contains an unsupported field'
    }
  }

  if (creating) {
    for (const key of [
      'name',
      'relay_url',
      'tls_mode',
      'bridge_encryption_mode',
    ]) {
      if (!Object.hasOwn(body, key)) return `${key} is required`
    }
  }

  for (const key of ['name', 'relay_url'] as const) {
    const field = body[key]
    if (field === undefined || field === null) {
      if (creating) return `${key} is required`
      continue
    }
    if (typeof field !== 'string') return `${key} must be a string`
  }

  if (
    body.enabled !== undefined &&
    body.enabled !== null &&
    typeof body.enabled !== 'boolean'
  ) {
    return 'enabled must be a boolean'
  }

  if (
    body.tls_mode !== undefined &&
    body.tls_mode !== null &&
    !['off', 'server', 'mtls'].includes(String(body.tls_mode))
  ) {
    return 'tls_mode is invalid'
  }
  if (
    body.bridge_encryption_mode !== undefined &&
    body.bridge_encryption_mode !== null &&
    !['off', 'required'].includes(String(body.bridge_encryption_mode))
  ) {
    return 'bridge_encryption_mode is invalid'
  }

  for (const field of RELAY_SECRET_FIELDS) {
    const patch = body[field]
    if (patch === undefined || patch === null) continue
    if (!patch || typeof patch !== 'object' || Array.isArray(patch)) {
      return `${field} must be a secret patch`
    }
    const secret = patch as Record<string, unknown>
    if (secret.mode === 'replace') {
      if (
        Object.keys(secret).some((key) => key !== 'mode' && key !== 'value')
      ) {
        return `${field} has an invalid secret patch`
      }
      if (typeof secret.value !== 'string') {
        return `${field} replace requires a string value`
      }
      if (!secret.value.trim()) return `${field} cannot be empty`
    } else if (secret.mode === 'keep' || secret.mode === 'clear') {
      if (Object.keys(secret).some((key) => key !== 'mode')) {
        return `${field} has an invalid secret patch`
      }
      if (creating) return `${field} cannot use ${secret.mode} on create`
    } else {
      return `${field} has an invalid secret mode`
    }
  }

  return null
}

function normalizeRelayUrl(value: string): string {
  return value.trim().replace(/\/+$/, '')
}

function relayUrlError(value: string, tlsMode: string): string | null {
  if (!value) return 'relay_url is required'
  const expectedProtocol = tlsMode === 'off' ? 'ws:' : 'wss:'
  const expectedPrefix = tlsMode === 'off' ? 'ws://' : 'wss://'
  if (!value.startsWith(expectedPrefix)) {
    return `relay_url must use ${expectedPrefix} when tls_mode=${tlsMode}`
  }
  try {
    const parsed = new URL(value)
    if (parsed.protocol !== expectedProtocol || !parsed.hostname) {
      return 'relay_url is invalid'
    }
  } catch {
    return 'relay_url is invalid'
  }
  return null
}

function hasSecret(
  patch: ManagedRelaySecretPatch | null | undefined,
  existing: boolean,
): boolean {
  if (!patch || patch.mode === 'keep') return existing
  if (patch.mode === 'clear') return false
  return Boolean(patch.value.trim())
}

function validateRelayMaterial(
  tlsMode: string,
  bridgeMode: string,
  hasClientCert: boolean,
  hasClientKey: boolean,
  hasBridgeKey: boolean,
): string | null {
  if (tlsMode === 'mtls' && (!hasClientCert || !hasClientKey)) {
    return 'client_cert_pem and client_key_pem are required when tls_mode=mtls'
  }
  if (bridgeMode === 'required' && !hasBridgeKey) {
    return 'bridge_encryption_key is required when bridge_encryption_mode=required'
  }
  return null
}

function findDuplicateRelayUrl(
  relays: ManagedRelay[],
  relayUrl: string,
  exceptRelayId?: string,
): boolean {
  return relays.some(
    (relay) =>
      relay.relay_id !== exceptRelayId &&
      normalizeRelayUrl(relay.relay_url) === relayUrl,
  )
}

function createRelayFixture(
  ctx: FixtureContext,
  body: ManagedRelayRequest,
): Response {
  const name = body.name.trim()
  const relayUrl = normalizeRelayUrl(body.relay_url)
  const tlsMode = body.tls_mode
  const bridgeMode = body.bridge_encryption_mode
  const urlError = relayUrlError(relayUrl, tlsMode)
  if (urlError) return error('bad_request', urlError)
  if (findDuplicateRelayUrl(ctx.state.relays, relayUrl)) {
    return error(
      'duplicate_relay_url',
      'relay_url must be unique after normalization',
    )
  }

  const hasRelayCa = hasSecret(body.relay_ca_pem, false)
  const hasClientCert = hasSecret(body.client_cert_pem, false)
  const hasClientKey = hasSecret(body.client_key_pem, false)
  const hasBridgeKey = hasSecret(body.bridge_encryption_key, false)
  const materialError = validateRelayMaterial(
    tlsMode,
    bridgeMode,
    hasClientCert,
    hasClientKey,
    hasBridgeKey,
  )
  if (materialError) return error('bad_request', materialError)

  const now = isoNow()
  const relay: ManagedRelay = {
    bridge_encryption_mode: bridgeMode,
    connected: false,
    created_at: now,
    enabled: body.enabled !== false,
    has_bridge_key: hasBridgeKey,
    has_client_cert: hasClientCert,
    has_client_key: hasClientKey,
    has_relay_ca: hasRelayCa,
    last_connected_at: null,
    last_disconnected_at: null,
    last_error: null,
    last_snapshot_version: null,
    name,
    relay_id: nextId(ctx.state, 'relay'),
    relay_url: relayUrl,
    tls_mode: tlsMode,
    updated_at: now,
  }
  ctx.state.relays.push(relay)
  return ok(relay)
}

function updateRelayFixture(
  ctx: FixtureContext,
  relay: ManagedRelay,
  body: ManagedRelayPatchRequest,
): Response {
  const name = body.name == null ? relay.name : body.name.trim()
  if (!name) return error('bad_request', 'name is required')
  const relayUrl = normalizeRelayUrl(body.relay_url ?? relay.relay_url)
  const tlsMode = body.tls_mode ?? relay.tls_mode
  const bridgeMode = body.bridge_encryption_mode ?? relay.bridge_encryption_mode
  const urlError = relayUrlError(relayUrl, tlsMode)
  if (urlError) return error('bad_request', urlError)
  if (findDuplicateRelayUrl(ctx.state.relays, relayUrl, relay.relay_id)) {
    return error(
      'duplicate_relay_url',
      'relay_url must be unique after normalization',
    )
  }

  const hasRelayCa = hasSecret(body.relay_ca_pem, relay.has_relay_ca)
  const hasClientCert = hasSecret(body.client_cert_pem, relay.has_client_cert)
  const hasClientKey = hasSecret(body.client_key_pem, relay.has_client_key)
  const hasBridgeKey = hasSecret(
    body.bridge_encryption_key,
    relay.has_bridge_key,
  )
  const materialError = validateRelayMaterial(
    tlsMode,
    bridgeMode,
    hasClientCert,
    hasClientKey,
    hasBridgeKey,
  )
  if (materialError) return error('bad_request', materialError)

  const updated: ManagedRelay = {
    ...relay,
    bridge_encryption_mode: bridgeMode,
    enabled: body.enabled ?? relay.enabled,
    has_bridge_key: hasBridgeKey,
    has_client_cert: hasClientCert,
    has_client_key: hasClientKey,
    has_relay_ca: hasRelayCa,
    name,
    relay_url: relayUrl,
    tls_mode: tlsMode,
    updated_at: isoNow(),
  }
  ctx.state.relays = ctx.state.relays.map((candidate) =>
    candidate.relay_id === relay.relay_id ? updated : candidate,
  )
  return ok(updated)
}

export async function handleMcpRelays(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  const isMcpPath = MCP_PATH.test(pathname)
  if (isMcpPath && !ctx.account) return unauthorized()

  const isRelayPath =
    RELAYS.test(pathname) ||
    RELAY_ONE.test(pathname) ||
    RELAY_RECONNECT.test(pathname)
  if (isRelayPath) {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
  }

  if (MCP_PROVIDERS.test(pathname) && method === 'GET') {
    return ok(ctx.state.mcpProviders)
  }

  if (MCP_SERVERS.test(pathname)) {
    if (method === 'GET') {
      const visibleServers = ctx.state.mcpServers.filter((server) =>
        serverVisibleToAccount(server, ctx),
      )
      return ok(
        page(
          visibleServers,
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'servers',
        ),
      )
    }
    if (method === 'POST') {
      const body = await readJson<McpServerRequest>(ctx)
      const serverId = nextId(ctx.state, 'mcp')
      const server: McpServer = {
        aggregate_naming_mode: 'passthrough_preferred',
        allowed_tools: [],
        args: [],
        auth_mode: 'none',
        basic_username: null,
        bearer_tokens: [],
        command: null,
        created_at: isoNow(),
        disabled_resources: [],
        disabled_tools: [],
        enabled: body.enabled ?? true,
        env_json: null,
        has_basic_password: false,
        has_proxy_url: false,
        http_headers_json: null,
        lifecycle_manual_protocol_version: null,
        lifecycle_policy: 'as_declared',
        name: body.name,
        owner_user_id: null,
        provider_kind: null,
        proxy_url: null,
        scope: 'global',
        server_id: serverId,
        timeout_ms: body.timeout_ms ?? 30000,
        tool_filter_mode: 'blacklist',
        transport: 'http',
        updated_at: isoNow(),
        url: body.url ?? '',
      }
      ctx.state.mcpServers.push(applyServerRequest(server, body, serverId, ctx))
      return ok(ctx.state.mcpServers.at(-1))
    }
  }

  const isMcpCatalogPath = MCP_CATALOG.test(pathname)
  const mcpServerMatch = MCP_ONE.exec(pathname)
  if (isMcpCatalogPath || mcpServerMatch) {
    if (!ctx.account) return unauthorized()
    const serverId = (
      isMcpCatalogPath ? MCP_CATALOG.exec(pathname) : mcpServerMatch
    )?.[1]
    const server = ctx.state.mcpServers.find(
      (candidate) => candidate.server_id === serverId,
    )
    if (!server || !serverVisibleToAccount(server, ctx)) {
      return error('not_found', 'fixture MCP server not found', 404)
    }
    if (isMcpCatalogPath && method === 'GET') {
      if (!server.enabled) {
        return ok({
          server_id: serverId,
          tools: [],
          resources: [],
          prompts: [],
        })
      }
      const catalog = fixtureCatalog(serverId)
      return ok({
        server_id: serverId,
        tools: catalog.tools,
        resources: catalog.resources,
        prompts: catalog.prompts,
      })
    }
    if (method === 'GET') return ok(server)
    if (method === 'PATCH') {
      const body = await readJson<McpServerRequest>(ctx)
      const next = applyServerRequest(server, body, server.server_id, ctx)
      ctx.state.mcpServers = ctx.state.mcpServers.map((candidate) =>
        candidate === server ? next : candidate,
      )
      return ok(next)
    }
    if (method === 'DELETE') {
      ctx.state.mcpServers = ctx.state.mcpServers.filter(
        (candidate) => candidate !== server,
      )
      return new Response(null, { status: 204 })
    }
  }

  if (MCP_TEST.test(pathname) && method === 'POST') {
    const serverId = MCP_TEST.exec(pathname)?.[1]
    const server = ctx.state.mcpServers.find(
      (candidate) => candidate.server_id === serverId,
    )
    if (!server || !serverVisibleToAccount(server, ctx)) {
      return error('not_found', 'fixture MCP server not found', 404)
    }
    if (!server.enabled) {
      return ok({
        duration_ms: 0,
        message: 'mcp server is disabled',
        ok: false,
        prompt_count: 0,
        prompts: [],
        resource_count: 0,
        resources: [],
        tool_count: 0,
        tools: [],
      })
    }
    const catalog = fixtureCatalog(serverId)
    return ok({
      duration_ms: 180,
      message: 'fixture MCP handshake succeeded',
      ok: true,
      prompt_count: catalog.prompts.length,
      prompts: catalog.prompts,
      resource_count: catalog.resources.length,
      resources: catalog.resources,
      tool_count: catalog.tools.length,
      tools: catalog.tools,
    })
  }

  if (isMcpPath) {
    return error(
      'not_found',
      'fixture does not implement this MCP endpoint',
      404,
    )
  }

  if (RELAYS.test(pathname)) {
    if (method === 'GET') {
      const first = Math.max(0, queryInt(ctx.url, 'first', 0))
      const rows = Math.max(1, Math.min(200, queryInt(ctx.url, 'rows', 10)))
      return ok({
        connected_count: ctx.state.relays.filter((relay) => relay.connected)
          .length,
        enabled_count: ctx.state.relays.filter((relay) => relay.enabled).length,
        first,
        relays: ctx.state.relays.slice(first, first + rows),
        rows,
        total: ctx.state.relays.length,
      })
    }
    if (method === 'POST') {
      const body = await readJson<ManagedRelayRequest>(ctx)
      const invalid = relayInputError(body, true)
      if (invalid) return error('bad_request', invalid)
      return createRelayFixture(ctx, body)
    }
  }

  const relayId = RELAY_ONE.exec(pathname)?.[1]
  if (relayId) {
    const relay = ctx.state.relays.find(
      (candidate) => candidate.relay_id === relayId,
    )
    if (!relay) return error('not_found', 'fixture relay not found', 404)
    if (method === 'GET') return ok(relay)
    if (method === 'PATCH') {
      const body = await readJson<ManagedRelayPatchRequest>(ctx)
      const invalid = relayInputError(body, false)
      if (invalid) return error('bad_request', invalid)
      return updateRelayFixture(ctx, relay, body)
    }
    if (method === 'DELETE') {
      ctx.state.relays = ctx.state.relays.filter(
        (candidate) => candidate !== relay,
      )
      return new Response(null, { status: 204 })
    }
  }

  const reconnectId = RELAY_RECONNECT.exec(pathname)?.[1]
  if (reconnectId && method === 'POST') {
    const relay = ctx.state.relays.find(
      (candidate) => candidate.relay_id === reconnectId,
    )
    if (!relay) return error('not_found', 'fixture relay not found', 404)
    if (!relay.enabled) {
      return error('relay_disabled', 'disabled relay cannot reconnect')
    }
    relay.connected = true
    relay.last_error = null
    relay.last_connected_at = isoNow()
    relay.last_snapshot_version = (relay.last_snapshot_version ?? 0) + 1
    relay.updated_at = isoNow()
    return ok(relay)
  }

  return null
}
