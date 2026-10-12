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
// The dialog is identified by the generation it was opened in. Every open and
// edit bumps it, so a save that settles late can tell that the surface it was
// started from is no longer the one on screen and must not close it.
let dialogGeneration = 0
const dialogLearned = ref<{
  mode: string | null
  protocolVersion: string | null
  learnedAt: string | null
} | null>(null)
const dialogCatalog = ref<McpCatalogResponse>({
  tools: [],
  resources: [],
  prompts: [],
})
const dialogCatalogLoading = ref(false)

const busy = computed(() => mcpStore.loading)
// The dialog's own write. It follows the record the draft belongs to, so a
// write against another server never freezes this dialog, and an unsaved draft
// is keyed the same way the row map names a create.
const mcpSavePending = computed(() =>
  Object.hasOwn(
    mcpStore.mcpPendingActions,
    dialogForm.value.server_id || 'create',
  ),
)
const dialogHeader = computed(() =>
  dialogForm.value.server_id ? t('edit') : t('newMcpServer'),
)

async function refresh(): Promise<void> {
  try {
    await Promise.all([
      mcpStore.refresh(),
      mcpStore.loadProviders(),
      session.isAdmin ? usersStore.loadUsers() : Promise.resolve(),
    ])
  } catch (cause) {
    notifyApiError(cause)
  }
}

function openMcpDialog(): void {
  dialogGeneration += 1
  dialogForm.value = createEmptyMcpForm()
  dialogLearned.value = null
  dialogCatalog.value = { tools: [], resources: [], prompts: [] }
  dialogCatalogLoading.value = false
  dialogVisible.value = true
}

async function editMcpServer(server: McpServer): Promise<void> {
  dialogGeneration += 1
  dialogForm.value = mcpServerToForm(server)
  dialogLearned.value = {
    mode: server.lifecycle_learned_mode ?? null,
    protocolVersion: server.lifecycle_learned_protocol_version ?? null,
    learnedAt: server.lifecycle_learned_at ?? null,
  }
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
  const generation = dialogGeneration
  const serverId = dialogForm.value.server_id || null
  // Built from the draft as it stands before the request is claimed, so a later
  // edit cannot change what this save sends.
  const body = mcpFormToRequest(dialogForm.value)
  try {
    const result = await mcpStore.saveServer(serverId, body)
    // A refused call is the guard collapsing a duplicate trigger: no close, no
    // toast, no error.
    if (!result.executed) return
    if (dialogGeneration === generation) {
      dialogVisible.value = false
      dialogForm.value = createEmptyMcpForm()
      dialogCatalog.value = { tools: [], resources: [], prompts: [] }
      dialogCatalogLoading.value = false
      dialogGeneration += 1
    }
    notifySuccess(t('saved'))
  } catch (cause) {
    // The dialog and its draft stay as they are so the save can be retried.
    notifyApiError(cause)
  }
}

async function testMcpServer(server: McpServer): Promise<void> {
  try {
    const outcome = await mcpStore.runTest(server.server_id)
    if (!outcome.executed) return
    const result = outcome.value
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
    const result = await mcpStore.saveServer(
      server.server_id,
      mcpFormToRequest(form),
    )
    if (result.executed) notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function deleteMcpServer(server: McpServer): Promise<void> {
  if (!window.confirm(`${t('delete')} ${server.name}?`)) return
  try {
    const result = await mcpStore.removeServer(server.server_id)
    if (result.executed) notifySuccess(t('delete'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onMcpPage(event: TablePageChange): Promise<void> {
  try {
    await mcpStore.setServerPage(event.first, event.rows)
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  await refresh()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro>
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
      :pending-keys="mcpStore.mcpPendingActions"
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
      :learned="dialogLearned"
      :providers="mcpStore.providers"
      :save-pending="mcpSavePending"
      :t="t"
      :users="usersStore.users"
      @save="saveMcpServer"
    />
  </div>
</template>
