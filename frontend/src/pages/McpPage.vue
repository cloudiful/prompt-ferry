<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PageIntro from '../components/PageIntro.vue'
import {
  createEmptyMcpForm,
  mcpFormToRequest,
  mcpServerToForm,
} from '../admin-mappers'
import { useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import McpDialog from '../components/mcp/McpDialog.vue'
import McpPanel from '../components/mcp/McpPanel.vue'
import type { McpCatalogResponse, McpServer } from '../generated/admin-api'
import type { McpForm } from '../models'
import { useMcpStore } from '../stores/mcp'
import { useSessionStore } from '../stores/session'
import { useUsersStore } from '../stores/users'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const session = useSessionStore()
const mcpStore = useMcpStore()
const usersStore = useUsersStore()

const dialogVisible = ref(false)
const dialogForm = ref<McpForm>(createEmptyMcpForm())
const dialogCatalog = ref<McpCatalogResponse>({
  tools: [],
  resources: [],
  prompts: [],
})
const dialogCatalogLoading = ref(false)

const busy = computed(() => mcpStore.loading)
const dialogHeader = computed(() =>
  dialogForm.value.server_id ? t('edit') : t('newMcpServer'),
)

async function refresh(): Promise<void> {
  try {
    await Promise.all([
      mcpStore.refresh(),
      session.isAdmin ? usersStore.loadUsers() : Promise.resolve(),
    ])
  } catch (cause) {
    notifyApiError(cause)
  }
}

function openMcpDialog(): void {
  dialogForm.value = createEmptyMcpForm()
  dialogCatalog.value = { tools: [], resources: [], prompts: [] }
  dialogCatalogLoading.value = false
  dialogVisible.value = true
}

async function editMcpServer(server: McpServer): Promise<void> {
  dialogForm.value = mcpServerToForm(server)
  dialogCatalog.value = mcpStore.getCachedCatalog(server.server_id) ?? {
    tools: [],
    resources: [],
    prompts: [],
  }
  dialogCatalogLoading.value = server.enabled
  dialogVisible.value = true
  if (!server.enabled) {
    dialogCatalogLoading.value = false
    dialogCatalog.value = { tools: [], resources: [], prompts: [] }
    return
  }
  try {
    dialogCatalog.value = await mcpStore.loadCatalog(server.server_id)
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    dialogCatalogLoading.value = false
  }
}

async function saveMcpServer(): Promise<void> {
  try {
    await mcpStore.saveServer(
      dialogForm.value.server_id || null,
      mcpFormToRequest(dialogForm.value),
    )
    dialogVisible.value = false
    dialogForm.value = createEmptyMcpForm()
    dialogCatalog.value = { tools: [], resources: [], prompts: [] }
    dialogCatalogLoading.value = false
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function testMcpServer(server: McpServer): Promise<void> {
  try {
    const result = await mcpStore.runTest(server.server_id)
    if (dialogForm.value.server_id === server.server_id) {
      dialogCatalog.value = {
        tools: result.tools,
        resources: result.resources,
        prompts: result.prompts,
      }
      dialogCatalogLoading.value = false
    }
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function toggleMcpServer(server: McpServer): Promise<void> {
  try {
    const form = mcpServerToForm(server)
    form.enabled = !form.enabled
    await mcpStore.saveServer(server.server_id, mcpFormToRequest(form))
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function deleteMcpServer(server: McpServer): Promise<void> {
  if (!window.confirm(`${t('delete')} ${server.name}?`)) return
  try {
    await mcpStore.removeServer(server.server_id)
    notifySuccess(t('delete'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

function onMcpPage(event: TablePageChange): void {
  mcpStore.setServerPage(event.first, event.rows)
}

onMounted(async () => {
  await refresh()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :eyebrow="t('tooling')" :title="t('mcp')">
      <template #actions>
        <UButton
          size="sm"
          :aria-label="t('newMcpServer')"
          @click="openMcpDialog"
        >
          <span aria-hidden="true" class="md:hidden">新增</span>
          <span aria-hidden="true" class="hidden md:inline">{{
            t('newMcpServer')
          }}</span>
        </UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="busy"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <span>{{ t('refresh') }}</span>
        </UButton>
      </template>
    </PageIntro>

    <McpPanel
      :busy="busy"
      :mcp-first="mcpStore.serverFirst"
      :mcp-rows="mcpStore.serverRows"
      :testing-mcp-server-id="mcpStore.testingServerId"
      :t="t"
      :workspace-view="mcpStore.selectedWorkspaceView"
      @delete-mcp-server="deleteMcpServer"
      @edit-mcp-server="editMcpServer"
      @mcp-page="onMcpPage"
      @test-mcp-server="testMcpServer"
      @toggle-mcp-server="toggleMcpServer"
    />

    <McpDialog
      v-model:visible="dialogVisible"
      v-model:form="dialogForm"
      :busy="busy"
      :catalog="dialogCatalog"
      :catalog-loading="dialogCatalogLoading"
      :header="dialogHeader"
      :is-admin="session.isAdmin"
      :t="t"
      :users="usersStore.users"
      @save="saveMcpServer"
    />
  </div>
</template>
