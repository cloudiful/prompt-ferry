<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import PageIntro from '@/components/PageIntro.vue'
import KeyDialog from '@/components/users/KeyDialog.vue'
import ApiKeysList from '@/components/users/ApiKeysList.vue'
import { copyText } from '@/composables/useClipboard'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import { createApiKeysWorkspaceView } from '@/models/api-keys'
import { useApiKeysStore, SELF_USER_ID } from '@/stores/api-keys'
import { useSessionStore } from '@/stores/session'

const { t } = useLocale()
const { notifyApiError, notifyError, notifySuccess } = useNotifier()
const apiKeysStore = useApiKeysStore()
const session = useSessionStore()

const visibleKeySecrets = ref<Record<number, boolean>>({})
const keyDialogVisible = ref(false)
const keyLabel = ref('')
const generatedSecret = ref('')

const busy = computed(
  () => apiKeysStore.loading || (session.isAdmin && apiKeysStore.usersLoading),
)
const userOptions = computed(() => {
  const selfOption = {
    label: `${t('self')} (${session.loginName || t('user')})`,
    value: SELF_USER_ID,
  }
  const adminOptions = apiKeysStore.users.map((user) => ({
    label: `${user.login_name}${user.display_name ? ` / ${user.display_name}` : ''}`,
    value: user.user_id,
  }))
  return [selfOption, ...adminOptions]
})
const workspace = computed(() =>
  createApiKeysWorkspaceView({
    keys: apiKeysStore.keys,
    selectedUser: apiKeysStore.selectedUser,
    visibleKeySecrets: visibleKeySecrets.value,
  }),
)

async function refresh(): Promise<void> {
  try {
    if (session.isAdmin) {
      await apiKeysStore.loadUsers()
    }
    await apiKeysStore.loadKeys()
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onUserChange(): Promise<void> {
  try {
    visibleKeySecrets.value = {}
    await apiKeysStore.changeSelectedUser(apiKeysStore.selectedUserId)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onPage(event: TablePageChange): Promise<void> {
  try {
    await apiKeysStore.loadKeys(undefined, event.first, event.rows)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function openCreateKey(): Promise<void> {
  keyLabel.value = ''
  generatedSecret.value = ''
  keyDialogVisible.value = true
}

async function submitCreateKey(): Promise<void> {
  const label = keyLabel.value.trim()
  if (!label) {
    notifyError(t('keyLabelRequired'))
    return
  }
  try {
    const response = await apiKeysStore.createKey(label)
    generatedSecret.value = response.secret
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

function toggleKeySecret(keyId: number): void {
  visibleKeySecrets.value[keyId] = !visibleKeySecrets.value[keyId]
}

function copyKeySecret(keyId: number): void {
  const secret =
    apiKeysStore.keys.find((key) => key.key_id === keyId)?.secret ?? ''
  void copyText(secret)
}

function copyGeneratedSecret(): void {
  void copyText(generatedSecret.value)
}

async function toggleKey(keyId: number): Promise<void> {
  const key = apiKeysStore.keys.find((item) => item.key_id === keyId)
  if (!key) return
  try {
    await apiKeysStore.saveKey(keyId, { enabled: !key.enabled })
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function renameKey(keyId: number, label: string): Promise<void> {
  try {
    await apiKeysStore.saveKey(keyId, { label })
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function deleteKey(keyId: number): Promise<void> {
  const key = apiKeysStore.keys.find((item) => item.key_id === keyId)
  if (!key) return
  if (!window.confirm(`${t('delete')} ${key.label}?`)) return
  try {
    await apiKeysStore.removeKey(keyId)
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
    <PageIntro :eyebrow="t('access')" :title="t('apiKeys')">
      <template #actions>
        <label v-if="session.isAdmin" class="flex min-w-52 items-center">
          <USelect
            v-model="apiKeysStore.selectedUserId"
            :items="userOptions"
            label-key="label"
            value-key="value"
            :aria-label="t('selectUser')"
            class="h-10 w-full"
            @change="onUserChange"
          />
        </label>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="busy"
          :aria-label="t('refresh')"
          class="h-10"
          @click="refresh"
        >
          <span>{{ t('refresh') }}</span>
        </UButton>
        <UButton
          size="sm"
          :aria-label="t('generateKey')"
          class="h-10"
          @click="openCreateKey"
        >
          <span aria-hidden="true" class="md:hidden">新增</span>
          <span aria-hidden="true" class="hidden md:inline">{{
            t('generateKey')
          }}</span>
        </UButton>
      </template>
    </PageIntro>

    <ApiKeysList
      :t="t"
      :workspace="workspace"
      :first="apiKeysStore.first"
      :rows="apiKeysStore.rows"
      :total="apiKeysStore.total"
      @toggle-key-secret="toggleKeySecret"
      @copy-key-secret="copyKeySecret"
      @toggle-key="toggleKey"
      @rename-key="renameKey"
      @delete-key="deleteKey"
      @page="onPage"
    />

    <KeyDialog
      v-model:visible="keyDialogVisible"
      v-model:key-label="keyLabel"
      :busy="busy"
      :generated-secret="generatedSecret"
      :t="t"
      @copy-secret="copyGeneratedSecret"
      @submit="submitCreateKey"
    />
  </div>
</template>
