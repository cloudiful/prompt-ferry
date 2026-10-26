import type { QueryClient } from '@tanstack/react-query'
import {
  createMcpServer,
  deleteMcpServer,
  getMcpCatalog,
  listMcpProviders,
  listMcpServers,
  listUserOptions,
  testMcpServer,
  updateMcpServer,
} from '@/generated/admin-api'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  McpServer,
  McpServerPageResponse,
  McpServerRequest,
  McpTestResponse,
  User,
  UserOptionsResponse,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { recordActionKey } from '@/stores/record-action-types'

const mcpServersNamespace = ['mcp-servers-page'] as const
const mcpProvidersNamespace = ['mcp-providers'] as const
const mcpUserOptionsNamespace = ['mcp-user-options'] as const

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

export function mcpServersQueryKey() {
  return mcpServersNamespace
}

export function mcpServerPageQueryKey(
  first: number,
  rows: number,
  userId: number,
  isAdmin: boolean,
) {
  return [...mcpServersNamespace, userId, isAdmin, first, rows] as const
}

export function mcpProvidersQueryKey() {
  return mcpProvidersNamespace
}

export function mcpUserOptionsQueryKey(adminId: number) {
  return [...mcpUserOptionsNamespace, adminId] as const
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

export async function fetchMcpServerPage(
  first: number,
  rows: number,
): Promise<McpServerPageResponse> {
  return expectData(
    await listMcpServers<true>(withData({ query: { first, rows } })),
  )
}

export async function fetchMcpProviders(): Promise<McpProviderDescriptor[]> {
  return expectData(await listMcpProviders<true>(withData()))
}

export async function fetchMcpUsers(): Promise<User[]> {
  const response = expectData(
    await listUserOptions<true>(withData()),
  ) as UserOptionsResponse
  return response.users
}

export async function fetchMcpCatalog(
  serverId: string,
): Promise<McpCatalogResponse> {
  return expectData(
    await getMcpCatalog<true>(withData({ path: { server_id: serverId } })),
  )
}

export async function saveMcpServerRecord(
  serverId: string | null,
  body: McpServerRequest,
): Promise<McpServer> {
  return serverId
    ? expectData(
        await updateMcpServer<true>(
          withData({ path: { server_id: serverId }, body }),
        ),
      )
    : expectData(await createMcpServer<true>(withData({ body })))
}

export async function removeMcpServerRecord(serverId: string): Promise<void> {
  await deleteMcpServer<true>(withData({ path: { server_id: serverId } }))
}

export async function runMcpServerTest(
  serverId: string,
): Promise<McpTestResponse> {
  return expectData(
    await testMcpServer<true>(withData({ path: { server_id: serverId } })),
  )
}

export async function refreshMcpServerPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
  userId: number,
  isAdmin: boolean,
): Promise<McpServerPageResponse> {
  const currentPageKey = mcpServerPageQueryKey(first, rows, userId, isAdmin)
  await queryClient.cancelQueries({ queryKey: currentPageKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: mcpServersQueryKey(),
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: mcpServerPageQueryKey(requestedFirst, rows, userId, isAdmin),
      queryFn: () => fetchMcpServerPage(requestedFirst, rows),
      staleTime: 0,
    })
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
