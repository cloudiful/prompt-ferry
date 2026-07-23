import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  createClientKey,
  deleteClientKey,
  listClientKeys,
  listUsers,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
  updateClientKey,
} from '../generated/admin-api'
import type {
  ClientKey,
  CreateClientKeyResponse,
  UpdateClientKeyRequest,
  User,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

export const SELF_USER_ID = 0

export const useApiKeysStore = defineStore('api-keys', () => {
  const users = ref<User[]>([])
  const selectedUserId = ref<number>(SELF_USER_ID)
  const keys = ref<ClientKey[]>([])
  const loading = ref(false)
  const usersLoading = ref(false)

  const hasUsers = computed(() => users.value.length > 0)
  const selectedUser = computed(() =>
    selectedUserId.value === SELF_USER_ID
      ? null
      : (users.value.find((user) => user.user_id === selectedUserId.value) ??
        null),
  )

  async function loadUsers(): Promise<User[]> {
    usersLoading.value = true
    try {
      const loaded = expectData(await listUsers<true>(withData()))
      users.value = loaded
      return loaded
    } finally {
      usersLoading.value = false
    }
  }

  async function loadKeys(forceUserId?: number): Promise<ClientKey[]> {
    const targetUserId = forceUserId ?? selectedUserId.value
    loading.value = true
    try {
      const loaded =
        targetUserId === SELF_USER_ID
          ? expectData(await meListClientKeys<true>(withData()))
          : expectData(
              await listClientKeys<true>(
                withData({ path: { user_id: targetUserId } }),
              ),
            )
      keys.value = loaded
      return loaded
    } finally {
      loading.value = false
    }
  }

  async function changeSelectedUser(userId: number): Promise<void> {
    selectedUserId.value = userId
    await loadKeys(userId)
  }

  async function createKey(
    label?: string | null,
    forceUserId?: number,
  ): Promise<CreateClientKeyResponse> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const created =
      targetUserId === SELF_USER_ID
        ? expectData(
            await meCreateClientKey<true>(
              withData({ body: { label: label ?? null } }),
            ),
          )
        : expectData(
            await createClientKey<true>(
              withData({
                path: { user_id: targetUserId },
                body: { label: label ?? null },
              }),
            ),
          )
    await loadKeys(targetUserId)
    return created
  }

  async function saveKey(
    keyId: number,
    input: UpdateClientKeyRequest,
    forceUserId?: number,
  ): Promise<ClientKey> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const saved =
      targetUserId === SELF_USER_ID
        ? expectData(
            await meUpdateClientKey<true>(
              withData({ path: { key_id: keyId }, body: input }),
            ),
          )
        : expectData(
            await updateClientKey<true>(
              withData({
                path: { user_id: targetUserId, key_id: keyId },
                body: input,
              }),
            ),
          )
    await loadKeys(targetUserId)
    return saved
  }

  async function removeKey(keyId: number, forceUserId?: number): Promise<void> {
    const targetUserId = forceUserId ?? selectedUserId.value
    if (targetUserId === SELF_USER_ID) {
      await meDeleteClientKey<true>(withData({ path: { key_id: keyId } }))
    } else {
      await deleteClientKey<true>(
        withData({ path: { user_id: targetUserId, key_id: keyId } }),
      )
    }
    await loadKeys(targetUserId)
  }

  return {
    changeSelectedUser,
    createKey,
    hasUsers,
    keys,
    loadKeys,
    loadUsers,
    loading,
    removeKey,
    saveKey,
    selectedUser,
    selectedUserId,
    users,
    usersLoading,
  }
})
