import type { McpServer, McpTestResponse } from '../generated/admin-api'

export type McpServerListItemView = {
  server: McpServer
  server_id: string
  name: string
  enabled: boolean
  transport: string
  endpoint_label: string
  scope_label: string
  timeout_label: string
  naming_mode_label: string
  test_message: string
  test_ok: boolean | null
}

export type McpServerWorkspaceView = {
  total_count: number
  list_items: McpServerListItemView[]
}

type McpViewLabels = {
  aggregateNamingModePassthroughPreferredShort: string
  aggregateNamingModeQualifiedOnlyShort: string
  managedMinimax: string
  privateScope: string
  publicScope: string
}

export function createMcpServerListItemView(
  server: McpServer,
  options: {
    labels: McpViewLabels
    testResult?: McpTestResponse | null
  },
): McpServerListItemView {
  return {
    server,
    server_id: server.server_id,
    name: server.name,
    enabled: server.enabled,
    transport:
      server.transport === 'builtin_minimax'
        ? options.labels.managedMinimax
        : server.transport,
    endpoint_label:
      server.transport === 'builtin_minimax'
        ? options.labels.managedMinimax
        : server.transport === 'http'
          ? (server.url ?? '-')
          : (server.command ?? '-'),
    scope_label:
      server.scope === 'admin'
        ? options.labels.publicScope
        : options.labels.privateScope,
    timeout_label: `${server.timeout_ms}ms`,
    naming_mode_label:
      server.aggregate_naming_mode === 'qualified_only'
        ? options.labels.aggregateNamingModeQualifiedOnlyShort
        : options.labels.aggregateNamingModePassthroughPreferredShort,
    test_message: options.testResult?.message || '-',
    test_ok: options.testResult?.ok ?? null,
  }
}

export function createMcpWorkspaceView(
  servers: McpServer[],
  options: {
    labels: McpViewLabels
    testResults: Record<string, McpTestResponse>
    total?: number
  },
): McpServerWorkspaceView {
  return {
    total_count: options.total ?? servers.length,
    list_items: servers.map((server) =>
      createMcpServerListItemView(server, {
        labels: options.labels,
        testResult: options.testResults[server.server_id] ?? null,
      }),
    ),
  }
}
