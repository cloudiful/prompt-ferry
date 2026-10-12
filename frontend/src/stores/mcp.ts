import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import {
  getMcpCatalog,
  listMcpProviders,
  listMcpServers,
} from '../generated/admin-api'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  McpServer,
  McpTestResponse,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createMcpWorkspaceView } from '../models/mcp'
import { useLocale } from '../composables/useLocale'
import { useAsyncAction } from '../composables/useAsyncAction'
import { MCP_KEY_PREFIX, createMcpActions } from './mcp-actions'
import { pendingRowsById } from './record-action-types'

export const useMcpStore = defineStore('mcp', () => {
  const { t } = useLocale()
  const servers = ref<McpServer[]>([])
  const providers = ref<McpProviderDescriptor[]>([])
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
        providerGenericLabel: t('providerGeneric'),
        providerLabels: Object.fromEntries(
          providers.value.map((provider) => [
            provider.id,
            provider.display_name,
          ]),
        ),
        hostedProviderIds: providers.value
          .filter((provider) => provider.default_url != null)
          .map((provider) => provider.id),
      },
      testResults: testResults.value,
      total: serverTotal.value,
    }),
  )

  async function setServerPage(first: number, rows: number): Promise<void> {
    await refresh(first, rows)
  }

  async function loadProviders(): Promise<void> {
    providers.value = expectData(await listMcpProviders<true>(withData({})))
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

  // The guard lives here rather than at each call site, so a server can be
  // gated against every operation that would write it, not just the one the
  // caller happened to remember.
  const guard = useAsyncAction()
  const actions = createMcpActions({
    guard,
    mcp: { servers, testResults, testingServerId, catalogCache },
    refresh,
  })

  // Per-row view of the guard. `loading` stays the fetch-only busy flag, so a
  // write never blanks the whole page.
  const mcpPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, MCP_KEY_PREFIX),
  )

  return {
    ...actions,
    catalogCache,
    getCachedCatalog,
    isPending: guard.isPending,
    loadCatalog,
    loadProviders,
    loading,
    mcpPendingActions,
    pendingActions: guard.pendingActions,
    primeCatalog,
    providers,
    refresh,
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
