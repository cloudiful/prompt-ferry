import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import {
  createMcpServer,
  deleteMcpServer,
  getMcpCatalog,
  listMcpServers,
  testMcpServer,
  updateMcpServer,
} from '../generated/admin-api'
import type {
  McpCatalogResponse,
  McpServer,
  McpServerRequest,
  McpTestResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createMcpWorkspaceView } from '../models/mcp'
import { useLocale } from '../composables/useLocale'

export const useMcpStore = defineStore('mcp', () => {
  const { t } = useLocale()
  const servers = ref<McpServer[]>([])
  const serverFirst = ref(0)
  const serverRows = useStoredPageSize(
    'mcp-servers',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const loading = ref(false)
  const testingServerId = ref('')
  const testResults = ref<Record<string, McpTestResponse>>({})
  const catalogCache = ref<Record<string, McpCatalogResponse>>({})
  const serverTotal = ref(0)
  const selectedWorkspaceView = computed(() =>
    createMcpWorkspaceView(servers.value, {
      labels: {
        aggregateNamingModePassthroughPreferredShort: t(
          'aggregateNamingModePassthroughPreferredShort',
        ),
        aggregateNamingModeQualifiedOnlyShort: t(
          'aggregateNamingModeQualifiedOnlyShort',
        ),
        managedMinimax: t('minimaxManaged'),
        privateScope: t('privateScope'),
        publicScope: t('publicScope'),
      },
      testResults: testResults.value,
      total: serverTotal.value,
    }),
  )

  async function setServerPage(first: number, rows: number): Promise<void> {
    await refresh(first, rows)
  }

  async function refresh(
    nextFirst = serverFirst.value,
    nextRows = serverRows.value,
  ): Promise<void> {
    serverFirst.value = nextFirst
    serverRows.value = nextRows
    loading.value = true
    try {
      const response = expectData(
        await listMcpServers<true>(
          withData({
            query: { first: serverFirst.value, rows: serverRows.value },
          }),
        ),
      )
      servers.value = response.servers
      serverTotal.value = response.total
      serverFirst.value = response.first
      serverRows.value = response.rows
      if (
        servers.value.length === 0 &&
        serverTotal.value > 0 &&
        serverFirst.value >= serverTotal.value
      ) {
        const previousFirst =
          Math.floor((serverTotal.value - 1) / serverRows.value) *
          serverRows.value
        await refresh(previousFirst, serverRows.value)
      }
    } finally {
      loading.value = false
    }
  }

  async function saveServer(
    serverId: string | null,
    body: McpServerRequest,
  ): Promise<McpServer> {
    const saved = serverId
      ? expectData(
          await updateMcpServer<true>(
            withData({ path: { server_id: serverId }, body }),
          ),
        )
      : expectData(await createMcpServer<true>(withData({ body })))
    if (serverId) {
      delete catalogCache.value[serverId]
    }
    await refresh()
    return saved
  }

  async function removeServer(serverId: string): Promise<void> {
    await deleteMcpServer<true>(withData({ path: { server_id: serverId } }))
    delete catalogCache.value[serverId]
    delete testResults.value[serverId]
    await refresh()
  }

  function getCachedCatalog(serverId: string): McpCatalogResponse | null {
    return catalogCache.value[serverId] ?? null
  }

  async function loadCatalog(
    serverId: string,
    force = false,
  ): Promise<McpCatalogResponse> {
    const cached = !force ? getCachedCatalog(serverId) : null
    if (cached) {
      return cached
    }
    const loaded = expectData(
      await getMcpCatalog<true>(withData({ path: { server_id: serverId } })),
    )
    catalogCache.value = {
      ...catalogCache.value,
      [serverId]: loaded,
    }
    return loaded
  }

  function primeCatalog(
    serverId: string,
    nextCatalog: McpCatalogResponse,
  ): void {
    catalogCache.value = {
      ...catalogCache.value,
      [serverId]: nextCatalog,
    }
  }

  async function runTest(serverId: string): Promise<McpTestResponse> {
    testingServerId.value = serverId
    try {
      const result = expectData(
        await testMcpServer<true>(withData({ path: { server_id: serverId } })),
      )
      testResults.value[serverId] = result
      primeCatalog(serverId, {
        tools: result.tools,
        resources: result.resources,
        prompts: result.prompts,
      })
      return result
    } finally {
      testingServerId.value = ''
    }
  }

  return {
    catalogCache,
    getCachedCatalog,
    loadCatalog,
    loading,
    primeCatalog,
    refresh,
    removeServer,
    runTest,
    saveServer,
    selectedWorkspaceView,
    serverFirst,
    serverRows,
    serverTotal,
    servers,
    setServerPage,
    testResults,
    testingServerId,
  }
})
