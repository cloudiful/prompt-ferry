import type { QueryClient } from '@tanstack/react-query'
import type {
  McpCatalogResponse,
  McpServerPageResponse,
  McpTestResponse,
} from '@/generated/admin-api'
import {
  getMcpOauthStatusQueryKey,
  listMcpServersOptions,
  listMcpServersQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { recordActionKey } from '@/stores/record-action-types'

export const MCP_KEY_PREFIX = 'mcp'
export const MCP_TEST_KEY_PREFIX = 'mcptest'
export const MCP_PROTOCOL_VERSION_AUTO = 'auto'
export const MCP_PROTOCOL_VERSIONS = [
  '2026-07-28',
  '2025-11-25',
  '2025-06-18',
  '2025-03-26',
  '2024-11-05',
] as const

export const EMPTY_MCP_CATALOG: McpCatalogResponse = {
  tools: [],
  resources: [],
  prompts: [],
}

export function mcpActionKey(serverId: string | null): string {
  return recordActionKey(MCP_KEY_PREFIX, serverId)
}

export function mcpTestKey(serverId: string): string {
  return `${MCP_TEST_KEY_PREFIX}:${serverId}`
}

export function mcpOAuthStatusQueryKey(serverId: string) {
  return getMcpOauthStatusQueryKey({ path: { server_id: serverId } })
}

export function mcpLastPageFirst(total: number, rows: number): number {
  return total > 0 && rows > 0 ? Math.floor((total - 1) / rows) * rows : 0
}

export function mcpCatalogFromTest(
  result: McpTestResponse,
): McpCatalogResponse {
  return {
    tools: result.tools,
    resources: result.resources,
    prompts: result.prompts,
  }
}

export async function refreshMcpServerPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<McpServerPageResponse> {
  const currentPageKey = listMcpServersQueryKey({ query: { first, rows } })
  await queryClient.cancelQueries({ queryKey: currentPageKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: listMcpServersQueryKey(),
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery(
      listMcpServersOptions({ query: { first: requestedFirst, rows } }),
    )
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.servers.length === 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst = mcpLastPageFirst(page.total, pageRows)
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}
