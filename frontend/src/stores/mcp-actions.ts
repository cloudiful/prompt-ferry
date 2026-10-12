import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  McpCatalogResponse,
  McpServer,
  McpServerRequest,
  McpTestResponse,
} from '@/generated/admin-api'
import {
  createMcpServer,
  deleteMcpServer,
  testMcpServer,
  updateMcpServer,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload, recordActionKey } from '@/stores/record-action-types'

/**
 * Key namespaces for the guarded MCP writes.
 *
 * Save and delete of one server share a key, because both write it. Create has
 * its own: it is not yet a server the list can act on. The connectivity probe
 * sits behind its own prefix so running a test never blocks a write.
 */
export const MCP_KEY_PREFIX = 'mcp'
export const MCP_TEST_KEY_PREFIX = 'mcptest'

export function mcpActionKey(serverId: string | null): string {
  return recordActionKey(MCP_KEY_PREFIX, serverId)
}

export function mcpTestKey(serverId: string): string {
  return `${MCP_TEST_KEY_PREFIX}:${serverId}`
}

type McpSlice = {
  servers: Ref<McpServer[]>
  testResults: Ref<Record<string, McpTestResponse>>
  testingServerId: Ref<string>
  catalogCache: Ref<Record<string, McpCatalogResponse>>
}

export type McpActionDeps = {
  guard: AsyncActionGuard
  mcp: McpSlice
  refresh: () => Promise<void>
}

/**
 * The MCP writes, injected rather than reached for, so the store stays a thin
 * state holder and these can be exercised on plain refs.
 */
export function createMcpActions(deps: McpActionDeps) {
  const { guard, mcp, refresh } = deps

  function saveServer(
    serverId: string | null,
    body: McpServerRequest,
  ): Promise<AsyncActionOutcome<McpServer>> {
    const captured = capturePayload(body)
    return guard.run(
      mcpActionKey(serverId),
      async () => {
        const saved = serverId
          ? expectData(
              await updateMcpServer<true>(
                withData({ path: { server_id: serverId }, body: captured }),
              ),
            )
          : expectData(
              await createMcpServer<true>(withData({ body: captured })),
            )
        if (serverId) {
          delete mcp.catalogCache.value[serverId]
        }
        // Held across the reload, so the spinner covers the refreshed list
        // rather than stopping at the write.
        await refresh()
        return saved
      },
      'save',
    )
  }

  function removeServer(serverId: string): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      mcpActionKey(serverId),
      async () => {
        await deleteMcpServer<true>(withData({ path: { server_id: serverId } }))
        delete mcp.catalogCache.value[serverId]
        delete mcp.testResults.value[serverId]
        await refresh()
      },
      'delete',
    )
  }

  function runTest(
    serverId: string,
  ): Promise<AsyncActionOutcome<McpTestResponse>> {
    return guard.run(
      mcpTestKey(serverId),
      async () => {
        mcp.testingServerId.value = serverId
        try {
          const result = expectData(
            await testMcpServer<true>(
              withData({ path: { server_id: serverId } }),
            ),
          )
          mcp.testResults.value[serverId] = result
          // The probe is also a catalog read, so it refreshes the cached view
          // the same way the row's own result is refreshed.
          mcp.catalogCache.value = {
            ...mcp.catalogCache.value,
            [serverId]: {
              tools: result.tools,
              resources: result.resources,
              prompts: result.prompts,
            },
          }
          return result
        } finally {
          mcp.testingServerId.value = ''
        }
      },
      'test',
    )
  }

  return { removeServer, runTest, saveServer }
}

export type McpActions = ReturnType<typeof createMcpActions>
